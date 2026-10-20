//! QA round 7: error cases of the file tools, by clicks on the real pages on a FakeMac.
//! - C1 Large & Old: disk full on one file; the others move, numbers against a recalculation,
//!   Undo; the action log that cannot be written moves nothing.
//! - C2 Duplicates and C3 Similar Images: permission denied, file in use and disk full on
//!   three selected copies; they stay with their reasons, the kept copy of each set stays,
//!   the other copy moves, Undo puts it back.
//! - D1, D3 Disk Map: disk full on one selected file; the other moves, the map's sizes are
//!   patched to the disk, Undo; the action log that cannot be written moves nothing.

mod common;

use std::path::PathBuf;

use cleancraft_core::executor;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_files::{ImageFormat, save_scene};
use cleancraft_core::{Fault, FeatureId, Plan, Settings};
use common::*;

fn plan(h: &H, f: FeatureId) -> Plan {
    h.state().plans[&f].plan.clone().expect("plan")
}

fn trashed(m: &FakeMac) -> u64 {
    cleancraft_core::fsutil::children(&m.env.user_trash())
        .iter()
        .map(|x| tree_size(x).0)
        .sum()
}

// C1 Large & Old.

fn lo_mac() -> FakeMac {
    let mac = FakeMac::new();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&mac.env)
    .unwrap();
    mac
}

fn open_large_old(h: &mut H) {
    click(h, "Files");
    click(h, "Large & Old Files");
    click(h, "Scan");
}

#[test]
fn qa7_c1_disk_full_keeps_that_file_and_says_so() {
    let mac = lo_mac();
    let a = mac.home_file("Movies/a.mov", 2_200_000);
    let b = mac.home_file("Movies/b.mov", 2_300_000);
    let c = mac.home_file("Movies/c.mov", 2_400_000);
    let mut h = harness(&mac);
    open_large_old(&mut h);
    click(&mut h, "Select All");
    let expect = tree_size(&b).0 + tree_size(&c).0;
    mac.env.inject_fault(&a, Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(a.exists(), "moved on a full disk");
    assert!(!b.exists() && !c.exists());
    assert!(has(&h, "the disk is full"));
    assert!(has(&h, &mac.env.display(&a)));
    assert!(has_text(&h, "Empty the Trash, then Try Again."));
    assert!(has(&h, "2 items moved. 1 skipped."));
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect))
    ));
    assert_eq!(trashed(&mac), expect);
    click(&mut h, "Undo");
    assert!(a.exists() && b.exists() && c.exists());
}

#[test]
fn qa7_c1_disk_full_on_the_action_log_moves_nothing() {
    let mac = lo_mac();
    let a = mac.home_file("Movies/a.mov", 2_200_000);
    let b = mac.home_file("Movies/b.mov", 2_300_000);
    let mut h = harness(&mac);
    open_large_old(&mut h);
    click(&mut h, "Select All");
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(a.exists() && b.exists());
    assert!(has_text(&h, "could not write the action log"));
}

// C2 Duplicates and C3 Similar Images.

fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_add(7);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

/// Puts a permission denied, an in use and a disk full on the first three selected copies,
/// runs, and checks: those three stay with their reasons, every kept copy stays, the fourth
/// moves with the right size, Undo puts it back.
fn three_errors_then_undo(mac: &FakeMac, h: &mut H, f: FeatureId) {
    let p = plan(h, f);
    let sel: Vec<PathBuf> = p.selected().map(|i| i.path.clone()).collect();
    let kept: Vec<PathBuf> = p
        .items
        .iter()
        .filter(|i| !i.selected)
        .map(|i| i.path.clone())
        .collect();
    assert_eq!(sel.len(), 4, "{:?}", p.items);
    // Every set keeps one copy.
    for ids in cleancraft_core::features::duplicates::groups(&p).values() {
        assert_eq!(ids.iter().filter(|i| !p.items[**i].selected).count(), 1);
    }
    mac.env.inject_fault(&sel[0], Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&sel[1]);
    mac.env.inject_fault(&sel[2], Fault::DiskFull);
    let mover = sel[3].clone();
    let size = tree_size(&mover).0;
    click(h, "Review and Clean");
    click(h, "Move to Trash");
    for s in &sel[..3] {
        assert!(s.exists(), "{} moved", s.display());
    }
    assert!(!mover.exists(), "the copy without an error stayed");
    for k in &kept {
        assert!(k.exists(), "kept copy {} moved", k.display());
    }
    assert!(has(h, "permission denied"));
    assert!(has(h, "open in a running app"));
    assert!(has(h, "the disk is full"));
    for s in &sel[..3] {
        assert!(has(h, &mac.env.display(s)), "{} not named", s.display());
    }
    assert!(has(h, "1 item moved. 3 skipped."));
    assert!(has(h, &format!("{} moved to the Trash", human_size(size))));
    assert_eq!(trashed(mac), size);
    assert_eq!(executor::history(&mac.env).len(), 1);
    click(h, "Undo");
    assert!(mover.exists(), "Undo did not put the copy back");
    assert!(sel.iter().chain(&kept).all(|x| x.exists()));
}

#[test]
fn qa7_c2_permission_denied_in_use_and_disk_full_then_undo() {
    let mac = FakeMac::new();
    let da = bytes(1, 300_000);
    for r in ["Downloads/a.bin", "Music/old/a.bin", "Projects/a.bin"] {
        mac.write(&mac.env.home_path(r), &da);
    }
    let db = bytes(2, 200_000);
    for r in ["Movies/b.bin", "Projects/b.bin"] {
        mac.write(&mac.env.home_path(r), &db);
    }
    let dc = bytes(3, 150_000);
    for r in ["Downloads/c.bin", "Projects/c.bin"] {
        mac.write(&mac.env.home_path(r), &dc);
    }
    let mut h = harness(&mac);
    click(&mut h, "Files");
    click(&mut h, "Duplicates");
    click(&mut h, "Scan");
    three_errors_then_undo(&mac, &mut h, FeatureId::Duplicates);
}

#[test]
fn qa7_c2_disk_full_on_the_action_log_moves_nothing() {
    let mac = FakeMac::new();
    let d = bytes(4, 200_000);
    let x = mac.env.home_path("Movies/x.bin");
    let y = mac.env.home_path("Projects/x.bin");
    mac.write(&x, &d);
    mac.write(&y, &d);
    let mut h = harness(&mac);
    click(&mut h, "Files");
    click(&mut h, "Duplicates");
    click(&mut h, "Scan");
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(x.exists() && y.exists());
    assert!(has_text(&h, "could not write the action log"));
}

fn similar_mac() -> FakeMac {
    let mac = FakeMac::new();
    let p = |r: &str| mac.env.home_path(r);
    // Set 1 (scene 0): the 800 x 600 one stays, three smaller ones go.
    save_scene(
        &p("Downloads/a_big.tiff"),
        800,
        600,
        0,
        5,
        3,
        ImageFormat::Tiff,
    );
    save_scene(
        &p("Pictures/Trip/a.png"),
        640,
        480,
        0,
        0,
        1,
        ImageFormat::Png,
    );
    save_scene(
        &p("Pictures/a_small.bmp"),
        320,
        240,
        0,
        -6,
        4,
        ImageFormat::Bmp,
    );
    save_scene(
        &p("Pictures/a_mid.png"),
        480,
        360,
        0,
        3,
        6,
        ImageFormat::Png,
    );
    // Set 2 (scene 1): the bigger stays.
    save_scene(
        &p("Pictures/b_big.png"),
        800,
        600,
        1,
        0,
        5,
        ImageFormat::Png,
    );
    save_scene(&p("Downloads/b.png"), 400, 300, 1, 4, 7, ImageFormat::Png);
    mac
}

#[test]
fn qa7_c3_permission_denied_in_use_and_disk_full_then_undo() {
    let mac = similar_mac();
    let mut h = harness(&mac);
    click(&mut h, "Files");
    click(&mut h, "Similar Images");
    click(&mut h, "Scan");
    let p = plan(&h, FeatureId::SimilarPhotos);
    for k in ["Downloads/a_big.tiff", "Pictures/b_big.png"] {
        let it = p
            .items
            .iter()
            .find(|i| i.path == mac.env.home_path(k))
            .unwrap_or_else(|| panic!("{k} not listed: {:?}", p.items));
        assert!(!it.selected, "{k} is the one to keep");
    }
    three_errors_then_undo(&mac, &mut h, FeatureId::SimilarPhotos);
}

#[test]
fn qa7_c3_undo_puts_back_every_moved_picture() {
    let mac = similar_mac();
    let mut h = harness(&mac);
    click(&mut h, "Files");
    click(&mut h, "Similar Images");
    click(&mut h, "Scan");
    let p = plan(&h, FeatureId::SimilarPhotos);
    let sel: Vec<PathBuf> = p.selected().map(|i| i.path.clone()).collect();
    let size: u64 = sel.iter().map(|x| tree_size(x).0).sum();
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(sel.iter().all(|x| !x.exists()));
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    assert_eq!(trashed(&mac), size);
    click(&mut h, "Undo");
    assert!(
        sel.iter().all(|x| x.exists()),
        "not every picture came back"
    );
    assert_eq!(trashed(&mac), 0);
    assert!(p.items.iter().all(|i| i.path.exists()));
}

#[test]
fn qa7_c3_disk_full_on_the_action_log_moves_nothing() {
    let mac = similar_mac();
    let mut h = harness(&mac);
    click(&mut h, "Files");
    click(&mut h, "Similar Images");
    click(&mut h, "Scan");
    let p = plan(&h, FeatureId::SimilarPhotos);
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(p.items.iter().all(|i| i.path.exists()));
    assert!(has_text(&h, "could not write the action log"));
}

// D1, D3 Disk Map.

fn map_mac() -> FakeMac {
    let mac = FakeMac::sample();
    mac.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    mac
}

fn map_work(h: &mut H) {
    click(h, "Files");
    click(h, "Disk Map");
    type_into(h, "Folder to map", "~/Work");
    h.key_press(egui::Key::Enter);
    steps(h);
}

#[test]
fn qa7_d1_d3_disk_full_keeps_that_file_says_so_and_sizes_follow() {
    let mac = map_mac();
    let a = mac.home_file("Work/a.bin", 400_000);
    let b = mac.home_file("Work/b.bin", 500_000);
    let c = mac.home_file("Work/c.bin", 600_000);
    let work = mac.env.home_path("Work");
    let mut h = harness(&mac);
    map_work(&mut h);
    assert!(has(&h, &human_size(tree_size(&work).0)));
    click(&mut h, "Select a.bin");
    click(&mut h, "Select b.bin");
    click(&mut h, "Select c.bin");
    mac.env.inject_fault(&a, Fault::DiskFull);
    let expect = tree_size(&b).0 + tree_size(&c).0;
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(a.exists(), "moved on a full disk");
    assert!(!b.exists() && !c.exists());
    assert!(has(&h, "the disk is full"));
    assert!(has(&h, &mac.env.display(&a)));
    assert!(has(&h, "2 items moved. 1 skipped."));
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect))
    ));
    assert_eq!(trashed(&mac), expect);
    // The map follows the disk.
    let t = h.state().disk_map.tree().unwrap().clone();
    assert_eq!(t.find(&work).unwrap().size, tree_size(&work).0);
    assert!(t.find(&a).is_some() && t.find(&b).is_none());
    click(&mut h, "Undo");
    assert!(a.exists() && b.exists() && c.exists());
    let t = h.state().disk_map.tree().unwrap().clone();
    assert_eq!(t.find(&work).unwrap().size, tree_size(&work).0);
}

#[test]
fn qa7_d1_d3_disk_full_on_the_action_log_moves_nothing() {
    let mac = map_mac();
    let a = mac.home_file("Work/a.bin", 400_000);
    let b = mac.home_file("Work/b.bin", 500_000);
    let mut h = harness(&mac);
    map_work(&mut h);
    click(&mut h, "Select a.bin");
    click(&mut h, "Select b.bin");
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(a.exists() && b.exists());
    assert!(has_text(&h, "could not write the action log"));
}
