//! System Junk J1 to J21 by clicks on the rich fake Mac: the numbers on screen against a
//! recalculation from disk, the review controls (search, sort, select all and none, category
//! checkbox, Reveal, Ignore surviving a rescan), locked items, cleaning and the error paths.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::junk_mac;
use cleancraft_core::{Fault, FeatureId, Plan, Settings};
use common::*;
use egui_kittest::kittest::Queryable;

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::SystemJunk]
        .plan
        .clone()
        .expect("plan")
}

/// Size of an item recalculated from disk (universal binaries: the unused slice lipo reported,
/// 600,000 bytes in the fixture).
fn disk_size(p: &Path, cat: &str) -> u64 {
    if cat == "universal_binaries" {
        600_000
    } else {
        tree_size(p).0
    }
}

/// Clear the search box the way a user does: focus it and press Backspace.
fn set_search(h: &mut H, text: &str) {
    let n = h.state().plans[&FeatureId::SystemJunk]
        .search
        .chars()
        .count();
    h.get_by_label("Search").focus();
    steps(h);
    for _ in 0..n {
        h.key_press(egui::Key::Backspace);
    }
    steps(h);
    if !text.is_empty() {
        type_into(h, "Search", text);
    }
    assert_eq!(h.state().plans[&FeatureId::SystemJunk].search, text);
}

fn open_junk(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    h
}

#[test]
fn numbers_match_disk_and_default_clean_moves_selected_only() {
    let mac = junk_mac();
    let mut h = open_junk(&mac);
    let p = plan(&h);
    // Headline: every item recalculated from disk.
    let total: u64 = p
        .items
        .iter()
        .map(|i| disk_size(&i.path, &i.category))
        .sum();
    assert!(
        has(&h, &human_size(total)),
        "total {} not shown",
        human_size(total)
    );
    assert!(has(&h, &format!("found in {} items", p.items.len())));
    // Category sizes on the left (the first categories fit in the window).
    for (cat, title) in [
        ("dev_caches", "Developer caches"),
        ("old_updates", "Old app updates"),
        ("ai_clutter", "AI tool clutter"),
        ("user_caches", "User cache files"),
    ] {
        let s: u64 = p
            .items
            .iter()
            .filter(|i| i.category == cat)
            .map(|i| tree_size(&i.path).0)
            .sum();
        assert!(
            has(&h, &format!("{title}  {}", human_size(s))),
            "{title} {}",
            human_size(s)
        );
    }
    // Bottom line: the default selection, recalculated.
    let sel: Vec<(PathBuf, u64)> = p
        .selected()
        .map(|i| (i.path.clone(), tree_size(&i.path).0))
        .collect();
    let sel_size: u64 = sel.iter().map(|(_, s)| s).sum();
    assert!(has(
        &h,
        &format!("{} selected, {}", sel.len(), human_size(sel_size))
    ));
    // Never-preselected and locked items are not in the confirmation.
    click(&mut h, "Review and Clean");
    assert!(has(
        &h,
        &format!(
            "Move {} items ({}) to the Trash?",
            sel.len(),
            human_size(sel_size)
        )
    ));
    click(&mut h, "Move to Trash");
    for (path, _) in &sel {
        assert!(!path.exists(), "{} not moved", path.display());
    }
    for it in p.items.iter().filter(|i| !i.selected) {
        assert!(
            it.path.exists(),
            "{} moved but not selected",
            it.path.display()
        );
    }
    assert!(mac.env.user_trash().join("com.example.Browser").exists());
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(sel_size))
    ));
    assert!(has(&h, &format!("{} items moved. 0 skipped.", sel.len())));
    // History lists them, Undo puts everything back.
    click(&mut h, "Undo");
    for (path, _) in &sel {
        assert!(path.exists(), "{} not restored", path.display());
    }
    assert!(has(&h, &format!("{} items put back.", sel.len())));
}

#[test]
fn review_controls_search_sort_select_reveal_ignore() {
    let mac = junk_mac();
    let mut h = open_junk(&mac);
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    // Search narrows the list (by path or reason).
    type_into(&mut h, "Search", "com.example");
    assert!(has(&h, "com.example.Editor"));
    assert!(!has(&h, "Homebrew"), "search did not filter");
    // Sort by size: the 2 MB system cache above the 1.4 MB Editor cache; by name the reverse.
    let y = |h: &H, l: &str| h.get_by_label(l).rect().min.y;
    assert!(y(&h, "com.example.updater") < y(&h, "com.example.Editor"));
    // The sort menu shows its current choice as its value.
    h.get_by_value("Sort by size").click();
    steps(&mut h);
    click(&mut h, "Sort by name");
    assert!(h.query_by_value("Sort by name").is_some());
    assert!(y(&h, "com.example.Editor") < y(&h, "com.example.updater"));
    // Select None / Select All act on what is visible.
    click(&mut h, "Select None");
    let p = plan(&h);
    let visible: Vec<_> = p
        .items
        .iter()
        .filter(|i| {
            i.path.to_string_lossy().contains("com.example") || i.reason.contains("com.example")
        })
        .map(|i| i.id)
        .collect();
    assert!(visible.iter().all(|id| !p.items[*id].selected));
    assert!(
        p.items.iter().any(|i| i.selected),
        "items outside the search kept"
    );
    click(&mut h, "Select All");
    let p = plan(&h);
    assert!(
        visible
            .iter()
            .all(|id| p.items[*id].selected || p.items[*id].locked.is_some())
    );
    // A row checkbox.
    click(&mut h, "com.example.Editor");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    assert!(
        !plan(&h)
            .items
            .iter()
            .find(|i| i.path == editor)
            .unwrap()
            .selected
    );
    // Category checkbox: Gradle starts unselected, so the box is off; one click selects the
    // whole category, a second click clears it.
    let dev = |h: &H| {
        plan(h)
            .items
            .iter()
            .filter(|i| i.category == "dev_caches")
            .map(|i| i.selected)
            .collect::<Vec<_>>()
    };
    assert!(dev(&h).contains(&false));
    click(&mut h, "Select Developer caches");
    assert!(dev(&h).iter().all(|s| *s));
    click(&mut h, "Select Developer caches");
    assert!(dev(&h).iter().all(|s| !*s));
    let p = plan(&h);
    let recalc: u64 = p.selected().map(|i| tree_size(&i.path).0).sum();
    assert!(has(
        &h,
        &format!("{} selected, {}", p.selected_count(), human_size(recalc))
    ));
    // Reveal asks Finder through the system layer.
    set_search(&mut h, "com.example.Browser");
    let before = mac.env.fake_sys().unwrap().calls().len();
    click_nth(&mut h, "Reveal", 0);
    let calls = mac.env.fake_sys().unwrap().calls();
    assert_eq!(calls.len(), before + 1);
    assert_eq!(
        calls.last().unwrap(),
        &format!("/usr/bin/open -R {}", browser.display())
    );
    // Ignore: saved in settings and gone after Scan Again.
    click_nth(&mut h, "Ignore", 0);
    let saved = Settings::load(&mac.env);
    assert!(saved.ignore.contains(&browser), "{:?}", saved.ignore);
    assert!(has(
        &h,
        "Cache of com.example.Browser (cannot be selected: on your ignore list)"
    ));
    click(&mut h, "Scan Again");
    assert!(!plan(&h).items.iter().any(|i| i.path == browser));
    assert!(browser.exists());
}

#[test]
fn locked_items_show_why_and_cannot_be_selected() {
    let mac = junk_mac();
    let mut h = open_junk(&mac);
    type_into(&mut h, "Search", "translation");
    assert!(has_text(
        &h,
        "Atlas: de translation (cannot be selected: Inside a signed app"
    ));
    click(&mut h, "de.lproj");
    let lproj = mac
        .env
        .sys_path("/Applications/Atlas.app/Contents/Resources/de.lproj");
    let p = plan(&h);
    assert!(!p.items.iter().find(|i| i.path == lproj).unwrap().selected);
    // Report-only categories: their checkbox on the left is disabled, Select All skips them.
    click(&mut h, "Select All");
    assert!(
        !plan(&h)
            .items
            .iter()
            .any(|i| i.locked.is_some() && i.selected)
    );
    set_search(&mut h, "Backup of");
    assert!(has_text(
        &h,
        "The newest backup of this device is always kept"
    ));
}

#[test]
fn errors_are_shown_per_item_and_log_failure_moves_nothing() {
    let mac = junk_mac();
    let mut h = open_junk(&mac);
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    let logs = mac.env.home_path("Library/Logs/Example");
    mac.env.inject_fault(&browser, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&editor.join("index"));
    mac.env.inject_fault(&logs, Fault::DiskFull);
    let p = plan(&h);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(browser.exists() && editor.exists() && logs.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has(&h, "the disk is full"));
    let moved: u64 = p
        .selected()
        .filter(|i| i.path != browser && i.path != editor && i.path != logs)
        .map(|i| i.size)
        .sum();
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(moved))
    ));
    assert!(has(
        &h,
        &format!("{} items moved. 3 skipped.", p.selected_count() - 3)
    ));

    // The action log cannot be written: an error, and nothing moved.
    let mac = junk_mac();
    let mut h = open_junk(&mac);
    mac.env.inject_fault(
        &cleancraft_core::executor::journal_path(&mac.env),
        Fault::DiskFull,
    );
    let p = plan(&h);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(has_text(&h, "could not write the action log"));
    assert!(p.selected().all(|i| i.path.exists()));
}
