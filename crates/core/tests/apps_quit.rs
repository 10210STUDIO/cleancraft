//! apps4 r4: an app that keeps helpers running from inside its bundle can be quit from App
//! Remover, then uninstalled. Everything on a FakeMac with FakeSys answers for `ps`,
//! `lsappinfo`, `launchctl print`, `osascript`, `kill` and the administrator dialog.

use std::path::PathBuf;
use std::time::Duration;

use cleancraft_core::app_quit::{
    self, ForceOptions, JobDomain, PartKind, QuitOptions, quit_args, running_parts,
};
use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::sys::{CmdOut, FakeEffect, RunningApp};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::testkit_quit::FakeMacQuit;
use cleancraft_core::{Fault, FeatureId, Settings};

const ID: &str = "com.example.photostudio5";
const MENU_ID: &str = "com.example.photostudio5.Menu";
const MONITOR_ID: &str = "com.example.photostudio5.HealthMonitor";
const AGENT: &str = "com.example.photostudio5.Agent";
const ME: u32 = 501;

fn quick() -> QuitOptions {
    QuitOptions {
        allow_admin: false,
        wait: Duration::ZERO,
        poll: Duration::from_millis(1),
    }
}

fn quick_force() -> ForceOptions {
    ForceOptions {
        allow_admin: false,
        grace: Duration::ZERO,
        wait: Duration::ZERO,
        poll: Duration::from_millis(1),
    }
}

struct Setup {
    mac: FakeMac,
    app: PathBuf,
    menu_exe: PathBuf,
    monitor_exe: PathBuf,
    helper_tool: PathBuf,
    cache: PathBuf,
}

/// PhotoStudio_5 as the user had it: the app, a menu bar helper (a login item), a health
/// monitor agent, and a privileged helper run by a launch daemon as root.
fn setup() -> Setup {
    let mac = FakeMac::new();
    let app = mac.app_spec(&AppSpec::new("PhotoStudio_5", ID, "5.7.2"));
    mac.user_id(ME);
    let menu_exe = mac.helper_app(
        &app,
        "Contents/Library/LoginItems/PhotoStudio_5 Menu.app",
        MENU_ID,
    );
    let monitor_exe = mac.helper_app(
        &app,
        "Contents/Library/LoginItems/HealthMonitor.app",
        MONITOR_ID,
    );
    let helper_tool = mac.sys_file(&format!("/Library/PrivilegedHelperTools/{AGENT}"), 4096);
    mac.write(
        &mac.env
            .sys_path(&format!("/Library/LaunchDaemons/{AGENT}.plist")),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>{AGENT}</string>
<key>Program</key><string>/Library/PrivilegedHelperTools/{AGENT}</string></dict></plist>"#
        )
        .as_bytes(),
    );
    let cache = mac.home_file(&format!("Library/Caches/{ID}/Cache.db"), 50_000);
    mac.home_file(&format!("Library/Preferences/{ID}.plist"), 2_000);
    if let Some(f) = mac.env.fake_sys() {
        f.answer("/usr/bin/mdfind", "");
    }
    Setup {
        mac,
        app,
        menu_exe,
        monitor_exe,
        helper_tool,
        cache: cache.parent().unwrap().to_path_buf(),
    }
}

/// Everything runs: the app (pid 600), the menu helper (601, login item launched by launchd),
/// the health monitor (602, launchd agent), the privileged helper (700, root daemon).
fn run_everything(s: &Setup) {
    let m = &s.mac;
    m.run_program(&s.app.join("Contents/MacOS/PhotoStudio_5"), 600, ME);
    m.run_program(&s.menu_exe, 601, ME);
    m.run_program(&s.monitor_exe, 602, ME);
    m.run_program(&s.helper_tool, 700, 0);
    // Something else that has nothing to do with the app.
    let other = m.app("Notes Plus", "com.example.Notes", "1.0");
    m.run_program(&other.join("Contents/MacOS/Notes Plus"), 900, ME);
    m.launchd(
        &format!("gui/{ME}"),
        &[
            (600, "application.com.example.photostudio5.1234.5678"),
            (601, MENU_ID),
            (602, MONITOR_ID),
            (0, "com.example.photostudio5.Updater"),
            (900, "application.com.example.Notes.1.2"),
            (0, "com.apple.something"),
        ],
    );
    m.launchd("system", &[(700, AGENT), (1, "com.apple.xpc.launchd")]);
    m.window_server(&[
        (600, "PhotoStudio_5", ID, "Foreground"),
        (601, "the reference app Menu", MENU_ID, "UIElement"),
        (900, "Notes Plus", "com.example.Notes", "Foreground"),
    ]);
}

fn fake(m: &FakeMac) -> &cleancraft_core::sys::FakeSys {
    m.env.fake_sys().unwrap()
}

fn bootout_key(label: &str) -> String {
    format!("/bin/launchctl bootout gui/{ME}/{label}")
}

fn quit_key(id: &str) -> String {
    std::iter::once("/usr/bin/osascript".to_string())
        .chain(quit_args(id))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Polite answers: each bootout stops its helper, the app quits when asked.
fn polite_answers(s: &Setup) {
    let f = fake(&s.mac);
    for (label, pid) in [(MENU_ID, 601), (MONITOR_ID, 602)] {
        f.answer(&bootout_key(label), "");
        s.mac.stops(&bootout_key(label), &[pid]);
    }
    f.answer(&bootout_key("com.example.photostudio5.Updater"), "");
    f.answer(&quit_key(ID), "");
    s.mac.stops(&quit_key(ID), &[600]);
}

#[test]
fn every_helper_inside_the_bundle_is_listed() {
    let s = setup();
    run_everything(&s);
    let st = running_parts(&s.mac.env, &s.app);
    assert!(st.notes.is_empty(), "{:?}", st.notes);
    let by_pid = |pid: u32| st.parts.iter().find(|p| p.pid == Some(pid)).unwrap();
    let app = by_pid(600);
    assert_eq!(app.kind, PartKind::App);
    assert_eq!(app.bundle_id.as_deref(), Some(ID));
    assert_eq!(app.gui.as_deref(), Some("Foreground"));
    assert!(app.job.is_none(), "LaunchServices' own label is not a job");
    assert!(app.locks());
    let menu = by_pid(601);
    assert_eq!(menu.kind, PartKind::HelperApp);
    assert_eq!(menu.name, "the reference app Menu");
    assert!(menu.login_item);
    assert_eq!(menu.job.as_ref().unwrap().label, MENU_ID);
    assert_eq!(menu.job.as_ref().unwrap().domain, JobDomain::Gui);
    assert_eq!(menu.role(), "menu bar helper, opens at login");
    let mon = by_pid(602);
    assert_eq!(
        mon.bundle_id.as_deref(),
        Some(MONITOR_ID),
        "from its Info.plist"
    );
    assert!(mon.locks());
    let daemon = by_pid(700);
    assert!(!daemon.inside_bundle && !daemon.locks());
    assert_eq!(daemon.uid, Some(0));
    assert_eq!(daemon.job.as_ref().unwrap().domain, JobDomain::System);
    assert_eq!(
        daemon.job.as_ref().unwrap().plist.as_deref(),
        Some("/Library/LaunchDaemons/com.example.photostudio5.Agent.plist")
    );
    assert!(daemon.needs_admin(st.me));
    assert_eq!(daemon.role(), "system service");
    // The idle updater job is listed as loaded, not running; nothing of other apps is.
    let upd = st
        .parts
        .iter()
        .find(|p| p.name == "com.example.photostudio5.Updater")
        .unwrap();
    assert!(!upd.running() && upd.kind == PartKind::Job);
    assert!(!st.parts.iter().any(|p| p.pid == Some(900)));
    assert!(!st.parts.iter().any(|p| p.name.contains("apple")));
    assert_eq!(st.parts[0].kind, PartKind::App, "the app comes first");
    assert!(st.locked());
    assert_eq!(
        st.headline(),
        "PhotoStudio_5 and 3 background helpers are running"
    );
    assert!(st.needs_admin());
    assert_eq!(
        st.admin_question().unwrap(),
        format!("{AGENT} runs as the system. Stopping it asks for your administrator password.")
    );
    // The lock agrees.
    assert!(uninstaller::is_running(&s.mac.env, &s.app));
}

#[test]
fn quit_sends_the_right_commands_and_unlocks_the_plan() {
    let s = setup();
    run_everything(&s);
    polite_answers(&s);
    let env = &s.mac.env;
    let plan = uninstaller::uninstall_plan(env, &Settings::default(), &s.app, false).unwrap();
    assert!(plan.items.iter().all(|i| i.locked.is_some() && !i.selected));

    let rep = app_quit::quit_app(env, &s.app, quick()).unwrap();
    let calls = fake(&s.mac).calls();
    // Agents and the login item first (so launchd does not start them again), then the app.
    let pos = |k: &str| calls.iter().position(|c| c == k);
    let menu = pos(&bootout_key(MENU_ID)).expect("menu login item booted out");
    let mon = pos(&bootout_key(MONITOR_ID)).expect("monitor booted out");
    let quit = pos(&quit_key(ID)).expect("app asked to quit");
    assert!(menu < quit && mon < quit, "{calls:#?}");
    assert!(pos(&bootout_key("com.example.photostudio5.Updater")).is_some());
    // The menu helper was stopped by its bootout, so it is not asked twice.
    assert!(pos(&quit_key(MENU_ID)).is_none(), "{calls:#?}");
    // Never a kill, never anything for the other app, never the administrator without a yes.
    assert!(!calls.iter().any(|c| c.contains("kill")), "{calls:#?}");
    assert!(
        !calls
            .iter()
            .any(|c| c.contains("com.example.Notes") && !c.contains("print"))
    );
    assert!(fake(&s.mac).admin_calls().is_empty());
    assert!(rep.done(), "{}", rep.message());
    assert_eq!(rep.admin_needed, vec![AGENT.to_string()]);
    assert!(rep.relaunched.is_empty());
    let msg = rep.message();
    assert!(
        msg.starts_with("PhotoStudio_5 has quit. You can uninstall it now."),
        "{msg}"
    );
    assert!(msg.contains("needs your administrator password"), "{msg}");

    // The next check unlocks the plan: the app and its files named after its id are selected.
    let plan = uninstaller::uninstall_plan(env, &Settings::default(), &s.app, false).unwrap();
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    let app_item = plan.items.iter().find(|i| i.path == s.app).unwrap();
    assert!(app_item.locked.is_none() && app_item.selected);
    assert!(plan.items.iter().any(|i| i.path == s.cache && i.selected));

    // Journal: each bootout with the command that loads it again, the quit as a command.
    let hist = executor::history(env);
    let toggles: Vec<_> = hist
        .iter()
        .filter_map(|r| match &r.kind {
            ActionKind::Toggled { what, undo_command } => {
                Some((what.clone(), undo_command.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(toggles.len(), 3, "{toggles:#?}");
    let (what, undo) = toggles.iter().find(|(w, _)| w.contains(MENU_ID)).unwrap();
    assert!(what.contains("login item of PhotoStudio_5"), "{what}");
    assert_eq!(
        undo.as_deref(),
        Some(
            "/usr/bin/open '/Applications/PhotoStudio_5.app/Contents/Library/LoginItems/PhotoStudio_5 Menu.app'"
        )
    );
    assert!(
        hist.iter()
            .all(|r| r.feature == Some(FeatureId::Uninstaller))
    );
    assert!(hist.iter().any(|r| matches!(&r.kind,
        ActionKind::Command { command, ok: true, .. } if command.starts_with("/usr/bin/osascript") && command.contains(ID))));
    // Quitting moved nothing.
    assert!(s.app.exists() && s.cache.exists() && s.helper_tool.exists());
}

#[test]
fn a_user_agent_plist_is_loaded_again_by_its_undo() {
    let mac = FakeMac::new();
    let app = mac.app_spec(&AppSpec::new("Syncer", "com.example.Syncer", "1.0"));
    mac.user_id(ME);
    let exe = app.join("Contents/Resources/syncd");
    mac.write(&exe, b"bin");
    let plist = mac
        .env
        .home_path("Library/LaunchAgents/com.example.Syncer.daemon.plist");
    mac.write(
        &plist,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>com.example.Syncer.daemon</string>
<key>Program</key><string>/Applications/Syncer.app/Contents/Resources/syncd</string>
<key>KeepAlive</key><true/></dict></plist>"#,
    );
    mac.run_program(&exe, 610, ME);
    mac.launchd(&format!("gui/{ME}"), &[(610, "com.example.Syncer.daemon")]);
    mac.launchd("system", &[]);
    let key = bootout_key("com.example.Syncer.daemon");
    fake(&mac).answer(&key, "");
    mac.stops(&key, &[610]);
    let st = running_parts(&mac.env, &app);
    assert_eq!(st.headline(), "A background helper of Syncer is running");
    assert_eq!(st.parts[0].role(), "background agent");
    let rep = app_quit::quit_app(&mac.env, &app, quick()).unwrap();
    assert!(rep.done(), "{}", rep.message());
    let undo = executor::history(&mac.env)
        .into_iter()
        .find_map(|r| match r.kind {
            ActionKind::Toggled { undo_command, .. } => undo_command,
            _ => None,
        })
        .unwrap();
    assert_eq!(
        undo,
        format!(
            "/bin/launchctl bootstrap gui/{ME} '/Users/tester/Library/LaunchAgents/com.example.Syncer.daemon.plist'"
        )
    );
}

#[test]
fn a_launch_daemon_needs_a_confirmed_administrator_password() {
    let s = setup();
    let m = &s.mac;
    // A system service running as root from INSIDE the bundle (SMAppService daemon): it locks.
    let svc = s
        .app
        .join("Contents/Library/LaunchServices/com.example.photostudio5.Service");
    m.write(&svc, b"bin");
    m.run_program(&svc, 710, 0);
    m.launchd(&format!("gui/{ME}"), &[]);
    m.launchd("system", &[(710, "com.example.photostudio5.Service")]);
    m.window_server(&[]);
    let st = running_parts(&m.env, &s.app);
    assert!(st.locked() && st.needs_admin());
    assert_eq!(
        st.headline(),
        "A background helper of PhotoStudio_5 is running"
    );

    // Without the user's yes: no password dialog, still locked, and the report says why.
    let rep = app_quit::quit_app(&m.env, &s.app, quick()).unwrap();
    assert!(fake(m).admin_calls().is_empty());
    assert!(!rep.done());
    assert_eq!(
        rep.admin_needed,
        vec!["com.example.photostudio5.Service".to_string()]
    );
    assert!(
        rep.message().contains("needs your administrator password"),
        "{}",
        rep.message()
    );

    // Cancelled dialog: nothing stops, said honestly.
    fake(m).with(|st| st.admin_cancelled = true);
    let yes = QuitOptions {
        allow_admin: true,
        ..quick()
    };
    let rep = app_quit::quit_app(&m.env, &s.app, yes).unwrap();
    assert!(rep.admin_cancelled && !rep.done());
    assert!(
        rep.message().contains("password dialog was cancelled"),
        "{}",
        rep.message()
    );

    // Confirmed: one administrator command boots the service out.
    fake(m).with(|st| st.admin_cancelled = false);
    let cmd = "/bin/launchctl bootout 'system/com.example.photostudio5.Service'";
    m.stops(&format!("admin:{cmd}"), &[710]);
    let rep = app_quit::quit_app(&m.env, &s.app, yes).unwrap();
    assert_eq!(fake(m).admin_calls().last().map(String::as_str), Some(cmd));
    assert!(rep.done(), "{}", rep.message());
    let what = executor::history(&m.env)
        .into_iter()
        .find_map(|r| match r.kind {
            ActionKind::Toggled { what, undo_command } if what.contains("system service") => {
                Some((what, undo_command))
            }
            _ => None,
        })
        .unwrap();
    // Its property list lives in the bundle: it starts again at restart, no undo command.
    assert!(
        what.0.contains("starts again at the next restart"),
        "{}",
        what.0
    );
    assert!(what.1.is_none());
}

#[test]
fn still_running_and_relaunched_are_reported_then_force_quit_stops_them() {
    let s = setup();
    run_everything(&s);
    let m = &s.mac;
    let f = fake(m);
    for label in [MENU_ID, MONITOR_ID, "com.example.photostudio5.Updater"] {
        f.answer(&bootout_key(label), "");
    }
    // The monitor stops but comes straight back with a new pid; the menu helper ignores the
    // bootout; the app asks the user something and says no.
    let monitor_again = RunningApp {
        pid: 612,
        name: "HealthMonitor".into(),
        bundle_id: None,
        path: Some(PathBuf::from(m.env.display(&s.monitor_exe))),
    };
    f.on_run(
        &bootout_key(MONITOR_ID),
        FakeEffect {
            stop: vec![602],
            start: vec![monitor_again],
            ..Default::default()
        },
    );
    f.answer_full(
        &quit_key(ID),
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "execution error: PhotoStudio_5 got an error: User canceled. (-128)".into(),
        },
    );
    f.answer(&quit_key(MENU_ID), "");
    // ps for the new pid (the program is the same).
    f.with(|st| {
        let k = cleancraft_core::testkit_quit::ps_key();
        let mut o = st.answers[&k].clone();
        o.stdout.push_str(&format!(
            "   612   {ME} {}\n",
            m.env.display(&s.monitor_exe)
        ));
        st.answers.insert(k, o);
    });
    let rep = app_quit::quit_app(&m.env, &s.app, quick()).unwrap();
    assert!(!rep.done());
    assert!(rep.can_force());
    assert_eq!(rep.relaunched, vec!["HealthMonitor".to_string()]);
    let msg = rep.message();
    assert!(msg.contains("Still running: PhotoStudio_5"), "{msg}");
    assert!(msg.contains("Use Force Quit"), "{msg}");
    assert!(
        msg.contains("HealthMonitor started again by itself"),
        "{msg}"
    );
    assert!(
        msg.contains("PhotoStudio_5 did not quit: it said no"),
        "{msg}"
    );

    // Nothing moves while something runs, whatever the plan says.
    let mut plan =
        uninstaller::uninstall_plan(&m.env, &Settings::default(), &s.app, false).unwrap();
    assert!(plan.items.iter().all(|i| i.locked.is_some()));
    for i in 0..plan.items.len() {
        plan.items[i].locked = None;
        plan.items[i].selected = true;
    }
    let d = plan.digest();
    let err = features::run_plan(&m.env, &Settings::default(), &plan, &d, &mut |_, _| {});
    assert!(err.is_err(), "the run must be refused");
    assert!(s.app.exists() && s.cache.exists());

    // Force Quit: TERM, then KILL for what ignores TERM, only inside the bundle.
    for pid in [600, 601, 612] {
        f.answer(&format!("/bin/kill -TERM {pid}"), "");
        f.answer(&format!("/bin/kill -KILL {pid}"), "");
    }
    m.stops("/bin/kill -TERM 601", &[601]);
    m.stops("/bin/kill -TERM 612", &[612]);
    m.stops("/bin/kill -KILL 600", &[600]);
    let rep = app_quit::force_quit(&m.env, &s.app, quick_force()).unwrap();
    let calls = f.calls();
    for c in [
        "/bin/kill -TERM 600",
        "/bin/kill -TERM 601",
        "/bin/kill -TERM 612",
        "/bin/kill -KILL 600",
    ] {
        assert!(calls.iter().any(|x| x == c), "{c} missing: {calls:#?}");
    }
    assert!(!calls.iter().any(|x| x == "/bin/kill -KILL 601"));
    // The root helper outside the bundle and the other app are never killed.
    assert!(
        !calls
            .iter()
            .any(|x| x.starts_with("/bin/kill") && (x.ends_with(" 700") || x.ends_with(" 900")))
    );
    assert!(f.admin_calls().is_empty());
    assert!(rep.done(), "{}", rep.message());
    assert!(executor::history(&m.env).iter().any(|r| matches!(&r.kind,
        ActionKind::Command { command, .. } if command.starts_with("/bin/kill -KILL 600"))));

    // Now the uninstall goes through, and moves the app to the Trash.
    let plan = uninstaller::uninstall_plan(&m.env, &Settings::default(), &s.app, false).unwrap();
    assert!(plan.items.iter().any(|i| i.path == s.app && i.selected));
    let rep = features::run_plan(
        &m.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(
        rep.moved
            .iter()
            .any(|r| matches!(&r.kind, ActionKind::Moved { original, .. } if original == &s.app))
    );
    assert!(!s.app.exists());
}

#[test]
fn macos_apps_and_cleancraft_are_never_quit() {
    let mac = FakeMac::new();
    let safari = mac.app("Safari", "com.apple.Safari", "17.0");
    mac.user_id(ME);
    mac.run_app(&safari);
    let e = app_quit::quit_app(&mac.env, &safari, quick()).unwrap_err();
    assert!(e.contains("part of macOS"), "{e}");
    let cc = mac.app("CleanCraft", "com.cleancraft.app", "1.0");
    let e = app_quit::force_quit(&mac.env, &cc, quick_force()).unwrap_err();
    assert!(e.contains("does not quit itself"), "{e}");
    assert!(
        fake(&mac)
            .calls()
            .iter()
            .all(|c| !c.contains("osascript") && !c.contains("kill"))
    );
}

#[test]
fn nothing_running_says_so_and_does_nothing() {
    let s = setup();
    s.mac.launchd(&format!("gui/{ME}"), &[]);
    s.mac.launchd("system", &[]);
    let st = running_parts(&s.mac.env, &s.app);
    assert!(st.is_empty() && !st.locked());
    assert_eq!(st.headline(), "");
    let rep = app_quit::quit_app(&s.mac.env, &s.app, quick()).unwrap();
    assert!(rep.done() && rep.stopped.is_empty());
    assert!(
        !fake(&s.mac)
            .calls()
            .iter()
            .any(|c| c.contains("osascript") || c.contains("bootout"))
    );
}

#[test]
fn launchctl_failure_is_a_note_and_the_lock_still_holds() {
    let s = setup();
    s.mac
        .run_program(&s.app.join("Contents/MacOS/PhotoStudio_5"), 600, ME);
    // No launchctl and no lsappinfo answers: the app is still found, from ps and its Info.plist.
    let st = running_parts(&s.mac.env, &s.app);
    assert_eq!(st.notes.len(), 2, "{:?}", st.notes);
    assert!(st.locked());
    assert_eq!(st.parts[0].bundle_id.as_deref(), Some(ID));
    assert_eq!(st.headline(), "PhotoStudio_5 is running");
}

#[test]
fn unreadable_leftover_folders_say_their_size_may_be_bigger() {
    let s = setup();
    let env = &s.mac.env;
    s.mac
        .home_file(&format!("Library/Caches/{ID}/private/blob"), 200_000);
    // The folder macOS keeps private (or another user owns): CleanCraft cannot look inside.
    env.inject_fault(&s.cache.join("private"), Fault::PermissionDenied);
    let plan = uninstaller::uninstall_plan(env, &Settings::default(), &s.app, false).unwrap();
    let cache = plan.items.iter().find(|i| i.path == s.cache).unwrap();
    assert!(uninstaller::size_is_partial(cache), "{}", cache.reason);
    assert!(cache.reason.contains("may be bigger than shown"));
    // What could be read is still counted with the shared formula.
    let readable = cleancraft_core::fsutil::tree_size(&s.cache.join("Cache.db")).0;
    assert!(
        cache.size >= readable && cache.size < readable + 200_000,
        "{}",
        cache.size
    );
    // Fully readable folders are not marked, and the plan has no note (a note blocks the app).
    let prefs = plan
        .items
        .iter()
        .find(|i| i.path.ends_with(format!("{ID}.plist")))
        .unwrap();
    assert!(!uninstaller::size_is_partial(prefs));
    assert!(plan.notes.is_empty());
    env.clear_faults();
    let plan = uninstaller::uninstall_plan(env, &Settings::default(), &s.app, false).unwrap();
    let cache = plan.items.iter().find(|i| i.path == s.cache).unwrap();
    assert!(!uninstaller::size_is_partial(cache));
    assert_eq!(cache.size, cleancraft_core::fsutil::tree_size(&s.cache).0);
}
