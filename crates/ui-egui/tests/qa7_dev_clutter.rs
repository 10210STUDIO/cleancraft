//! QA round 7: Developer Clutter rows that were driven in core only or without disk full,
//! now by clicks on the real page on a FakeMac.
//! - DV3, DV4: disk full on a Rust `target` and on a `node_modules`: those stay with the
//!   reason and its next step, the others move, the freed size equals a recalculation, Undo
//!   puts back what moved; the action log that cannot be written moves nothing.
//! - DV5: Python environments listed, never preselected, moved only when ticked, Undo.
//! - DV6: Next.js cache, Gradle build and a `.gitignore`d dist moved; a dist that is not
//!   ignored and an active project's dist are never listed.
//! - DV8: a code folder in Documents is listed locked with the reason and the way forward,
//!   cannot be ticked; a Photos library inside a code folder is never walked; the home folder
//!   as a code folder is refused with a note.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::executor;
use cleancraft_core::features::dev_clutter as dev;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId, Plan, Settings};
use common::*;

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

fn rust(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("Cargo.toml"), b"[package]\n");
    m.write(&dir.join("src/main.rs"), b"fn main() {}\n");
    m.write(&dir.join("target/debug/app"), &vec![1u8; 300_000]);
    age_tree(dir, days);
    dir.join("target")
}

fn node(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("package.json"), b"{}");
    m.write(&dir.join("node_modules/a/index.js"), &vec![2u8; 120_000]);
    age_tree(dir, days);
    dir.join("node_modules")
}

fn open_page(h: &mut H) {
    click(h, "Clean");
    click(h, "Developer Clutter");
}

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::DevClutter]
        .plan
        .clone()
        .expect("plan")
}

fn sizes(v: &[&PathBuf]) -> u64 {
    v.iter().map(|p| tree_size(p).0).sum()
}

#[test]
fn qa7_dv3_dv4_disk_full_keeps_those_folders_and_says_so() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Projects");
    let t_full = rust(&mac, &p.join("alpha"), 90);
    let t_ok = rust(&mac, &p.join("bravo"), 90);
    let n_full = node(&mac, &p.join("charlie"), 90);
    let n_ok = node(&mac, &p.join("delta"), 90);
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let pl = plan(&h);
    assert_eq!(pl.selected_count(), 4, "{:?}", pl.items);
    mac.env.inject_fault(&t_full, Fault::DiskFull);
    mac.env.inject_fault(&n_full, Fault::DiskFull);
    let moved = sizes(&[&t_ok, &n_ok]);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(
        t_full.exists() && n_full.exists(),
        "a disk full folder moved"
    );
    assert!(!t_ok.exists() && !n_ok.exists(), "the others stayed");
    for proj in ["alpha", "bravo", "charlie", "delta"] {
        assert!(p.join(proj).exists(), "project {proj} touched");
    }
    assert!(p.join("bravo/Cargo.toml").exists() && p.join("delta/package.json").exists());
    assert_eq!(h.query_all_by_label("the disk is full").count(), 2);
    assert!(has(&h, &mac.env.display(&t_full)));
    assert!(has(&h, &mac.env.display(&n_full)));
    assert!(has_text(&h, "Empty the Trash, then Try Again."));
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(moved))
    ));
    assert!(has(&h, "2 items moved. 2 skipped."));
    let trashed: u64 = cleancraft_core::fsutil::children(&mac.env.user_trash())
        .iter()
        .map(|x| tree_size(x).0)
        .sum();
    assert_eq!(trashed, moved);
    click(&mut h, "Undo");
    assert!(t_ok.exists() && n_ok.exists());
    assert!(t_full.exists() && n_full.exists());
}

use egui_kittest::kittest::Queryable;

#[test]
fn qa7_dv3_dv4_disk_full_on_the_action_log_moves_nothing() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Code");
    let t = rust(&mac, &p.join("alpha"), 90);
    let n = node(&mac, &p.join("beta"), 90);
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(t.exists() && n.exists());
    assert!(has_text(&h, "could not write the action log"));
    assert!(executor::history(&mac.env).is_empty());
}

#[test]
fn qa7_dv5_python_environment_listed_unticked_moves_only_when_ticked() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Projects");
    let target = rust(&mac, &p.join("engine"), 90);
    let py = p.join("tool");
    mac.write(&py.join("main.py"), b"print(1)");
    mac.write(&py.join(".venv/pyvenv.cfg"), b"home = /usr/bin");
    mac.write(&py.join(".venv/lib/site.py"), &vec![5u8; 60_000]);
    // An "env" folder without pyvenv.cfg is not an environment.
    let conf = p.join("cfg/env");
    mac.write(&conf.join("settings.txt"), &vec![6u8; 20_000]);
    age_tree(&p, 90);
    let venv = py.join(".venv");
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let pl = plan(&h);
    let it = pl
        .items
        .iter()
        .find(|i| i.path == venv)
        .expect("venv listed");
    assert_eq!(it.category, dev::CAT_PYTHON);
    assert!(!it.selected, "a Python environment was preselected");
    assert!(it.locked.is_none());
    assert_eq!(it.size, tree_size(&venv).0);
    assert!(
        !pl.items.iter().any(|i| i.path == conf),
        "env without pyvenv.cfg"
    );
    assert!(has_text(&h, "Python environments"));
    assert!(has(&h, &human_size(tree_size(&venv).0)));
    // Clean without ticking it: only the Rust folder moves.
    click(&mut h, "Review and Clean");
    assert_eq!(
        h.query_all_by_label(&mac.env.display(&venv)).count(),
        1,
        "the unticked environment is in the confirmation"
    );
    click(&mut h, "Move to Trash");
    assert!(!target.exists() && venv.exists());
    click(&mut h, "Undo");
    assert!(target.exists());
    // Tick it by hand: it moves, with the right size, and Undo brings it back.
    click(&mut h, "Scan Again");
    click(&mut h, "Select None");
    click(&mut h, ".venv");
    let sz = tree_size(&venv).0;
    assert!(has(&h, &format!("1 selected, {}", human_size(sz))));
    click(&mut h, "Review and Clean");
    assert_eq!(h.query_all_by_label(&mac.env.display(&venv)).count(), 2);
    click(&mut h, "Move to Trash");
    assert!(!venv.exists() && target.exists());
    assert!(py.join("main.py").exists());
    assert!(has(&h, &format!("{} moved to the Trash", human_size(sz))));
    click(&mut h, "Undo");
    assert!(venv.join("pyvenv.cfg").exists());
}

#[test]
fn qa7_dv6_other_build_output_moves_and_unlisted_dist_stays() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Projects");
    let next = p.join("site");
    mac.write(&next.join("package.json"), b"{}");
    mac.write(&next.join(".next/cache/webpack/a.pack"), &vec![1u8; 40_000]);
    mac.write(&next.join(".next/server/page.js"), &vec![1u8; 10_000]);
    let gradle = p.join("android");
    mac.write(&gradle.join("build.gradle"), b"x");
    mac.write(&gradle.join("build/outputs/app.apk"), &vec![2u8; 90_000]);
    let web = p.join("dashboard");
    mac.write(&web.join(".gitignore"), b"# output\n/dist/\n*.log\n");
    mac.write(&web.join("main.ts"), b"x");
    mac.write(&web.join("dist/bundle.js"), &vec![3u8; 25_000]);
    mac.write(&web.join("build/keep.js"), &vec![3u8; 25_000]);
    age_tree(&p, 60);
    // An active project's ignored dist is not even listed.
    let busy = p.join("busy");
    mac.write(&busy.join(".gitignore"), b"dist\n");
    mac.write(&busy.join("dist/a.js"), &vec![4u8; 9_000]);
    let want = [
        next.join(".next/cache"),
        gradle.join("build"),
        web.join("dist"),
    ];
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let pl = plan(&h);
    let mut listed: Vec<PathBuf> = pl.items.iter().map(|i| i.path.clone()).collect();
    listed.sort();
    let mut w = want.to_vec();
    w.sort();
    assert_eq!(listed, w, "{:?}", pl.notes);
    assert!(
        pl.items
            .iter()
            .all(|i| i.category == dev::CAT_BUILD && i.selected)
    );
    let total: u64 = want.iter().map(|x| tree_size(x).0).sum();
    assert!(has(&h, &human_size(total)), "total {}", human_size(total));
    assert!(has_text(&h, "Other build output"));
    click(&mut h, "Review and Clean");
    for x in &want {
        assert_eq!(h.query_all_by_label(&mac.env.display(x)).count(), 2);
    }
    click(&mut h, "Move to Trash");
    for x in &want {
        assert!(!x.exists(), "{} not moved", x.display());
    }
    assert!(
        next.join(".next/server/page.js").exists(),
        "only the cache moves"
    );
    assert!(
        web.join("build/keep.js").exists(),
        "build not in .gitignore moved"
    );
    assert!(
        busy.join("dist/a.js").exists(),
        "active project's dist moved"
    );
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(total))
    ));
    click(&mut h, "Undo");
    assert!(want.iter().all(|x| x.exists()));
}

#[test]
fn qa7_dv8_code_folder_in_documents_is_locked_with_the_way_forward() {
    let mac = FakeMac::new();
    let root = mac.env.home_path("Documents/code");
    let locked = rust(&mac, &root.join("app"), 90);
    // A Photos library inside a normal code folder is never walked.
    let lib = mac.env.home_path("Projects/Pics.photoslibrary/resources");
    let in_lib = rust(&mac, &lib.join("proj"), 90);
    let ok = rust(&mac, &mac.env.home_path("Projects/engine"), 90);
    Settings {
        dev_roots: vec![root.clone(), mac.env.home_path("Projects")],
        ..Settings::default()
    }
    .save(&mac.env)
    .unwrap();
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let pl = plan(&h);
    assert!(
        !pl.items.iter().any(|i| i.path == in_lib),
        "Photos library walked"
    );
    let it = pl.items.iter().find(|i| i.path == locked).expect("listed");
    let why = it.locked.clone().expect("locked");
    assert!(why.contains("the contents of Documents"), "{why}");
    assert!(why.contains(dev::PROTECTED_STEPS), "{why}");
    assert!(!it.selected);
    // Shown with the reason and the way forward.
    assert!(has_text(&h, &format!("(cannot be selected: {why})")));
    // Clicking it and Select All never tick it.
    let n = h.query_all_by_label("target").count();
    assert_eq!(n, 2);
    for k in 0..n {
        click_nth(&mut h, "target", k);
    }
    let pl = plan(&h);
    assert!(!pl.items.iter().find(|i| i.path == locked).unwrap().selected);
    click(&mut h, "Select All");
    let pl = plan(&h);
    assert!(!pl.items.iter().find(|i| i.path == locked).unwrap().selected);
    click(&mut h, "Review and Clean");
    assert_eq!(
        h.query_all_by_label(&mac.env.display(&locked)).count(),
        1,
        "the locked folder is in the confirmation"
    );
    click(&mut h, "Move to Trash");
    assert!(!ok.exists());
    assert!(locked.exists() && in_lib.exists());

    // The home folder as a code folder: refused with a note, nothing listed.
    let mac = FakeMac::new();
    rust(&mac, &mac.env.home_path("Documents/stray"), 90);
    Settings {
        dev_roots: vec![mac.env.home().to_path_buf()],
        ..Settings::default()
    }
    .save(&mac.env)
    .unwrap();
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    assert!(plan(&h).items.is_empty());
    assert!(has(&h, "Nothing to clean here"));
    click(&mut h, "Details");
    assert!(has_text(&h, "holds your whole home folder"));
}
