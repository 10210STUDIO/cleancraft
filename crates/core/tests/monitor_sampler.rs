//! The Live Monitor sampler on a fake Mac: values come from the scripted tools, unknown values
//! stay unknown, speeds come from two samples, alerts fire at their thresholds, and Free Up
//! RAM runs `purge` as administrator, logs it, and handles a cancelled password dialog.

use std::fs;

use cleancraft_core::executor::history;
use cleancraft_core::features::trash;
use cleancraft_core::monitor::*;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{ActionKind, Fault, FeatureId};

const NOW: f64 = 1_760_100_000.0;

fn mac() -> FakeMac {
    let m = FakeMac::new();
    answer_all(m.env.fake_sys().unwrap());
    m
}

#[test]
fn full_snapshot_from_tools() {
    let m = mac();
    m.home_file(".Trash/old.zip", 3_000_000);
    m.home_file(".Trash/folder/a.bin", 1_000_000);
    let mut s = Sampler::new();
    let snap = s.sample(&m.env, NOW);
    assert!(snap.problems.is_empty(), "{:?}", snap.problems);

    let cpu = snap.cpu.as_ref().unwrap();
    assert!((cpu.load - 17.4).abs() < 1e-6);
    assert_eq!(cpu.cores, Some(8));
    assert_eq!(cpu.load_avg, Some([1.85, 2.10, 2.30]));
    assert_eq!(cpu.top[0].name, "Google Chrome Helper");
    assert_eq!(cpu.top.len(), TOP_APPS);
    assert_eq!(snap.uptime_secs, Some(100_000));

    let mem = snap.memory.as_ref().unwrap();
    assert_eq!(mem.total, 17_179_869_184);
    assert_eq!(mem.used, (330_000 + 130_000 + 50_000) * 16384);
    assert_eq!(mem.pressure, Some(Pressure::Normal));
    assert_eq!(mem.swap_total, Some(2048 * 1048576));
    assert_eq!(mem.top[0].name, "Xcode");

    assert_eq!(snap.storage.total, Some(488_245_288 * 1024));
    assert_eq!(snap.storage.free, Some(80_000_000 * 1024));
    assert_eq!(
        snap.storage.trash,
        Some(trash::contents_size(&m.env.user_trash()).0)
    );
    assert!(snap.storage.trash.unwrap() >= 4_000_000);

    match snap.battery.as_ref().unwrap() {
        BatteryReading::Present(b) => {
            assert_eq!(b.percent, 64);
            assert_eq!(b.cycles, Some(213));
            assert_eq!(b.health, Some(95));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(snap.network, None, "a speed needs two samples");

    // The exact read-only commands.
    let calls = m.env.fake_sys().unwrap().calls();
    let home = m.env.home().to_string_lossy().into_owned();
    for c in [
        format!("/usr/sbin/sysctl {}", SYSCTL_KEYS.join(" ")),
        "/usr/bin/top -l 2 -n 0 -s 1".into(),
        "/bin/ps -axo pid=,%cpu=,rss=,comm=".into(),
        "/usr/bin/vm_stat".into(),
        format!("/bin/df -Pk {home}"),
        "/usr/bin/pmset -g batt".into(),
        "/usr/sbin/ioreg -rn AppleSmartBattery".into(),
        "/usr/sbin/netstat -ib".into(),
    ] {
        assert!(calls.contains(&c), "missing `{c}` in {calls:?}");
    }
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
}

#[test]
fn network_speed_from_two_samples() {
    let m = mac();
    let mut s = Sampler::new();
    s.sample(&m.env, NOW);
    m.env.fake_sys().unwrap().answer("netstat", NETSTAT_2);
    let snap = s.sample(&m.env, NOW + 2.0);
    let n = snap.network.unwrap();
    // en0 + utun0: rx +4,000,000, tx +200,000 over 2 s.
    assert_eq!((n.down, n.up), (2_000_000, 100_000));
    assert_eq!(s.cpu_history.len(), 2);
}

#[test]
fn every_tool_missing_means_unknown_not_zero() {
    let m = FakeMac::new(); // FakeSys answers nothing
    let mut s = Sampler::new();
    let snap = s.sample(&m.env, NOW);
    assert!(snap.cpu.is_none());
    assert!(snap.memory.is_none());
    assert!(snap.uptime_secs.is_none());
    assert!(snap.storage.total.is_none() && snap.storage.free.is_none());
    assert!(snap.battery.is_none());
    assert!(snap.network.is_none());
    assert!(snap.problems.len() >= 6, "{:?}", snap.problems);
    // The Trash itself is a folder, still measured.
    assert_eq!(
        snap.storage.trash,
        Some(trash::contents_size(&m.env.user_trash()).0)
    );
    assert_eq!(menu_bar_title(&snap), "CPU ? RAM ?");
    assert!(alerts(&snap).is_empty());
}

#[test]
fn garbage_output_is_unknown() {
    let m = mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("vm_stat", "something else\n");
    f.answer("pmset", "nope\n");
    f.answer("netstat", "nothing\n");
    break_tool(f, "top");
    let snap = Sampler::new().sample(&m.env, NOW);
    assert!(snap.memory.is_none());
    assert!(snap.battery.is_none());
    assert!(snap.network.is_none());
    // top broken: load falls back to the ps sum over cores: (0.3+12.5+48+0+2+1.5)/8.
    let cpu = snap.cpu.unwrap();
    assert!((cpu.load - 64.3 / 8.0).abs() < 1e-9, "{}", cpu.load);
    assert_eq!(cpu.user, None);
    assert!(
        snap.problems
            .iter()
            .any(|p| p.contains("memory statistics"))
    );
}

#[test]
fn desktop_mac_has_no_battery() {
    let m = mac();
    m.env.fake_sys().unwrap().answer("pmset", PMSET_DESKTOP);
    let snap = Sampler::new().sample(&m.env, NOW);
    assert_eq!(snap.battery, Some(BatteryReading::None));
    assert!(
        !m.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("ioreg"))
    );
}

#[test]
fn trash_without_full_disk_access() {
    let m = mac();
    m.env
        .inject_fault(&m.env.user_trash(), Fault::PermissionDenied);
    let snap = Sampler::new().sample(&m.env, NOW);
    assert_eq!(snap.storage.trash, None);
    assert!(
        snap.storage
            .trash_note
            .as_deref()
            .unwrap()
            .contains("Full Disk Access")
    );
}

#[test]
fn alerts_at_thresholds() {
    let m = mac();
    let f = m.env.fake_sys().unwrap();
    let mut snap = Sampler::new().sample(&m.env, NOW);
    assert!(alerts(&snap).is_empty(), "{:?}", alerts(&snap));

    // 9 GB free of 500 GB: under 10 GB.
    f.answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488281250 479492188 8789062 99% /\n",
    );
    snap = Sampler::new().sample(&m.env, NOW);
    assert_eq!(kinds(&snap), vec![AlertKind::LowSpace]);

    // Pressure warning raises heavy memory even below 90%.
    answer_all(f);
    f.answer(
        "sysctl",
        &SYSCTL.replace("pressure_level: 1", "pressure_level: 2"),
    );
    snap = Sampler::new().sample(&m.env, NOW);
    assert_eq!(kinds(&snap), vec![AlertKind::HeavyMemory]);

    // Over 90% used.
    answer_all(f);
    let mut mem = snap.memory.clone().unwrap();
    mem.pressure = Some(Pressure::Normal);
    mem.used = (mem.total as f64 * 0.91) as u64;
    snap.memory = Some(mem);
    assert_eq!(kinds(&snap), vec![AlertKind::HeavyMemory]);

    // Trash over 5 GB.
    let mut s2 = Sampler::new().sample(&m.env, NOW);
    s2.storage.trash = Some(LARGE_TRASH_BYTES + 1);
    assert_eq!(kinds(&s2), vec![AlertKind::LargeTrash]);
    s2.storage.trash = Some(LARGE_TRASH_BYTES);
    assert!(kinds(&s2).is_empty());
}

fn kinds(s: &Snapshot) -> Vec<AlertKind> {
    alerts(s).into_iter().map(|a| a.kind).collect()
}

#[test]
fn menu_bar_texts() {
    let m = mac();
    let snap = Sampler::new().sample(&m.env, NOW);
    let mem = snap.memory.as_ref().unwrap();
    let ram = (mem.used as f64 / mem.total as f64 * 100.0).round();
    assert_eq!(menu_bar_title(&snap), format!("CPU 17% RAM {ram}%"));
    let e = menu_bar_entries(&snap);
    assert_eq!(e.last().unwrap(), "Quit CleanCraft");
    assert!(e.contains(&"Battery: 64%, discharging".to_string()));
    assert!(e.contains(&"Network: Unknown".to_string()));
}

#[test]
fn free_up_ram_runs_purge_and_logs() {
    let m = mac();
    assert_eq!(free_up_ram(&m.env), PurgeOutcome::Done);
    assert_eq!(
        m.env.fake_sys().unwrap().admin_calls(),
        vec![PURGE_COMMAND.to_string()]
    );
    let h = history(&m.env);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].feature, Some(FeatureId::Monitor));
    assert!(!h[0].can_undo());
    match &h[0].kind {
        ActionKind::Command { command, ok, .. } => {
            assert_eq!(command, PURGE_COMMAND);
            assert!(ok);
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn free_up_ram_cancelled_changes_nothing() {
    let m = mac();
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    assert_eq!(free_up_ram(&m.env), PurgeOutcome::Cancelled);
    assert!(history(&m.env).is_empty());
}

#[test]
fn free_up_ram_failure_is_logged_and_reported() {
    let m = mac();
    m.env.fake_sys().unwrap().answer_full(
        &format!("admin:{PURGE_COMMAND}"),
        cleancraft_core::sys::CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "purge: unable to purge".into(),
        },
    );
    assert_eq!(
        free_up_ram(&m.env),
        PurgeOutcome::Failed("purge: unable to purge".into())
    );
    let h = history(&m.env);
    assert!(matches!(&h[0].kind, ActionKind::Command { ok: false, .. }));
}

#[test]
fn free_up_ram_with_unwritable_log() {
    let m = mac();
    fs::create_dir_all(m.env.data_dir()).unwrap();
    // A folder where the journal file should be: the log cannot be written.
    fs::create_dir_all(cleancraft_core::executor::journal_path(&m.env)).unwrap();
    match free_up_ram(&m.env) {
        PurgeOutcome::Failed(e) => assert!(e.contains("nothing was run"), "{e}"),
        o => panic!("{o:?}"),
    }
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
}

#[test]
fn battery_estimate_label_follows_charging() {
    let mut b = Battery {
        percent: 87,
        state: "charging".into(),
        on_ac: true,
        remaining_minutes: Some(90),
        cycles: None,
        health: None,
    };
    assert_eq!(b.estimate_label(), "Time to full");
    b.state = "finishing charge".into();
    assert_eq!(b.estimate_label(), "Time to full");
    b.state = "discharging".into();
    assert_eq!(b.estimate_label(), "Time left");
    b.state = "AC attached".into();
    assert_eq!(b.estimate_label(), "Time left");
}
