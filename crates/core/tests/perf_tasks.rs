//! Tune-up Tasks on a fake Mac: exact commands, administrator dialog cancelled, failures,
//! availability checks, free space before and after, journal and last run.

use cleancraft_core::executor::{self, journal_path};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::tasks::{self, Outcome, Step};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_perf::{self, TMUTIL_LIST};
use cleancraft_core::{ActionKind, Fault, FeatureId};

fn mac() -> FakeMac {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    m
}

#[test]
fn every_task_has_text_and_a_command() {
    assert_eq!(tasks::TASKS.len(), 10);
    for t in tasks::TASKS {
        assert!(
            !t.description.is_empty() && !t.command.is_empty(),
            "{}",
            t.id
        );
        assert!(!t.description.contains('\u{2014}'), "em dash in {}", t.id);
        assert!(!t.description.to_lowercase().contains(" nor "));
    }
}

#[test]
fn admin_task_runs_through_password_dialog_and_is_logged() {
    let m = mac();
    let p = tasks::prepare(&m.env, "flush_dns").unwrap();
    assert_eq!(
        p.steps,
        vec![Step::Admin {
            shell: "dscacheutil -flushcache; killall -HUP mDNSResponder".into()
        }]
    );
    let r = tasks::run(&m.env, &p);
    assert_eq!(r.outcome, Outcome::Done, "{}", r.message);
    let f = m.env.fake_sys().unwrap();
    assert_eq!(
        f.admin_calls(),
        vec!["dscacheutil -flushcache; killall -HUP mDNSResponder"]
    );
    let h = executor::history(&m.env);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].feature, Some(FeatureId::Maintenance));
    match &h[0].kind {
        ActionKind::Command { command, ok, .. } => {
            assert!(command.contains("mDNSResponder"));
            assert!(*ok);
        }
        k => panic!("{k:?}"),
    }
    assert!(!h[0].can_undo());
    let last = tasks::last_runs(&m.env);
    assert_eq!(last.get("flush_dns").map(|x| x.1), Some(true));
}

#[test]
fn cancelled_password_dialog_changes_nothing_and_logs_nothing() {
    let m = mac();
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    let r = tasks::run_by_id(&m.env, "free_ram").unwrap();
    assert_eq!(r.outcome, Outcome::Cancelled);
    assert_eq!(r.message, "Cancelled, nothing changed");
    assert_eq!(m.env.fake_sys().unwrap().admin_calls(), vec!["purge"]);
    assert!(executor::history(&m.env).is_empty());
    assert!(tasks::last_runs(&m.env).is_empty());
}

#[test]
fn failure_is_reported_and_logged_as_failed() {
    let m = mac();
    m.env.fake_sys().unwrap().answer_full(
        "tmutil thinlocalsnapshots / 999999999999 4",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Error: thinning failed (sample)\n".into(),
        },
    );
    let r = tasks::run_by_id(&m.env, "purgeable_space").unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.message.contains("thinning failed"), "{}", r.message);
    let h = executor::history(&m.env);
    assert!(matches!(h[0].kind, ActionKind::Command { ok: false, .. }));
    assert!(!tasks::last_runs(&m.env)["purgeable_space"].1);
}

#[test]
fn missing_program_is_a_failure_not_a_crash() {
    // FakeSys has no answer for lsregister: like a missing tool.
    let m = mac();
    let r = tasks::run_by_id(&m.env, "rebuild_launch_services").unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.message.contains("lsregister"), "{}", r.message);
}

#[test]
fn purgeable_space_shows_free_space_before_and_after() {
    let mut m = mac();
    m.env.fake_sys().unwrap().answer(
        "tmutil thinlocalsnapshots / 999999999999 4",
        "Thinned local snapshots:\n2026-10-17-101530\n",
    );
    testkit_perf::free_space_rises_after(&mut m, "thinlocalsnapshots", 3_000_000_000);
    let r = tasks::run_by_id(&m.env, "purgeable_space").unwrap();
    assert_eq!(r.outcome, Outcome::Done);
    assert_eq!(r.free_before, Some(100_000_000_000));
    assert_eq!(r.free_after, Some(103_000_000_000));
    assert!(r.output.contains("Thinned"));
    assert_eq!(executor::history(&m.env)[0].size, 3_000_000_000);
}

#[test]
fn periodic_only_on_macos_13_and_earlier() {
    let m = mac();
    let e = tasks::prepare(&m.env, "periodic_scripts").unwrap_err();
    assert!(
        e.contains("macOS 13 and earlier") && e.contains("14.6"),
        "{e}"
    );
    m.env
        .fake_sys()
        .unwrap()
        .answer("sw_vers -productVersion", "13.6.1\n");
    let p = tasks::prepare(&m.env, "periodic_scripts").unwrap();
    assert_eq!(p.commands(), vec!["periodic daily weekly monthly"]);
    assert!(p.needs_admin());
}

#[test]
fn speed_up_mail_needs_mail_quit_and_measures_the_index() {
    let m = mac();
    let idx = testkit_perf::mail_index(&m, 40_000);
    testkit_perf::set_mail_running(&m, true);
    let e = tasks::prepare(&m.env, "speed_up_mail").unwrap_err();
    assert!(e.contains("Quit Mail"), "{e}");
    testkit_perf::set_mail_running(&m, false);
    let p = tasks::prepare(&m.env, "speed_up_mail").unwrap();
    assert_eq!(
        p.commands(),
        vec!["/usr/bin/sqlite3 '/Users/tester/Library/Mail/V10/MailData/Envelope Index' 'VACUUM;'"]
    );
    // Mail opened between the confirmation and the run: nothing runs.
    testkit_perf::set_mail_running(&m, true);
    let r = tasks::run(&m.env, &p);
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(
        !m.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("sqlite3"))
    );
    testkit_perf::set_mail_running(&m, false);
    m.env.fake_sys().unwrap().answer("sqlite3", "");
    let r = tasks::run(&m.env, &p);
    assert_eq!(r.outcome, Outcome::Done, "{}", r.message);
    assert_eq!(r.file_before, Some(40_000));
    assert_eq!(r.file_after, Some(std::fs::metadata(&idx).unwrap().len()));
}

#[test]
fn speed_up_mail_without_full_disk_access() {
    let m = mac();
    testkit_perf::mail_index(&m, 10);
    m.env
        .inject_fault(&m.env.home_path("Library/Mail"), Fault::PermissionDenied);
    let e = tasks::prepare(&m.env, "speed_up_mail").unwrap_err();
    assert!(e.contains("Full Disk Access"), "{e}");
}

#[test]
fn thin_snapshots_lists_dates_then_deletes_each() {
    let m = mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("tmutil listlocalsnapshots /", "Snapshots for disk /:\n");
    let e = tasks::prepare(&m.env, "thin_snapshots").unwrap_err();
    assert!(e.contains("no local"), "{e}");
    f.answer("tmutil listlocalsnapshots /", TMUTIL_LIST);
    let p = tasks::prepare(&m.env, "thin_snapshots").unwrap();
    assert_eq!(
        p.commands(),
        vec![
            "tmutil deletelocalsnapshots 2026-10-17-101530 && tmutil deletelocalsnapshots 2026-10-18-091200"
        ]
    );
}

#[test]
fn repair_permissions_uses_the_user_id() {
    let m = mac();
    let p = tasks::prepare(&m.env, "repair_permissions").unwrap();
    assert_eq!(p.commands(), vec!["diskutil resetUserPermissions / 501"]);
}

#[test]
fn nothing_runs_when_the_log_cannot_be_written() {
    let m = mac();
    m.env.inject_fault(&journal_path(&m.env), Fault::DiskFull);
    let r = tasks::run_by_id(&m.env, "free_ram").unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.message.contains("nothing ran"));
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
}

#[test]
fn not_offered_outside_macos() {
    let m = mac();
    m.env.fake_sys().unwrap().with(|s| s.macos = false);
    assert!(tasks::prepare(&m.env, "flush_dns").is_err());
}

#[test]
fn exact_commands_of_the_simple_tasks() {
    let m = mac();
    let cmd = |id: &str| tasks::prepare(&m.env, id).unwrap().commands();
    assert_eq!(cmd("reindex_spotlight"), vec!["mdutil -E /"]);
    assert!(
        tasks::prepare(&m.env, "reindex_spotlight")
            .unwrap()
            .needs_admin()
    );
    assert_eq!(
        cmd("rebuild_launch_services"),
        vec![format!(
            "{} -kill -r -domain local -domain system -domain user",
            tasks::LSREGISTER
        )]
    );
    assert_eq!(
        cmd("verify_disk"),
        vec!["/usr/sbin/diskutil verifyVolume /"]
    );
    assert_eq!(cmd("free_ram"), vec!["purge"]);
    assert_eq!(
        cmd("purgeable_space"),
        vec!["/usr/bin/tmutil thinlocalsnapshots / 999999999999 4"]
    );
}
