//! Tune-up Tasks by clicks: tick tasks, see the exact commands in the confirmation, run, see
//! per-task results, free space before and after, the log, the last run; the administrator
//! dialog cancelled; a failing command; tasks that cannot run here.

mod common;

use cleancraft_core::executor;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_perf;
use cleancraft_core::{ActionKind, FeatureId};
use common::*;
use egui_kittest::kittest::Queryable;

fn mac() -> FakeMac {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    m
}

/// Scroll a control into view, then click it like a user.
fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

fn open(h: &mut H) {
    // The tool sits in the "Speed" section of the sidebar.
    click(h, "Speed");
    click(h, "Tune-up Tasks");
    assert!(h.state().tasks.loaded);
}

#[test]
fn run_two_tasks_with_exact_commands_results_and_free_space() {
    let mut m = mac();
    m.env.fake_sys().unwrap().answer(
        "tmutil thinlocalsnapshots / 999999999999 4",
        "Thinned local snapshots:\n2026-10-17-101530\n",
    );
    testkit_perf::free_space_rises_after(&mut m, "thinlocalsnapshots", 2_500_000_000);
    let mut h = harness(&m);
    open(&mut h);
    assert!(has_text(&h, "Never run by CleanCraft"));
    click(&mut h, "Flush DNS cache");
    click(&mut h, "Free up purgeable space");
    assert!(has(&h, "2 tasks selected"));
    click(&mut h, "Run Selected");
    // The confirmation shows the exact commands.
    assert!(has(&h, "Run 2 tasks?"));
    assert!(has(
        &h,
        "dscacheutil -flushcache; killall -HUP mDNSResponder"
    ));
    assert!(has(
        &h,
        "/usr/bin/tmutil thinlocalsnapshots / 999999999999 4"
    ));
    assert!(has(&h, "macOS will ask for the administrator password."));
    let f = m.env.fake_sys().unwrap();
    assert!(f.admin_calls().is_empty(), "nothing runs before Run");
    click(&mut h, "Run");
    assert_eq!(
        f.admin_calls(),
        vec!["dscacheutil -flushcache; killall -HUP mDNSResponder"]
    );
    assert!(
        f.calls()
            .contains(&"/usr/bin/tmutil thinlocalsnapshots / 999999999999 4".to_string())
    );
    // Results per task.
    assert_eq!(h.query_all_by_label("Done").count(), 2);
    assert!(has_text(&h, "2 done, 0 failed, 0 cancelled."));
    let r = &h.state().tasks.results["purgeable_space"];
    let (b, a) = (r.free_before.unwrap(), r.free_after.unwrap());
    // Recalculated from the fake disk: 100 GB free, 2.5 GB more after thinning.
    assert_eq!(b, 100_000_000_000);
    assert_eq!(a, 102_500_000_000);
    assert!(has(
        &h,
        &format!(
            "Free space before {}, after {}",
            human_size(b),
            human_size(a)
        )
    ));
    // The log of the command.
    reach(&mut h, "Show log of Free up purgeable space");
    assert!(has_text(&h, "Thinned local snapshots"));
    // Journal and last run.
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 2);
    assert!(
        hist.iter()
            .all(|r| r.feature == Some(FeatureId::Maintenance)
                && matches!(r.kind, ActionKind::Command { ok: true, .. }))
    );
    assert_eq!(h.query_all_by_label_contains("Last run ").count(), 2);
    // History lists the commands.
    // (The sidebar is taller than the window, so History needs a scroll.)
    reach(&mut h, "History");
    assert!(has_text(&h, "mDNSResponder"));
}

#[test]
fn cancelled_password_dialog_says_nothing_changed() {
    let m = mac();
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    let mut h = harness(&m);
    open(&mut h);
    click(&mut h, "Free up RAM");
    click(&mut h, "Run Selected");
    click(&mut h, "Run");
    assert!(has(&h, "Cancelled, nothing changed"));
    assert!(has_text(&h, "0 done, 0 failed, 1 cancelled."));
    assert_eq!(m.env.fake_sys().unwrap().admin_calls(), vec!["purge"]);
    assert!(executor::history(&m.env).is_empty());
    assert_eq!(h.query_all_by_label("Never run by CleanCraft").count(), 10);
}

#[test]
fn failing_command_is_shown_and_logged() {
    let m = mac();
    m.env.fake_sys().unwrap().answer_full(
        "tmutil thinlocalsnapshots / 999999999999 4",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Error: thinning failed (sample)\n".into(),
        },
    );
    let mut h = harness(&m);
    open(&mut h);
    click(&mut h, "Free up purgeable space");
    click(&mut h, "Run Selected");
    click(&mut h, "Run");
    assert!(has(&h, "Failed: Error: thinning failed (sample)"));
    assert!(has_text(&h, "0 done, 1 failed, 0 cancelled."));
    assert!(has_text(&h, "(failed)"));
    let hist = executor::history(&m.env);
    assert!(matches!(
        hist[0].kind,
        ActionKind::Command { ok: false, .. }
    ));
}

#[test]
fn tasks_that_cannot_run_explain_why_and_cancel_runs_nothing() {
    let m = mac();
    testkit_perf::mail_index(&m, 1000);
    testkit_perf::set_mail_running(&m, true);
    let mut h = harness(&m);
    open(&mut h);
    assert!(has_text(&h, "Only for macOS 13 and earlier"));
    assert!(has(&h, "Mail is open. Quit Mail first."));
    assert!(
        has(
            &h,
            "There are no local Time Machine snapshots on the startup disk."
        ) || has_text(&h, "Could not list local snapshots")
    );
    // A task that cannot run cannot be ticked.
    reach(&mut h, "Speed up Mail");
    assert!(h.state().tasks.selected.is_empty());
    assert!(has(&h, "Tick the tasks you want to run."));
    // Cancel in the confirmation runs nothing.
    reach(&mut h, "Verify startup disk");
    click(&mut h, "Run Selected");
    assert!(has(&h, "/usr/sbin/diskutil verifyVolume /"));
    click(&mut h, "Cancel");
    assert!(
        m.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .all(|c| !c.contains("diskutil"))
    );
    // Mail closed: Check Again makes it available.
    testkit_perf::set_mail_running(&m, false);
    click(&mut h, "Check Again");
    assert!(!has(&h, "Mail is open. Quit Mail first."));
    assert!(has_text(&h, "Envelope Index"));
}
