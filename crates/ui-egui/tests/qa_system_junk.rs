//! System Junk end to end, by clicks: scan, check the numbers against a recalculation,
//! deselect, confirm the exact list, see files really move, undo, and the error paths.

mod common;

use cleancraft_core::Fault;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use common::*;
use egui_kittest::kittest::Queryable;

#[test]
fn scan_review_clean_undo() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let plan = h.state().plans[&cleancraft_core::FeatureId::SystemJunk]
        .plan
        .clone()
        .expect("plan");
    // The big number is the sum of every proposed item, recalculated from disk.
    let total: u64 = plan.items.iter().map(|i| tree_size(&i.path).0).sum();
    assert!(
        has(&h, &human_size(total)),
        "total {} not shown",
        human_size(total)
    );
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    assert!(has(&h, "com.example.Editor"));
    // Keep the editor cache.
    click(&mut h, "com.example.Editor");
    let expect_moved: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.selected && i.path != editor)
        .map(|i| i.path.clone())
        .collect();
    let expect_size: u64 = expect_moved.iter().map(|p| tree_size(p).0).sum();
    click(&mut h, "Review and Clean");
    // The dialog lists the exact files.
    // (Each path also shows once in the list behind the dialog.)
    let count = |h: &H, p: &std::path::Path| h.query_all_by_label(&mac.env.display(p)).count();
    assert_eq!(
        count(&h, &browser),
        2,
        "selected item missing from the confirmation"
    );
    assert_eq!(
        count(&h, &editor),
        1,
        "deselected item listed in the confirmation"
    );
    click(&mut h, "Move to Trash");
    assert!(!browser.exists());
    assert!(editor.exists());
    for p in &expect_moved {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect_size))
    ));
    // Undo puts everything back.
    click(&mut h, "Undo");
    for p in &expect_moved {
        assert!(p.exists(), "{} not restored", p.display());
    }
}

#[test]
fn errors_are_shown_per_file() {
    let mac = FakeMac::sample();
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    mac.env.inject_fault(&browser, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&editor.join("index"));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(browser.exists() && editor.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
}

#[test]
fn cancel_moves_nothing() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    click(&mut h, "Cancel");
    let plan = h.state().plans[&cleancraft_core::FeatureId::SystemJunk]
        .plan
        .clone()
        .unwrap();
    assert!(plan.items.iter().all(|i| i.path.exists()));
    assert!(has(&h, "Review and Clean"));
}

#[test]
fn every_sidebar_entry_is_inside_the_window() {
    let mac = FakeMac::sample();
    let h = harness(&mac);
    let mut labels: Vec<&str> = cleancraft_core::FeatureId::ALL
        .iter()
        .map(|f| f.title())
        .collect();
    labels.extend(["History", "Settings"]);
    for l in labels {
        let r = h
            .get_all_by_label(l)
            .next()
            .unwrap_or_else(|| panic!("{l} missing"))
            .rect();
        assert!(r.max.y <= HGT && r.min.y >= 0.0, "{l} at {r:?}");
    }
}
