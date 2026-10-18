//! Startup Items by clicks: the lists with program, signer and state; turning agents, daemons
//! and login items off and on; undo from History; the administrator dialog cancelled; a
//! launchctl failure; removing an agent's file through the confirmation; heavy apps and Quit.

mod common;

use cleancraft_core::executor;
use cleancraft_core::startup;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit_perf::{self as tp, UID};
use cleancraft_core::{ActionKind, Fault, FeatureId};
use common::*;
use egui_kittest::kittest::Queryable;

fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

fn open(m: &cleancraft_core::testkit::FakeMac) -> H {
    let mut h = harness(m);
    click(&mut h, "Startup Items");
    assert!(h.state().startup.data.is_some());
    h
}

#[test]
fn lists_everything_with_signer_state_and_count() {
    let m = tp::startup_mac();
    let h = open(&m);
    for label in [
        "com.example.helper",
        "com.genieo.engine",
        "com.example.updater",
        "com.example.old",
        "com.example.global",
        "com.apple.sampled",
        "com.example.daemon",
        "Example",
        "Dropbox",
    ] {
        assert!(has(&h, label), "{label} not shown");
    }
    assert!(has_text(&h, "Example Corp (ABCDE12345)"));
    assert!(has(&h, "Not signed"));
    assert!(has(&h, "Ad hoc signature (no developer identity)"));
    assert!(has(&h, "Program missing: this item does nothing"));
    assert!(has(&h, "Part of macOS"));
    // The Apple item has no buttons.
    assert!(!has(&h, "Turn Off com.apple.sampled"));
    // Global agent is off (launchctl print-disabled), so it offers Turn On.
    assert!(has(&h, "Turn On com.example.global"));
    // Root-owned folders: no Remove button.
    assert!(!has(&h, "Remove com.example.global"));
    assert!(!has(&h, "Remove com.example.daemon"));
    assert!(has(&h, "Remove com.example.helper"));
    // The count, recalculated from the same sources.
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let n = items.iter().filter(|i| i.enabled).count()
        + startup::list_login_items(&m.env).unwrap().len();
    assert_eq!(n, 8);
    assert!(has(&h, &format!("{n} items start automatically")));
}

#[test]
fn turn_agents_off_and_on_then_undo_from_history() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("sh", "");
    let mut h = open(&m);
    click(&mut h, "Turn Off com.example.helper");
    assert!(
        f.calls()
            .contains(&"/bin/launchctl disable gui/501/com.example.helper".to_string())
    );
    assert!(f.calls().contains(
        &"/bin/launchctl bootout gui/501 /Users/tester/Library/LaunchAgents/com.example.helper.plist"
            .to_string()
    ));
    assert!(has_text(&h, "Turned off com.example.helper."));
    reach(&mut h, "Turn On com.example.global");
    assert!(
        f.calls()
            .contains(&"/bin/launchctl enable gui/501/com.example.global".to_string())
    );
    reach(&mut h, "Turn Off com.example.daemon");
    assert_eq!(
        f.admin_calls(),
        vec![
            "/bin/launchctl disable 'system/com.example.daemon' && { /bin/launchctl bootout system '/Library/LaunchDaemons/com.example.daemon.plist' || true; }"
        ]
    );
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 3);
    assert!(
        hist.iter()
            .all(|r| r.feature == Some(FeatureId::LoginItems) && r.can_undo())
    );
    // Undo the daemon change from History (newest first): the password dialog again.
    reach(&mut h, "History");
    assert!(has_text(&h, "Turned off com.example.daemon"));
    click_nth(&mut h, "Undo", 0);
    assert!(f.calls().iter().any(|c| {
        c.starts_with("/bin/sh -c /usr/bin/osascript -e 'do shell script")
            && c.contains("enable")
            && c.contains("system/com.example.daemon")
    }));
    assert!(executor::history(&m.env)[2].undone.is_some());
}

#[test]
fn cancelled_password_dialog_and_launchctl_failure() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.with(|s| s.admin_cancelled = true);
    f.answer_full(
        "launchctl disable gui/501/com.example.helper",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Could not disable service: 1: Operation not permitted\n".into(),
        },
    );
    let mut h = open(&m);
    reach(&mut h, "Turn Off com.example.daemon");
    assert!(has(&h, "Cancelled, nothing changed"));
    reach(&mut h, "Turn Off com.example.helper");
    assert!(has_text(&h, "Operation not permitted"));
    assert!(executor::history(&m.env).is_empty());
    // Still shown as on.
    assert!(has(&h, "Turn Off com.example.helper"));
}

#[test]
fn login_item_off_and_back_on() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("sh", "");
    f.answer(
        "osascript -e tell application \"System Events\" to delete login item \"Dropbox\"",
        "",
    );
    let mut h = open(&m);
    // What System Events will list once Dropbox is gone.
    f.answer(
        &tp::login_items_key(),
        "Example\t/Applications/Example.app\n",
    );
    click(&mut h, "Turn Off Dropbox");
    assert!(f.calls().contains(
        &"/usr/bin/osascript -e tell application \"System Events\" to delete login item \"Dropbox\""
            .to_string()
    ));
    assert!(has_text(&h, "Dropbox no longer opens at login."));
    assert!(has(&h, "Turn On Dropbox"), "removed item still shown, off");
    f.answer(&tp::login_items_key(), tp::LOGIN_ITEMS_SAMPLE);
    click(&mut h, "Turn On Dropbox");
    assert!(f.calls().iter().any(|c| c.contains(
        "make login item at end with properties {path:\"/Applications/Dropbox.app\", hidden:false}"
    )));
    assert!(has_text(&h, "Dropbox opens at login again."));
    assert!(has(&h, "Turn Off Dropbox"));
    let hist = executor::history(&m.env);
    assert!(hist[0].undone.is_some());
}

#[test]
fn automation_refusal_is_explained() {
    let m = tp::startup_mac();
    m.env.fake_sys().unwrap().answer_full(
        &tp::login_items_key(),
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr:
                "execution error: Not authorized to send Apple events to System Events. (-1743)\n"
                    .into(),
        },
    );
    let h = open(&m);
    assert!(has_text(&h, "Privacy & Security > Automation"));
}

#[test]
fn remove_agent_file_after_confirmation_and_undo() {
    let m = tp::startup_mac();
    let plist = m
        .env
        .home_path("Library/LaunchAgents/com.example.helper.plist");
    let mut h = open(&m);
    click(&mut h, "Remove com.example.helper");
    assert!(has(&h, "Move this startup file to the Trash?"));
    assert!(has(
        &h,
        "/Users/tester/Library/LaunchAgents/com.example.helper.plist"
    ));
    // Cancel keeps it.
    click(&mut h, "Cancel");
    assert!(plist.exists());
    click(&mut h, "Remove com.example.helper");
    click(&mut h, "Move to Trash");
    assert!(!plist.exists());
    assert!(m.env.user_trash().join("com.example.helper.plist").exists());
    assert!(
        m.env
            .fake_sys()
            .unwrap()
            .calls()
            .contains(&"/bin/launchctl bootout gui/501/com.example.helper".to_string())
    );
    assert!(has_text(&h, "Moved 1 startup file."));
    assert!(!has(&h, "com.example.helper"), "gone from the list");
    let hist = executor::history(&m.env);
    let moved = hist
        .iter()
        .find(|r| matches!(r.kind, ActionKind::Moved { .. }))
        .unwrap();
    executor::undo(&m.env, &moved.id).unwrap();
    assert!(plist.exists());
}

#[test]
fn remove_agent_file_permission_denied() {
    let m = tp::startup_mac();
    let plist = m
        .env
        .home_path("Library/LaunchAgents/com.example.updater.plist");
    m.env.inject_fault(&plist, Fault::PermissionDenied);
    let mut h = open(&m);
    click(&mut h, "Remove com.example.updater");
    click(&mut h, "Move to Trash");
    assert!(plist.exists());
    assert!(has_text(
        &h,
        "Not moved: com.example.updater.plist (permission denied)"
    ));
}

#[test]
fn heavy_apps_and_quit_after_confirmation() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("osascript", "");
    f.answer("kill", "");
    let mut h = open(&m);
    for name in ["Quit Example", "Quit render", "Quit Big Editor"] {
        assert!(has(&h, name), "{name}");
    }
    for name in [
        "Quit Dock",
        "Quit CleanCraft",
        "Quit Quiet",
        "Quit Other",
        "Quit launchd",
    ] {
        assert!(!has(&h, name), "{name} must not be offered");
    }
    assert!(has(&h, "CPU 92.5%  Memory 6.1%"));
    reach(&mut h, "Quit Example");
    assert!(has(&h, "Quit Example?"));
    assert!(has(
        &h,
        "/usr/bin/osascript -e 'tell application id \"com.example.app\" to quit'"
    ));
    click(&mut h, "Cancel");
    assert!(!f.calls().iter().any(|c| c.contains("to quit")));
    reach(&mut h, "Quit Example");
    click(&mut h, "Quit App");
    assert!(f.calls().contains(
        &"/usr/bin/osascript -e tell application id \"com.example.app\" to quit".to_string()
    ));
    assert!(has_text(&h, "Asked Example to quit."));
    reach(&mut h, "Quit render");
    assert!(has(&h, "/bin/kill -TERM 702"));
    click(&mut h, "Quit App");
    assert!(f.calls().contains(&"/bin/kill -TERM 702".to_string()));
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 2);
    assert!(
        hist.iter()
            .all(|r| matches!(r.kind, ActionKind::Command { ok: true, .. }))
    );
}

#[test]
fn quit_failure_is_shown() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer_full(
        "kill -TERM 702",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "kill: 702: Operation not permitted\n".into(),
        },
    );
    let mut h = open(&m);
    reach(&mut h, "Quit render");
    click(&mut h, "Quit App");
    assert!(has_text(
        &h,
        "render did not quit: kill: 702: Operation not permitted"
    ));
}
