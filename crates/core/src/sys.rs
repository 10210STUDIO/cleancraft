//! Everything CleanCraft asks of the operating system beyond plain file access goes through
//! [`Sys`]: running system tools (`osascript`, `lsof`, `codesign`, `launchctl`...), the list of
//! open files and running apps, free disk space, opening URLs.
//!
//! [`RealSys`] runs the real tools. [`FakeSys`] answers from data the test set up and records
//! every command, so tests can check what would have run without running anything.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a system command may run before CleanCraft stops it ([`Sys::run`]).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long an administrator command may take, the password dialog included.
pub const ADMIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The error a command that ran past its time limit gives: kind `TimedOut`, and words the
/// user understands.
pub fn timed_out(program: &str, limit: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "{program} did not finish within {} and was stopped",
            limit_text(limit)
        ),
    )
}

fn limit_text(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 120 && s.is_multiple_of(60) {
        format!("{} minutes", s / 60)
    } else if s == 1 {
        "1 second".into()
    } else if s == 0 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{s} seconds")
    }
}

/// Quote `s` for `/bin/sh` so it is one word, taken literally: wrapped in single quotes, each
/// single quote written as `'\''`. Spaces, `$`, backticks, backslashes and newlines need
/// nothing else inside single quotes.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The administrator shell command that moves each `(from, to)` without overwriting anything:
/// one `/bin/mv -n -- 'from' 'to'` line per pair. This is the only shape of administrator move
/// CleanCraft runs; [`parse_admin_moves`] reads it back.
pub fn admin_move_script(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(a, b)| format!("{MV_PREFIX}{} {}", shell_quote(a), shell_quote(b)))
        .collect::<Vec<_>>()
        .join("\n")
}

const MV_PREFIX: &str = "/bin/mv -n -- ";

/// The `(from, to)` pairs of a script made by [`admin_move_script`]; `None` for anything else.
pub fn parse_admin_moves(script: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = script;
    loop {
        rest = rest.strip_prefix(MV_PREFIX)?;
        let (a, r) = parse_quoted(rest)?;
        let r = r.strip_prefix(' ')?;
        let (b, r) = parse_quoted(r)?;
        out.push((a, b));
        if r.is_empty() {
            return Some(out);
        }
        rest = r.strip_prefix('\n')?;
    }
}

/// One word written by [`shell_quote`]: the text and what follows it.
fn parse_quoted(s: &str) -> Option<(String, &str)> {
    let mut rest = s.strip_prefix('\'')?;
    let mut word = String::new();
    loop {
        let end = rest.find('\'')?;
        word.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix("\\''") {
            Some(r) => {
                word.push('\'');
                rest = r;
            }
            None => return Some((word, rest)),
        }
    }
}

/// True when an administrator command's output says the password dialog was cancelled
/// (`osascript` error -128).
pub fn admin_cancelled(out: &CmdOut) -> bool {
    !out.ok()
        && (out.stderr.contains("(-128)")
            || out.stderr.contains("User canceled")
            || out.stderr.contains("User cancelled"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CmdOut {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOut {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningApp {
    pub pid: u32,
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: Option<PathBuf>,
}

pub trait Sys: Send + Sync {
    /// Run a program and wait for it, at most [`DEFAULT_TIMEOUT`]; a program still running
    /// then is stopped and the call fails with `io::ErrorKind::TimedOut`.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut>;
    /// [`Sys::run`] with its own time limit, for work known to take long (tune-up tasks).
    fn run_timeout(&self, program: &str, args: &[&str], limit: Duration) -> io::Result<CmdOut> {
        let _ = limit;
        self.run(program, args)
    }
    /// Run a shell command as administrator. On macOS this is
    /// `osascript -e 'do shell script "..." with administrator privileges'`, which shows the
    /// system password dialog. The user can cancel it.
    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut>;
    /// Every file currently open by any process this user can see.
    fn open_files(&self) -> io::Result<HashSet<PathBuf>>;
    /// [`Sys::open_files`] with the name of a process holding each file open ("Safari"),
    /// empty when unknown, so a skipped item can say which app to quit (REVIEW-U U9).
    fn open_files_by(&self) -> io::Result<Vec<(PathBuf, String)>> {
        Ok(self
            .open_files()?
            .into_iter()
            .map(|p| (p, String::new()))
            .collect())
    }
    /// Apps currently running.
    fn running_apps(&self) -> Vec<RunningApp>;
    /// Free bytes on the volume holding `path`.
    fn free_space(&self, path: &Path) -> io::Result<u64>;
    /// Open a URL or file with the default handler (System Settings panes, Finder, web).
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// True on macOS.
    fn is_macos(&self) -> bool;
    /// Start a program and return at once, without waiting for it or reading its output
    /// (short fire-and-forget helpers such as `afplay` for interface sounds). The default
    /// refuses, so an implementation has to opt in.
    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let _ = args;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{program} cannot be started without waiting here"),
        ))
    }
}

/// The real operating system.
#[derive(Debug, Default)]
pub struct RealSys;

/// Run `cmd` and wait at most `limit`. Output is read on two threads so a chatty program
/// cannot block on a full pipe. On time out the child CleanCraft started is killed (and
/// reaped) and the error is [`timed_out`].
pub fn run_with_limit(mut cmd: Command, program: &str, limit: Duration) -> io::Result<CmdOut> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        if let Some(mut p) = pipe {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = p.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        rx
    };
    let out_rx = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err_rx = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(timed_out(program, limit));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // A program can leave a helper running that keeps the pipes open: wait for the output only
    // until the deadline (and at least a moment).
    let left = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(200));
    let stdout = out_rx.recv_timeout(left).unwrap_or_default();
    let stderr = err_rx
        .recv_timeout(
            Duration::from_millis(200).max(deadline.saturating_duration_since(Instant::now())),
        )
        .unwrap_or_default();
    Ok(CmdOut {
        status: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

impl Sys for RealSys {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut> {
        self.run_timeout(program, args, DEFAULT_TIMEOUT)
    }

    fn run_timeout(&self, program: &str, args: &[&str], limit: Duration) -> io::Result<CmdOut> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        run_with_limit(cmd, program, limit)
    }

    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut> {
        if !self.is_macos() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "administrator commands need macOS",
            ));
        }
        // An AppleScript string literal: backslash, quote and line breaks escaped.
        let escaped = shell_command
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let script = format!("do shell script \"{escaped}\" with administrator privileges");
        self.run_timeout("/usr/bin/osascript", &["-e", &script], ADMIN_TIMEOUT)
    }

    fn open_files(&self) -> io::Result<HashSet<PathBuf>> {
        // -F n: one "n<path>" line per open file; -w: no warnings. lsof exits 1 when some
        // processes cannot be inspected, which is normal without root, so the status is ignored.
        let out = self
            .run("/usr/sbin/lsof", &["-F", "n", "-w"])
            .or_else(|_| self.run("lsof", &["-F", "n", "-w"]))?;
        Ok(parse_lsof(&out.stdout))
    }

    fn open_files_by(&self) -> io::Result<Vec<(PathBuf, String)>> {
        // +c 0: whole command names; -F pcn: "p<pid>", "c<command>", "n<path>" lines.
        let args = ["+c", "0", "-F", "pcn", "-w"];
        let out = self
            .run("/usr/sbin/lsof", &args)
            .or_else(|_| self.run("lsof", &args))?;
        Ok(parse_lsof_by(&out.stdout))
    }

    fn running_apps(&self) -> Vec<RunningApp> {
        let Ok(out) = self.run("/bin/ps", &["-axo", "pid=,comm="]) else {
            return Vec::new();
        };
        parse_ps(&out.stdout)
    }

    fn free_space(&self, path: &Path) -> io::Result<u64> {
        let p = path.to_string_lossy();
        let out = self.run("/bin/df", &["-Pk", &p])?;
        parse_df_free(&out.stdout)
            .ok_or_else(|| io::Error::other(format!("could not read free space: {}", out.stderr)))
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        let prog = if self.is_macos() {
            "/usr/bin/open"
        } else {
            "xdg-open"
        };
        let out = self.run(prog, &[url])?;
        if out.ok() {
            Ok(())
        } else {
            Err(io::Error::other(out.stderr))
        }
    }

    fn is_macos(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        // Reap it when it ends, so no zombie process is left behind.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

///
/// `lsof -F n` prints the whole path, spaces and parentheses included ("Invoice (1).pdf").
/// It may append one note in parentheses after the path, such as " (deleted)". A trailing
/// " (...)" cannot be told apart from a name that ends that way, so both readings go in the
/// set: an extra entry only makes CleanCraft more careful (a file looks open), never less.
pub fn parse_lsof(stdout: &str) -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    for p in stdout
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter(|p| p.starts_with('/'))
    {
        set.insert(PathBuf::from(p));
        if p.ends_with(')')
            && let Some(i) = p.rfind(" (")
            && i > 0
        {
            set.insert(PathBuf::from(&p[..i]));
        }
    }
    set
}

/// `lsof +c 0 -F pcn` output: each open path with the command holding it, both readings of
/// a trailing " (...)" as in [`parse_lsof`]. A helper process is named after its app
/// ("Google Chrome Helper (Renderer)" holds it for "Google Chrome").
pub fn parse_lsof_by(stdout: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut cmd = String::new();
    for l in stdout.lines() {
        if let Some(c) = l.strip_prefix('c') {
            cmd = app_of_command(c);
        } else if l.starts_with('p') {
            cmd.clear();
        } else if let Some(p) = l.strip_prefix('n').filter(|p| p.starts_with('/')) {
            out.push((PathBuf::from(p), cmd.clone()));
            if p.ends_with(')')
                && let Some(i) = p.rfind(" (")
                && i > 0
            {
                out.push((PathBuf::from(&p[..i]), cmd.clone()));
            }
        }
    }
    out
}

/// The app a process belongs to, by its command name: "Safari" stays "Safari", "Google
/// Chrome Helper (GPU)" and "Slack Helper" give "Google Chrome" and "Slack".
pub fn app_of_command(c: &str) -> String {
    let c = c.trim();
    match c.find(" Helper") {
        Some(i) if i > 0 => c[..i].to_string(),
        _ => c.to_string(),
    }
}

/// `ps -axo pid=,comm=` lines: "  123 /Applications/Safari.app/Contents/MacOS/Safari".
pub fn parse_ps(stdout: &str) -> Vec<RunningApp> {
    stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (pid, comm) = l.split_once(char::is_whitespace)?;
            let pid: u32 = pid.parse().ok()?;
            let comm = comm.trim();
            let path = PathBuf::from(comm);
            let app_path = app_bundle_of(&path);
            let name = app_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .or_else(|| path.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some(RunningApp {
                pid,
                name,
                bundle_id: None,
                path: app_path.or(Some(path)),
            })
        })
        .collect()
}

/// `/Applications/X.app/Contents/MacOS/X` -> `/Applications/X.app`.
pub fn app_bundle_of(path: &Path) -> Option<PathBuf> {
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if cur.extension().is_some_and(|e| e == "app") {
            return Some(cur.to_path_buf());
        }
        cur = parent;
    }
    None
}

/// `df -Pk` second line, fourth column is free 1024-byte blocks.
pub fn parse_df_free(stdout: &str) -> Option<u64> {
    let line = stdout.lines().nth(1)?;
    let avail: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail * 1024)
}

/// A scripted operating system for tests. Nothing runs; every call is recorded.
#[derive(Debug, Default)]
pub struct FakeSys {
    pub state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
pub struct FakeState {
    /// Every command asked for, as "program arg arg".
    pub calls: Vec<String>,
    /// Every administrator command asked for.
    pub admin_calls: Vec<String>,
    /// Answers by exact "program arg arg" or by program alone.
    pub answers: BTreeMap<String, CmdOut>,
    pub open_files: HashSet<PathBuf>,
    /// Who holds an open file, for the ones set with [`FakeSys::set_open_by`].
    pub open_by: BTreeMap<PathBuf, String>,
    pub running: Vec<RunningApp>,
    pub free_space: Option<u64>,
    pub opened_urls: Vec<String>,
    /// When true, the administrator password dialog is "cancelled".
    pub admin_cancelled: bool,
    pub macos: bool,
    /// The fake Mac's folder. Administrator moves must stay inside it.
    pub root: Option<PathBuf>,
    /// Commands (same keys as `answers`) that run past their time limit.
    pub timeouts: BTreeSet<String>,
    /// Time limits asked for with `run_timeout`, as ("program arg arg", limit).
    pub limits: Vec<(String, Duration)>,
    /// Sources an administrator `mv` fails on ("Operation not permitted").
    pub admin_move_fails: HashSet<PathBuf>,
    /// Files a command writes when it runs with a successful answer (same keys as
    /// `answers`): image tools such as `sips` and `qlmanage` that write their output to disk.
    pub creates: BTreeMap<String, Vec<(PathBuf, Vec<u8>)>>,
    /// Programs started without waiting ([`Sys::spawn`]), as "program arg arg". Kept apart
    /// from `calls` so fire-and-forget helpers do not disturb tests that count commands.
    pub spawned: Vec<String>,
    /// What a command changes in the fake Mac when it runs with a successful answer (same keys
    /// as `answers`; administrator commands as "admin:<command>"): processes that stop or
    /// start, answers that change (`launchctl print` after a `bootout`).
    pub effects: BTreeMap<String, FakeEffect>,
}

/// The side effects of one scripted command ([`FakeState::effects`]).
#[derive(Clone, Debug, Default)]
pub struct FakeEffect {
    /// Pids that stop running.
    pub stop: Vec<u32>,
    /// Processes that start (a helper that relaunches itself).
    pub start: Vec<RunningApp>,
    /// Answers replaced: (key, stdout).
    pub answers: Vec<(String, String)>,
}

impl FakeState {
    fn apply_effect(&mut self, key: &str) {
        let Some(e) = self.effects.get(key).cloned() else {
            return;
        };
        self.running.retain(|r| !e.stop.contains(&r.pid));
        self.running.extend(e.start);
        for (k, out) in e.answers {
            self.answers.insert(
                k,
                CmdOut {
                    status: 0,
                    stdout: out,
                    stderr: String::new(),
                },
            );
        }
    }
}

impl FakeSys {
    pub fn new() -> FakeSys {
        FakeSys {
            state: Mutex::new(FakeState {
                macos: true,
                free_space: Some(100_000_000_000),
                ..Default::default()
            }),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        let mut g = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut g)
    }

    pub fn answer(&self, key: &str, stdout: &str) {
        self.with(|s| {
            s.answers.insert(
                key.to_string(),
                CmdOut {
                    status: 0,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                },
            )
        });
    }

    pub fn answer_full(&self, key: &str, out: CmdOut) {
        self.with(|s| s.answers.insert(key.to_string(), out));
    }

    /// Answer `key` with `stdout` (status 0), and write `bytes` to `file` (inside the fake
    /// root) each time that command runs, like a tool writing its output file.
    pub fn answer_creating(&self, key: &str, stdout: &str, file: &Path, bytes: &[u8]) {
        self.answer(key, stdout);
        self.with(|s| {
            s.creates
                .entry(key.to_string())
                .or_default()
                .push((file.to_path_buf(), bytes.to_vec()))
        });
    }

    pub fn calls(&self) -> Vec<String> {
        self.with(|s| s.calls.clone())
    }

    /// Every program started with [`Sys::spawn`], as "program arg arg".
    pub fn spawned(&self) -> Vec<String> {
        self.with(|s| s.spawned.clone())
    }

    pub fn admin_calls(&self) -> Vec<String> {
        self.with(|s| s.admin_calls.clone())
    }

    /// When `key` runs with a successful answer, apply `effect` (see [`FakeState::effects`]).
    pub fn on_run(&self, key: &str, effect: FakeEffect) {
        self.with(|s| s.effects.insert(key.to_string(), effect));
    }

    pub fn set_open(&self, path: &Path) {
        self.with(|s| s.open_files.insert(path.to_path_buf()));
    }

    /// `path` is open in `app` (lsof's command name).
    pub fn set_open_by(&self, path: &Path, app: &str) {
        self.with(|s| {
            s.open_files.insert(path.to_path_buf());
            s.open_by.insert(path.to_path_buf(), app.to_string())
        });
    }

    /// Make a command (exact line, short line or program) run past its time limit.
    pub fn time_out(&self, key: &str) {
        self.with(|s| s.timeouts.insert(key.to_string()));
    }

    /// Make the administrator `mv` of `src` fail.
    pub fn fail_admin_move(&self, src: &Path) {
        self.with(|s| s.admin_move_fails.insert(src.to_path_buf()));
    }

    /// Carry out an administrator move script inside the fake root, like `/bin/mv -n` would.
    fn admin_moves(s: &FakeState, pairs: &[(String, String)]) -> CmdOut {
        let root = s
            .root
            .clone()
            .expect("FakeSys: administrator moves need the fake root");
        let mut out = CmdOut::default();
        for (a, b) in pairs {
            let (a, b) = (Path::new(a), Path::new(b));
            for p in [a, b] {
                assert!(
                    p.is_absolute() && p.starts_with(&root) && !crate::env::has_parent_component(p),
                    "FakeSys: administrator move outside the fake root: {}",
                    p.display()
                );
            }
            if s.admin_move_fails.contains(a) {
                out.status = 1;
                out.stderr.push_str(&format!(
                    "mv: rename {}: Operation not permitted\n",
                    a.display()
                ));
                continue;
            }
            // -n: never replace something already there.
            if std::fs::symlink_metadata(b).is_ok() {
                continue;
            }
            if let Err(e) = std::fs::rename(a, b) {
                out.status = 1;
                out.stderr
                    .push_str(&format!("mv: rename {}: {e}\n", a.display()));
            }
        }
        out
    }
}

impl FakeSys {
    /// The recorded, scripted answer to a command, or a time out when the test asked for one.
    fn run_limited(&self, program: &str, args: &[&str], limit: Duration) -> io::Result<CmdOut> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.with(|s| {
            s.calls.push(line.clone());
            let short = Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let short_line = std::iter::once(short.as_str())
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            if [&line, &short_line, &program.to_string(), &short]
                .iter()
                .any(|k| s.timeouts.contains(*k))
            {
                return Err(timed_out(program, limit));
            }
            let keys = [line.as_str(), short_line.as_str(), program, short.as_str()];
            let Some(key) = keys.iter().find(|k| s.answers.contains_key(**k)) else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("fake: no answer for `{line}`"),
                ));
            };
            let key = key.to_string();
            let out = s.answers[&key].clone();
            if out.ok()
                && let Some(k) = keys.iter().find(|k| s.effects.contains_key(**k))
            {
                let k = k.to_string();
                s.apply_effect(&k);
            }
            if out.ok()
                && let Some(files) = s.creates.get(&key)
            {
                for (f, bytes) in files {
                    if let Some(root) = &s.root {
                        assert!(
                            f.starts_with(root) && !crate::env::has_parent_component(f),
                            "FakeSys: a command wrote outside the fake root: {}",
                            f.display()
                        );
                    }
                    if let Some(dir) = f.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(f, bytes);
                }
            }
            Ok(out)
        })
    }
}

impl Sys for FakeSys {
    fn run_timeout(&self, program: &str, args: &[&str], limit: Duration) -> io::Result<CmdOut> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.with(|s| s.limits.push((line, limit)));
        self.run_limited(program, args, limit)
    }

    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut> {
        self.run_limited(program, args, DEFAULT_TIMEOUT)
    }

    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut> {
        self.with(|s| {
            s.admin_calls.push(shell_command.to_string());
            if s.admin_cancelled {
                Ok(CmdOut {
                    status: 1,
                    stdout: String::new(),
                    stderr: "User canceled. (-128)".into(),
                })
            } else if shell_command.starts_with(MV_PREFIX) {
                // Administrator moves: only the exact script admin_move_script writes.
                match parse_admin_moves(shell_command) {
                    Some(pairs) => Ok(FakeSys::admin_moves(s, &pairs)),
                    None => Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("fake: refused administrator command `{shell_command}`"),
                    )),
                }
            } else {
                let key = format!("admin:{shell_command}");
                let out = s.answers.get(&key).cloned().unwrap_or_default();
                if out.ok() {
                    s.apply_effect(&key);
                }
                Ok(out)
            }
        })
    }

    fn open_files(&self) -> io::Result<HashSet<PathBuf>> {
        Ok(self.with(|s| s.open_files.clone()))
    }

    fn open_files_by(&self) -> io::Result<Vec<(PathBuf, String)>> {
        Ok(self.with(|s| {
            s.open_files
                .iter()
                .map(|p| (p.clone(), s.open_by.get(p).cloned().unwrap_or_default()))
                .collect()
        }))
    }

    fn running_apps(&self) -> Vec<RunningApp> {
        self.with(|s| s.running.clone())
    }

    fn free_space(&self, _path: &Path) -> io::Result<u64> {
        self.with(|s| s.free_space)
            .ok_or_else(|| io::Error::other("fake: free space unknown"))
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        self.with(|s| s.opened_urls.push(url.to_string()));
        Ok(())
    }

    fn is_macos(&self) -> bool {
        self.with(|s| s.macos)
    }

    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.with(|s| s.spawned.push(line));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_lines_name_the_holder() {
        let v = parse_lsof_by(
            "p12\ncSafari\nn/Users/a/x.db\nnlocalhost:443\np40\ncGoogle Chrome Helper (Renderer)\nn/Users/a/c (deleted)\n",
        );
        assert!(v.contains(&(PathBuf::from("/Users/a/x.db"), "Safari".into())));
        assert!(v.contains(&(PathBuf::from("/Users/a/c (deleted)"), "Google Chrome".into())));
        assert!(v.contains(&(PathBuf::from("/Users/a/c"), "Google Chrome".into())));
        assert_eq!(v.len(), 3);
        assert_eq!(app_of_command("Slack Helper"), "Slack");
        assert_eq!(app_of_command("Helper"), "Helper");
    }

    #[test]
    fn lsof_lines() {
        let s = parse_lsof("p12\nn/Users/a/x.db\nn/dev/null\nnlocalhost:443\nn/tmp/a (deleted)\n");
        assert!(s.contains(Path::new("/Users/a/x.db")));
        assert!(s.contains(Path::new("/tmp/a")));
        let s = parse_lsof("p1\nn/Users/a/Invoice (1).pdf\nn/Users/a/My (old) notes.txt\n");
        assert!(s.contains(Path::new("/Users/a/Invoice (1).pdf")));
        assert!(s.contains(Path::new("/Users/a/My (old) notes.txt")));
        assert!(!s.iter().any(|p| p.to_string_lossy().contains("localhost")));
    }

    #[test]
    fn ps_lines() {
        let apps =
            parse_ps("  1 /sbin/launchd\n 512 /Applications/Safari.app/Contents/MacOS/Safari\n");
        assert_eq!(apps[1].name, "Safari");
        assert_eq!(
            apps[1].path.as_deref(),
            Some(Path::new("/Applications/Safari.app"))
        );
    }

    #[test]
    fn df_free() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 50 40 50% /\n";
        assert_eq!(parse_df_free(out), Some(40 * 1024));
    }
    #[test]
    fn shell_quote_keeps_every_character_literal() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_quote("`id`"), "'`id`'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
        assert_eq!(shell_quote(r#"back\slash "q""#), r#"'back\slash "q"'"#);
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn admin_move_script_round_trips_awkward_names() {
        let names = [
            "/Library/LaunchAgents/com.x.agent.plist",
            "/Applications/My App.app",
            "/Library/Application Support/it's here",
            "/Library/Caches/$(rm -rf ~)",
            "/Library/Caches/`touch /tmp/x`",
            "/Library/Caches/line\nbreak",
            "/Library/Caches/'''",
            r#"/Library/Caches/back\slash "quoted""#,
        ];
        let pairs: Vec<(String, String)> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), format!("/q/{i} {n}")))
            .collect();
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let script = admin_move_script(&refs);
        assert_eq!(script.matches("/bin/mv -n -- ").count(), names.len());
        assert_eq!(parse_admin_moves(&script), Some(pairs));
        // Anything else is refused.
        assert_eq!(parse_admin_moves("/bin/rm -rf /"), None);
        assert_eq!(parse_admin_moves("/bin/mv -n -- 'a' 'b'; rm -rf /"), None);
        assert_eq!(parse_admin_moves("/bin/mv -n -- 'a' 'b' 'c'"), None);
        assert_eq!(parse_admin_moves("/bin/mv -n -- 'a'"), None);
        assert_eq!(parse_admin_moves("/bin/mv -n -- a b"), None);
        assert_eq!(parse_admin_moves(""), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_shell_reads_a_quoted_word_back_unchanged() {
        // Harmless: /bin/sh only prints the word back (no macOS tool, no file touched).
        for w in [
            "it's",
            "$HOME `id`",
            "a\nb",
            r#"back\slash "q""#,
            "*",
            "'''",
        ] {
            let script = format!("printf %s {}", shell_quote(w));
            let out = RealSys.run("/bin/sh", &["-c", &script]).unwrap();
            assert_eq!(out.stdout, w);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_command_past_its_limit_is_stopped() {
        // Harmless: `sleep`, started and killed by this test.
        let start = Instant::now();
        let err = RealSys
            .run_timeout("/bin/sleep", &["30"], Duration::from_millis(300))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("did not finish within"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(10));
        // A quick command still answers, with its output and status.
        let out = RealSys
            .run_timeout(
                "/bin/sh",
                &["-c", "echo hi; echo err >&2; exit 3"],
                Duration::from_secs(20),
            )
            .unwrap();
        assert_eq!(
            (out.status, out.stdout.as_str(), out.stderr.as_str()),
            (3, "hi\n", "err\n")
        );
    }

    #[test]
    fn fake_spawn_is_recorded_apart_from_calls() {
        let fake = FakeSys::new();
        fake.spawn("/usr/bin/afplay", &["-v", "0.50", "/x/a.wav"])
            .expect("spawn");
        assert_eq!(fake.spawned(), vec!["/usr/bin/afplay -v 0.50 /x/a.wav"]);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn fake_time_out_and_admin_moves() {
        let d = tempfile::tempdir().unwrap();
        let f = FakeSys::new();
        f.with(|s| s.root = Some(d.path().to_path_buf()));
        f.answer("tmutil", "ok");
        f.time_out("tmutil thinlocalsnapshots /");
        let e = f
            .run("/usr/bin/tmutil", &["thinlocalsnapshots", "/"])
            .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        assert!(
            f.run("/usr/bin/tmutil", &["listlocalsnapshots", "/"])
                .is_ok()
        );
        let _ = f.run_timeout("/usr/bin/mdutil", &["-E", "/"], Duration::from_secs(600));
        assert!(f.with(|s| {
            s.limits
                .iter()
                .any(|(l, t)| l == "/usr/bin/mdutil -E /" && t.as_secs() == 600)
        }));

        let a = d.path().join("it's a");
        std::fs::write(&a, "x").unwrap();
        let b = d.path().join("b");
        let script = admin_move_script(&[(a.to_str().unwrap(), b.to_str().unwrap())]);
        assert!(f.run_admin(&script).unwrap().ok());
        assert!(!a.exists() && b.exists());
        // -n: an existing destination is left alone.
        std::fs::write(&a, "y").unwrap();
        f.run_admin(&script).unwrap();
        assert!(a.exists());
        assert!(f.run_admin("/bin/mv -n -- 'x' 'y'; reboot").is_err());
        // A cancelled password dialog moves nothing.
        f.with(|s| s.admin_cancelled = true);
        let out = f.run_admin(&script).unwrap();
        assert!(admin_cancelled(&out));
    }

    #[test]
    #[should_panic(expected = "outside the fake root")]
    fn fake_admin_move_outside_the_root_panics() {
        let d = tempfile::tempdir().unwrap();
        let f = FakeSys::new();
        f.with(|s| s.root = Some(d.path().to_path_buf()));
        let _ = f.run_admin(&admin_move_script(&[("/etc/hosts", "/tmp/hosts")]));
    }
}
