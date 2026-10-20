//! Round 4, user report: "when I remove something the list is not getting updated".
//!
//! End to end on a fake Mac, for every plan module design4 owns (System Junk, Mail
//! Downloads, Privacy Traces, Large & Old Files, Duplicates, Similar Images) and Quick Sweep:
//! scan, select, confirm, done, back to the list. After that the moved items are gone from
//! every list on every page, and the list equals a fresh scan of the disk (same paths, same
//! total). Plus the cross-page cases: a cleanup on one page drops the same files from another
//! page's results and from Quick Sweep's tiles, and an Undo makes the other lists scan again.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_files::{ImageFormat, save_scene};
use cleancraft_core::{FeatureId, Settings, features, testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::pages::{plan_page, sweep};
use common::*;

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
}

/// Big, old, duplicate and look-alike files, with thresholds low enough that the duplicate
/// copies are also Large & Old Files.
fn files_mac() -> FakeMac {
    let m = FakeMac::sample();
    Settings {
        large_min_bytes: 200_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&m.env)
    .expect("save settings");
    m.home_file("Downloads/big.zip", 2_500_000);
    let old = m.home_file("Downloads/old.pdf", 1_200_000);
    age(&old, 400);
    let dup: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(7919) >> 3) as u8)
        .collect();
    for p in ["Downloads/a.bin", "Music/old/a.bin", "Projects/a.bin"] {
        m.write(&m.env.home_path(p), &dup);
    }
    let pics = m.env.home_path("Pictures");
    save_scene(&pics.join("a.png"), 320, 240, 0, 0, 1, ImageFormat::Png);
    save_scene(
        &pics.join("a-bright.png"),
        400,
        300,
        0,
        5,
        3,
        ImageFormat::Png,
    );
    save_scene(&pics.join("other.png"), 320, 240, 1, 0, 5, ImageFormat::Png);
    base(&m);
    m
}

fn junk_mac() -> FakeMac {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    m
}

fn mail_mac() -> FakeMac {
    let m = testkit_cleanup::mail_mac();
    base(&m);
    m
}

fn privacy_mac() -> FakeMac {
    let m = testkit_cleanup::privacy_mac();
    base(&m);
    m.env
        .fake_sys()
        .unwrap()
        .answer("tccutil", "Successfully reset\n");
    m
}

thread_local! {
    /// What the list showed right before the cleanup.
    static BEFORE: std::cell::RefCell<BTreeSet<PathBuf>> = Default::default();
}

fn go(h: &mut H, page: &str) {
    h.state_mut().navigate(page);
    steps(h);
}

fn paths(h: &mut H, f: FeatureId) -> BTreeSet<PathBuf> {
    h.state_mut()
        .plan_state(f)
        .plan
        .as_ref()
        .map(|p| p.items.iter().map(|i| i.path.clone()).collect())
        .unwrap_or_default()
}

/// Selected paths of the list on screen.
fn selected(h: &mut H, f: FeatureId) -> Vec<PathBuf> {
    h.state_mut()
        .plan_state(f)
        .plan
        .as_ref()
        .map(|p| p.selected().map(|i| i.path.clone()).collect())
        .unwrap_or_default()
}

/// Every item of every list the app keeps (plan pages and Quick Sweep tiles) still exists.
fn stale_items(h: &mut H) -> Vec<String> {
    let app = h.state_mut();
    let mut bad = Vec::new();
    // Labels inside a file (a Wi-Fi network in a plist, a permission in TCC.db) are not
    // files of their own.
    let gone = |p: &std::path::Path| {
        !p.exists()
            && !p
                .ancestors()
                .skip(1)
                .any(|a| a.is_file() || a.extension().is_some_and(|e| e == "plist" || e == "db"))
    };
    for (f, st) in &app.plans {
        if let Some(p) = &st.plan {
            for it in &p.items {
                if gone(&it.path) {
                    bad.push(format!("{}: {}", f.title(), it.path.display()));
                }
            }
        }
    }
    if let Some(res) = &app.sweep.result {
        for t in &res.tiles {
            if let Some(p) = t.plan() {
                for it in &p.items {
                    if gone(&it.path) {
                        bad.push(format!(
                            "Quick Sweep {}: {}",
                            t.id.title(),
                            it.path.display()
                        ));
                    }
                }
            }
        }
    }
    bad
}

/// Scan, keep the preselection (Mail Downloads: tick everything), confirm, Done; returns what
/// was selected.
fn clean(h: &mut H, f: FeatureId) -> Vec<PathBuf> {
    go(h, f.as_str());
    click(h, "Scan");
    steps(h);
    if f == FeatureId::MailAttachments {
        // Mail Downloads preselects nothing: the user ticks every attachment.
        assert!(selected(h, f).is_empty(), "Mail Downloads: preselected");
        click(h, "Select All");
    }
    let chosen = selected(h, f);
    assert!(!chosen.is_empty(), "{}: nothing preselected", f.title());
    let before = paths(h, f);
    BEFORE.with(|b| *b.borrow_mut() = before);
    click(h, "Review and Clean");
    let action = ["Move to Trash", "Move to Quarantine"]
        .into_iter()
        .find(|l| has(h, l))
        .expect("confirm button");
    click(h, action);
    steps(h);
    assert!(
        matches!(h.state_mut().plan_state(f).phase, plan_page::Phase::Done(_)),
        "{}: no Done screen",
        f.title()
    );
    click(h, "Done");
    chosen
}

/// The list after Done equals a fresh scan of the disk: same paths with the same sizes, and
/// the total is their sum. (A fresh scan may also list something that was not on the list
/// before the cleanup, such as a folder whose big children moved: that is new, not stale.)
fn check_against_fresh_scan(h: &mut H, mac: &FakeMac, f: FeatureId, moved: &[PathBuf]) {
    let shown = paths(h, f);
    for m in moved {
        assert!(
            !shown.contains(m),
            "{}: {} still listed",
            f.title(),
            m.display()
        );
    }
    let settings = Settings::load(&mac.env);
    let fresh = features::scan(&mac.env, &settings, f).expect("fresh scan");
    let fresh_paths: BTreeSet<PathBuf> = fresh.items.iter().map(|i| i.path.clone()).collect();
    let only_shown: Vec<_> = shown.difference(&fresh_paths).collect();
    let only_fresh: Vec<_> = fresh_paths
        .difference(&shown)
        .filter(|p| BEFORE.with(|b| b.borrow().contains(*p)))
        .collect();
    assert!(
        only_shown.is_empty() && only_fresh.is_empty(),
        "{}: list differs from a fresh scan\nonly listed: {only_shown:?}\nonly in a fresh scan: {only_fresh:?}\nmoved: {moved:?}",
        f.title()
    );
    let st = h.state_mut().plan_state(f);
    let total = st.plan.as_ref().map(|p| p.total_size()).unwrap_or(0);
    let fresh_total: u64 = fresh
        .items
        .iter()
        .filter(|i| shown.contains(&i.path))
        .map(|i| i.size)
        .sum();
    assert_eq!(total, fresh_total, "{}: total differs", f.title());
    if let Some(p) = &st.plan {
        for it in &p.items {
            let again = fresh
                .items
                .iter()
                .find(|i| i.path == it.path)
                .map(|i| i.size);
            assert_eq!(
                Some(it.size),
                again,
                "{}: size of {}",
                f.title(),
                it.path.display()
            );
        }
    }
    if shown.is_empty() {
        assert!(
            matches!(st.phase, plan_page::Phase::Idle | plan_page::Phase::Results),
            "{}: empty list",
            f.title()
        );
    } else {
        assert!(
            matches!(st.phase, plan_page::Phase::Results),
            "{}: Done goes back to the list",
            f.title()
        );
    }
}

fn module_flow(mac: FakeMac, f: FeatureId) {
    let mut h = harness(&mac);
    let moved = clean(&mut h, f);
    for m in &moved {
        assert!(!m.exists(), "{} did not move", m.display());
    }
    check_against_fresh_scan(&mut h, &mac, f, &moved);
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));
}

#[test]
fn system_junk_list_drops_what_moved() {
    module_flow(junk_mac(), FeatureId::SystemJunk);
}

#[test]
fn mail_downloads_list_drops_what_moved() {
    module_flow(mail_mac(), FeatureId::MailAttachments);
}

#[test]
fn privacy_list_drops_what_moved() {
    module_flow(privacy_mac(), FeatureId::Privacy);
}

#[test]
fn large_old_files_list_drops_what_moved() {
    let mac = files_mac();
    let mut h = harness(&mac);
    go(&mut h, "large_old_files");
    click(&mut h, "Scan");
    // Large & Old selects nothing on its own: pick the first item like a user.
    let first = h
        .state_mut()
        .plan_state(FeatureId::LargeOldFiles)
        .plan
        .as_ref()
        .and_then(|p| p.items.iter().find(|i| i.locked.is_none()).map(|i| i.id))
        .expect("an item");
    if let Some(p) = h
        .state_mut()
        .plan_state(FeatureId::LargeOldFiles)
        .plan
        .as_mut()
    {
        p.set_selected(first, true);
    }
    steps(&mut h);
    let moved = selected(&mut h, FeatureId::LargeOldFiles);
    let before = paths(&mut h, FeatureId::LargeOldFiles);
    BEFORE.with(|b| *b.borrow_mut() = before);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    click(&mut h, "Done");
    check_against_fresh_scan(&mut h, &mac, FeatureId::LargeOldFiles, &moved);
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));
}

#[test]
fn duplicates_list_drops_what_moved() {
    module_flow(files_mac(), FeatureId::Duplicates);
}

#[test]
fn similar_images_list_drops_what_moved() {
    module_flow(files_mac(), FeatureId::SimilarPhotos);
}

/// A cleanup on Duplicates drops the moved copies from Large & Old Files' results (same files
/// on two pages), without a rescan.
#[test]
fn a_cleanup_drops_the_same_files_from_other_pages() {
    let mac = files_mac();
    let mut h = harness(&mac);
    go(&mut h, "large_old_files");
    click(&mut h, "Scan");
    let before = paths(&mut h, FeatureId::LargeOldFiles);
    let moved = clean(&mut h, FeatureId::Duplicates);
    BEFORE.with(|b| *b.borrow_mut() = before.clone());
    assert!(
        moved.iter().any(|m| before.contains(m)),
        "the test needs a file on both pages"
    );
    let after = paths(&mut h, FeatureId::LargeOldFiles);
    for m in &moved {
        assert!(
            !after.contains(m),
            "Large & Old still lists {}",
            m.display()
        );
    }
    // Nothing else left the list, and it equals a fresh scan when shown again.
    assert_eq!(
        after.len(),
        before.len() - moved.iter().filter(|m| before.contains(*m)).count()
    );
    go(&mut h, "large_old_files");
    check_against_fresh_scan(&mut h, &mac, FeatureId::LargeOldFiles, &moved);
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));
}

/// Quick Sweep's tiles lose what a module page moved; a Quick Sweep cleanup drops what it
/// moved from the module pages that showed it.
#[test]
fn quick_sweep_and_module_pages_stay_in_step() {
    let mac = testkit_monitor::sweep_mac();
    base(&mac);
    let mut h = harness(&mac);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    go(&mut h, "smart_care");
    click(&mut h, "Scan");
    assert!(matches!(h.state_mut().sweep.phase, sweep::Phase::Results));
    let tile_before: usize = h
        .state_mut()
        .sweep
        .result
        .as_ref()
        .and_then(|r| r.tile(FeatureId::SystemJunk))
        .and_then(|t| t.plan())
        .map(|p| p.items.len())
        .unwrap_or(0);
    assert!(
        tile_before > 0,
        "the sweep has a System Junk tile with items"
    );
    let moved = clean(&mut h, FeatureId::SystemJunk);
    let tile = h
        .state_mut()
        .sweep
        .result
        .as_ref()
        .and_then(|r| r.tile(FeatureId::SystemJunk))
        .and_then(|t| t.plan())
        .cloned()
        .expect("tile");
    for m in &moved {
        assert!(
            !tile.items.iter().any(|i| &i.path == m),
            "the Quick Sweep tile still lists {}",
            m.display()
        );
    }
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));

    // The other way: module page results, then a Quick Sweep cleanup.
    let mac = testkit_monitor::sweep_mac();
    base(&mac);
    let mut h = harness(&mac);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    go(&mut h, "smart_care");
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    steps(&mut h);
    assert!(matches!(h.state_mut().sweep.phase, sweep::Phase::Done(_)));
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));
}

/// Undo on the Done screen puts the files back: the page's own list and the other pages'
/// lists show them again on the next look.
#[test]
fn undo_brings_items_back_on_every_page() {
    let mac = files_mac();
    let mut h = harness(&mac);
    go(&mut h, "large_old_files");
    click(&mut h, "Scan");
    let before = paths(&mut h, FeatureId::LargeOldFiles);
    go(&mut h, "duplicates");
    click(&mut h, "Scan");
    let dup_before = paths(&mut h, FeatureId::Duplicates);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    click(&mut h, "Undo");
    steps(&mut h);
    assert_eq!(paths(&mut h, FeatureId::Duplicates), dup_before);
    go(&mut h, "large_old_files");
    steps(&mut h);
    assert_eq!(paths(&mut h, FeatureId::LargeOldFiles), before);
    assert!(stale_items(&mut h).is_empty(), "{:?}", stale_items(&mut h));
}

/// Moves made anywhere else (App Remover, Disk Map, History, or CleanCraft's agent API) reach
/// the plan pages through the action log: the list drops what moved; an Undo there makes the
/// list scan again when it is shown.
#[test]
fn moves_from_other_pages_reach_the_lists_through_the_log() {
    let mac = files_mac();
    let mut h = harness(&mac);
    go(&mut h, "large_old_files");
    click(&mut h, "Scan");
    let before = paths(&mut h, FeatureId::LargeOldFiles);
    // Another page moves one of the listed files (same executor, same log).
    let settings = Settings::load(&mac.env);
    let mut plan = features::scan(&mac.env, &settings, FeatureId::LargeOldFiles).expect("scan");
    let target = plan.items[0].path.clone();
    assert!(before.contains(&target));
    for i in 0..plan.items.len() {
        plan.set_selected(i, i == 0);
    }
    let rep = features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {})
        .expect("move");
    assert_eq!(rep.moved.len(), 1);
    steps(&mut h);
    let after = paths(&mut h, FeatureId::LargeOldFiles);
    assert!(
        !after.contains(&target),
        "the list still shows the moved file"
    );
    assert_eq!(after.len(), before.len() - 1);
    // Put back from elsewhere: the list scans again and shows it.
    let (n, errs) = cleancraft_core::executor::undo_plan(&mac.env, &rep.plan_id);
    assert_eq!((n, errs.len()), (1, 0));
    steps(&mut h);
    go(&mut h, "large_old_files");
    steps(&mut h);
    assert_eq!(paths(&mut h, FeatureId::LargeOldFiles), before);
}
