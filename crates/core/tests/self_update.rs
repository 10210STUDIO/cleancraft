//! CleanCraft's self-update on a fake Mac: the release check (newer, same, older, no
//! network), the 24 hour limit, the safe install (checksum, bundle id, signature, writable
//! folder, cleanup running), the swap with the old app in the Trash and Undo in History, and
//! "Install automatically" off only telling the person.

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::progress::Progress;
use cleancraft_core::self_update::{
    self, Auto, AutoCtx, CHECK_EVERY_SECS, Found, Target, UpdateState,
};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_update::*;
use cleancraft_core::{Fault, Settings};

const CUR: &str = "0.1.2";

fn mac() -> FakeMac {
    let m = FakeMac::new();
    install_app(&m, CUR);
    m
}

fn release(m: &FakeMac, v: &str) -> self_update::Release {
    answer_check(m, &release_json(v, true, "Faster scans.\r\n"));
    match self_update::check_against(&m.env, CUR) {
        Ok(Found::Available { release }) => release,
        other => panic!("{other:?}"),
    }
}

fn target(m: &FakeMac) -> Target {
    Target {
        app: m.env.sys_path("/Applications/CleanCraft.app"),
        current: CUR.into(),
        busy: None,
    }
}

#[test]
fn a_newer_release_is_available_with_its_arm64_dmg_and_notes() {
    let m = mac();
    answer_check(
        &m,
        &release_json("0.1.3", true, "  Faster scans.\r\nFixes.  "),
    );
    let f = self_update::check_against(&m.env, CUR).expect("check");
    let Found::Available { release } = f else {
        panic!("{f:?}")
    };
    assert_eq!(release.version, "0.1.3");
    assert_eq!(release.tag, "v0.1.3");
    assert_eq!(release.dmg_name, "CleanCraft-0.1.3-arm64.dmg");
    assert_eq!(release.sha_url.as_deref(), Some(sha_url("0.1.3").as_str()));
    assert_eq!(release.notes, "Faster scans.\nFixes.");
    let calls = m.env.fake_sys().expect("fake").calls();
    assert_eq!(calls, vec![check_line()]);
    assert!(calls[0].contains("User-Agent: CleanCraft/"));
    assert!(calls[0].contains("--max-time"));
    assert!(!calls[0].contains("Authorization"));
}

#[test]
fn same_and_older_releases_install_nothing() {
    let m = mac();
    answer_check(&m, &release_json(CUR, true, ""));
    let f = self_update::check_against(&m.env, CUR).expect("check");
    assert_eq!(f, Found::UpToDate { latest: CUR.into() });
    assert_eq!(f.message(CUR), "CleanCraft 0.1.2 is the latest version.");

    answer_check(&m, &release_json("0.1.1", true, ""));
    let f = self_update::check_against(&m.env, CUR).expect("check");
    assert_eq!(
        f,
        Found::Ahead {
            latest: "0.1.1".into()
        }
    );
    assert!(
        f.message(CUR)
            .starts_with("You are ahead of the latest release")
    );
    // 0.1.10 is newer than 0.1.9 (numbers, not text).
    answer_check(&m, &release_json("0.1.10", true, ""));
    assert!(matches!(
        self_update::check_against(&m.env, "0.1.9"),
        Ok(Found::Available { .. })
    ));
}

#[test]
fn no_network_is_a_calm_message() {
    let m = mac();
    fail_check(&m);
    let e = self_update::check_against(&m.env, CUR).expect_err("offline");
    assert_eq!(e, "the update server could not be found (no internet?)");
    let s = Settings::default();
    let ctx = AutoCtx {
        now: 1_000_000,
        notify: true,
        install: None,
        current: CUR,
    };
    let a = self_update::auto_check(&m.env, &s, &ctx, &|e| self_update::check_against(e, CUR));
    assert_eq!(
        a,
        Auto::Failed(
            "Could not check for updates (the update server could not be found (no internet?)). CleanCraft will try again later.".into()
        )
    );
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(
        !calls.iter().any(|c| c.contains("osascript")),
        "no notification"
    );
    let st = UpdateState::load(&m.env);
    assert_eq!(st.last_check, Some(1_000_000));
    assert!(st.error.is_some());
}

#[test]
fn automatic_checks_wait_24_hours_and_obey_the_switch() {
    let m = mac();
    answer_check(&m, &release_json(CUR, true, ""));
    let s = Settings::default();
    assert!(s.update_check_auto, "on by default");
    assert!(!s.update_install_auto, "off by default");
    let chk = |e: &cleancraft_core::Env| self_update::check_against(e, CUR);
    let ctx = |now| AutoCtx {
        now,
        notify: true,
        install: None,
        current: CUR,
    };
    let t0 = 2_000_000;
    assert!(matches!(
        self_update::auto_check(&m.env, &s, &ctx(t0), &chk),
        Auto::Current(_)
    ));
    assert_eq!(
        self_update::auto_check(&m.env, &s, &ctx(t0 + 3600), &chk),
        Auto::NotDue
    );
    assert_eq!(
        self_update::auto_check(&m.env, &s, &ctx(t0 + CHECK_EVERY_SECS - 1), &chk),
        Auto::NotDue
    );
    assert!(matches!(
        self_update::auto_check(&m.env, &s, &ctx(t0 + CHECK_EVERY_SECS), &chk),
        Auto::Current(_)
    ));
    let curls = |m: &FakeMac| {
        m.env
            .fake_sys()
            .expect("fake")
            .calls()
            .iter()
            .filter(|c| c.starts_with("/usr/bin/curl"))
            .count()
    };
    assert_eq!(curls(&m), 2);
    let off = Settings {
        update_check_auto: false,
        ..Settings::default()
    };
    assert_eq!(
        self_update::auto_check(&m.env, &off, &ctx(t0 + 10 * CHECK_EVERY_SECS), &chk),
        Auto::Off
    );
    assert_eq!(curls(&m), 2);
}

#[test]
fn with_auto_install_off_a_new_version_is_only_notified_once() {
    let m = mac();
    answer_check(&m, &release_json("0.1.3", true, "Notes"));
    answer_install(&m, &Script::good("0.1.3"));
    let s = Settings::default();
    let chk = |e: &cleancraft_core::Env| self_update::check_against(e, CUR);
    let ctx = |now| AutoCtx {
        now,
        notify: true,
        install: Some(target(&m)),
        current: CUR,
    };
    let a = self_update::auto_check(&m.env, &s, &ctx(5_000_000), &chk);
    assert!(
        matches!(&a, Auto::Told { release, notified: true } if release.version == "0.1.3"),
        "{a:?}"
    );
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains("osascript") && c.contains("A CleanCraft update is available"))
    );
    assert!(
        !calls.iter().any(|c| c.contains("hdiutil")),
        "nothing installed"
    );
    assert!(
        !calls.iter().any(|c| c.contains(" -o ")),
        "nothing downloaded"
    );
    assert_eq!(installed_version(&m).as_deref(), Some(CUR));
    assert!(executor::history(&m.env).is_empty());
    // A day later: the same version is not notified again.
    let a = self_update::auto_check(&m.env, &s, &ctx(5_000_000 + CHECK_EVERY_SECS), &chk);
    assert!(
        matches!(
            a,
            Auto::Told {
                notified: false,
                ..
            }
        ),
        "{a:?}"
    );
    assert_eq!(
        UpdateState::load(&m.env)
            .available(CUR)
            .map(|r| r.version.clone())
            .as_deref(),
        Some("0.1.3")
    );
}

#[test]
fn with_auto_install_on_the_agent_installs_and_notifies() {
    let m = mac();
    answer_check(&m, &release_json("0.1.3", true, "Notes"));
    answer_install(&m, &Script::good("0.1.3"));
    let s = Settings {
        update_install_auto: true,
        ..Settings::default()
    };
    let ctx = AutoCtx {
        now: 5_000_000,
        notify: true,
        install: Some(target(&m)),
        current: CUR,
    };
    let a = self_update::auto_check(&m.env, &s, &ctx, &|e| self_update::check_against(e, CUR));
    assert!(matches!(&a, Auto::Installed(i) if i.to == "0.1.3"), "{a:?}");
    assert_eq!(installed_version(&m).as_deref(), Some("0.1.3"));
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(calls.iter().any(|c| c.contains("CleanCraft was updated")));
}

#[test]
fn install_swaps_the_app_and_the_old_one_goes_to_the_trash_with_undo() {
    let m = mac();
    let fake = m.env.fake_sys().expect("fake");
    // The menu bar agent is on: it restarts on the new version.
    let plist = cleancraft_core::login_agent::plist_path(&m.env);
    std::fs::create_dir_all(plist.parent().expect("dir")).expect("dir");
    std::fs::write(&plist, "x").expect("plist");
    let rel = release(&m, "0.1.3");
    answer_install(&m, &Script::good("0.1.3"));
    let meter = Progress::new();
    let done = self_update::install(&m.env, &rel, &target(&m), &meter).expect("install");
    let app = m.env.sys_path("/Applications/CleanCraft.app");
    assert_eq!(installed_version(&m).as_deref(), Some("0.1.3"));
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/MacOS/CleanCraft")).expect("exe"),
        "CleanCraft 0.1.3"
    );
    let old = m.env.user_trash().join("CleanCraft 0.1.2.app");
    assert_eq!(done.old_in_trash, old);
    assert_eq!(
        self_update::bundle_info(&old).map(|(_, v)| v).as_deref(),
        Some(CUR),
        "the old version is whole in the Trash"
    );
    assert!(done.agent_restarted);
    assert!(meter.fraction().is_some_and(|f| f >= 0.74), "{meter:?}");
    // No staging copy left next to the app.
    let left: Vec<_> = std::fs::read_dir(m.env.sys_path("/Applications"))
        .expect("dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["CleanCraft.app".to_string()]);

    let calls = fake.calls();
    let pos = |needle: &str| {
        calls
            .iter()
            .position(|c| c.contains(needle))
            .unwrap_or_else(|| panic!("no {needle} in {calls:#?}"))
    };
    assert!(pos("hdiutil attach -nobrowse -readonly") < pos("codesign --verify --deep --strict"));
    assert!(pos("/usr/bin/ditto") < pos("hdiutil detach"));
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("/usr/bin/xattr -dr com.apple.quarantine")
                && c.contains(".CleanCraft-0.1.3-update.app"))
    );
    let uid = cleancraft_core::features::trash::uid(&m.env);
    assert!(calls.contains(&format!(
        "/bin/launchctl kickstart -k gui/{uid}/{}",
        cleancraft_core::login_agent::LABEL
    )));

    // History: one line with Undo; Undo runs a command that puts the old version back.
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 1);
    let ActionKind::Toggled { what, undo_command } = &hist[0].kind else {
        panic!("{hist:?}")
    };
    assert!(
        what.starts_with("Updated CleanCraft from 0.1.2 to 0.1.3"),
        "{what}"
    );
    let cmd = undo_command.clone().expect("undo");
    assert!(cmd.contains(&format!(
        "/bin/mv -n -- '{}' '{}'",
        old.display(),
        app.display()
    )));
    assert!(cmd.contains(&format!(
        "'{}'",
        m.env.user_trash().join("CleanCraft 0.1.3.app").display()
    )));
    fake.answer("sh", "");
    executor::undo(&m.env, &hist[0].id).expect("undo");
    assert!(
        fake.calls()
            .last()
            .is_some_and(|c| c == &format!("/bin/sh -c {cmd}"))
    );
    assert!(executor::history(&m.env)[0].undone.is_some());
}

#[test]
fn the_undo_command_really_puts_the_old_version_back() {
    // The shell command itself, run by /bin/sh inside the temporary folder only.
    let m = mac();
    let rel = release(&m, "0.1.3");
    answer_install(&m, &Script::good("0.1.3"));
    self_update::install(&m.env, &rel, &target(&m), &Progress::new()).expect("install");
    let ActionKind::Toggled {
        undo_command: Some(cmd),
        ..
    } = executor::history(&m.env)[0].kind.clone()
    else {
        panic!()
    };
    // Only mkdir and mv on paths inside the fake Mac; nothing else runs.
    assert!(!cmd.contains("launchctl"), "{cmd}");
    let root = m.env.root().to_string_lossy().into_owned();
    assert!(
        cmd.matches('\'').count() >= 8 && cmd.contains(&root),
        "{cmd}"
    );
    let st = std::process::Command::new("/bin/sh")
        .args(["-c", &cmd])
        .status()
        .expect("sh");
    assert!(st.success());
    assert_eq!(installed_version(&m).as_deref(), Some(CUR));
    assert_eq!(
        self_update::bundle_info(&m.env.user_trash().join("CleanCraft 0.1.3.app"))
            .map(|(_, v)| v)
            .as_deref(),
        Some("0.1.3")
    );
}

fn refused(m: &FakeMac, s: &Script, t: &Target) -> String {
    let rel = release(m, "0.1.3");
    answer_install(m, s);
    let e = self_update::install(&m.env, &rel, t, &Progress::new()).expect_err("refused");
    assert_eq!(installed_version(m).as_deref(), Some(CUR), "{e}");
    assert!(executor::history(&m.env).is_empty());
    assert!(!m.env.user_trash().join("CleanCraft 0.1.2.app").exists());
    e
}

#[test]
fn a_checksum_mismatch_is_refused() {
    let m = mac();
    let s = Script {
        sha_line: format!("{}  CleanCraft-0.1.3-arm64.dmg\n", "0".repeat(64)),
        ..Script::good("0.1.3")
    };
    let e = refused(&m, &s, &target(&m));
    assert!(e.contains("does not match its checksum"), "{e}");
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(
        !calls.iter().any(|c| c.contains("hdiutil")),
        "never mounted"
    );
}

#[test]
fn a_release_without_checksum_is_refused() {
    let m = mac();
    answer_check(&m, &release_json("0.1.3", false, ""));
    let Ok(Found::Available { release }) = self_update::check_against(&m.env, CUR) else {
        panic!()
    };
    assert_eq!(release.sha_url, None);
    let e =
        self_update::install(&m.env, &release, &target(&m), &Progress::new()).expect_err("no sha");
    assert!(e.contains("has no checksum file"), "{e}");
    assert!(
        m.env.fake_sys().expect("fake").calls().len() == 1,
        "nothing downloaded"
    );
}

#[test]
fn a_wrong_bundle_id_is_refused_and_the_image_is_ejected() {
    let m = mac();
    let s = Script {
        dmg_id: "com.example.other",
        ..Script::good("0.1.3")
    };
    let e = refused(&m, &s, &target(&m));
    assert!(
        e.contains("is not CleanCraft (its id is com.example.other)"),
        "{e}"
    );
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(calls.iter().any(|c| c.contains("hdiutil detach")));
    assert!(!calls.iter().any(|c| c.contains("ditto")));
}

#[test]
fn a_wrong_version_or_bad_signature_is_refused() {
    let m = mac();
    let s = Script {
        dmg_version: "0.1.4",
        ..Script::good("0.1.3")
    };
    let e = refused(&m, &s, &target(&m));
    assert!(e.contains("is version 0.1.4"), "{e}");

    let m = mac();
    let rel = release(&m, "0.1.3");
    let s = Script::good("0.1.3");
    answer_install(&m, &s);
    // answer_install answered codesign with success: fail it again after.
    m.env.fake_sys().expect("fake").answer_full(
        self_update::CODESIGN,
        cleancraft_core::sys::CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "a sealed resource is missing or invalid".into(),
        },
    );
    let e = self_update::install(&m.env, &rel, &target(&m), &Progress::new()).expect_err("sig");
    assert!(e.contains("signature is not valid"), "{e}");
    assert_eq!(installed_version(&m).as_deref(), Some(CUR));
}

#[test]
fn a_running_cleanup_blocks_the_install() {
    let m = mac();
    let t = Target {
        busy: Some("A cleanup is running in System Junk.".into()),
        ..target(&m)
    };
    let e = refused(&m, &Script::good("0.1.3"), &t);
    assert_eq!(
        e,
        "A cleanup is running in System Junk. Wait for it to finish, then install the update."
    );
    let calls = m.env.fake_sys().expect("fake").calls();
    assert_eq!(calls.len(), 1, "only the check ran: {calls:?}");
}

#[test]
fn a_folder_the_user_cannot_change_is_refused_with_what_to_do() {
    let m = mac();
    m.env
        .inject_fault(&m.env.sys_path("/Applications"), Fault::PermissionDenied);
    let e = refused(&m, &Script::good("0.1.3"), &target(&m));
    assert!(
        e.starts_with("CleanCraft cannot replace itself in /Applications"),
        "{e}"
    );
    assert!(
        e.contains("github.com/cristianizzo/cleancraft/releases"),
        "{e}"
    );
}

#[test]
fn an_older_or_same_release_is_never_installed() {
    let m = mac();
    answer_check(&m, &release_json("0.1.3", true, ""));
    let Ok(Found::Available { release }) = self_update::check_against(&m.env, CUR) else {
        panic!()
    };
    let t = Target {
        current: "0.2.0".into(),
        ..target(&m)
    };
    let e = self_update::install(&m.env, &release, &t, &Progress::new()).expect_err("older");
    assert!(e.contains("is not newer than 0.2.0"), "{e}");
}

#[test]
fn relaunch_waits_for_the_app_in_a_detached_helper() {
    let m = mac();
    let app = m.env.sys_path("/Applications/CleanCraft.app");
    self_update::relaunch(&m.env, &app, 4242).expect("spawn");
    let sp = m.env.fake_sys().expect("fake").spawned();
    assert_eq!(
        sp,
        vec![format!(
            "/bin/sh -c {} sh 4242 {}",
            self_update::RELAUNCH_SCRIPT,
            app.display()
        )]
    );
}
