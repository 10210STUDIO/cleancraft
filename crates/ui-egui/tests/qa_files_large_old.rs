//! Large & Old Files by clicks: pick a location, scan, numbers against a recalculation,
//! nothing preselected, select, confirm, files move, undo; typed folders checked (protected
//! refused, Enter and Esc); Downloads by source app; permission denied and file in use.

mod common;

use cleancraft_core::features::large_old::{Location, ScanOptions};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId, Settings};
use common::*;
use egui_kittest::kittest::Queryable;

fn mac() -> FakeMac {
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

fn plan(h: &H) -> cleancraft_core::Plan {
    h.state().plans[&FeatureId::LargeOldFiles]
        .plan
        .clone()
        .expect("plan")
}

#[test]
fn downloads_scan_select_clean_undo() {
    let mac = mac();
    let big = mac.home_file("Downloads/big.zip", 2_500_000);
    let old = mac.home_file("Downloads/old.pdf", 1_200_000);
    age(&old, 400);
    let clip = mac.home_file("Movies/clip.mov", 3_000_000);
    let mut h = harness(&mac);
    click(&mut h, "Large & Old Files");
    click(&mut h, "Downloads");
    assert!(has(&h, "Looking in: Downloads"));
    assert_eq!(
        ScanOptions::load(&mac.env).large_old,
        Location::Downloads,
        "choice not remembered"
    );
    click(&mut h, "Scan");
    let p = plan(&h);
    let paths: Vec<_> = p.items.iter().map(|i| i.path.clone()).collect();
    assert!(paths.contains(&big) && paths.contains(&old) && !paths.contains(&clip));
    let total = tree_size(&big).0 + tree_size(&old).0;
    assert!(has(&h, &human_size(total)), "total {}", human_size(total));
    assert!(has(&h, "0 selected, Zero KB"), "something was preselected");
    assert!(has(
        &h,
        &format!("Archives  {}", human_size(tree_size(&big).0))
    ));
    assert!(has_text(&h, "Not opened for 400 days"));
    click(&mut h, "big.zip");
    let size = tree_size(&big).0;
    assert!(has(&h, &format!("1 selected, {}", human_size(size))));
    click(&mut h, "Review and Clean");
    // Listed in the dialog (and once in the list behind it).
    assert_eq!(h.query_all_by_label(&mac.env.display(&big)).count(), 2);
    assert_eq!(h.query_all_by_label(&mac.env.display(&old)).count(), 1);
    click(&mut h, "Move to Trash");
    assert!(!big.exists() && old.exists());
    assert!(mac.env.user_trash().join("big.zip").exists());
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    click(&mut h, "Undo");
    assert!(big.exists());
}

#[test]
fn typed_folder_checked_with_enter_and_esc() {
    let mac = mac();
    let w = mac.home_file("Work/video.mp4", 2_400_000);
    mac.home_file("Downloads/other.zip", 2_400_000);
    let mut h = harness(&mac);
    click(&mut h, "Large & Old Files");
    click(&mut h, "Another folder");
    type_into(&mut h, "Folder path", "~/Documents");
    click(&mut h, "Use Folder");
    assert!(has_text(&h, "is protected"), "protected folder accepted");
    assert_eq!(ScanOptions::load(&mac.env).large_old, Location::Home);
    // Esc drops the typed text and closes the folder box.
    h.get_by_label("Folder path").focus();
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(!has(&h, "Folder path"));
    assert!(!has_text(&h, "is protected"));
    // Enter confirms a good folder.
    click(&mut h, "Another folder");
    type_into(&mut h, "Folder path", "~/Work");
    h.key_press(egui::Key::Enter);
    steps(&mut h);
    assert_eq!(
        ScanOptions::load(&mac.env).large_old,
        Location::Folder(mac.env.home_path("Work"))
    );
    assert!(has(&h, "Looking in: /Users/tester/Work"));
    click(&mut h, "Scan");
    let p = plan(&h);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].path, w);
}

#[test]
fn downloads_by_source_app() {
    let mac = mac();
    let a = mac.home_file("Downloads/a.dmg", 300_000);
    let b = mac.home_file("Downloads/b.zip", 200_000);
    let c = mac.home_file("Downloads/notes/x.txt", 5_000);
    let fake = mac.env.fake_sys().unwrap();
    fake.answer(
        &format!("/usr/bin/xattr -p com.apple.quarantine {}", a.display()),
        "0083;65f1c2a0;Safari;1\n",
    );
    fake.answer(
        &format!("/usr/bin/xattr -p com.apple.quarantine {}", b.display()),
        "0083;65f1c2a0;Safari;2\n",
    );
    let mut h = harness(&mac);
    click(&mut h, "Large & Old Files");
    click(&mut h, "Downloads by source app");
    click(&mut h, "Scan");
    let safari = tree_size(&a).0 + tree_size(&b).0;
    assert!(has(&h, &format!("Saved by Safari  {}", human_size(safari))));
    let none = tree_size(c.parent().unwrap()).0;
    assert!(has(
        &h,
        &format!("No source recorded  {}", human_size(none))
    ));
    assert!(has(&h, "0 selected, Zero KB"));
    // Show one category, select it whole, clean it.
    click(&mut h, "Select Saved by Safari");
    assert!(has(&h, &format!("2 selected, {}", human_size(safari))));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(!a.exists() && !b.exists() && c.exists());
}

#[test]
fn permission_denied_and_file_in_use() {
    let mac = mac();
    let a = mac.home_file("Movies/a.mov", 2_200_000);
    let b = mac.home_file("Movies/b.mov", 2_300_000);
    let mut h = harness(&mac);
    click(&mut h, "Large & Old Files");
    click(&mut h, "Scan");
    click(&mut h, "Select All");
    mac.env.inject_fault(&a, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&b);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(a.exists() && b.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has(&h, "0 items moved. 2 skipped."));
}
