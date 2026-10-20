//! Round 6 file features by clicks: the Duplicate Downloads tile in Quick Sweep (numbers vs a
//! recalculation, Review opens Duplicates on that plan, Run keeps one copy, Undo, a copy in
//! use), Large & Old "Group by" Size and Age with select all/none per group, the Disk Map
//! location picker (startup disk, external volume from fake df/diskutil), unreadable folders
//! reported, locked places, an external volume's folder removed and put back, permission
//! denied.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use cleancraft_core::features::duplicates::DownloadDuplicates;
use cleancraft_core::features::large_old::{GroupBy, ScanOptions};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sweep::Sources;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId, Plan, Settings};
use cleancraft_ui_egui::app::Page;
use common::*;

fn blob(seed: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect()
}

fn put(m: &FakeMac, rel: &str, b: &[u8]) -> PathBuf {
    let p = m.env.home_path(rel);
    m.write(&p, b);
    p
}

fn dl_mac() -> (FakeMac, Vec<PathBuf>) {
    let m = FakeMac::new();
    let a = blob(3, 400_000);
    let v = vec![
        put(&m, "Downloads/photo.jpg", &a),
        put(&m, "Downloads/photo (1).jpg", &a),
        put(&m, "Downloads/photo (2).jpg", &a),
    ];
    let b = blob(9, 300_000);
    for p in [put(&m, "Downloads/old.zip", &b), put(&m, "Downloads/old 2.zip", &b)] {
        age(&p, 150);
    }
    (m, v)
}

fn sweep_with_downloads(m: &FakeMac) -> H {
    let mut h = harness(m);
    h.state_mut().sweep.sources = Some(Sources {
        plans: vec![Arc::new(DownloadDuplicates)],
        counts: vec![],
    });
    click(&mut h, "Scan");
    h
}

fn tile_plan(h: &H) -> Plan {
    h.state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tile(FeatureId::Duplicates)
        .unwrap()
        .plan()
        .unwrap()
        .clone()
}

#[test]
fn duplicate_downloads_tile_review_run_undo() {
    let (m, copies) = dl_mac();
    let mut h = sweep_with_downloads(&m);
    assert!(has(&h, "Duplicate Downloads"));
    let p = tile_plan(&h);
    // Recalculated from disk: two extra copies of one set (the old set is left out).
    let extra = tree_size(&copies[1]).0 + tree_size(&copies[2]).0;
    assert_eq!(p.selected_size(), extra);
    assert!(has(&h, &human_size(extra)));
    assert!(has(&h, "2 of 2 extra copies selected. One of each set stays."));
    // Review opens Duplicates on that plan.
    click(&mut h, "Review");
    assert_eq!(h.state().page, Page::Feature(FeatureId::Duplicates));
    let shown = h.state().plans[&FeatureId::Duplicates].plan.clone().unwrap();
    assert_eq!(shown.id, p.id);
    assert!(has(&h, "photo (1).jpg"));
    assert!(!has(&h, "old.zip"));
    // Back to Quick Sweep: Run moves the two copies, the kept one stays.
    click(&mut h, "Quick Sweep");
    click(&mut h, "Review and Clean");
    assert!(has(&h, &m.env.display(&copies[1])));
    click(&mut h, "Cancel");
    assert!(copies.iter().all(|c| c.exists()));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(copies[0].exists());
    assert!(!copies[1].exists() && !copies[2].exists());
    assert!(has_text(&h, &format!("{} moved to the Trash", human_size(extra))));
    click(&mut h, "Undo");
    assert!(copies.iter().all(|c| c.exists()));
}

#[test]
fn duplicate_downloads_copy_in_use_is_skipped() {
    let (m, copies) = dl_mac();
    m.env.fake_sys().unwrap().set_open_by(&copies[2], "Preview");
    let mut h = sweep_with_downloads(&m);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(copies[2].exists(), "open in Preview: left alone");
    assert!(!copies[1].exists());
    assert!(has_text(&h, "Preview"));
}

#[test]
fn registered_quick_sweep_shows_the_tile() {
    let (m, _) = dl_mac();
    let mut h = harness(&m);
    click(&mut h, "Scan");
    assert!(has(&h, "Duplicate Downloads"));
}

fn lo_mac() -> (FakeMac, PathBuf, PathBuf, PathBuf) {
    let m = FakeMac::new();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&m.env)
    .unwrap();
    // 51 MB: the "50 MB to 1 GB" group (real bytes, so the size on disk is real).
    let big = m.home_file("Downloads/big.mov", 51_000_000);
    let old = m.home_file("Downloads/old.pdf", 3_000_000);
    age(&old, 400);
    let mid = m.home_file("Downloads/mid.zip", 2_500_000);
    age(&mid, 120);
    (m, big, old, mid)
}

fn lo_plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::LargeOldFiles].plan.clone().unwrap()
}

#[test]
fn large_old_group_by_size_and_age() {
    let (m, big, old, mid) = lo_mac();
    let mut h = harness(&m);
    click(&mut h, "Files");
    click(&mut h, "Large & Old Files");
    click(&mut h, "Scan");
    assert!(has(&h, "0 selected, 0 KB"), "nothing preselected");
    click(&mut h, "Group by: Kind");
    click(&mut h, "Size");
    let b = tree_size(&big).0;
    let small = tree_size(&old).0 + tree_size(&mid).0;
    assert!(has(&h, &format!("50 MB to 1 GB  {}", human_size(b))));
    assert!(has(&h, &format!("Under 50 MB  {}", human_size(small))));
    assert_eq!(ScanOptions::load(&m.env).large_old_group, GroupBy::Size);
    assert_eq!(lo_plan(&h).selected_count(), 0);
    // Select all in one group, then none.
    click(&mut h, "Select Under 50 MB");
    assert!(has(&h, &format!("2 selected, {}", human_size(small))));
    click(&mut h, "Select Under 50 MB");
    assert!(has(&h, "0 selected, 0 KB"));
    // Age.
    click(&mut h, "Group by: Size");
    click(&mut h, "Age");
    assert!(has(&h, &format!("Not opened for over a year  {}", human_size(tree_size(&old).0))));
    assert!(has(&h, &format!("Not opened in 3 to 6 months  {}", human_size(tree_size(&mid).0))));
    assert!(has(&h, &format!("Opened in the last 3 months  {}", human_size(b))));
    click(&mut h, "Select Not opened for over a year");
    assert!(has(&h, &format!("1 selected, {}", human_size(tree_size(&old).0))));
    // The grouping stays for the next scan; the move works from a group.
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(!old.exists());
    assert!(m.env.user_trash().join("old.pdf").exists());
    click(&mut h, "Undo");
    assert!(old.exists());
}

#[test]
fn large_old_group_selection_with_permission_denied() {
    let (m, _big, old, mid) = lo_mac();
    let mut h = harness(&m);
    click(&mut h, "Files");
    click(&mut h, "Large & Old Files");
    click(&mut h, "Scan");
    click(&mut h, "Group by: Kind");
    click(&mut h, "Size");
    click(&mut h, "Select Under 50 MB");
    m.env.inject_fault(&old, Fault::PermissionDenied);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(old.exists(), "permission denied: stays");
    assert!(!mid.exists());
}

const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1s1 488245288 300000000 188245288 62% /
/dev/disk5s1 1000000 400000 600000 40% /Volumes/My Backup
/dev/disk6s1 50000 1000 49000 2% /Volumes/Installer
";

const DISKUTIL: &str = r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>AllDisks</key><array><string>disk5</string><string>disk5s1</string></array><key>AllDisksAndPartitions</key><array><dict><key>DeviceIdentifier</key><string>disk5</string><key>Partitions</key><array><dict><key>DeviceIdentifier</key><string>disk5s1</string><key>MountPoint</key><string>/Volumes/My Backup</string><key>VolumeName</key><string>My Backup</string></dict></array></dict></array></dict></plist>"#;

fn map_mac() -> FakeMac {
    let m = FakeMac::new();
    m.home_file("Movies/film.mov", 2_000_000);
    m.sys_file("/Library/Caches/sys.cache", 300_000);
    m.sys_file("/Volumes/My Backup/Movies/old.mov", 700_000);
    m.sys_file("/Volumes/My Backup/.Spotlight-V100/store.db", 100_000);
    std::fs::create_dir_all(m.env.sys_path("/Volumes/Installer")).unwrap();
    let f = m.env.fake_sys().unwrap();
    f.answer("/bin/df -Pk", DF);
    // `df -Pk <folder>` for the storage bar (the startup disk).
    f.answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 488245288 300000000 188245288 62% /\n",
    );
    f.answer("/usr/sbin/diskutil list -plist external", DISKUTIL);
    m
}

fn open_map(m: &FakeMac) -> H {
    let mut h = harness(m);
    click(&mut h, "Files");
    click(&mut h, "Disk Map");
    h
}

#[test]
fn location_picker_lists_disks_from_df_and_diskutil() {
    let m = map_mac();
    let mut h = open_map(&m);
    click(&mut h, "Home folder");
    assert!(has(&h, "Startup Disk"));
    let label = format!(
        "My Backup ({}, {} free)",
        human_size(1_000_000 * 1024),
        human_size(600_000 * 1024)
    );
    assert!(has(&h, &label), "external volume with its capacity");
    assert!(!has_text(&h, "Installer"), "not an external disk per diskutil");
    click(&mut h, &label);
    assert!(has(&h, "Map My Backup"));
    assert!(has(
        &h,
        &format!(
            "My Backup: {} used of {}, {} free",
            human_size(400_000 * 1024),
            human_size(1_000_000 * 1024),
            human_size(600_000 * 1024)
        )
    ));
}

#[test]
fn external_volume_folder_removed_and_put_back() {
    let m = map_mac();
    let vol = m.env.sys_path("/Volumes/My Backup");
    let movie = vol.join("Movies/old.mov");
    let mut h = open_map(&m);
    click(&mut h, "Home folder");
    click(&mut h, &format!(
        "My Backup ({}, {} free)",
        human_size(1_000_000 * 1024),
        human_size(600_000 * 1024)
    ));
    click(&mut h, "Map My Backup");
    assert!(has(&h, &human_size(tree_size(&vol).0)), "volume total");
    // The volume's system folder is locked with its reason.
    click(&mut h, "Select .Spotlight-V100");
    assert!(h.state().disk_map.selected.is_empty());
    assert!(has_text(&h, "system data of this disk"));
    click(&mut h, "Movies");
    click(&mut h, "Select old.mov");
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(!movie.exists());
    assert!(vol.join(".Trashes").exists(), "the volume's own Trash");
    click(&mut h, "Undo");
    assert!(movie.exists());
    let t = h.state().disk_map.tree().unwrap().clone();
    assert_eq!(t.find(&vol).unwrap().size, tree_size(&vol).0);
}

#[test]
fn startup_disk_locks_and_unreadable_folders() {
    let m = map_mac();
    let secret = m.env.home_path("Private");
    m.home_file("Private/a.bin", 300_000);
    m.env.inject_fault(&secret, Fault::PermissionDenied);
    let mut h = open_map(&m);
    click(&mut h, "Home folder");
    click(&mut h, "Startup Disk");
    click(&mut h, "Map Startup Disk");
    let root = m.env.sys_path("/");
    let t = h.state().disk_map.tree().unwrap().clone();
    assert!(t.find(&m.env.sys_path("/Volumes")).is_none(), "other disks left out");
    assert_eq!(t.unreadable, 1);
    assert!(has_text(&h, "Could not be read: 1 folder"));
    assert!(has_text(&h, "Not measured:"), "used space not walked is totalled");
    // Outside the home folder: locked with the reason.
    click(&mut h, "Select Library");
    assert!(h.state().disk_map.selected.is_empty());
    assert!(has_text(&h, "part of macOS"));
    click(&mut h, "Users");
    click(&mut h, "tester");
    assert!(has(&h, "Unknown"), "the unreadable folder's size is not shown as 0");
    assert!(has_text(&h, "Could not be read. Full Disk Access may help."));
    let _ = root;
    // A file in the home folder is selectable and moves; in use is skipped.
    click(&mut h, "Movies");
    let film = m.env.home_path("Movies/film.mov");
    m.env.fake_sys().unwrap().set_open_by(&film, "QuickTime Player");
    click(&mut h, "Select film.mov");
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(film.exists());
    assert!(has_text(&h, "QuickTime Player"));
}
