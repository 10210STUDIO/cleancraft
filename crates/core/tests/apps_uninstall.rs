//! A2, A3, A4: uninstall and reset plans, carried out through features::run_plan.

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::{FeatureId, Settings};

struct Setup {
    mac: FakeMac,
    app: std::path::PathBuf,
    sure: Vec<std::path::PathBuf>,
    maybe: std::path::PathBuf,
    shared: std::path::PathBuf,
    other_app_files: Vec<std::path::PathBuf>,
}

fn setup() -> Setup {
    let mac = FakeMac::new();
    let app = mac.app("Editor", "com.example.Editor", "1.0");
    mac.app("Editor Pro", "com.example.Editor.pro", "1.0");
    let sure = vec![
        mac.home_file("Library/Application Support/com.example.Editor/db", 40_000),
        mac.home_file("Library/Caches/com.example.Editor/c", 90_000),
        mac.home_file("Library/Preferences/com.example.Editor.plist", 2_000),
        mac.home_file("Library/Containers/com.example.Editor/Data/x", 10_000),
        mac.home_file(
            "Library/Group Containers/ABCDE12345.com.example.Editor/y",
            5_000,
        ),
        mac.home_file(
            "Library/Saved Application State/com.example.Editor.savedState/w",
            1_000,
        ),
        mac.home_file("Library/HTTPStorages/com.example.Editor/h", 1_000),
        mac.home_file("Library/WebKit/com.example.Editor/k", 1_000),
        mac.home_file("Library/Cookies/com.example.Editor.binarycookies", 1_000),
        mac.home_file("Library/LaunchAgents/com.example.Editor.helper.plist", 500),
    ];
    let maybe = mac.home_file("Library/Logs/Editor/run.log", 3_000);
    let shared = mac.home_file("Library/Application Support/Example/shared.db", 8_000);
    let other_app_files = vec![
        mac.home_file("Library/Caches/com.example.Editor.pro/c", 7_000),
        mac.home_file("Library/Preferences/com.example.Editor.pro.plist", 700),
        mac.home_file("Library/Caches/com.example.EditorX/c", 700),
        mac.home_file("Documents/com.example.Editor/mine.txt", 700),
    ];
    Setup {
        mac,
        app,
        sure: sure.into_iter().map(|p| top_entry(&p)).collect(),
        maybe: top_entry(&maybe),
        shared: top_entry(&shared),
        other_app_files,
    }
}

/// The entry directly inside the Library folder (what the plan proposes).
fn top_entry(p: &std::path::Path) -> std::path::PathBuf {
    let mut cur = p.to_path_buf();
    while let Some(parent) = cur.parent() {
        let gp = parent
            .parent()
            .and_then(|g| g.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        if gp.as_deref() == Some("Library") {
            return cur;
        }
        cur = parent.to_path_buf();
    }
    p.to_path_buf()
}

#[test]
fn plan_finds_leftovers_with_confidence() {
    let s = setup();
    let plan =
        uninstaller::uninstall_plan(&s.mac.env, &Settings::default(), &s.app, false).unwrap();
    let item = |p: &std::path::Path| plan.items.iter().find(|i| i.path == p);
    let bundle = item(&s.app).expect("bundle");
    assert!(bundle.selected);
    assert_eq!(bundle.size, tree_size(&s.app).0);
    for p in &s.sure {
        let it = item(p).unwrap_or_else(|| panic!("{} not found", p.display()));
        assert!(it.selected, "{} should be selected", p.display());
        assert_eq!(it.category, uninstaller::CAT_HIGH);
        assert_eq!(it.size, tree_size(p).0);
    }
    let m = item(&s.maybe).expect("name match");
    assert!(!m.selected);
    assert_eq!(m.category, uninstaller::CAT_MEDIUM);
    let sh = item(&s.shared).expect("vendor folder");
    assert!(!sh.selected);
    assert_eq!(sh.category, uninstaller::CAT_SHARED);
    for p in &s.other_app_files {
        assert!(
            !plan.items.iter().any(|i| p.starts_with(&i.path)),
            "{} must not be proposed",
            p.display()
        );
    }
    assert_eq!(plan.items.len(), 1 + s.sure.len() + 2);
}

#[test]
fn uninstall_moves_bundle_and_selected_leftovers_and_undo_restores() {
    let s = setup();
    let env = &s.mac.env;
    let settings = Settings::default();
    let plan = uninstaller::uninstall_plan(env, &settings, &s.app, false).unwrap();
    let expect: u64 = plan.selected().map(|i| tree_size(&i.path).0).sum();
    let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert!(!s.app.exists());
    for p in &s.sure {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    assert!(s.maybe.exists() && s.shared.exists(), "unselected stay");
    for p in &s.other_app_files {
        assert!(p.exists());
    }
    assert_eq!(rep.freed, expect);
    assert!(env.user_trash().join("Editor.app").exists());
    let hist = executor::history(env);
    assert_eq!(hist.len(), 1 + s.sure.len());
    assert!(
        hist.iter()
            .all(|r| r.feature == Some(FeatureId::Uninstaller)
                && matches!(r.kind, ActionKind::Moved { .. }))
    );
    let (n, errs) = executor::undo_plan(env, &plan.id);
    assert!(errs.is_empty());
    assert_eq!(n, 1 + s.sure.len());
    assert!(s.app.join("Contents/Info.plist").exists());
    for p in &s.sure {
        assert!(p.exists(), "{} not restored", p.display());
    }
}

#[test]
fn reset_keeps_the_app() {
    let s = setup();
    let env = &s.mac.env;
    let settings = Settings::default();
    let plan = uninstaller::uninstall_plan(env, &settings, &s.app, true).unwrap();
    let b = plan.items.iter().find(|i| i.path == s.app).unwrap();
    assert!(!b.selected);
    assert!(b.locked.as_deref().unwrap().contains("keeps the app"));
    let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), s.sure.len());
    assert!(s.app.exists());
    for p in &s.sure {
        assert!(!p.exists());
    }
}

#[test]
fn running_app_is_locked_and_refused() {
    let s = setup();
    let env = &s.mac.env;
    let settings = Settings::default();
    s.mac.run_app(&s.app);
    let plan = uninstaller::uninstall_plan(env, &settings, &s.app, false).unwrap();
    assert_eq!(plan.selected_count(), 0);
    assert!(
        plan.items
            .iter()
            .all(|i| i.locked.as_deref() == Some("Quit Editor first"))
    );
    // Started while it was closed, then the app was opened: nothing moves.
    let s2 = setup();
    let plan2 = uninstaller::uninstall_plan(&s2.mac.env, &settings, &s2.app, false).unwrap();
    s2.mac.run_app(&s2.app);
    let err = features::run_plan(
        &s2.mac.env,
        &settings,
        &plan2,
        &plan2.digest(),
        &mut |_, _| {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("Quit Editor first"), "{err}");
    assert!(s2.app.exists());
    for p in &s2.sure {
        assert!(p.exists());
    }
    assert!(executor::history(&s2.mac.env).is_empty());
}

#[test]
fn apple_apps_are_locked_and_refused() {
    let mac = FakeMac::new();
    let safari = mac.app("Safari", "com.apple.Safari", "18.0");
    mac.home_file("Library/Caches/com.apple.Safari/x", 1000);
    let settings = Settings::default();
    let mut plan = uninstaller::uninstall_plan(&mac.env, &settings, &safari, false).unwrap();
    assert_eq!(plan.selected_count(), 0);
    assert!(plan.items.iter().all(|i| i.locked.is_some()));
    // Even a tampered plan is refused.
    plan.items[0].locked = None;
    plan.items[0].selected = true;
    let err =
        features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap_err();
    assert!(err.to_string().contains("part of macOS"));
    assert!(safari.exists());
    let sys = mac.app_spec(&AppSpec {
        dir: "/System/Applications",
        ..AppSpec::new("Chess", "com.thirdparty.Chess", "1")
    });
    let plan = uninstaller::uninstall_plan(&mac.env, &settings, &sys, false).unwrap();
    assert_eq!(plan.selected_count(), 0);
}

#[test]
fn ignore_list_and_errors() {
    let s = setup();
    let env = &s.mac.env;
    let settings = Settings {
        ignore: vec![s.sure[1].clone()],
        ..Default::default()
    };
    let plan = uninstaller::uninstall_plan(env, &settings, &s.app, false).unwrap();
    assert!(!plan.items.iter().any(|i| i.path == s.sure[1]), "ignored");
    env.inject_fault(&s.sure[0], cleancraft_core::Fault::PermissionDenied);
    env.fake_sys().unwrap().set_open(&s.sure[2]);
    let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.skipped.len(), 2);
    assert!(s.sure[0].exists() && s.sure[2].exists());
    assert!(!s.app.exists());
    assert!(
        uninstaller::uninstall_plan(
            env,
            &settings,
            &env.sys_path("/Applications/Nope.app"),
            false
        )
        .is_err()
    );
}
