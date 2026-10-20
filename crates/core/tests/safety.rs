//! The safety core, end to end on a fake Mac: scan, confirm, move, log, undo, and every error
//! path. Run: `cargo test -p cleancraft-core --test safety`.

use std::fs;
use std::path::PathBuf;

use cleancraft_core::executor::{self, ExecError, UndoError, execute, history, undo, undo_plan};
use cleancraft_core::features::{self, allow_roots};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::sys::RunningApp;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{ActionKind, Fault, FeatureId, Method, Plan, Risk, Settings, SkipReason};

fn junk_plan(m: &FakeMac) -> Plan {
    features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap()
}

fn run(m: &FakeMac, plan: &Plan) -> executor::Report {
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    execute(
        &m.env,
        plan,
        &allow,
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .unwrap()
}

#[test]
fn scan_lists_exact_sizes_and_never_protected_places() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    for it in &plan.items {
        assert_eq!(
            it.size,
            tree_size(&it.path).0,
            "size of {}",
            it.path.display()
        );
        assert!(!it.reason.is_empty());
        let p = it.path.to_string_lossy();
        assert!(!p.contains("/Documents") && !p.contains("/Desktop"), "{p}");
        assert!(!p.ends_with("com.apple.bird"), "iCloud cache proposed");
    }
    let browser = m.env.home_path("Library/Caches/com.example.Browser");
    assert!(plan.items.iter().any(|i| i.path == browser));
    assert_eq!(
        plan.total_size(),
        plan.items.iter().map(|i| tree_size(&i.path).0).sum::<u64>()
    );
}

#[test]
fn clean_moves_to_trash_and_undo_restores() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let before: Vec<(PathBuf, u64)> = plan.selected().map(|i| (i.path.clone(), i.size)).collect();
    assert!(!before.is_empty());
    let report = run(&m, &plan);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.moved.len(), before.len());
    assert_eq!(report.freed, before.iter().map(|(_, s)| s).sum::<u64>());
    for (p, _) in &before {
        assert!(!p.exists(), "{} still there", p.display());
    }
    for rec in &report.moved {
        let ActionKind::Moved { stored, .. } = &rec.kind else {
            panic!()
        };
        assert!(
            stored.starts_with(m.env.user_trash()),
            "{}",
            stored.display()
        );
        assert!(stored.exists());
    }
    // The log has one line per move.
    assert_eq!(history(&m.env).len(), before.len());
    // Undo the whole cleanup.
    let (ok, errs) = undo_plan(&m.env, &plan.id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(ok, before.len());
    for (p, s) in &before {
        assert_eq!(tree_size(p).0, *s, "{} not restored intact", p.display());
    }
    assert!(history(&m.env).iter().all(|r| r.undone.is_some()));
    // Undo twice is refused.
    assert_eq!(
        undo(&m.env, &report.moved[0].id),
        Err(UndoError::AlreadyUndone)
    );
}

#[test]
fn quarantine_method() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let r = execute(
        &m.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Quarantine,
        &mut |_, _| {},
    )
    .unwrap();
    assert!(!r.moved.is_empty());
    for rec in &r.moved {
        let ActionKind::Moved { stored, method, .. } = &rec.kind else {
            panic!()
        };
        assert_eq!(*method, Method::Quarantine);
        assert!(stored.starts_with(m.env.quarantine_dir()));
    }
}

#[test]
fn changed_list_is_refused() {
    let m = FakeMac::sample();
    let mut plan = junk_plan(&m);
    let digest = plan.digest();
    plan.set_selected(0, !plan.items[0].selected);
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let r = execute(
        &m.env,
        &plan,
        &allow,
        &digest,
        Method::Trash,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(ExecError::DigestMismatch)));
    assert!(
        plan.items.iter().all(|i| i.path.exists()),
        "nothing may move"
    );
}

#[test]
fn forged_items_outside_allow_list_are_refused() {
    let m = FakeMac::sample();
    let mut plan = junk_plan(&m);
    let doc = m.env.home_path("Documents/report.pdf");
    let outside = m.home_file("Projects/code.rs", 1000);
    plan.push(doc.clone(), 1, "user_caches", "forged", Risk::Low, true);
    plan.push(outside.clone(), 1, "user_caches", "forged", Risk::Low, true);
    plan.push(
        PathBuf::from("/etc/hosts"),
        1,
        "user_caches",
        "forged",
        Risk::Low,
        true,
    );
    let r = run(&m, &plan);
    assert!(doc.exists() && outside.exists());
    let refused: Vec<_> = r
        .skipped
        .iter()
        .filter(|(_, s)| matches!(s, SkipReason::Refused(_)))
        .collect();
    assert_eq!(refused.len(), 3, "{:?}", r.skipped);
}

#[test]
fn open_files_and_running_apps_are_skipped() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let open = m
        .env
        .home_path("Library/Caches/com.example.Browser/Cache.db");
    let sys = m.env.fake_sys().unwrap();
    sys.set_open(&open);
    let r = run(&m, &plan);
    let browser = m.env.home_path("Library/Caches/com.example.Browser");
    assert!(browser.exists());
    assert!(
        r.skipped
            .iter()
            .any(|(p, s)| *p == browser && *s == SkipReason::InUse)
    );
    assert!(!r.moved.is_empty(), "other items still cleaned");

    // An app that is running cannot be moved.
    let m2 = FakeMac::sample();
    let app = m2.app("Editor", "com.example.Editor", "1.0");
    let mut plan2 = Plan::new(FeatureId::Uninstaller);
    plan2.push(app.clone(), 1, "apps", "r", Risk::High, true);
    m2.env.fake_sys().unwrap().with(|s| {
        s.running.push(RunningApp {
            pid: 9,
            name: "Editor".into(),
            bundle_id: None,
            path: Some(PathBuf::from("/Applications/Editor.app")),
        })
    });
    let allow = vec![m2.env.sys_path("/Applications")];
    let r2 = execute(
        &m2.env,
        &plan2,
        &allow,
        &plan2.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .unwrap();
    assert!(
        matches!(r2.skipped[0].1, SkipReason::AppRunning(_)),
        "{:?}",
        r2.skipped
    );
    assert!(app.exists());
}

#[test]
fn permission_denied_and_disk_full_are_reported() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let browser = m.env.home_path("Library/Caches/com.example.Browser");
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    m.env.inject_fault(&browser, Fault::PermissionDenied);
    m.env.inject_fault(&editor, Fault::DiskFull);
    let r = run(&m, &plan);
    assert!(
        r.skipped
            .contains(&(browser.clone(), SkipReason::PermissionDenied))
    );
    assert!(r.skipped.contains(&(editor.clone(), SkipReason::DiskFull)));
    assert!(browser.exists() && editor.exists());
}

#[test]
fn nothing_moves_when_the_log_cannot_be_written() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let r = execute(
        &m.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(ExecError::Journal(_))));
    assert!(plan.items.iter().all(|i| i.path.exists()));
}

#[test]
fn locked_system_folders_are_listed_not_selected() {
    let m = FakeMac::sample();
    m.env
        .inject_fault(&m.env.sys_path("/Library/Caches"), Fault::PermissionDenied);
    let plan = junk_plan(&m);
    let sys_items: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.category == "system_caches")
        .collect();
    assert!(!sys_items.is_empty());
    // FIX-1: marked for the administrator password instead of locked, never preselected.
    assert!(sys_items.iter().all(|i| i.needs_admin && !i.selected));
}

#[test]
fn undo_refuses_when_something_took_the_place() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let r = run(&m, &plan);
    let rec = &r.moved[0];
    let ActionKind::Moved { original, .. } = &rec.kind else {
        panic!()
    };
    fs::create_dir_all(original).unwrap();
    assert!(matches!(
        undo(&m.env, &rec.id),
        Err(UndoError::OriginalTaken(_))
    ));
}

#[test]
fn trash_names_do_not_collide() {
    let m = FakeMac::sample();
    m.home_file(".Trash/com.example.Browser", 10);
    let plan = junk_plan(&m);
    let r = run(&m, &plan);
    let stored: Vec<PathBuf> = r
        .moved
        .iter()
        .filter_map(|rec| match &rec.kind {
            ActionKind::Moved { stored, .. } => Some(stored.clone()),
            _ => None,
        })
        .collect();
    assert!(
        stored.iter().any(|p| p.ends_with("com.example.Browser 2")),
        "{stored:?}"
    );
    assert!(
        m.env.user_trash().join("com.example.Browser").is_file(),
        "existing Trash item untouched"
    );
}

#[test]
fn ignore_list_is_honoured() {
    let m = FakeMac::sample();
    let mut s = Settings::default();
    s.ignore
        .push(m.env.home_path("Library/Caches/com.example.Browser"));
    let plan = features::scan(&m.env, &s, FeatureId::SystemJunk).unwrap();
    assert!(
        !plan
            .items
            .iter()
            .any(|i| i.path.ends_with("com.example.Browser"))
    );
}

/// REVIEW-U U9: a file open in an app says which app, so the user knows what to quit.
#[test]
fn a_file_open_in_an_app_names_the_app() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let held = plan.items[0].path.clone();
    let inner = if held.is_dir() {
        let f = held.join("in-use.db");
        m.write(&f, b"x");
        f
    } else {
        held.clone()
    };
    let plan = junk_plan(&m);
    m.env.fake_sys().unwrap().set_open_by(&inner, "Safari");
    let r = run(&m, &plan);
    let why = r
        .skipped
        .iter()
        .find(|(p, _)| *p == held)
        .map(|(_, w)| w.clone());
    assert_eq!(why, Some(SkipReason::InUseBy("Safari".into())));
    assert_eq!(why.unwrap().to_string(), "open in Safari");
    assert!(held.exists(), "an open file never moves");
}
