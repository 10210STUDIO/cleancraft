//! Uninstalling several apps at once (apps6): one batch plan built from the per-app plans,
//! running apps locked per app, apps of macOS refused, one administrator dialog for every
//! app that needs the password, per-app results, Undo All and Undo per app.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, SkipReason};
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::sys::parse_admin_moves;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::{Fault, Plan, Settings};

struct Mac {
    mac: FakeMac,
    editor: PathBuf,
    viewer: PathBuf,
    notes: PathBuf,
    safari: PathBuf,
    /// Files named after each app's id: (app, files).
    sure: Vec<(PathBuf, Vec<PathBuf>)>,
}

fn mac() -> Mac {
    let mac = FakeMac::new();
    let editor = mac.app("Editor", "com.example.Editor", "2.0");
    let viewer = mac.app("Viewer", "com.maker.Viewer", "1.0");
    let notes = mac.app("Jotter", "org.jot.Jotter", "3.1");
    let safari = mac.app("Safari", "com.apple.Safari", "18.0");
    mac.spotlight_finds_nothing();
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Library/Logs/Editor/run.log", 5_000);
    mac.home_file("Library/Caches/com.maker.Viewer/c", 120_000);
    mac.home_file(
        "Library/Application Support/com.maker.Viewer/db.sqlite",
        40_000,
    );
    mac.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    mac.home_file("Library/Caches/com.apple.Safari/x", 1_000);
    let h = |r: &str| mac.env.home_path(r);
    let sure = vec![
        (
            editor.clone(),
            vec![
                h("Library/Caches/com.example.Editor"),
                h("Library/Preferences/com.example.Editor.plist"),
            ],
        ),
        (
            viewer.clone(),
            vec![
                h("Library/Caches/com.maker.Viewer"),
                h("Library/Application Support/com.maker.Viewer"),
            ],
        ),
        (
            notes.clone(),
            vec![h("Library/Preferences/org.jot.Jotter.plist")],
        ),
    ];
    Mac {
        mac,
        editor,
        viewer,
        notes,
        safari,
        sure,
    }
}

fn set() -> Settings {
    Settings::default()
}

fn three(m: &Mac) -> Vec<PathBuf> {
    vec![m.editor.clone(), m.viewer.clone(), m.notes.clone()]
}

fn run(m: &Mac, plan: &Plan, apps: &[uninstaller::BatchApp]) -> executor::Report {
    uninstaller::run_batch(
        &m.mac.env,
        &set(),
        plan,
        apps,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap()
}

fn sel(p: &Plan) -> Vec<(PathBuf, u64)> {
    let mut v: Vec<_> = p.selected().map(|i| (i.path.clone(), i.size)).collect();
    v.sort();
    v
}

#[test]
fn batch_plan_is_the_union_of_the_single_plans() {
    let m = mac();
    let env = &m.mac.env;
    let (plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    assert!(uninstaller::is_batch(&plan));
    assert_eq!(apps.len(), 3);
    let mut all_items = Vec::new();
    let mut all_sel = Vec::new();
    for a in three(&m) {
        let one = uninstaller::uninstall_plan(env, &set(), &a, false).unwrap();
        // Every item of the app's own plan is in the batch, in its group, same size, same
        // preselection, same lock and administrator mark.
        let g = uninstaller::group_key(&a);
        for it in &one.items {
            let b = plan
                .items
                .iter()
                .find(|b| b.path == it.path)
                .unwrap_or_else(|| panic!("{} missing", it.path.display()));
            assert_eq!(b.group.as_deref(), Some(g.as_str()));
            assert_eq!(
                (b.size, b.selected, &b.category, b.needs_admin, &b.locked),
                (
                    it.size,
                    it.selected,
                    &it.category,
                    it.needs_admin,
                    &it.locked
                )
            );
            all_items.push(it.path.clone());
        }
        all_sel.extend(sel(&one));
    }
    all_sel.sort();
    assert_eq!(sel(&plan), all_sel, "selection is the union");
    assert_eq!(plan.items.len(), all_items.len(), "nothing extra");
    // Ids are positions.
    assert!(plan.items.iter().enumerate().all(|(k, i)| i.id == k));
    // The bundle and the files named after the id are preselected, the name match is not.
    for (app, files) in &m.sure {
        assert!(plan.items.iter().any(|i| &i.path == app && i.selected));
        for f in files {
            assert!(
                plan.items.iter().any(|i| &i.path == f && i.selected),
                "{}",
                f.display()
            );
        }
    }
    let logs = env.home_path("Library/Logs/Editor");
    assert!(plan.items.iter().any(|i| i.path == logs && !i.selected));
    // The bar's per-app sizes add up to the plan's selection.
    let sizes = uninstaller::preselected_sizes(env, &set(), &three(&m));
    assert_eq!(sizes.iter().map(|s| s.1).sum::<u64>(), plan.selected_size());
    // And to a recalculation from disk.
    let disk: u64 = m
        .sure
        .iter()
        .flat_map(|(a, f)| std::iter::once(a).chain(f))
        .map(|p| tree_size(p).0)
        .sum();
    assert_eq!(plan.selected_size(), disk);
}

#[test]
fn apps_of_macos_cannot_be_in_a_batch_even_through_an_edited_plan() {
    let m = mac();
    let env = &m.mac.env;
    let err =
        uninstaller::batch_plan(env, &set(), &[m.editor.clone(), m.safari.clone()]).unwrap_err();
    assert!(err.to_string().contains("Safari is part of macOS"), "{err}");
    // An edited plan: Safari's bundle and its cache pushed in, selected, unlocked.
    let (mut plan, apps) = uninstaller::batch_plan(env, &set(), &[m.editor.clone()]).unwrap();
    for p in [
        m.safari.clone(),
        env.home_path("Library/Caches/com.apple.Safari"),
    ] {
        let id = plan.items.len();
        plan.push(
            p,
            1,
            uninstaller::CAT_APP,
            "edited",
            cleancraft_core::Risk::High,
            true,
        );
        plan.items[id].group = Some(uninstaller::group_key(&m.editor));
    }
    let err = uninstaller::run_batch(env, &set(), &plan, &apps, &plan.digest(), &mut |_, _| {})
        .unwrap_err();
    assert!(err.to_string().contains("part of macOS"), "{err}");
    assert!(m.safari.exists() && m.editor.exists(), "nothing moved");
    assert!(executor::history(env).is_empty());
}

#[test]
fn a_running_app_locks_only_its_own_group() {
    let m = mac();
    let env = &m.mac.env;
    m.mac.run_app(&m.viewer);
    let (plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    let viewer = apps.iter().find(|a| a.path == m.viewer).unwrap();
    assert!(viewer.running);
    assert_eq!(viewer.lock.as_deref(), Some("Quit Viewer first"));
    for it in &plan.items {
        let mine = it.group.as_deref() == Some(viewer.group.as_str());
        assert_eq!(it.locked.is_some(), mine, "{}", it.path.display());
    }
    let rep = run(&m, &plan, &apps);
    assert!(!m.editor.exists() && !m.notes.exists());
    assert!(m.viewer.exists());
    for f in &m.sure[1].1 {
        assert!(f.exists(), "running app's file moved: {}", f.display());
    }
    let res = uninstaller::batch_results(&plan, &apps, &rep);
    assert!(res[0].removed && res[2].removed && !res[1].removed);
    assert!(res[1].moved.is_empty());
}

#[test]
fn an_app_opened_after_the_review_keeps_its_files_and_the_others_go() {
    let m = mac();
    let env = &m.mac.env;
    let (mut plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    // Even its "maybe" file ticked by hand.
    let logs = env.home_path("Library/Logs/Editor");
    let k = plan.items.iter().position(|i| i.path == logs).unwrap();
    plan.set_selected(k, true);
    m.mac.run_app(&m.editor);
    let rep = run(&m, &plan, &apps);
    assert!(m.editor.exists() && logs.exists());
    for f in &m.sure[0].1 {
        assert!(f.exists());
    }
    assert!(!m.viewer.exists() && !m.notes.exists());
    let res = uninstaller::batch_results(&plan, &apps, &rep);
    assert!(!res[0].removed);
    assert_eq!(res[0].skipped.len(), 4, "{:?}", res[0].skipped);
    assert!(
        res[0]
            .skipped
            .iter()
            .all(|(_, w)| matches!(w, SkipReason::Refused(s) if s == "Quit Editor first"))
    );
    // An edited plan that drops the groups: the files named after the running app's id are
    // still found to be its own.
    let m = mac();
    let env = &m.mac.env;
    let (mut plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    for it in plan.items.iter_mut() {
        it.group = None;
    }
    m.mac.run_app(&m.editor);
    run(&m, &plan, &apps);
    assert!(m.editor.exists());
    for f in &m.sure[0].1 {
        assert!(f.exists(), "{} moved", f.display());
    }
    assert!(!m.viewer.exists());
}

#[test]
fn apps_needing_the_password_share_one_dialog() {
    let m = mac();
    let env = &m.mac.env;
    // Installed by a package: the bundles belong to the system.
    env.inject_fault(&m.editor, Fault::PermissionDenied);
    env.inject_fault(&m.notes, Fault::PermissionDenied);
    let (plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    assert_eq!(plan.selected_admin_count(), 2);
    assert_eq!(features::admin_count(&plan), 2);
    let rep = run(&m, &plan, &apps);
    let calls = env.fake_sys().unwrap().admin_calls();
    assert_eq!(calls.len(), 1, "one password dialog: {calls:?}");
    let froms: Vec<String> = parse_admin_moves(&calls[0])
        .unwrap()
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    assert_eq!(froms.len(), 2);
    assert!(!m.editor.exists() && !m.notes.exists() && !m.viewer.exists());
    let res = uninstaller::batch_results(&plan, &apps, &rep);
    assert!(res.iter().all(|r| r.removed));
    // Undo All: the administrator moves go back with one more dialog.
    let (n, errs) = executor::undo_plan(env, &plan.id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, rep.moved.len());
    assert_eq!(env.fake_sys().unwrap().admin_calls().len(), 2);
    assert!(m.editor.exists() && m.notes.exists() && m.viewer.exists());
}

#[test]
fn undo_one_app_then_the_rest() {
    let m = mac();
    let env = &m.mac.env;
    let (plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    let rep = run(&m, &plan, &apps);
    assert_eq!(rep.moved.len(), 3 + 2 + 2 + 1);
    let res = uninstaller::batch_results(&plan, &apps, &rep);
    let sizes: u64 = res.iter().map(|r| r.moved_size).sum();
    assert_eq!(sizes, rep.freed);
    // Remembered per app, for History.
    let entries = uninstaller::batch_entries(env, &plan.id);
    assert_eq!(entries.len(), 3);
    assert_eq!(
        uninstaller::batch_app_records(env, &plan.id, &m.viewer).len(),
        3
    );
    let (n, errs) = uninstaller::undo_batch_app(env, &plan.id, &m.viewer);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, 3);
    assert!(m.viewer.exists() && m.sure[1].1.iter().all(|p| p.exists()));
    assert!(!m.editor.exists() && !m.notes.exists());
    assert!(uninstaller::batch_app_records(env, &plan.id, &m.viewer).is_empty());
    // Undo All puts back the rest.
    let (n, errs) = executor::undo_plan(env, &plan.id);
    assert!(errs.is_empty());
    assert_eq!(n, 5);
    for (a, f) in &m.sure {
        assert!(a.exists() && f.iter().all(|p| p.exists()));
    }
}

#[test]
fn permission_denied_and_file_in_use_are_per_item() {
    let m = mac();
    let env = &m.mac.env;
    let (plan, apps) = uninstaller::batch_plan(env, &set(), &three(&m)).unwrap();
    let cache = &m.sure[0].1[0];
    let db = &m.sure[1].1[1];
    env.inject_fault(cache, Fault::PermissionDenied);
    env.fake_sys().unwrap().set_open(&db.join("db.sqlite"));
    let rep = run(&m, &plan, &apps);
    assert!(cache.exists() && db.exists());
    assert!(!m.editor.exists() && !m.viewer.exists() && !m.notes.exists());
    let res = uninstaller::batch_results(&plan, &apps, &rep);
    assert_eq!(
        res[0].skipped,
        vec![(cache.clone(), SkipReason::PermissionDenied)]
    );
    assert_eq!(res[1].skipped.len(), 1);
    assert_eq!(&res[1].skipped[0].0, db);
    assert!(res[2].skipped.is_empty());
    assert!(res.iter().all(|r| r.removed));
}

#[test]
fn a_file_two_apps_list_is_listed_once() {
    let m = FakeMac::new();
    let a = m.app("Studio", "com.vendor.Studio", "1");
    let b = m.app("Studio Lite", "com.vendor.StudioLite", "1");
    m.spotlight_finds_nothing();
    // The maker's shared folder: both plans list it, neither selects it.
    m.home_file("Library/Application Support/vendor/x", 1_000);
    let (plan, _) =
        uninstaller::batch_plan(&m.env, &Settings::default(), &[a.clone(), b.clone()]).unwrap();
    let shared = m.env.home_path("Library/Application Support/vendor");
    let hits: Vec<_> = plan.items.iter().filter(|i| i.path == shared).collect();
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].selected);
    assert_eq!(
        hits[0].group.as_deref(),
        Some(uninstaller::group_key(&a).as_str())
    );
}

#[test]
fn app_sizes_include_the_whole_bundle_and_sort_biggest_first() {
    let m = mac();
    let env = &m.mac.env;
    m.mac.write(
        &m.viewer.join("Contents/Resources/big.bin"),
        &vec![1u8; 5_000_000],
    );
    let list = cleancraft_core::apps::inventory(env, true, &|_| {});
    for a in &list {
        assert_eq!(a.size, tree_size(&a.path).0, "{}", a.name);
    }
    let mut v = list.clone();
    v.sort_by_key(|a| std::cmp::Reverse(a.size));
    assert_eq!(v[0].path, m.viewer);
    let _: &Path = &m.safari;
}
