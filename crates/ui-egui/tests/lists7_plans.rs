//! lists7: the shared list toolbar on the results pages (System Junk, Mail Downloads,
//! Privacy, Trash Bins, Large & Old Files, Duplicates) by clicks and keys on a FakeMac.
//! Search narrows the list and the "Showing" line, the Selected / Not selected / Locked
//! filters and the kind and location filters work with their counts, sorts and directions
//! work, filters and sort are remembered after a relaunch, selected items the filters hide
//! are counted in the bottom bar and listed in the confirmation, and they move only after it;
//! Clear Filters restores the list.

mod common;

use std::path::PathBuf;

use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_cleanup;
use cleancraft_core::{FeatureId, Plan, Settings};
use cleancraft_ui_egui::pages::plan_page::{self, Row};
use common::*;
use egui_kittest::kittest::Queryable;

fn plan(h: &H, f: FeatureId) -> Plan {
    h.state().plans[&f].plan.clone().expect("plan")
}

/// The rows of items outside any set, in the order the list shows them (sets keep their
/// own order).
fn loose(h: &H, f: FeatureId) -> Vec<PathBuf> {
    let st = &h.state().plans[&f];
    let p = st.plan.as_ref().expect("plan");
    shown(h, f)
        .into_iter()
        .filter(|x| p.items.iter().any(|i| &i.path == x && i.group.is_none()))
        .collect()
}

/// The item rows in the order the list shows them.
fn shown(h: &H, f: FeatureId) -> Vec<PathBuf> {
    let st = &h.state().plans[&f];
    let p = st.plan.as_ref().expect("plan");
    plan_page::rows(st)
        .into_iter()
        .filter_map(|r| match r {
            Row::Item(id) => Some(p.items[id].path.clone()),
            Row::Group(_) => None,
        })
        .collect()
}

fn open(mac: &FakeMac, f: FeatureId) -> H {
    let mut h = harness(mac);
    h.state_mut().navigate(f.as_str());
    steps(&mut h);
    click(&mut h, "Scan");
    for _ in 0..3 {
        steps(&mut h);
    }
    h
}

fn filter(h: &mut H, entry: &str) {
    click(h, "Filter");
    click(h, entry);
    h.key_press(egui::Key::Escape);
    steps(h);
}

fn sort(h: &mut H, entry: &str) {
    click(h, "Sort");
    click(h, entry);
}

fn name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[test]
fn system_junk_search_filters_sort_remembered_and_clear() {
    let mac = testkit_cleanup::junk_mac();
    let f = FeatureId::SystemJunk;
    let mut h = open(&mac, f);
    let p = plan(&h, f);
    let total = p.items.len();
    assert!(!has_text(&h, "Showing"), "no line while nothing narrows");
    // Search: path and reason.
    type_into(&mut h, "Search", "example");
    let want = p
        .items
        .iter()
        .filter(|i| {
            i.path.to_string_lossy().to_lowercase().contains("example")
                || i.reason.to_lowercase().contains("example")
        })
        .count();
    assert!(want > 0 && want < total);
    assert_eq!(shown(&h, f).len(), want);
    assert!(has(&h, &format!("Showing {want} of {total} items")));
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(
        h.state().plans[&f].search.is_empty(),
        "Esc clears the search"
    );
    // Selection filters, each with its count.
    let sel = p.items.iter().filter(|i| i.selected).count();
    let unsel = p
        .items
        .iter()
        .filter(|i| !i.selected && i.locked.is_none())
        .count();
    let locked = p.items.iter().filter(|i| i.locked.is_some()).count();
    filter(&mut h, &format!("Selected ({sel})"));
    assert_eq!(shown(&h, f).len(), sel);
    assert!(has(&h, &format!("Showing {sel} of {total} items")));
    click(&mut h, "Clear Filters");
    assert_eq!(shown(&h, f).len(), total);
    filter(&mut h, &format!("Not selected ({unsel})"));
    assert_eq!(shown(&h, f).len(), unsel);
    click(&mut h, "Clear Filters");
    if locked > 0 {
        filter(&mut h, &format!("Locked ({locked})"));
        assert_eq!(shown(&h, f).len(), locked);
        click(&mut h, "Remove filter Locked");
        assert_eq!(shown(&h, f).len(), total);
    }
    // Sorts: largest first by default, then name both ways.
    let sizes: Vec<u64> = loose(&h, f)
        .iter()
        .map(|x| p.items.iter().find(|i| &i.path == x).unwrap().size)
        .collect();
    assert!(
        sizes.windows(2).all(|w| w[0] >= w[1]),
        "largest first: {sizes:?}"
    );
    sort(&mut h, "Smallest first");
    let sizes: Vec<u64> = loose(&h, f)
        .iter()
        .map(|x| p.items.iter().find(|i| &i.path == x).unwrap().size)
        .collect();
    assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "smallest first");
    sort(&mut h, "Sort by name");
    let names: Vec<String> = loose(&h, f).iter().map(|p| name(p)).collect();
    assert!(names.windows(2).all(|w| w[0] <= w[1]), "A to Z: {names:?}");
    sort(&mut h, "Z to A");
    let names: Vec<String> = loose(&h, f).iter().map(|p| name(p)).collect();
    assert!(names.windows(2).all(|w| w[0] >= w[1]), "Z to A: {names:?}");
    assert_eq!(
        h.get_by_label("Sort").value().as_deref(),
        Some("Sort by name, Z to A")
    );
    filter(&mut h, &format!("Not selected ({unsel})"));
    // Remembered after a relaunch (and a new scan).
    let h2 = open(&mac, f);
    assert_eq!(
        h2.get_by_label("Sort").value().as_deref(),
        Some("Sort by name, Z to A")
    );
    assert_eq!(shown(&h2, f).len(), unsel);
    assert!(has(&h2, &format!("Showing {unsel} of {total} items")));
    let prefs = &Settings::load(&mac.env).lists["plan.system_junk"];
    assert_eq!(prefs.filters, ["unselected"]);
}

#[test]
fn hidden_selected_items_are_counted_listed_and_moved_only_after_confirming() {
    let mac = testkit_cleanup::junk_mac();
    let f = FeatureId::SystemJunk;
    let mut h = open(&mac, f);
    let p = plan(&h, f);
    let selected: Vec<PathBuf> = p
        .items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.path.clone())
        .collect();
    let n = selected.len();
    assert!(n > 0);
    let unsel = p
        .items
        .iter()
        .filter(|i| !i.selected && i.locked.is_none())
        .count();
    filter(&mut h, &format!("Not selected ({unsel})"));
    assert!(
        has_text(&h, &format!("{n} selected ({n} hidden by filters)")),
        "the bar counts every selected item"
    );
    // A short selection made while searching (Select All ticks only the items shown),
    // then hidden by a filter.
    click(&mut h, "Clear Filters");
    click(&mut h, "Select None");
    assert_eq!(plan(&h, f).selected_count(), 0);
    type_into(&mut h, "Search", "com.example.notes");
    let matching = shown(&h, f);
    assert!(!matching.is_empty() && matching.len() < 6);
    click(&mut h, "Select All");
    let selected: Vec<PathBuf> = plan(&h, f)
        .items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.path.clone())
        .collect();
    let mut want = matching
        .iter()
        .filter(|m| {
            plan(&h, f)
                .items
                .iter()
                .any(|i| &i.path == *m && i.locked.is_none())
        })
        .cloned()
        .collect::<Vec<_>>();
    want.sort();
    let mut got = selected.clone();
    got.sort();
    assert_eq!(got, want, "Select All ticks only what the search shows");
    click(&mut h, "Clear Filters");
    let unsel = plan(&h, f)
        .items
        .iter()
        .filter(|i| !i.selected && i.locked.is_none())
        .count();
    filter(&mut h, &format!("Not selected ({unsel})"));
    let n = selected.len();
    assert!(has_text(
        &h,
        &format!("{n} selected ({n} hidden by filters)")
    ));
    // Nothing moved yet; the confirmation lists every selected item, filters or not.
    assert!(selected.iter().all(|p| p.exists()));
    click(&mut h, "Review and Clean");
    for s in &selected {
        assert!(
            has(&h, &mac.env.display(s)),
            "{} is not in the confirmation",
            s.display()
        );
    }
    click(&mut h, "Move to Trash");
    for s in &selected {
        assert!(!s.exists(), "{} did not move", s.display());
    }
}

#[test]
fn large_and_old_kind_location_and_last_opened() {
    let mac = FakeMac::new();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&mac.env)
    .unwrap();
    let zip = mac.home_file("Downloads/big.zip", 2_500_000);
    let pdf = mac.home_file("Downloads/old.pdf", 1_200_000);
    age(&pdf, 400);
    let mov = mac.home_file("Movies/clip.mov", 3_000_000);
    let wav = mac.home_file("Music/song.wav", 2_200_000);
    age(&wav, 100);
    let f = FeatureId::LargeOldFiles;
    let mut h = open(&mac, f);
    assert_eq!(shown(&h, f).len(), 4);
    filter(&mut h, "Archives (1)");
    assert_eq!(shown(&h, f), [zip.clone()]);
    click(&mut h, "Clear Filters");
    filter(&mut h, "~/Downloads (2)");
    let mut d = shown(&h, f);
    d.sort();
    let mut want = vec![zip.clone(), pdf.clone()];
    want.sort();
    assert_eq!(d, want);
    // A kind of another place: kind and location must both match.
    filter(&mut h, "Movies (1)");
    assert!(shown(&h, f).is_empty());
    assert!(has(&h, "Showing 0 of 4 items"));
    assert!(has(&h, "No items match"));
    // Two places: either one.
    click(&mut h, "Remove filter Movies");
    filter(&mut h, "~/Movies (1)");
    assert_eq!(shown(&h, f).len(), 3);
    click(&mut h, "Clear Filters");
    // Last opened: longest unopened first, then the other way.
    sort(&mut h, "Sort by last opened");
    let s = shown(&h, f);
    assert_eq!(&s[..2], &[pdf.clone(), wav.clone()]);
    sort(&mut h, "Recently opened first");
    let s = shown(&h, f);
    assert_eq!(&s[2..], &[wav.clone(), pdf.clone()]);
    // Remembered.
    let h2 = open(&mac, f);
    assert_eq!(
        h2.get_by_label("Sort").value().as_deref(),
        Some("Sort by last opened, recently opened first")
    );
    let _ = mov;
}

#[test]
fn duplicates_filter_by_location() {
    let mac = FakeMac::new();
    let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let a = mac.env.home_path("Downloads/a.bin");
    let b = mac.env.home_path("Music/a.bin");
    let c = mac.env.home_path("Projects/a.bin");
    for p in [&a, &b, &c] {
        mac.write(p, &data);
    }
    let f = FeatureId::Duplicates;
    let mut h = open(&mac, f);
    assert_eq!(shown(&h, f).len(), 3);
    filter(&mut h, "~/Music (1)");
    assert_eq!(shown(&h, f), [b.clone()]);
    assert!(has(&h, "Showing 1 of 3 items"));
    click(&mut h, "Clear Filters");
    assert_eq!(shown(&h, f).len(), 3);
}

#[test]
fn every_results_page_offers_the_toolbar() {
    for (mac, f) in [
        (testkit_cleanup::mail_mac(), FeatureId::MailAttachments),
        (testkit_cleanup::privacy_mac(), FeatureId::Privacy),
        (testkit_cleanup::junk_mac(), FeatureId::SystemJunk),
    ] {
        let mut h = open(&mac, f);
        let p = plan(&h, f);
        assert!(!p.items.is_empty(), "{f:?} found nothing");
        assert!(has(&h, "Search") && has(&h, "Sort"), "{f:?}");
        let unsel = p
            .items
            .iter()
            .filter(|i| !i.selected && i.locked.is_none())
            .count();
        filter(&mut h, &format!("Not selected ({unsel})"));
        assert_eq!(shown(&h, f).len(), unsel, "{f:?}");
        click(&mut h, "Clear Filters");
        assert_eq!(shown(&h, f).len(), p.items.len(), "{f:?}");
    }
    // Trash Bins: nothing can be ticked there, so no selection filters to offer.
    let h = open(&testkit_cleanup::trash_mac(), FeatureId::TrashBins);
    let p = plan(&h, FeatureId::TrashBins);
    if p.items.iter().all(|i| i.locked.is_some()) {
        assert!(!has(&h, "Filter"), "a filter with nothing to choose");
    }
    assert!(has(&h, "Search"));
}
