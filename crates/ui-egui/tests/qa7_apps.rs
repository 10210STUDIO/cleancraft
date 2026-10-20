//! QA round 7: App Remover error cases by clicks on the real page on a FakeMac.
//! - A13, A14 (single app): a leftover on a full disk stays with the reason, the rest moves,
//!   numbers against a recalculation, Undo puts back what moved.
//! - A13, A14 (several apps): a leftover and a whole app bundle on a full disk; per app
//!   results (uninstalled, kept), every file that stayed with its reason, Undo per app and
//!   Undo All; the action log that cannot be written moves nothing.
//! - A6 (installer packages): one installer on a full disk and one open in an app stay with
//!   the reason, the other moves, Undo puts it back.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::Fault;
use cleancraft_core::executor;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use common::*;
use egui_kittest::kittest::Queryable;

fn go(h: &mut H) {
    click(h, "Apps");
    click(h, "App Remover");
}

fn sizes(v: &[&PathBuf]) -> u64 {
    v.iter().map(|p| tree_size(p).0).sum()
}

/// Click the one widget whose label contains `needle` (group rows carry their counts).
fn click_contains(h: &mut H, needle: &str) {
    let n = h.query_all_by_label_contains(needle).count();
    assert_eq!(n, 1, "'{needle}' matches {n} widgets");
    h.get_by_label_contains(needle).click();
    steps(h);
}

// Single app.

struct One {
    mac: FakeMac,
    editor: PathBuf,
    /// Application Support, Caches, Preferences (preselected).
    sure: Vec<PathBuf>,
}

fn one() -> One {
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        size: 3_000_000,
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.spotlight_finds_nothing();
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    let sure = vec![
        mac.env
            .home_path("Library/Application Support/com.example.Editor"),
        mac.env.home_path("Library/Caches/com.example.Editor"),
        mac.env
            .home_path("Library/Preferences/com.example.Editor.plist"),
    ];
    One { mac, editor, sure }
}

#[test]
fn qa7_a13_a14_single_app_disk_full_keeps_that_file_and_says_so() {
    let m = one();
    let mut h = harness(&m.mac);
    go(&mut h);
    click(&mut h, "Editor");
    let full = m.sure[1].clone();
    m.mac.env.inject_fault(&full, Fault::DiskFull);
    let moved = [&m.editor, &m.sure[0], &m.sure[2]];
    let size = sizes(&moved);
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(full.exists(), "the disk full cache moved");
    for p in moved {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    assert!(has(&h, "the disk is full"));
    assert!(has(&h, &m.mac.env.display(&full)));
    assert!(has_text(&h, "Empty the Trash, then Try Again."));
    assert!(has_text(&h, "Uninstalled. 3 items moved. 1 skipped."));
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    assert_eq!(executor::history(&m.mac.env).len(), 3);
    click(&mut h, "Undo");
    for p in moved {
        assert!(p.exists(), "{} not put back", p.display());
    }
    assert!(full.exists());
    assert!(has_text(&h, "3 items put back."));
}

#[test]
fn qa7_a13_single_app_disk_full_on_the_action_log_moves_nothing() {
    let m = one();
    let mut h = harness(&m.mac);
    go(&mut h);
    click(&mut h, "Editor");
    m.mac
        .env
        .inject_fault(&executor::journal_path(&m.mac.env), Fault::DiskFull);
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(m.editor.exists() && m.sure.iter().all(|p| p.exists()));
    assert!(has_text(&h, "could not write the action log"));
    assert!(executor::history(&m.mac.env).is_empty());
}

// Several apps.

struct Batch {
    mac: FakeMac,
    /// (app, its preselected leftovers): Editor, Viewer, Jotter.
    apps: Vec<(PathBuf, Vec<PathBuf>)>,
}

fn batch() -> Batch {
    let mac = FakeMac::new();
    let editor = mac.app("Editor", "com.example.Editor", "2.0");
    let viewer = mac.app("Viewer", "com.maker.Viewer", "1.0");
    let jotter = mac.app("Jotter", "org.jot.Jotter", "3.1");
    mac.write(
        &viewer.join("Contents/Resources/r.bin"),
        &vec![1u8; 3_000_000],
    );
    mac.write(
        &editor.join("Contents/Resources/r.bin"),
        &vec![1u8; 2_000_000],
    );
    mac.spotlight_finds_nothing();
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Library/Caches/com.maker.Viewer/c", 120_000);
    mac.home_file(
        "Library/Application Support/com.maker.Viewer/db.sqlite",
        40_000,
    );
    mac.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    let h = |r: &str| mac.env.home_path(r);
    let apps = vec![
        (
            editor,
            vec![
                h("Library/Caches/com.example.Editor"),
                h("Library/Preferences/com.example.Editor.plist"),
            ],
        ),
        (
            viewer,
            vec![
                h("Library/Caches/com.maker.Viewer"),
                h("Library/Application Support/com.maker.Viewer"),
            ],
        ),
        (jotter, vec![h("Library/Preferences/org.jot.Jotter.plist")]),
    ];
    Batch { mac, apps }
}

fn all_files(b: &Batch) -> Vec<PathBuf> {
    b.apps
        .iter()
        .flat_map(|(a, l)| std::iter::once(a.clone()).chain(l.iter().cloned()))
        .collect()
}

fn review_three(h: &mut H) {
    go(h);
    for n in ["Editor", "Viewer", "Jotter"] {
        click(h, &format!("Select {n}"));
    }
    click(h, "Review and Uninstall");
}

#[test]
fn qa7_a13_a14_batch_disk_full_per_file_and_per_app_then_undo() {
    let b = batch();
    let mut h = harness(&b.mac);
    review_three(&mut h);
    let viewer_cache = b.apps[1].1[0].clone();
    let jotter = b.apps[2].0.clone();
    b.mac.env.inject_fault(&viewer_cache, Fault::DiskFull);
    b.mac.env.inject_fault(&jotter, Fault::DiskFull);
    let stays = [viewer_cache.clone(), jotter.clone()];
    let go_paths: Vec<PathBuf> = all_files(&b)
        .into_iter()
        .filter(|p| !stays.contains(p))
        .collect();
    let size: u64 = go_paths.iter().map(|p| tree_size(p).0).sum();
    click(&mut h, "Uninstall 3 Apps");
    click(&mut h, "Move to Trash");
    for p in &stays {
        assert!(p.exists(), "{} moved on a full disk", p.display());
    }
    for p in &go_paths {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    // Per file and per app.
    assert!(has_text(&h, "com.maker.Viewer: the disk is full"));
    assert!(has_text(&h, "Jotter.app: the disk is full"));
    assert!(has_text(
        &h,
        "2 apps uninstalled. 6 items moved. 2 skipped."
    ));
    assert!(has_text(&h, "Kept: the app itself did not move"));
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    assert_eq!(executor::history(&b.mac.env).len(), 6);
    // Undo Viewer only: its moved files come back, the others stay in the Trash.
    b.mac.env.clear_faults();
    click(&mut h, "Undo Viewer");
    assert!(b.apps[1].0.exists() && b.apps[1].1.iter().all(|p| p.exists()));
    assert!(!b.apps[0].0.exists(), "Editor came back with Viewer");
    // Undo All puts back the rest.
    click(&mut h, "Undo All");
    for p in all_files(&b) {
        assert!(p.exists(), "{} not put back", p.display());
    }
}

#[test]
fn qa7_a13_batch_disk_full_on_the_action_log_moves_nothing() {
    let b = batch();
    let mut h = harness(&b.mac);
    review_three(&mut h);
    b.mac
        .env
        .inject_fault(&executor::journal_path(&b.mac.env), Fault::DiskFull);
    click(&mut h, "Uninstall 3 Apps");
    click(&mut h, "Move to Trash");
    assert!(all_files(&b).iter().all(|p| p.exists()));
    assert!(has_text(&h, "could not write the action log"));
    assert!(executor::history(&b.mac.env).is_empty());
}

// Installer packages.

fn shown(m: &FakeMac, p: &Path) -> String {
    m.env.display(p)
}

#[test]
fn qa7_a6_installer_errors_are_shown_the_rest_moves_and_undo() {
    let mac = FakeMac::new();
    mac.spotlight_finds_nothing();
    let pkg = mac.home_file("Downloads/Tool-1.0.pkg", 80_000);
    let dmg = mac.home_file("Downloads/Viewer-2.dmg", 150_000);
    let open = mac.home_file("Downloads/Setup.pkg", 60_000);
    let mut h = harness(&mac);
    go(&mut h);
    let all = tree_size(&pkg).0 + tree_size(&dmg).0 + tree_size(&open).0;
    assert!(has_text(
        &h,
        &format!("Installers  3  ·  {}", human_size(all))
    ));
    click_contains(&mut h, "Installers  ");
    for p in [&pkg, &dmg, &open] {
        click(&mut h, &shown(&mac, p));
    }
    assert!(has(&h, &format!("3 selected, {}", human_size(all))));
    mac.env.inject_fault(&pkg, Fault::DiskFull);
    mac.env.fake_sys().unwrap().set_open(&open);
    click(&mut h, "Remove Selected");
    assert!(has_text(&h, "Move 3 items"));
    click(&mut h, "Move to Trash");
    assert!(pkg.exists() && open.exists());
    assert!(!dmg.exists());
    assert!(mac.env.user_trash().join("Viewer-2.dmg").exists());
    assert!(has(&h, "the disk is full"));
    assert!(has(&h, "open in a running app"));
    assert!(has_text(&h, "1 item moved. 2 skipped."));
    assert!(has(
        &h,
        &format!(
            "{} moved to the Trash",
            human_size(tree_size(&mac.env.user_trash().join("Viewer-2.dmg")).0)
        )
    ));
    click(&mut h, "Undo");
    assert!(dmg.exists() && pkg.exists() && open.exists());
    assert!(has_text(&h, "1 item put back."));
}
