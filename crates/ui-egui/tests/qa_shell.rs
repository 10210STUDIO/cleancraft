//! The app shell by clicks (FEATURES U2, U3, U4): History lists every moved item with the
//! numbers on disk and puts one back; Settings changes are saved to disk and change what the
//! next scan and cleanup do; the Full Disk Access banner opens the right pane and re-checks.

mod common;

use cleancraft_core::executor::{self, ActionKind, Method};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::permissions::{Access, FDA_SETTINGS_URL};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{Fault, FeatureId, Settings};
use common::*;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;

fn clean_junk(h: &mut H) {
    // The tool sits in the "Clean" section of the sidebar.
    click(h, "Clean");
    click(h, "System Junk");
    click(h, "Scan");
    click(h, "Review and Clean");
    click(h, "Move to Trash");
}

#[test]
fn history_lists_moves_with_disk_numbers_and_puts_one_back() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    clean_junk(&mut h);
    click(&mut h, "Show History");
    assert_eq!(h.state().page, cleancraft_ui_egui::Page::History);
    let recs = executor::history(&mac.env);
    assert!(recs.len() >= 3, "{} records", recs.len());
    // The files of a cleanup wait behind a disclosure next to Undo All (fix5).
    click(&mut h, &format!("Show {} items of System Junk", recs.len()));
    for r in &recs {
        let ActionKind::Moved {
            original, stored, ..
        } = &r.kind
        else {
            panic!("only moves expected: {r:?}");
        };
        assert!(!original.exists() && stored.exists());
        assert_eq!(r.size, tree_size(stored).0, "{}", original.display());
        assert!(
            has(&h, &mac.env.display(original)),
            "{}",
            original.display()
        );
        assert!(
            has_text(
                &h,
                &format!(
                    "System Junk · {} · now in {}",
                    human_size(r.size),
                    mac.env.display(stored)
                )
            ),
            "row of {}",
            original.display()
        );
    }
    assert_eq!(h.query_all_by_label("Undo").count(), recs.len());

    // Undo the newest row only: that item comes back, the others stay in the Trash.
    let newest = recs.last().unwrap();
    let ActionKind::Moved {
        original, stored, ..
    } = &newest.kind
    else {
        unreachable!()
    };
    click_nth(&mut h, "Undo", 0);
    assert!(original.exists() && !stored.exists());
    assert!(has(&h, "Put back."));
    assert!(has(&h, "Put back"));
    assert_eq!(h.query_all_by_label("Undo").count(), recs.len() - 1);
    for r in &recs[..recs.len() - 1] {
        if let ActionKind::Moved { original, .. } = &r.kind {
            assert!(!original.exists(), "{} came back too", original.display());
        }
    }

    // Something took the place of the next one: it is not overwritten, the user is told.
    let next = &recs[recs.len() - 2];
    let ActionKind::Moved {
        original, stored, ..
    } = &next.kind
    else {
        unreachable!()
    };
    mac.write(original, b"new file in the same place");
    click_nth(&mut h, "Undo", 0);
    assert!(has_text(&h, "Could not put it back"));
    assert!(stored.exists(), "the stored copy must stay");
    assert_eq!(
        std::fs::read(original).unwrap(),
        b"new file in the same place".to_vec()
    );
}

#[test]
fn settings_are_saved_and_change_what_happens() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    click(&mut h, "Settings");

    // Removal method: quarantine.
    click(&mut h, "CleanCraft quarantine");
    assert_eq!(Settings::load(&mac.env).method, Method::Quarantine);

    // Ignore list: empty, then a real path.
    click(&mut h, "Add to Ignore List");
    assert!(has(&h, "Type a path first."));
    type_into(
        &mut h,
        "Path to ignore",
        "~/Library/Caches/com.example.Browser",
    );
    click(&mut h, "Add to Ignore List");
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    assert_eq!(Settings::load(&mac.env).ignore, vec![browser.clone()]);
    assert!(has(&h, &mac.env.display(&browser)));

    // Agents may clean and a threshold: each saved.
    // Lower on the page (15 pt body text since round 3): scroll to it like a user.
    h.get_by_label("Let AI assistants clean after you confirm")
        .scroll_to_me();
    steps(&mut h);
    click(&mut h, "Let AI assistants clean after you confirm");
    assert!(Settings::load(&mac.env).agents_may_clean);
    // Above the AI group since round 6 (Full Disk Access and Developer Clutter groups):
    // scroll it into view first.
    h.get_by_role_and_label(Role::Slider, "Large file minimum (MB)")
        .scroll_to_me();
    steps(&mut h);
    h.get_by_role_and_label(Role::Slider, "Large file minimum (MB)")
        .click();
    steps(&mut h);
    let saved = Settings::load(&mac.env);
    assert_eq!(saved.large_min_bytes, h.state().settings.large_min_bytes);
    assert_ne!(saved.large_min_bytes, Settings::default().large_min_bytes);

    // The next scan leaves the ignored folder out; the cleanup goes to the quarantine.
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let plan = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .clone()
        .unwrap();
    assert!(plan.items.iter().all(|i| !i.path.starts_with(&browser)));
    assert!(!has(&h, "com.example.Browser"));
    click(&mut h, "Review and Clean");
    assert!(has_text(&h, "to the CleanCraft quarantine?"));
    click(&mut h, "Move to Quarantine");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    assert!(!editor.exists());
    assert!(browser.exists(), "ignored folder moved");
    let quarantined = executor::history(&mac.env).iter().any(|r| {
        matches!(&r.kind, ActionKind::Moved { original, stored, method: Method::Quarantine, .. }
            if *original == editor && stored.starts_with(mac.env.quarantine_dir()) && stored.exists())
    });
    assert!(quarantined, "editor cache not in the quarantine");

    // Remove from the ignore list: saved, and the folder is proposed again.
    click(&mut h, "Settings");
    // The page keeps its scroll position (the agents switch is lower down): scroll back.
    h.get_by_label(&format!("Remove {}", mac.env.display(&browser)))
        .scroll_to_me();
    steps(&mut h);
    click(&mut h, &format!("Remove {}", mac.env.display(&browser)));
    assert!(Settings::load(&mac.env).ignore.is_empty());
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Done");
    click(&mut h, "Scan");
    assert!(has(&h, "com.example.Browser"));
}

#[test]
fn settings_that_cannot_be_saved_say_so() {
    let mac = FakeMac::new();
    // A folder where the settings file should be: writing it fails.
    std::fs::create_dir_all(Settings::path(&mac.env)).unwrap();
    let mut h = harness(&mac);
    click(&mut h, "Settings");
    // Lower on the page: scroll to it like a user (the switch row is taller than the old
    // checkbox, fix5 U26).
    h.get_by_label("Let AI assistants clean after you confirm")
        .scroll_to_me();
    steps(&mut h);
    click(&mut h, "Let AI assistants clean after you confirm");
    assert!(has_text(&h, "Settings could not be saved"));
    assert!(!Settings::load(&mac.env).agents_may_clean);
}

#[test]
fn full_disk_access_banner_opens_the_pane_and_checks_again() {
    let mac = FakeMac::new();
    let probe = mac.home_file("Library/Safari/Bookmarks.plist", 10);
    mac.env.inject_fault(&probe, Fault::PermissionDenied);
    let mut h = harness(&mac);
    assert_eq!(h.state().fda, Access::Missing);
    assert!(has_text(&h, "Turn on Full Disk Access for CleanCraft"));
    click(&mut h, "Open Privacy Settings");
    let urls = mac.env.fake_sys().unwrap().with(|s| s.opened_urls.clone());
    assert_eq!(urls, vec![FDA_SETTINGS_URL.to_string()]);
    click(&mut h, "Check Again");
    assert!(has(&h, "Full Disk Access is still off."));
    assert!(has_text(&h, "Turn on Full Disk Access for CleanCraft"));
    // The user turns it on in System Settings.
    mac.env.clear_faults();
    click(&mut h, "Check Again");
    assert_eq!(h.state().fda, Access::Granted);
    assert!(has(&h, "Full Disk Access is on."));
    assert!(!has_text(&h, "Turn on Full Disk Access for CleanCraft"));
    assert!(!has(&h, "Open Privacy Settings"));
}
