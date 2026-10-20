//! tray6: "Show CleanCraft in the menu bar" (the per-user LaunchAgent) and "open the app at
//! this page" requests, on a fake Mac. Plist content, exact launchctl commands, journal lines
//! with undo, the plist moved to the Trash (never deleted), error paths; open requests read
//! once and only while fresh; one agent at a time.

use std::path::Path;

use cleancraft_core::executor::{self, history};
use cleancraft_core::login_agent::{self, LABEL};
use cleancraft_core::notify::{self, OpenInbox};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{ActionKind, Fault, FeatureId};

const PROGRAM: &str = "/Applications/CleanCraft.app/Contents/Library/LoginItems/CleanCraftAgent.app/Contents/MacOS/CleanCraftAgent";

fn uid(m: &FakeMac) -> u32 {
    cleancraft_core::features::trash::uid(&m.env)
}

#[test]
fn the_plist_starts_the_agent_at_login_and_is_not_kept_alive() {
    let xml = login_agent::plist_xml(
        Path::new(PROGRAM),
        Path::new("/Users/me/Library/Logs/CleanCraft/agent.log"),
    );
    let v = plist::Value::from_reader_xml(xml.as_bytes()).expect("valid plist");
    let d = v.as_dictionary().expect("dict");
    assert_eq!(d.get("Label").and_then(|x| x.as_string()), Some(LABEL));
    let args: Vec<&str> = d
        .get("ProgramArguments")
        .and_then(|x| x.as_array())
        .expect("args")
        .iter()
        .filter_map(|x| x.as_string())
        .collect();
    assert_eq!(args, [PROGRAM, "agent"]);
    assert_eq!(d.get("RunAtLoad").and_then(|x| x.as_boolean()), Some(true));
    assert_eq!(d.get("KeepAlive").and_then(|x| x.as_boolean()), Some(false));
    assert_eq!(
        d.get("LimitLoadToSessionType").and_then(|x| x.as_string()),
        Some("Aqua")
    );
    assert_eq!(
        d.get("StandardErrorPath").and_then(|x| x.as_string()),
        Some("/Users/me/Library/Logs/CleanCraft/agent.log")
    );
    // Awkward characters stay one argument (XML escaped, read back the same).
    let odd = "/Applications/Clean & \"Craft\" <x>.app/Contents/MacOS/CleanCraft";
    let v = plist::Value::from_reader_xml(
        login_agent::plist_xml(Path::new(odd), Path::new("/l")).as_bytes(),
    )
    .expect("valid plist");
    let first = v
        .as_dictionary()
        .and_then(|d| d.get("ProgramArguments"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_string())
        .map(str::to_string);
    assert_eq!(first.as_deref(), Some(odd));
}

#[test]
fn the_agent_program_is_the_helper_app_when_installed() {
    let m = FakeMac::new();
    let exe = m
        .env
        .sys_path("/Applications/CleanCraft.app/Contents/MacOS/CleanCraft");
    // No helper: the app's own executable.
    assert_eq!(login_agent::agent_program(&exe), exe);
    let helper = m
        .env
        .sys_path("/Applications/CleanCraft.app")
        .join(login_agent::HELPER_REL);
    m.write(&helper, b"bin");
    assert_eq!(login_agent::agent_program(&exe), helper);
    // A build outside any app bundle.
    let loose = m.env.sys_path("/opt/cleancraft/cleancraft");
    assert_eq!(login_agent::agent_program(&loose), loose);
}

#[test]
fn turning_on_writes_the_plist_bootstraps_and_journals_with_undo() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("launchctl", "");
    assert!(!login_agent::is_on(&m.env));
    let ch = login_agent::turn_on(&m.env, Path::new(PROGRAM)).expect("on");
    assert!(ch.note.is_none());
    let plist = login_agent::plist_path(&m.env);
    assert_eq!(
        plist,
        m.env
            .home_path("Library/LaunchAgents/com.cristianizzo.cleancraft.agent.plist")
    );
    assert!(login_agent::is_on(&m.env));
    let text = std::fs::read_to_string(&plist).expect("plist");
    assert!(text.contains(PROGRAM));
    let u = uid(&m);
    assert_eq!(
        fake.calls(),
        vec![format!(
            "/bin/launchctl bootstrap gui/{u} {}",
            plist.display()
        )]
    );
    let h = history(&m.env);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].feature, Some(FeatureId::Monitor));
    assert!(h[0].can_undo());
    let ActionKind::Toggled { what, undo_command } = &h[0].kind else {
        panic!("{:?}", h[0].kind)
    };
    assert_eq!(
        what,
        &format!(
            "{} (/Users/tester/Library/LaunchAgents/{LABEL}.plist)",
            login_agent::ON_TEXT
        )
    );
    let trash = m.env.user_trash().join(format!("{LABEL}.plist"));
    assert_eq!(
        undo_command.as_deref(),
        Some(
            format!(
                "/bin/launchctl bootout 'gui/{u}/{LABEL}'; /bin/mkdir -p '{}' && /bin/mv -n -- '{}' '{}'",
                m.env.user_trash().display(),
                plist.display(),
                trash.display()
            )
            .as_str()
        )
    );
    // Undo from History runs exactly that command through the shell.
    fake.answer("sh", "");
    executor::undo(&m.env, &h[0].id).expect("undo");
    assert!(
        fake.calls()
            .last()
            .is_some_and(|c| c.starts_with("/bin/sh -c /bin/launchctl bootout"))
    );
}

#[test]
fn a_bootstrap_failure_keeps_the_login_item_and_says_so() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer_full(
        "launchctl",
        CmdOut {
            status: 5,
            stdout: String::new(),
            stderr: "Bootstrap failed: 5: Input/output error\n".into(),
        },
    );
    let ch = login_agent::turn_on(&m.env, Path::new(PROGRAM)).expect("saved");
    assert_eq!(
        ch.note.as_deref(),
        Some(
            "It starts at your next login; launchctl did not start it now: Bootstrap failed: 5: Input/output error"
        )
    );
    assert!(login_agent::is_on(&m.env));
    assert_eq!(history(&m.env).len(), 1);
}

#[test]
fn turning_off_boots_out_and_moves_the_plist_to_the_trash() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("launchctl", "");
    login_agent::turn_on(&m.env, Path::new(PROGRAM)).expect("on");
    let plist = login_agent::plist_path(&m.env);
    let body = std::fs::read(&plist).expect("plist");
    // Something with the same name is in the Trash already: Finder's next name.
    m.write(&m.env.user_trash().join(format!("{LABEL}.plist")), b"older");
    let ch = login_agent::turn_off(&m.env).expect("off");
    assert!(!login_agent::is_on(&m.env));
    assert!(!plist.exists());
    let stored = m.env.user_trash().join(format!("{LABEL} 2.plist"));
    assert_eq!(
        std::fs::read(&stored).expect("in the Trash"),
        body,
        "moved, not deleted"
    );
    let u = uid(&m);
    assert_eq!(
        fake.calls().last().map(String::as_str),
        Some(format!("/bin/launchctl bootout gui/{u}/{LABEL}").as_str())
    );
    let ActionKind::Toggled { what, undo_command } = &ch.record.kind else {
        panic!()
    };
    assert!(what.starts_with(login_agent::OFF_TEXT), "{what}");
    assert!(what.ends_with("moved to the Trash)"), "{what}");
    assert_eq!(
        undo_command.as_deref(),
        Some(
            format!(
                "/bin/mv -n -- '{}' '{}' && /bin/launchctl bootstrap 'gui/{u}' '{}'",
                stored.display(),
                plist.display(),
                plist.display()
            )
            .as_str()
        )
    );
    assert_eq!(history(&m.env).len(), 2);
    // Off again: refused, nothing run.
    let calls = fake.calls().len();
    assert!(login_agent::turn_off(&m.env).is_err());
    assert_eq!(fake.calls().len(), calls);
    assert_eq!(history(&m.env).len(), 2);
}

#[test]
fn nothing_changes_when_the_action_log_cannot_be_written() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("launchctl", "");
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    let e = login_agent::turn_on(&m.env, Path::new(PROGRAM)).expect_err("refused");
    assert!(
        e.starts_with("Could not write the action log, nothing changed"),
        "{e}"
    );
    assert!(!login_agent::is_on(&m.env));
    assert!(fake.calls().is_empty());
}

#[test]
fn open_requests_run_open_by_bundle_id_and_are_read_once_while_fresh() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("open", "");
    let req = notify::request_open(&m.env, "scan:quick_sweep").expect("open");
    assert_eq!(
        fake.calls(),
        vec!["/usr/bin/open -b com.cristianizzo.cleancraft".to_string()]
    );
    let mut inbox = OpenInbox::default();
    assert_eq!(
        inbox.poll(&m.env, req.time).as_deref(),
        Some("scan:quick_sweep")
    );
    assert_eq!(inbox.poll(&m.env, req.time + 1), None, "handled once");
    // A request older than two minutes is ignored by an app started later.
    let mut late = OpenInbox::default();
    assert_eq!(
        late.poll(&m.env, req.time + notify::REQUEST_MAX_AGE + 1),
        None
    );
    // A damaged file is ignored.
    std::fs::write(notify::request_path(&m.env), b"{not json").expect("write");
    assert_eq!(OpenInbox::default().poll(&m.env, req.time), None);
    // Bad routes are refused before anything runs.
    assert!(notify::request_open(&m.env, "a\nb").is_err());
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn an_app_that_cannot_be_opened_is_reported() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer_full(
        "open",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Unable to find application with bundle identifier com.cristianizzo.cleancraft"
                .into(),
        },
    );
    let e = notify::request_open(&m.env, "settings").expect_err("not installed");
    assert_eq!(
        e,
        "CleanCraft could not be opened: Unable to find application with bundle identifier com.cristianizzo.cleancraft"
    );
}

#[test]
fn one_agent_at_a_time() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    assert!(notify::claim_agent(&m.env, 100).is_ok());
    // Process 100 is a running CleanCraft: a second agent is refused.
    fake.answer("/bin/ps -p 100 -o comm=", "/Applications/CleanCraft.app/Contents/Library/LoginItems/CleanCraftAgent.app/Contents/MacOS/CleanCraftAgent\n");
    let e = notify::claim_agent(&m.env, 200).expect_err("refused");
    assert!(e.contains("already running (process 100)"), "{e}");
    // Process 100 is gone (ps prints nothing, exit 1): the place is free.
    fake.answer_full(
        "/bin/ps -p 100 -o comm=",
        CmdOut {
            status: 1,
            ..CmdOut::default()
        },
    );
    assert!(notify::claim_agent(&m.env, 200).is_ok());
    assert_eq!(
        std::fs::read_to_string(notify::agent_pid_path(&m.env))
            .ok()
            .as_deref(),
        Some("200")
    );
}
