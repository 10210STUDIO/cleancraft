//! Round 6 safety review (safety6) of apps6's batch uninstall, on a FakeMac. Kept apart from
//! review_safety6.rs until agent6/apps6 merges with watch6 (both changed uninstaller.rs).
//! Run: `cargo test -p cleancraft-core --test review_safety6_apps -- --include-ignored`.

use std::path::PathBuf;

use cleancraft_core::features::uninstaller;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::testkit_cleanup::set_running;
use cleancraft_core::{Plan, Risk, Settings};

fn s() -> Settings {
    Settings::default()
}

fn sel(p: &Plan) -> Vec<PathBuf> {
    p.selected().map(|i| i.path.clone()).collect()
}

/// A1 (R2): a second copy of Editor stays installed: files named after its id stay selected
/// for nobody, the trashed copy's bundle is the only Editor item selected.
#[test]
fn a1_twin_that_stays_keeps_its_files() {
    let m = FakeMac::new();
    let ed = m.app("Editor", "com.example.Editor", "2.0");
    let twin = m.env.home_path("Applications/Editor.app");
    m.write(
        &twin.join("Contents/Info.plist"),
        &std::fs::read(ed.join("Contents/Info.plist")).unwrap(),
    );
    m.write(&twin.join("Contents/MacOS/Editor"), &[1u8; 1000]);
    let v = m.app("Viewer", "com.maker.Viewer", "1.0");
    m.spotlight_finds_nothing();
    let data = m.home_file("Library/Application Support/com.example.Editor/db", 50_000);
    m.home_file("Library/Caches/com.maker.Viewer/c", 50_000);
    let (p, _) = uninstaller::batch_plan(&m.env, &s(), &[ed.clone(), v]).expect("plan");
    let selected = sel(&p);
    assert!(
        !selected.iter().any(|x| data.starts_with(x)),
        "the twin's data is selected: {selected:?}"
    );
}

/// A2 (R3): a running app in the batch locks only its own rows; the run moves the other app
/// and nothing of the running one.
#[test]
fn a2_running_app_locks_only_itself() {
    let m = FakeMac::new();
    let ed = m.app("Editor", "com.example.Editor", "2.0");
    let v = m.app("Viewer", "com.maker.Viewer", "1.0");
    m.spotlight_finds_nothing();
    let vdata = m.home_file("Library/Caches/com.maker.Viewer/c", 50_000);
    m.home_file("Library/Caches/com.example.Editor/c", 50_000);
    set_running(&m.env, "Viewer", "/Applications/Viewer.app");
    let (p, apps) = uninstaller::batch_plan(&m.env, &s(), &[ed.clone(), v.clone()]).expect("plan");
    let vgroup = uninstaller::group_key(&v);
    assert!(
        p.items
            .iter()
            .filter(|i| i.group.as_deref() == Some(vgroup.as_str()))
            .all(|i| i.locked.is_some() && !i.selected),
        "{:?}",
        p.items
    );
    let _ = uninstaller::run_batch(&m.env, &s(), &p, &apps, &p.digest(), &mut |_, _| {});
    assert!(!ed.exists(), "Editor stays");
    assert!(
        v.exists() && vdata.exists(),
        "something of the running Viewer moved"
    );
}

/// A3 (R4): apps of macOS are refused in a batch.
#[test]
fn a3_apple_apps_refused() {
    let m = FakeMac::new();
    let ed = m.app("Editor", "com.example.Editor", "2.0");
    let sa = m.app("Safari", "com.apple.Safari", "18.0");
    m.spotlight_finds_nothing();
    assert!(uninstaller::batch_plan(&m.env, &s(), &[ed, sa]).is_err());
}

/// A4 (R2): a maker's shared folder used by an app that stays is never selected, whatever the
/// two uninstalled apps of the same maker say.
#[test]
fn a4_shared_maker_folder_stays_unselected() {
    let m = FakeMac::new();
    let a = m.app("Draw", "com.example.Draw", "1.0");
    let b = m.app("Paint", "com.example.Paint", "1.0");
    m.app("Other", "com.example.Other", "1.0");
    m.spotlight_finds_nothing();
    let shared = m.home_file("Library/Application Support/Example/license.dat", 2_000);
    let (p, _) = uninstaller::batch_plan(&m.env, &s(), &[a, b]).expect("plan");
    assert!(
        !sel(&p).iter().any(|x| shared.starts_with(x)),
        "shared folder selected: {:?}",
        sel(&p)
    );
}

/// SAF6-8 S2: an edited uninstall plan (a saved plan is plain JSON; `cleancraft clean --yes`
/// and an agent with "agents may clean" load it and compute its digest themselves) moves
/// another installed app and its settings: `Feature::validate` only checks running apps, so
/// the allow-list (Applications, Library) is the only fence. Expected: each item is the plan's
/// own app or a file `find_leftovers` gives for one of the plan's apps now.
/// A5: an edited batch plan (a saved plan is plain JSON) that adds another app's files, an
/// app that stays, or a file outside every app is refused item by item.
#[test]
fn a5_edited_batch_plan_cannot_take_other_apps_files() {
    let m = FakeMac::new();
    let ed = m.app("Editor", "com.example.Editor", "2.0");
    let keep = m.app("Jotter", "org.jot.Jotter", "3.1");
    m.spotlight_finds_nothing();
    m.home_file("Library/Caches/com.example.Editor/c", 50_000);
    let jot = m.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    let pic = m.home_file("Pictures/holiday.jpg", 2_000);
    let (mut p, apps) =
        uninstaller::batch_plan(&m.env, &s(), std::slice::from_ref(&ed)).expect("plan");
    let cat = p.items[0].category.clone();
    let group = uninstaller::group_key(&ed);
    for x in [jot.clone(), keep.clone(), pic.clone()] {
        let id = p.push(x, 1, &cat, "edited".to_string(), Risk::High, true);
        p.items[id].group = Some(group.clone());
    }
    let rep = uninstaller::run_batch(&m.env, &s(), &p, &apps, &p.digest(), &mut |_, _| {});
    let gone: Vec<_> = [&jot, &keep, &pic]
        .iter()
        .filter(|x| !x.exists())
        .map(|x| x.to_path_buf())
        .collect();
    assert!(
        gone.is_empty(),
        "moved from an edited plan: {gone:?} ({rep:?})"
    );
}

/// The same edit on a one-app uninstall plan (the round 5 path), for comparison.
#[test]
fn a5b_edited_single_plan_cannot_take_other_apps_files() {
    let m = FakeMac::new();
    let ed = m.app("Editor", "com.example.Editor", "2.0");
    let keep = m.app("Jotter", "org.jot.Jotter", "3.1");
    m.spotlight_finds_nothing();
    m.home_file("Library/Caches/com.example.Editor/c", 50_000);
    let jot = m.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    let pic = m.home_file("Pictures/holiday.jpg", 2_000);
    let mut p = uninstaller::uninstall_plan(&m.env, &s(), &ed, false).expect("plan");
    let cat = p
        .items
        .iter()
        .find(|i| i.path != ed)
        .map(|i| i.category.clone())
        .unwrap();
    for x in [jot.clone(), keep.clone(), pic.clone()] {
        p.push(x, 1, &cat, "edited".to_string(), Risk::High, true);
    }
    let d = p.digest();
    let rep = cleancraft_core::features::run_plan(&m.env, &s(), &p, &d, &mut |_, _| {});
    let gone: Vec<_> = [&jot, &keep, &pic]
        .iter()
        .filter(|x| !x.exists())
        .map(|x| x.to_path_buf())
        .collect();
    assert!(
        gone.is_empty(),
        "moved from an edited plan: {gone:?} ({rep:?})"
    );
}
