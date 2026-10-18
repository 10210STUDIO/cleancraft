//! Startup Items on a fake Mac: listing login items, agents and daemons with signature and
//! enabled state; turning them off and on with undo; the administrator dialog cancelled;
//! removing an agent through the plan; heavy apps and Quit.

use cleancraft_core::executor;
use cleancraft_core::features;
use cleancraft_core::startup::{self, ChangeError, Domain, LoginItem, Signer};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit_perf::{self as tp, UID};
use cleancraft_core::{ActionKind, FeatureId, Settings};

fn find<'a>(items: &'a [startup::LaunchItem], label: &str) -> &'a startup::LaunchItem {
    items
        .iter()
        .find(|i| i.label == label)
        .unwrap_or_else(|| panic!("{label} not listed"))
}

#[test]
fn lists_agents_and_daemons_with_signature_and_state() {
    let m = tp::startup_mac();
    let (items, notes) = startup::list_launch_items(&m.env, Some(UID));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(items.len(), 7);
    let helper = find(&items, "com.example.helper");
    assert_eq!(helper.domain, Domain::UserAgent);
    assert!(helper.enabled && helper.program_exists && !helper.apple);
    assert_eq!(helper.signer.label(), "Example Corp (ABCDE12345)");
    assert_eq!(helper.program.as_deref(), Some(tp::HELPER_PROG));
    let global = find(&items, "com.example.global");
    assert_eq!(global.domain, Domain::GlobalAgent);
    assert!(!global.enabled, "print-disabled says disabled");
    let apple = find(&items, "com.apple.sampled");
    assert!(apple.apple);
    assert_eq!(apple.signer, Signer::Apple);
    let old = find(&items, "com.example.old");
    assert!(!old.program_exists);
    assert_eq!(old.signer, Signer::Missing);
    assert_eq!(find(&items, "com.genieo.engine").signer, Signer::Unsigned);
    assert_eq!(find(&items, "com.example.updater").signer, Signer::AdHoc);
    let daemon = find(&items, "com.example.daemon");
    assert_eq!(daemon.domain, Domain::Daemon);
    assert!(daemon.enabled);
    let calls = m.env.fake_sys().unwrap().calls();
    assert!(calls.contains(&"/bin/launchctl print-disabled gui/501".to_string()));
    assert!(calls.contains(&"/bin/launchctl print-disabled system".to_string()));
}

#[test]
fn login_items_listed_and_automation_refusal_explained() {
    let m = tp::startup_mac();
    let v = startup::list_login_items(&m.env).unwrap();
    assert_eq!(
        v,
        vec![
            LoginItem {
                name: "Example".into(),
                path: Some("/Applications/Example.app".into())
            },
            LoginItem {
                name: "Dropbox".into(),
                path: Some("/Applications/Dropbox.app".into())
            },
        ]
    );
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
    let e = startup::list_login_items(&m.env).unwrap_err();
    assert!(e.contains("Automation"), "{e}");
}

#[test]
fn turn_off_user_agent_then_undo_from_history() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("sh", "");
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let helper = find(&items, "com.example.helper");
    let rec = startup::set_launch_item_enabled(&m.env, helper, UID, false).unwrap();
    let calls = f.calls();
    let plist = "/Users/tester/Library/LaunchAgents/com.example.helper.plist";
    assert!(calls.contains(&"/bin/launchctl disable gui/501/com.example.helper".to_string()));
    assert!(calls.contains(&format!("/bin/launchctl bootout gui/501 {plist}")));
    assert!(f.admin_calls().is_empty(), "user agents need no password");
    let ActionKind::Toggled { what, undo_command } = &rec.kind else {
        panic!()
    };
    assert!(what.starts_with("Turned off com.example.helper"));
    let undo = undo_command.clone().unwrap();
    assert_eq!(
        undo,
        format!(
            "/bin/launchctl enable 'gui/501/com.example.helper' && {{ /bin/launchctl bootstrap gui/501 '{plist}' || true; }}"
        )
    );
    // History's Undo runs exactly that command.
    executor::undo(&m.env, &rec.id).unwrap();
    assert!(f.calls().contains(&format!("/bin/sh -c {undo}")));
    assert!(executor::history(&m.env)[0].undone.is_some());
}

#[test]
fn turn_on_uses_enable_and_bootstrap() {
    let m = tp::startup_mac();
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let global = find(&items, "com.example.global");
    startup::set_launch_item_enabled(&m.env, global, UID, true).unwrap();
    let calls = m.env.fake_sys().unwrap().calls();
    assert!(calls.contains(&"/bin/launchctl enable gui/501/com.example.global".to_string()));
    assert!(
        calls.contains(
            &"/bin/launchctl bootstrap gui/501 /Library/LaunchAgents/com.example.global.plist"
                .to_string()
        )
    );
}

#[test]
fn apple_items_are_never_changed() {
    let m = tp::startup_mac();
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let before = m.env.fake_sys().unwrap().calls().len();
    let e = startup::set_launch_item_enabled(&m.env, find(&items, "com.apple.sampled"), UID, false)
        .unwrap_err();
    assert!(matches!(e, ChangeError::Refused(_)));
    assert_eq!(m.env.fake_sys().unwrap().calls().len(), before);
    assert!(executor::history(&m.env).is_empty());
}

#[test]
fn daemon_needs_password_and_cancel_changes_nothing() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let d = find(&items, "com.example.daemon");
    f.with(|s| s.admin_cancelled = true);
    let e = startup::set_launch_item_enabled(&m.env, d, UID, false).unwrap_err();
    assert_eq!(e, ChangeError::Cancelled);
    assert_eq!(e.to_string(), "Cancelled, nothing changed");
    assert!(executor::history(&m.env).is_empty());
    f.with(|s| s.admin_cancelled = false);
    let rec = startup::set_launch_item_enabled(&m.env, d, UID, false).unwrap();
    assert_eq!(
        f.admin_calls().last().unwrap(),
        "/bin/launchctl disable 'system/com.example.daemon' && { /bin/launchctl bootout system '/Library/LaunchDaemons/com.example.daemon.plist' || true; }"
    );
    let ActionKind::Toggled { undo_command, .. } = rec.kind else {
        panic!()
    };
    let undo = undo_command.unwrap();
    assert!(undo.starts_with("/usr/bin/osascript -e 'do shell script"));
    assert!(undo.contains("enable"));
    assert!(undo.contains("with administrator privileges"));
}

#[test]
fn launchctl_failure_is_reported_and_not_logged() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer_full(
        "launchctl disable gui/501/com.example.helper",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Could not disable service: 1: Operation not permitted\n".into(),
        },
    );
    let (items, _) = startup::list_launch_items(&m.env, Some(UID));
    let e =
        startup::set_launch_item_enabled(&m.env, find(&items, "com.example.helper"), UID, false)
            .unwrap_err();
    assert!(e.to_string().contains("Operation not permitted"), "{e}");
    assert!(executor::history(&m.env).is_empty());
}

#[test]
fn remove_login_item_and_add_it_back() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer(
        "osascript -e tell application \"System Events\" to delete login item \"Dropbox\"",
        "",
    );
    f.answer("sh", "");
    let items = startup::list_login_items(&m.env).unwrap();
    let rec = startup::remove_login_item(&m.env, &items[1]).unwrap();
    assert!(f.calls().contains(
        &"/usr/bin/osascript -e tell application \"System Events\" to delete login item \"Dropbox\""
            .to_string()
    ));
    // Still listed by System Events here, so not shown as removed.
    assert!(startup::removed_login_items(&m.env, &items).is_empty());
    let now = vec![items[0].clone()];
    let removed = startup::removed_login_items(&m.env, &now);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].0, rec.id);
    assert_eq!(
        removed[0].1.path.as_deref(),
        Some("/Applications/Dropbox.app")
    );
    executor::undo(&m.env, &rec.id).unwrap();
    assert!(f.calls().iter().any(|c| c.starts_with("/bin/sh -c /usr/bin/osascript -e")
        && c.contains("make login item at end with properties {path:\"/Applications/Dropbox.app\", hidden:false}")));
    assert!(startup::removed_login_items(&m.env, &now).is_empty());
}

#[test]
fn login_item_without_path_is_refused() {
    let m = tp::startup_mac();
    let e = startup::remove_login_item(
        &m.env,
        &LoginItem {
            name: "Odd".into(),
            path: None,
        },
    )
    .unwrap_err();
    assert!(matches!(e, ChangeError::Refused(_)));
}

#[test]
fn heavy_apps_are_this_users_and_never_system_or_cleancraft() {
    let m = tp::startup_mac();
    let procs = startup::list_processes(&m.env, UID).unwrap();
    assert!(procs.iter().all(|p| p.uid == UID));
    let heavy = startup::heavy_apps(&procs);
    let names: Vec<&str> = heavy.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Example", "render", "Big Editor"]);
    assert_eq!(heavy[0].bundle_id.as_deref(), Some("com.example.app"));
    assert!(procs.iter().any(|p| p.name == "CleanCraft" && p.is_self));
    assert!(procs.iter().any(|p| p.name == "Dock" && p.system));
}

#[test]
fn quit_politely_or_with_term_and_log() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("osascript", "");
    f.answer("kill", "");
    let procs = startup::list_processes(&m.env, UID).unwrap();
    let heavy = startup::heavy_apps(&procs);
    startup::quit_process(&m.env, &heavy[0], UID).unwrap();
    assert!(f.calls().contains(
        &"/usr/bin/osascript -e tell application id \"com.example.app\" to quit".to_string()
    ));
    let render = heavy.iter().find(|p| p.name == "render").unwrap();
    startup::quit_process(&m.env, render, UID).unwrap();
    assert!(f.calls().contains(&"/bin/kill -TERM 702".to_string()));
    let h = executor::history(&m.env);
    assert_eq!(h.len(), 2);
    assert!(matches!(h[1].kind, ActionKind::Command { ok: true, .. }));
    // CleanCraft and other users' apps are refused.
    let me = procs.iter().find(|p| p.is_self).unwrap();
    assert!(matches!(
        startup::quit_process(&m.env, me, UID),
        Err(ChangeError::Refused(_))
    ));
    let mut other = heavy[0].clone();
    other.uid = 502;
    assert!(matches!(
        startup::quit_process(&m.env, &other, UID),
        Err(ChangeError::Refused(_))
    ));
}

#[test]
fn quit_rechecks_the_pid() {
    let m = tp::startup_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("kill", "");
    let procs = startup::list_processes(&m.env, UID).unwrap();
    let render = procs.iter().find(|p| p.name == "render").unwrap().clone();
    // render exited and pid 702 now belongs to something else.
    f.answer(
        &tp::ps_key(),
        "  702   501   1.0  0.1 /Applications/Quiet.app/Contents/MacOS/Quiet\n",
    );
    let e = startup::quit_process(&m.env, &render, UID).unwrap_err();
    assert!(e.to_string().contains("no longer running"), "{e}");
    assert!(!f.calls().iter().any(|c| c.contains("kill")));
}

#[test]
fn remove_agent_plist_through_the_plan_with_bootout_first_and_undo() {
    let m = tp::startup_mac();
    let settings = Settings::default();
    let mut plan = features::scan(&m.env, &settings, FeatureId::LoginItems).unwrap();
    assert!(
        plan.items.iter().all(|i| !i.selected),
        "nothing preselected"
    );
    assert!(
        !plan
            .items
            .iter()
            .any(|i| i.path.to_string_lossy().contains("com.apple.sampled"))
    );
    let global = plan
        .items
        .iter()
        .find(|i| i.path.ends_with("com.example.global.plist"))
        .unwrap();
    assert!(global.locked.is_some(), "root-owned folder");
    let helper_path = m
        .env
        .home_path("Library/LaunchAgents/com.example.helper.plist");
    let id = plan
        .items
        .iter()
        .find(|i| i.path == helper_path)
        .unwrap()
        .id;
    plan.set_selected(id, true);
    let rep = features::run_plan(&m.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 1);
    assert!(!helper_path.exists());
    assert!(m.env.user_trash().join("com.example.helper.plist").exists());
    let calls = m.env.fake_sys().unwrap().calls();
    assert!(calls.contains(
        &"/bin/launchctl bootout gui/501 /Users/tester/Library/LaunchAgents/com.example.helper.plist"
            .to_string()
    ));
    let h = executor::history(&m.env);
    assert!(
        matches!(&h[0].kind, ActionKind::Command { command, .. } if command.contains("bootout"))
    );
    executor::undo_plan(&m.env, &rep.plan_id);
    assert!(helper_path.exists());
}
