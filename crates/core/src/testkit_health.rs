//! Fake Macs for Mac Health tests and renders: one calm Mac, and helpers that create each
//! condition a card is about. Command answers follow the samples of testkit_monitor (marked
//! SAMPLE there).

use std::path::PathBuf;

use crate::env::Env;
use crate::health::{self, ScanNote};
use crate::plan::FeatureId;
use crate::space_history::{self, History, Sample};
use crate::sys::{CmdOut, FakeSys};
use crate::testkit::FakeMac;
use crate::testkit_monitor as mon;

pub const GB: u64 = 1_000_000_000;

/// `df -Pk` output for a disk of `total` bytes with `free` bytes free.
pub fn df(total: u64, free: u64) -> String {
    let (t, f) = (total / 1024, free / 1024);
    format!(
        "Filesystem     1024-blocks      Used Available Capacity  Mounted on\n/dev/disk3s1     {t} {} {f}    50%    /System/Volumes/Data\n",
        t - f
    )
}

fn fake(env: &Env) -> &FakeSys {
    env.fake_sys().expect("fake env")
}

/// Set the startup disk's size and free space.
pub fn set_disk(env: &Env, total: u64, free: u64) {
    fake(env).answer("df", &df(total, free));
    fake(env).with(|s| s.free_space = Some(free));
}

/// Memory pressure level as sysctl reports it: 1 normal, 2 elevated, 4 critical.
pub fn set_pressure(env: &Env, level: u8) {
    let s = mon::SYSCTL.replace(
        "kern.memorystatus_vm_pressure_level: 1",
        &format!("kern.memorystatus_vm_pressure_level: {level}"),
    );
    fake(env).answer("sysctl", &s);
}

/// A MacBook battery with `max` of `design` mAh and `cycles` charge cycles.
pub fn set_battery(env: &Env, max: u32, design: u32, cycles: u32) {
    fake(env).answer("pmset", mon::PMSET_BATT);
    fake(env).answer(
        "ioreg",
        &format!(
            "    {{\n      \"AppleRawMaxCapacity\" = {max}\n      \"CycleCount\" = {cycles}\n      \"DesignCapacity\" = {design}\n    }}\n"
        ),
    );
}

/// A desktop Mac (no battery).
pub fn set_desktop(env: &Env) {
    fake(env).answer("pmset", mon::PMSET_DESKTOP);
}

/// An app last opened `days` ago (Spotlight's date).
pub fn app_opened(mac: &FakeMac, name: &str, days: i64, now: i64) -> PathBuf {
    let p = mac.app(name, &format!("com.example.{}", name.to_lowercase()), "1.0");
    let shown = mac.env.display(&p);
    let t = chrono::DateTime::from_timestamp(now - days * 86_400, 0).unwrap_or_default();
    fake(&mac.env).answer(
        &format!("mdls -name kMDItemLastUsedDate -raw {shown}"),
        &format!("{}\n", t.format("%Y-%m-%d %H:%M:%S +0000")),
    );
    p
}

/// A launch agent in ~/Library/LaunchAgents that is turned on, signed or not.
pub fn launch_agent(mac: &FakeMac, label: &str, signed: bool) {
    let prog = format!("/Users/Shared/{label}/run");
    let sig = if signed {
        CmdOut {
            status: 0,
            stdout: String::new(),
            stderr: "Authority=Developer ID Application: Example Corp (ABCDE12345)\nTeamIdentifier=ABCDE12345\n".into(),
        }
    } else {
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: format!("{prog}: code object is not signed at all\n"),
        }
    };
    crate::testkit_perf::add_launch_item(mac, "~/Library/LaunchAgents", label, &prog, Some(sig));
}

/// A scan note dated `days` before `now`, replacing the module's note whatever its date.
pub fn note(env: &Env, f: FeatureId, days: i64, bytes: u64, items: usize, now: i64) {
    let mut all = health::scan_notes(env);
    all.insert(
        f,
        ScanNote {
            at: now - days * 86_400,
            bytes,
            items,
            plan_id: format!("{}-test-{days}", f.as_str()),
        },
    );
    let _ = std::fs::create_dir_all(env.data_dir());
    std::fs::write(
        env.data_dir().join("scan-notes.json"),
        serde_json::to_vec(&all).expect("json"),
    )
    .expect("write notes");
}

/// Free space samples: (days before `now`, free bytes), written straight to the history file.
pub fn history(env: &Env, now: i64, samples: &[(f64, u64)], total: u64) {
    let mut h = History {
        version: 1,
        samples: samples
            .iter()
            .map(|(d, free)| Sample {
                at: now - (d * 86_400.0) as i64,
                free: *free,
                total: Some(total),
                trash: Some(0),
            })
            .collect(),
    };
    h.samples.sort_by_key(|s| s.at);
    let p = space_history::path(env);
    let _ = std::fs::create_dir_all(env.data_dir());
    std::fs::write(p, serde_json::to_vec(&h).expect("json")).expect("write history");
}

/// A Mac where nothing needs attention and no tip applies: big free disk, normal memory, a
/// healthy battery, recent scans with nothing found, a signed startup item, two apps used
/// this month.
pub fn calm_mac(now: i64) -> FakeMac {
    let mac = FakeMac::new();
    let f = fake(&mac.env);
    mon::answer_all(f);
    f.answer("sw_vers -productVersion", "15.1\n");
    f.answer("id -u", "501\n");
    f.answer("launchctl", "");
    set_disk(&mac.env, 500 * GB, 300 * GB);
    set_battery(&mac.env, 4300, 4382, 213);
    note(&mac.env, FeatureId::SystemJunk, 2, 200_000_000, 40, now);
    note(&mac.env, FeatureId::Malware, 3, 0, 0, now);
    note(&mac.env, FeatureId::Updater, 1, 0, 0, now);
    app_opened(&mac, "Editor", 3, now);
    app_opened(&mac, "Browser", 10, now);
    launch_agent(&mac, "com.example.sync", true);
    mac
}

/// Seconds since the epoch now (the app reads facts at the real time).
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Free space readings over 90 days ending at `end_free`: slowly falling, with a cleanup on
/// day 40 that gave back 25 GB.
pub fn sample_history(env: &Env, now: i64, total: u64, end_free: u64) {
    let mut s = Vec::new();
    let end = end_free as f64 / GB as f64;
    for i in 0..(90 * 4) {
        let d = 90.0 - i as f64 / 4.0;
        let base = end + d * 0.9 - if d > 40.0 { 25.0 } else { 0.0 };
        let wobble = ((i as f64) * 0.7).sin() * 1.5;
        s.push((d, ((base + wobble).max(5.0) * GB as f64) as u64));
    }
    history(env, now, &s, total);
}

/// A calm Mac at the real time, with 90 days of free space readings.
pub fn calm_mac_now() -> FakeMac {
    let n = now();
    let mac = calm_mac(n);
    sample_history(&mac.env, n, 500 * GB, 300 * GB);
    mac
}

/// A Mac with something for most cards: low space falling fast, a threat found, an unsigned
/// startup item, elevated memory pressure, a worn battery, junk found, unused apps, and an
/// old update check.
pub fn troubled_mac_now() -> FakeMac {
    let n = now();
    let mac = calm_mac(n);
    set_disk(&mac.env, 500 * GB, 45 * GB);
    sample_history(&mac.env, n, 500 * GB, 70 * GB);
    let mut h = space_history::load(&mac.env).history;
    // The last week: from 70 GB down to 45 GB.
    h.samples.retain(|s| s.at < n - 7 * 86_400);
    let mut week: Vec<(f64, u64)> = (0..28)
        .map(|i| {
            let d = 7.0 - i as f64 / 4.0;
            (d, ((45.0 + d * 3.5) * GB as f64) as u64)
        })
        .collect();
    week.push((0.0, 45 * GB));
    let older: Vec<(f64, u64)> = h
        .samples
        .iter()
        .map(|s| ((n - s.at) as f64 / 86_400.0, s.free))
        .collect();
    let all: Vec<(f64, u64)> = older.into_iter().chain(week).collect();
    history(&mac.env, n, &all, 500 * GB);
    note(&mac.env, FeatureId::Malware, 2, 0, 1, n);
    note(&mac.env, FeatureId::SystemJunk, 4, 2_300_000_000, 1_204, n);
    note(&mac.env, FeatureId::Updater, 20, 0, 0, n);
    launch_agent(&mac, "com.unknown.updater", false);
    set_pressure(&mac.env, 2);
    set_battery(&mac.env, 3300, 4382, 812);
    app_opened(&mac, "Atlas", 200, n);
    app_opened(&mac, "Beacon", 400, n);
    app_opened(&mac, "Comet", 260, n);
    mac
}

/// CleanCraft installed, Claude Desktop with one other server, Claude Code's command.
pub fn ai_mac() -> FakeMac {
    let mac = FakeMac::new();
    mac.sys_file(crate::mcp_setup::APP_BINARY, 100);
    let p = crate::mcp_setup::desktop_config(&mac.env);
    mac.write(
        &p,
        b"{\n  \"mcpServers\": {\n    \"files\": {\"command\": \"npx\", \"args\": [\"server-filesystem\"]}\n  }\n}\n",
    );
    mac.home_file(".local/bin/claude", 10);
    let f = fake(&mac.env);
    f.answer(
        "claude mcp list",
        "files: npx server-filesystem - Connected\n",
    );
    f.answer(
        &format!(
            "claude mcp add --scope user cleancraft -- {} serve",
            crate::mcp_setup::APP_BINARY
        ),
        "Added stdio MCP server cleancraft\n",
    );
    f.answer("sh", "");
    f.answer("/usr/bin/open", "");
    mac
}
