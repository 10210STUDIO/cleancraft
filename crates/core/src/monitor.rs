//! Live Monitor: CPU, memory, storage, battery and network, read from macOS command-line tools
//! through [`crate::sys::Sys`], so tests answer them with `FakeSys`.
//!
//! Commands (each one cheap, all read-only):
//! - `/usr/sbin/sysctl hw.memsize hw.ncpu kern.boottime vm.loadavg vm.swapusage
//!   kern.memorystatus_vm_pressure_level` ("name: value" lines, parsed by name so a key the
//!   system does not know only loses that value).
//! - `/usr/bin/top -l 2 -n 0 -s 1` for the CPU load (the second sample covers the last second).
//!   When it fails, the load is the sum of `ps` CPU shares divided by the number of cores.
//! - `/bin/ps -axco pid=,%cpu=,rss=,comm=` for the top apps by CPU and by memory.
//! - `/usr/bin/vm_stat` for memory use (page size from its header).
//! - `/bin/df -Pk <home>` for the startup disk, `~/.Trash` walked with [`tree_size`].
//! - `/usr/bin/pmset -g batt` and `/usr/sbin/ioreg -rn AppleSmartBattery` for the battery.
//! - `/usr/sbin/netstat -ib` byte counters; speed is the difference between two samples.
//!
//! A value that cannot be read is `None` and shown as "Unknown", never as zero. Each reason is
//! kept in [`Snapshot::problems`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::env::{Env, Fault};
use crate::executor::{self, ActionKind, ActionRecord};
use crate::fsutil::{human_size, tree_size};
use crate::plan::FeatureId;

/// Free space below this share of the disk raises the low-space alert.
pub const LOW_FREE_FRACTION: f64 = 0.10;
/// Free space below this many bytes raises the low-space alert.
pub const LOW_FREE_BYTES: u64 = 10_000_000_000;
/// Memory use above this share raises the heavy-memory alert.
pub const HEAVY_MEMORY_FRACTION: f64 = 0.90;
/// A Trash bigger than this raises the large-Trash alert.
pub const LARGE_TRASH_BYTES: u64 = 5_000_000_000;
/// How many apps the top lists show.
pub const TOP_APPS: usize = 5;
/// Points kept for the CPU history line.
pub const CPU_HISTORY: usize = 60;

pub const SYSCTL_KEYS: [&str; 6] = [
    "hw.memsize",
    "hw.ncpu",
    "kern.boottime",
    "vm.loadavg",
    "vm.swapusage",
    "kern.memorystatus_vm_pressure_level",
];

/// The administrator command behind "Free Up RAM".
pub const PURGE_COMMAND: &str = "/usr/sbin/purge";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AppUse {
    pub pid: u32,
    pub name: String,
    /// Percent of one core, as `ps` reports it.
    pub cpu: f64,
    /// Resident memory in bytes.
    pub memory: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Cpu {
    /// Percent of the whole machine, 0 to 100.
    pub load: f64,
    pub user: Option<f64>,
    pub system: Option<f64>,
    /// 1, 5 and 15 minute load averages.
    pub load_avg: Option<[f64; 3]>,
    pub cores: Option<u32>,
    pub top: Vec<AppUse>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

impl Pressure {
    pub fn label(self) -> &'static str {
        match self {
            Pressure::Normal => "Normal",
            Pressure::Warning => "Elevated",
            Pressure::Critical => "Critical",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Memory {
    pub total: u64,
    /// App memory + wired + compressed, like Activity Monitor's "Memory Used".
    pub used: u64,
    pub app: u64,
    pub wired: u64,
    pub compressed: u64,
    /// File cache macOS gives back on demand.
    pub cached: u64,
    pub pressure: Option<Pressure>,
    pub swap_used: Option<u64>,
    pub swap_total: Option<u64>,
    pub top: Vec<AppUse>,
}

impl Memory {
    pub fn available(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }

    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Storage {
    pub total: Option<u64>,
    pub free: Option<u64>,
    /// Bytes in ~/.Trash; None when it could not be read.
    pub trash: Option<u64>,
    /// Why the Trash size is unknown.
    pub trash_note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Battery {
    pub percent: u8,
    /// "charging", "discharging", "charged", "finishing charge", "AC attached"...
    pub state: String,
    pub on_ac: bool,
    /// Minutes, when pmset gives an estimate.
    pub remaining_minutes: Option<u32>,
    pub cycles: Option<u32>,
    /// Full-charge capacity as a percent of the design capacity.
    pub health: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BatteryReading {
    /// A desktop Mac.
    None,
    Present(Battery),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NetRate {
    /// Bytes per second.
    pub down: u64,
    pub up: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetCounters {
    pub rx: u64,
    pub tx: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Snapshot {
    /// Seconds since the epoch.
    pub taken: i64,
    pub cpu: Option<Cpu>,
    pub uptime_secs: Option<u64>,
    pub memory: Option<Memory>,
    pub storage: Storage,
    pub battery: Option<BatteryReading>,
    /// None on the first sample (a speed needs two) or when the counters could not be read.
    pub network: Option<NetRate>,
    /// What could not be read, in plain words.
    pub problems: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    LowSpace,
    HeavyMemory,
    LargeTrash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Alert {
    pub kind: AlertKind,
    pub text: String,
}

// ---------------------------------------------------------------------------------------------
// Parsers. Each takes the tool's stdout and returns None when it does not look right.

/// `sysctl a b c` lines: "name: value". Values may span the rest of the line.
pub fn parse_sysctl(stdout: &str) -> BTreeMap<String, String> {
    stdout
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(": ")?;
            let k = k.trim();
            // Names always have a dot; "sysctl: unknown oid" lines do not.
            if !k.contains('.') || k.contains(' ') {
                return None;
            }
            Some((k.to_string(), v.trim().to_string()))
        })
        .collect()
}

/// `kern.boottime`: "{ sec = 1760000000, usec = 123456 } Thu Oct  9 10:13:20 2025".
pub fn parse_boottime(v: &str) -> Option<i64> {
    let rest = v.split("sec =").nth(1)?;
    rest.split(',').next()?.trim().parse().ok()
}

/// `vm.loadavg`: "{ 1.85 2.10 2.30 }".
pub fn parse_loadavg(v: &str) -> Option<[f64; 3]> {
    let nums: Vec<f64> = v
        .trim_matches(|c| c == '{' || c == '}' || c == ' ')
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    (nums.len() >= 3).then(|| [nums[0], nums[1], nums[2]])
}

/// Size with a sysctl unit suffix: "1024.50M" (MiB), "2.00G", "512.00K".
fn parse_sysctl_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()? {
        'K' => (&s[..s.len() - 1], 1024.0),
        'M' => (&s[..s.len() - 1], 1024.0 * 1024.0),
        'G' => (&s[..s.len() - 1], 1024.0 * 1024.0 * 1024.0),
        'T' => (&s[..s.len() - 1], 1024.0f64.powi(4)),
        c if c.is_ascii_digit() => (s, 1.0),
        _ => return None,
    };
    let v: f64 = num.parse().ok()?;
    Some((v * mult).round() as u64)
}

/// `vm.swapusage`: "total = 2048.00M  used = 1024.50M  free = 1023.50M  (encrypted)".
/// Returns (used, total).
pub fn parse_swapusage(v: &str) -> Option<(u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        let rest = v.split(&format!("{name} =")).nth(1)?;
        parse_sysctl_size(rest.split_whitespace().next()?)
    };
    Some((field("used")?, field("total")?))
}

/// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warning, 4 critical.
pub fn parse_pressure_level(v: &str) -> Option<Pressure> {
    match v.trim() {
        "1" => Some(Pressure::Normal),
        "2" => Some(Pressure::Warning),
        "4" => Some(Pressure::Critical),
        _ => None,
    }
}

/// `top -l 2 -n 0 -s 1`: the last "CPU usage: 7.69% user, 10.25% sys, 82.5% idle" line.
/// Returns (load, user, sys) in percent.
pub fn parse_top_cpu(stdout: &str) -> Option<(f64, f64, f64)> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("CPU usage:"))?;
    let rest = line.split_once(':')?.1;
    let mut user = None;
    let mut sys = None;
    let mut idle = None;
    for part in rest.split(',') {
        let mut w = part.split_whitespace();
        let num: f64 = w.next()?.trim_end_matches('%').parse().ok()?;
        match w.next()? {
            "user" => user = Some(num),
            "sys" => sys = Some(num),
            "idle" => idle = Some(num),
            _ => {}
        }
    }
    let (user, sys, idle) = (user?, sys?, idle?);
    Some(((100.0 - idle).clamp(0.0, 100.0), user, sys))
}

/// `ps -axco pid=,%cpu=,rss=,comm=` lines: "  512  12.3  204800 Safari". rss is in KiB.
/// The name is the rest of the line and may contain spaces.
pub fn parse_ps_usage(stdout: &str) -> Option<Vec<AppUse>> {
    let mut out = Vec::new();
    for l in stdout.lines() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        let parts: Vec<&str> = l.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }
        let pid: u32 = parts[0].parse().ok()?;
        let cpu: f64 = parts[1].replace(',', ".").parse().ok()?;
        let rss: u64 = parts[2].parse().ok()?;
        // The name: everything after the third column, spacing kept.
        let mut rest = l;
        for _ in 0..3 {
            rest = rest.trim_start();
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = &rest[end..];
        }
        out.push(AppUse {
            pid,
            name: rest.trim().to_string(),
            cpu,
            memory: rss * 1024,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// The `n` biggest by CPU share, then by name for ties.
pub fn top_by_cpu(apps: &[AppUse], n: usize) -> Vec<AppUse> {
    let mut v = apps.to_vec();
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

/// The `n` biggest by memory, then by name for ties.
pub fn top_by_memory(apps: &[AppUse], n: usize) -> Vec<AppUse> {
    let mut v = apps.to_vec();
    v.sort_by(|a, b| b.memory.cmp(&a.memory).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

/// `vm_stat`: header "Mach Virtual Memory Statistics: (page size of 16384 bytes)" then
/// "Pages free:   12345." lines. Returns (page size, counters by name).
pub fn parse_vm_stat(stdout: &str) -> Option<(u64, BTreeMap<String, u64>)> {
    let mut lines = stdout.lines();
    let header = lines.next()?;
    let page: u64 = header
        .split("page size of")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let mut map = BTreeMap::new();
    for l in lines {
        let Some((k, v)) = l.rsplit_once(':') else {
            continue;
        };
        let k = k.trim().trim_matches('"').to_string();
        if let Ok(n) = v.trim().trim_end_matches('.').parse::<u64>() {
            map.insert(k, n);
        }
    }
    (page > 0 && map.contains_key("Pages wired down")).then_some((page, map))
}

/// Memory use from `vm_stat` counters, like Activity Monitor: app = anonymous - purgeable,
/// used = app + wired + compressed, cached = file-backed + purgeable.
pub fn memory_from_vm_stat(total: u64, page: u64, m: &BTreeMap<String, u64>) -> Option<Memory> {
    let get = |k: &str| m.get(k).copied();
    let wired = get("Pages wired down")? * page;
    let compressed = get("Pages occupied by compressor").unwrap_or(0) * page;
    let purgeable = get("Pages purgeable").unwrap_or(0) * page;
    let app = match get("Anonymous pages") {
        Some(a) => (a * page).saturating_sub(purgeable),
        // Older systems: active + inactive - file-backed approximations are unreliable;
        // fall back to active pages.
        None => get("Pages active")? * page,
    };
    let cached = get("File-backed pages").unwrap_or(0) * page + purgeable;
    let used = (app + wired + compressed).min(total);
    Some(Memory {
        total,
        used,
        app,
        wired,
        compressed,
        cached,
        pressure: None,
        swap_used: None,
        swap_total: None,
        top: Vec::new(),
    })
}

/// `df -Pk <path>`: second line "dev 1024-blocks used available capacity mount".
/// Returns (total, free) in bytes.
pub fn parse_df(stdout: &str) -> Option<(u64, u64)> {
    let line = stdout.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    // The device name can contain spaces only on odd mounts; read from the capacity column.
    let cap = cols.iter().position(|c| c.ends_with('%'))?;
    if cap < 3 {
        return None;
    }
    let total: u64 = cols[cap - 3].parse().ok()?;
    let avail: u64 = cols[cap - 1].parse().ok()?;
    Some((total * 1024, avail * 1024))
}

/// `pmset -g batt`. Ok(None) when there is no battery (a desktop Mac).
///
/// ```text
/// Now drawing from 'Battery Power'
///  -InternalBattery-0 (id=4653155)<TAB>64%; discharging; 4:12 remaining present: true
/// ```
pub fn parse_pmset(stdout: &str) -> Option<Option<Battery>> {
    let first = stdout.lines().next()?;
    if !first.contains("drawing from") {
        return None;
    }
    let on_ac = first.contains("AC Power");
    let Some(line) = stdout.lines().find(|l| l.contains("InternalBattery")) else {
        return Some(None);
    };
    let after_tab = line.split_once('\t').map(|x| x.1).or_else(|| {
        // Some versions use spaces: take from the first "NN%".
        let i = line.find('%')?;
        let start = line[..i].rfind(|c: char| !c.is_ascii_digit())? + 1;
        Some(&line[start..])
    })?;
    let mut parts = after_tab.split(';').map(str::trim);
    let percent: u8 = parts.next()?.trim_end_matches('%').trim().parse().ok()?;
    let state = parts.next().unwrap_or("").to_string();
    let remaining = parts.next().unwrap_or("");
    let remaining_minutes = remaining.split_whitespace().next().and_then(|t| {
        let (h, m) = t.split_once(':')?;
        let h: u32 = h.parse().ok()?;
        let m: u32 = m.parse().ok()?;
        // "0:00 remaining" while charged or still estimating means no estimate.
        (h + m > 0).then_some(h * 60 + m)
    });
    Some(Some(Battery {
        percent: percent.min(100),
        state,
        on_ac,
        remaining_minutes,
        cycles: None,
        health: None,
    }))
}

/// Top-level `"Key" = value` integers of `ioreg -rn AppleSmartBattery`.
pub fn parse_ioreg_ints(stdout: &str) -> BTreeMap<String, i64> {
    stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_start_matches('|').trim();
            let rest = l.strip_prefix('"')?;
            let (k, v) = rest.split_once("\" = ")?;
            Some((k.to_string(), v.trim().parse().ok()?))
        })
        .collect()
}

/// (cycles, health percent) from ioreg values. Apple silicon reports MaxCapacity as a
/// percent and the real figure in AppleRawMaxCapacity; Intel reports mAh in MaxCapacity.
pub fn battery_health(io: &BTreeMap<String, i64>) -> (Option<u32>, Option<u8>) {
    let cycles = io.get("CycleCount").and_then(|c| u32::try_from(*c).ok());
    let design = io.get("DesignCapacity").copied().filter(|d| *d > 0);
    let max = io
        .get("AppleRawMaxCapacity")
        .or_else(|| io.get("MaxCapacity"))
        .copied()
        .filter(|m| *m > 0);
    let health = match (max, design) {
        (Some(m), Some(d)) if m > 100 || d <= 100 => {
            Some(((m as f64 / d as f64) * 100.0).round().clamp(0.0, 100.0) as u8)
        }
        _ => None,
    };
    (cycles, health)
}

/// `netstat -ib`: sum of the byte counters of every `<Link#n>` row except loopback.
/// Columns are read from the right, because the Address column is empty on some rows.
pub fn parse_netstat(stdout: &str) -> Option<NetCounters> {
    let mut lines = stdout.lines();
    let header = lines.next()?;
    if !header.contains("Ibytes") || !header.contains("Obytes") {
        return None;
    }
    let mut c = NetCounters::default();
    let mut seen = false;
    for l in lines {
        let cols: Vec<&str> = l.split_whitespace().collect();
        if cols.len() < 8 || !cols[2].starts_with("<Link#") {
            continue;
        }
        let name = cols[0].trim_end_matches('*');
        if name.starts_with("lo") {
            continue;
        }
        let n = cols.len();
        // ... Ibytes Opkts Oerrs Obytes Coll
        let (Ok(rx), Ok(tx)) = (cols[n - 5].parse::<u64>(), cols[n - 2].parse::<u64>()) else {
            continue;
        };
        c.rx += rx;
        c.tx += tx;
        seen = true;
    }
    seen.then_some(c)
}

/// Speed between two samples `secs` apart. None when time did not pass or a counter went
/// back (an interface was reset).
pub fn net_rate(prev: NetCounters, cur: NetCounters, secs: f64) -> Option<NetRate> {
    if secs <= 0.0 || cur.rx < prev.rx || cur.tx < prev.tx {
        return None;
    }
    Some(NetRate {
        down: ((cur.rx - prev.rx) as f64 / secs).round() as u64,
        up: ((cur.tx - prev.tx) as f64 / secs).round() as u64,
    })
}

// ---------------------------------------------------------------------------------------------
// Sampling.

/// Keeps what a speed or a history needs between samples.
#[derive(Clone, Debug, Default)]
pub struct Sampler {
    prev_net: Option<(f64, NetCounters)>,
    /// CPU load of recent samples, oldest first.
    pub cpu_history: Vec<f64>,
}

fn run_text(env: &Env, prog: &str, args: &[&str]) -> Result<String, String> {
    match env.sys.run(prog, args) {
        Ok(o) if o.ok() || !o.stdout.trim().is_empty() => Ok(o.stdout),
        Ok(o) => Err(format!(
            "{} failed: {}",
            prog.rsplit('/').next().unwrap_or(prog),
            o.stderr.trim()
        )),
        Err(e) => Err(format!(
            "{} could not run: {e}",
            prog.rsplit('/').next().unwrap_or(prog)
        )),
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler::default()
    }

    /// Read everything once. `now` is seconds since the epoch (fractions allowed); speeds use
    /// the time between this call and the previous one.
    pub fn sample(&mut self, env: &Env, now: f64) -> Snapshot {
        let mut problems = Vec::new();
        let sysctl = match run_text(env, "/usr/sbin/sysctl", &SYSCTL_KEYS) {
            Ok(s) => parse_sysctl(&s),
            Err(e) => {
                problems.push(e);
                BTreeMap::new()
            }
        };
        let memsize = sysctl.get("hw.memsize").and_then(|v| v.parse::<u64>().ok());
        let cores = sysctl.get("hw.ncpu").and_then(|v| v.parse::<u32>().ok());
        let uptime_secs = sysctl
            .get("kern.boottime")
            .and_then(|v| parse_boottime(v))
            .and_then(|b| u64::try_from(now as i64 - b).ok());
        if uptime_secs.is_none() {
            problems.push("uptime could not be read".into());
        }

        // Processes.
        let apps = match run_text(env, "/bin/ps", &["-axco", "pid=,%cpu=,rss=,comm="]) {
            Ok(s) => match parse_ps_usage(&s) {
                Some(a) => Some(a),
                None => {
                    problems.push("the process list was not understood".into());
                    None
                }
            },
            Err(e) => {
                problems.push(e);
                None
            }
        };

        // CPU.
        let top = run_text(env, "/usr/bin/top", &["-l", "2", "-n", "0", "-s", "1"])
            .ok()
            .and_then(|s| parse_top_cpu(&s));
        let cpu = match (top, &apps, cores) {
            (Some((load, user, sys)), _, _) => Some((load, Some(user), Some(sys))),
            (None, Some(a), Some(n)) if n > 0 => {
                let sum: f64 = a.iter().map(|x| x.cpu).sum();
                Some(((sum / n as f64).clamp(0.0, 100.0), None, None))
            }
            _ => None,
        };
        let cpu = match cpu {
            Some((load, user, system)) => {
                self.cpu_history.push(load);
                if self.cpu_history.len() > CPU_HISTORY {
                    self.cpu_history.remove(0);
                }
                Some(Cpu {
                    load,
                    user,
                    system,
                    load_avg: sysctl.get("vm.loadavg").and_then(|v| parse_loadavg(v)),
                    cores,
                    top: apps
                        .as_deref()
                        .map(|a| top_by_cpu(a, TOP_APPS))
                        .unwrap_or_default(),
                })
            }
            None => {
                problems.push("CPU load could not be read".into());
                None
            }
        };

        // Memory.
        let memory = match (memsize, run_text(env, "/usr/bin/vm_stat", &[])) {
            (Some(total), Ok(s)) => {
                match parse_vm_stat(&s).and_then(|(page, m)| memory_from_vm_stat(total, page, &m)) {
                    Some(mut m) => {
                        m.pressure = sysctl
                            .get("kern.memorystatus_vm_pressure_level")
                            .and_then(|v| parse_pressure_level(v));
                        if let Some((u, t)) =
                            sysctl.get("vm.swapusage").and_then(|v| parse_swapusage(v))
                        {
                            m.swap_used = Some(u);
                            m.swap_total = Some(t);
                        }
                        m.top = apps
                            .as_deref()
                            .map(|a| top_by_memory(a, TOP_APPS))
                            .unwrap_or_default();
                        Some(m)
                    }
                    None => {
                        problems.push("memory statistics were not understood".into());
                        None
                    }
                }
            }
            (None, _) => {
                problems.push("the amount of memory could not be read".into());
                None
            }
            (_, Err(e)) => {
                problems.push(e);
                None
            }
        };

        let storage = read_storage(env, &mut problems);

        // Battery.
        let battery = match run_text(env, "/usr/bin/pmset", &["-g", "batt"]) {
            Ok(s) => match parse_pmset(&s) {
                Some(None) => Some(BatteryReading::None),
                Some(Some(mut b)) => {
                    match run_text(env, "/usr/sbin/ioreg", &["-rn", "AppleSmartBattery"]) {
                        Ok(io) => {
                            let (c, h) = battery_health(&parse_ioreg_ints(&io));
                            b.cycles = c;
                            b.health = h;
                        }
                        Err(e) => problems.push(e),
                    }
                    Some(BatteryReading::Present(b))
                }
                None => {
                    problems.push("the battery status was not understood".into());
                    None
                }
            },
            Err(e) => {
                problems.push(e);
                None
            }
        };

        // Network.
        let network = match run_text(env, "/usr/sbin/netstat", &["-ib"]) {
            Ok(s) => match parse_netstat(&s) {
                Some(cur) => {
                    let rate = self
                        .prev_net
                        .and_then(|(t, prev)| net_rate(prev, cur, now - t));
                    self.prev_net = Some((now, cur));
                    rate
                }
                None => {
                    self.prev_net = None;
                    problems.push("network counters were not understood".into());
                    None
                }
            },
            Err(e) => {
                self.prev_net = None;
                problems.push(e);
                None
            }
        };

        Snapshot {
            taken: now as i64,
            cpu,
            uptime_secs,
            memory,
            storage,
            battery,
            network,
            problems,
        }
    }
}

fn read_storage(env: &Env, problems: &mut Vec<String>) -> Storage {
    let home = env.home().to_string_lossy().into_owned();
    let (total, free) = match run_text(env, "/bin/df", &["-Pk", &home]) {
        Ok(s) => match parse_df(&s) {
            Some((t, f)) => (Some(t), Some(f)),
            None => {
                problems.push("disk space was not understood".into());
                (None, None)
            }
        },
        Err(e) => {
            problems.push(e);
            (None, None)
        }
    };
    let (trash, trash_note) = trash_size(env);
    if let Some(n) = &trash_note {
        problems.push(n.clone());
    }
    Storage {
        total,
        free,
        trash,
        trash_note,
    }
}

/// Size of the user's Trash. macOS hides `~/.Trash` from apps without Full Disk Access.
pub fn trash_size(env: &Env) -> (Option<u64>, Option<String>) {
    let t = env.user_trash();
    let denied = "The Trash size needs Full Disk Access".to_string();
    if matches!(env.fault_for(&t), Some(Fault::PermissionDenied)) {
        return (None, Some(denied));
    }
    match std::fs::read_dir(&t) {
        Ok(_) => {
            let (size, unreadable) = tree_size(&t);
            if unreadable > 0 {
                (
                    Some(size),
                    Some(format!(
                        "{unreadable} items in the Trash could not be measured"
                    )),
                )
            } else {
                (Some(size), None)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Some(0), None),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => (None, Some(denied)),
        Err(e) => (None, Some(format!("The Trash could not be read: {e}"))),
    }
}

// ---------------------------------------------------------------------------------------------
// Alerts and texts.

pub fn alerts(s: &Snapshot) -> Vec<Alert> {
    let mut v = Vec::new();
    if let (Some(total), Some(free)) = (s.storage.total, s.storage.free)
        && total > 0
        && ((free as f64) < total as f64 * LOW_FREE_FRACTION || free < LOW_FREE_BYTES)
    {
        v.push(Alert {
            kind: AlertKind::LowSpace,
            text: format!(
                "Your startup disk is almost full: {} free.",
                human_size(free)
            ),
        });
    }
    if let Some(m) = &s.memory {
        let pressed = matches!(m.pressure, Some(Pressure::Warning | Pressure::Critical));
        if pressed || m.used_fraction() > HEAVY_MEMORY_FRACTION {
            v.push(Alert {
                kind: AlertKind::HeavyMemory,
                text: format!(
                    "Memory is heavily used: {} of {}{}.",
                    human_size(m.used),
                    human_size(m.total),
                    match m.pressure {
                        Some(p) if pressed => format!(", pressure {}", p.label().to_lowercase()),
                        _ => String::new(),
                    }
                ),
            });
        }
    }
    if let Some(t) = s.storage.trash
        && t > LARGE_TRASH_BYTES
    {
        v.push(Alert {
            kind: AlertKind::LargeTrash,
            text: format!("The Trash holds {}.", human_size(t)),
        });
    }
    v
}

/// "12%" with no decimals.
pub fn pct(v: f64) -> String {
    format!("{:.0}%", v)
}

/// "1.2 MB/s".
pub fn rate_text(bytes_per_sec: u64) -> String {
    format!("{}/s", human_size(bytes_per_sec))
}

/// "3 d 4 h", "5 h 12 min", "12 min".
pub fn uptime_text(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

pub const UNKNOWN: &str = "Unknown";

/// The menu bar title: "CPU 12% RAM 61%". Unknown values show as "?".
pub fn menu_bar_title(s: &Snapshot) -> String {
    let cpu = s.cpu.as_ref().map(|c| pct(c.load)).unwrap_or("?".into());
    let ram = s
        .memory
        .as_ref()
        .map(|m| pct(m.used_fraction() * 100.0))
        .unwrap_or("?".into());
    format!("CPU {cpu} RAM {ram}")
}

/// The menu bar menu, top to bottom. The last two entries open the app and quit.
pub fn menu_bar_entries(s: &Snapshot) -> Vec<String> {
    let mut v = Vec::new();
    v.push(match &s.memory {
        Some(m) => format!(
            "Memory: {} of {} used",
            human_size(m.used),
            human_size(m.total)
        ),
        None => format!("Memory: {UNKNOWN}"),
    });
    v.push(match s.storage.free {
        Some(f) => format!("Free space: {}", human_size(f)),
        None => format!("Free space: {UNKNOWN}"),
    });
    v.push(match s.storage.trash {
        Some(t) => format!("Trash: {}", human_size(t)),
        None => format!("Trash: {UNKNOWN}"),
    });
    match &s.battery {
        Some(BatteryReading::Present(b)) => {
            v.push(format!("Battery: {}%, {}", b.percent, b.state));
        }
        Some(BatteryReading::None) => {}
        None => v.push(format!("Battery: {UNKNOWN}")),
    }
    v.push(match s.network {
        Some(n) => format!(
            "Network: down {}, up {}",
            rate_text(n.down),
            rate_text(n.up)
        ),
        None => format!("Network: {UNKNOWN}"),
    });
    for a in alerts(s) {
        v.push(a.text);
    }
    v.push("Open CleanCraft".into());
    v.push("Quit CleanCraft".into());
    v
}

// ---------------------------------------------------------------------------------------------
// Free Up RAM.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurgeOutcome {
    Done,
    /// The user closed the password dialog.
    Cancelled,
    Failed(String),
}

/// True when an administrator command's output says the password dialog was cancelled.
pub fn admin_cancelled(out: &crate::sys::CmdOut) -> bool {
    !out.ok() && (out.stderr.contains("-128") || out.stderr.contains("User canceled"))
}

/// Run `purge` as administrator (macOS asks for the password) and log it. A cancelled
/// password dialog changes nothing and is not logged.
pub fn free_up_ram(env: &Env) -> PurgeOutcome {
    // Like every action: no log, no action.
    let writable = std::fs::create_dir_all(env.data_dir()).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(executor::journal_path(env))
            .map(|_| ())
    });
    if let Err(e) = writable {
        return PurgeOutcome::Failed(format!(
            "the action log could not be written, nothing was run: {e}"
        ));
    }
    let out = match env.sys.run_admin(PURGE_COMMAND) {
        Ok(o) => o,
        Err(e) => return PurgeOutcome::Failed(e.to_string()),
    };
    if admin_cancelled(&out) {
        return PurgeOutcome::Cancelled;
    }
    let now = chrono::Utc::now();
    let rec = ActionRecord {
        id: format!("monitor-purge-{}#0", now.format("%Y%m%d%H%M%S%3f")),
        plan_id: format!("monitor-purge-{}", now.format("%Y%m%d%H%M%S%3f")),
        feature: Some(FeatureId::Monitor),
        time: now.timestamp(),
        size: 0,
        kind: ActionKind::Command {
            command: PURGE_COMMAND.into(),
            ok: out.ok(),
            output: format!("{}{}", out.stdout, out.stderr).trim().to_string(),
        },
        undone: None,
    };
    let logged = executor::append_journal(env, &rec);
    if !out.ok() {
        return PurgeOutcome::Failed(if out.stderr.trim().is_empty() {
            format!("purge ended with status {}", out.status)
        } else {
            out.stderr.trim().to_string()
        });
    }
    match logged {
        Ok(()) => PurgeOutcome::Done,
        Err(e) => PurgeOutcome::Failed(format!(
            "memory was freed but the action log could not be written: {e}"
        )),
    }
}

/// Seconds since the epoch, with fractions, for [`Sampler::sample`].
pub fn now_secs() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
