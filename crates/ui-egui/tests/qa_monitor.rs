//! Live Monitor by clicks on a fake Mac whose system tools answer with recorded samples:
//! every card shows exactly the parsed reading, unknown values say "Unknown" (never zero),
//! alerts appear at their thresholds, Free Up Space opens Quick Sweep, and Free Up RAM asks
//! first, runs `purge` as administrator, logs it, and handles a cancelled password dialog.

mod common;

use std::time::Duration;

use cleancraft_core::executor::history;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::monitor::{BatteryReading, PURGE_COMMAND, pct, rate_text, uptime_text};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{ActionKind, Fault, FeatureId};
use cleancraft_ui_egui::app::Page;
use common::*;
use egui_kittest::kittest::Queryable;

fn mac() -> FakeMac {
    let m = FakeMac::new();
    answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    m
}

fn open(m: &FakeMac) -> H {
    let mut h = harness(m);
    // The last sidebar row sits below the fold at 820 px: scroll to it like a user.
    h.get_by_label("Live Monitor").scroll_to_me();
    steps(&mut h);
    click(&mut h, "Live Monitor");
    h
}

#[test]
fn cards_show_the_parsed_reading() {
    let m = mac();
    let h = open(&m);
    let s = h.state().monitor.last.clone().expect("a reading");
    assert!(s.problems.is_empty(), "{:?}", s.problems);

    let cpu = s.cpu.as_ref().unwrap();
    assert!(has(&h, &pct(cpu.load)), "cpu {}", pct(cpu.load));
    assert_eq!(pct(cpu.load), "17%", "17.4% from the sample");
    assert!(has(&h, &uptime_text(s.uptime_secs.unwrap())));
    assert!(has(&h, "1.85, 2.10, 2.30"));
    for a in &cpu.top {
        assert!(has(&h, &a.name), "{}", a.name);
        assert!(has(&h, &pct(a.cpu)), "{} {}", a.name, pct(a.cpu));
    }

    let mem = s.memory.as_ref().unwrap();
    assert!(has(
        &h,
        &format!("{} of {}", human_size(mem.used), human_size(mem.total))
    ));
    // Recalculated from the vm_stat sample: (anonymous - purgeable + wired + compressor) pages.
    assert_eq!(mem.used, (330_000 + 130_000 + 50_000) * 16384);
    assert!(has(&h, &human_size(mem.available())));
    assert!(has(&h, "Normal"));
    assert!(has(&h, &human_size(mem.swap_used.unwrap())));
    for a in &mem.top {
        assert!(has(&h, &human_size(a.memory)), "{}", a.name);
    }

    let free = s.storage.free.unwrap();
    assert_eq!(free, 80_000_000 * 1024);
    assert!(has(&h, &format!("{} free", human_size(free))));
    assert!(has(&h, &human_size(s.storage.total.unwrap())));
    // Trash size recalculated from disk.
    let trash = tree_size(&m.env.user_trash()).0;
    assert_eq!(s.storage.trash, Some(trash));
    assert!(has(&h, &human_size(trash)));

    let Some(BatteryReading::Present(b)) = &s.battery else {
        panic!("battery");
    };
    assert!(has(&h, "64%"));
    assert!(has(&h, "discharging"));
    assert!(has(&h, &uptime_text(252 * 60)));
    assert_eq!(b.health, Some(95));
    assert!(has(&h, "95%"));
    assert!(has(&h, "213"));
    // One reading so far: the speed needs a second one.
    assert!(has(&h, "Measuring speed..."));
    assert!(!has(&h, "Unknown"));
}

#[test]
fn network_speed_after_two_readings_and_refresh_off_the_ui_thread_logic() {
    let m = mac();
    let mut h = open(&m);
    m.env.fake_sys().unwrap().answer("netstat", NETSTAT_2);
    h.state_mut().monitor.interval = Duration::ZERO;
    h.step();
    h.state_mut().monitor.interval = Duration::from_secs(3600);
    steps(&mut h);
    let st = &h.state().monitor;
    assert!(st.samples >= 2);
    let n = st.last.as_ref().unwrap().network.expect("speed");
    assert!(n.down > 0 && n.up > 0);
    assert!(has(&h, &rate_text(n.down)));
    assert!(has(&h, &rate_text(n.up)));
    assert!(st.history.len() >= 2, "CPU history grows");
    // No more readings while the interval has not passed.
    let calls = m.env.fake_sys().unwrap().calls().len();
    steps(&mut h);
    assert_eq!(m.env.fake_sys().unwrap().calls().len(), calls);
}

#[test]
fn unreadable_tools_show_unknown_not_zero() {
    let m = FakeMac::new(); // no tool answers
    let h = open(&m);
    let unknown = h.query_all_by_label("Unknown").count();
    // CPU, uptime, memory, storage, battery, network.
    assert!(unknown >= 6, "only {unknown} Unknown labels");
    assert!(!has(&h, "0%"));
    assert!(!has(&h, "Zero KB free"));
    assert!(has_text(&h, "Not read:"));
}

#[test]
fn trash_without_full_disk_access_says_so() {
    let m = mac();
    m.env
        .inject_fault(&m.env.user_trash(), Fault::PermissionDenied);
    let h = open(&m);
    assert!(has(&h, "Unknown"));
    assert!(has(&h, "The Trash size needs Full Disk Access"));
}

#[test]
fn desktop_mac_shows_no_battery() {
    let m = mac();
    m.env.fake_sys().unwrap().answer("pmset", PMSET_DESKTOP);
    let h = open(&m);
    assert!(has(&h, "No battery"));
}

#[test]
fn alerts_appear() {
    let m = mac();
    let f = m.env.fake_sys().unwrap();
    f.answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488281250 479492188 8789062 99% /\n",
    );
    f.answer("sysctl", &SYSCTL.replace("pressure_level: 1", "pressure_level: 4"));
    let h = open(&m);
    let free = 8_789_062u64 * 1024;
    assert!(has(
        &h,
        &format!("Your startup disk is almost full: {} free.", human_size(free))
    ));
    assert!(has_text(&h, "Memory is heavily used"));
    assert!(has_text(&h, "pressure critical"));
}

#[test]
fn free_up_space_opens_quick_sweep() {
    let m = mac();
    let mut h = open(&m);
    click(&mut h, "Free Up Space");
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
    assert!(has(&h, "Scan"));
}

#[test]
fn free_up_ram_asks_then_runs_purge_and_logs() {
    let m = mac();
    let mut h = open(&m);
    click(&mut h, "Free Up RAM");
    assert!(has(&h, "Free up memory?"));
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty(), "asked first");
    click(&mut h, "Free Up RAM Now");
    assert_eq!(
        m.env.fake_sys().unwrap().admin_calls(),
        vec![PURGE_COMMAND.to_string()]
    );
    assert!(has_text(&h, "Memory freed."));
    let hist = history(&m.env);
    assert_eq!(hist.len(), 1);
    assert!(matches!(&hist[0].kind, ActionKind::Command { command, ok: true, .. } if command == PURGE_COMMAND));
}

#[test]
fn free_up_ram_dialog_cancel_runs_nothing() {
    let m = mac();
    let mut h = open(&m);
    click(&mut h, "Free Up RAM");
    click(&mut h, "Cancel");
    assert!(!has(&h, "Free up memory?"));
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
    assert!(history(&m.env).is_empty());
}

#[test]
fn free_up_ram_password_cancelled() {
    let m = mac();
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    let mut h = open(&m);
    click(&mut h, "Free Up RAM");
    click(&mut h, "Free Up RAM Now");
    assert_eq!(m.env.fake_sys().unwrap().admin_calls().len(), 1);
    assert!(has_text(&h, "Cancelled. Nothing was run."));
    assert!(history(&m.env).is_empty());
}

#[test]
fn free_up_ram_failure_is_shown() {
    let m = mac();
    m.env.fake_sys().unwrap().answer_full(
        &format!("admin:{PURGE_COMMAND}"),
        cleancraft_core::sys::CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "purge: unable to purge".into(),
        },
    );
    let mut h = open(&m);
    click(&mut h, "Free Up RAM");
    click(&mut h, "Free Up RAM Now");
    assert!(has_text(&h, "Could not free memory: purge: unable to purge"));
}

#[test]
fn no_readings_on_other_pages() {
    let m = mac();
    let h = harness(&m);
    assert!(m.env.fake_sys().unwrap().calls().is_empty());
    drop(h);
}
