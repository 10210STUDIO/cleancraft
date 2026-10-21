//! Quick Sweep by clicks: Scan, tile numbers against each module's plan recalculated from
//! disk, one confirmation listing every file of every included module, Run moves exactly
//! those, Undo per module, Include toggle, Review opens the module with its plan loaded, and
//! the error paths (file in use, failed module, cancel).
//!
//! Most tests use System Junk (real) plus stand-ins for Mail, Trash, a failing Threat Check
//! and an updates counter (`testkit_monitor::sweep_sources`), because those modules belong to
//! other builders. `registered_modules_only` runs on the real registry.

mod common;

use std::path::PathBuf;

use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sweep::{Sources, TileData};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{FeatureId, Plan, Settings};
use cleancraft_ui_egui::app::Page;
use cleancraft_ui_egui::pages::plan_page;
use common::*;
use egui_kittest::kittest::Queryable;

fn open(m: &FakeMac, stand_ins: bool) -> H {
    let mut h = harness(m);
    if stand_ins {
        h.state_mut().sweep.sources = Some(sweep_sources());
        // The start screen lays out its chips for these sources first (fix7: the chips are
        // toggles now, so their lines can change).
        for _ in 0..4 {
            h.step();
        }
    }
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
    h
}

fn scanned(m: &FakeMac) -> H {
    let mut h = open(m, true);
    click(&mut h, "Scan");
    h
}

fn plan_of(h: &H, id: FeatureId) -> Plan {
    h.state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tile(id)
        .unwrap()
        .plan()
        .unwrap()
        .clone()
}

fn selected_on_disk(p: &Plan) -> u64 {
    p.selected().map(|i| tree_size(&i.path).0).sum()
}

fn count(h: &H, m: &FakeMac, p: &std::path::Path) -> usize {
    h.query_all_by_label(&m.env.display(p)).count()
}

#[test]
fn tiles_show_each_module_plan() {
    let m = sweep_mac();
    let h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    let trash = plan_of(&h, FeatureId::TrashBins);
    let j = selected_on_disk(&junk);
    let ml = selected_on_disk(&mail);
    assert!(has(&h, &human_size(j)), "junk tile {}", human_size(j));
    assert!(has(&h, &human_size(ml)), "mail tile {}", human_size(ml));
    assert!(has(
        &h,
        &format!(
            "{} of {} items selected, {} found",
            mail.selected_count(),
            mail.items.len(),
            human_size(mail.total_size())
        )
    ));
    assert_eq!(mail.selected_count(), 2);
    let t: u64 = trash.items.iter().map(|i| tree_size(&i.path).0).sum();
    assert!(has(&h, &human_size(t)), "trash tile");
    assert!(has(&h, "3 updates"));
    assert!(has(&h, "Could not scan"));
    assert!(has(&h, "Rules could not be loaded."));
    // Header: what Run would move (Trash excluded).
    assert!(has(&h, &human_size(j + ml)));
    // One Review per tile, one Include per runnable tile (System Junk, Mail).
    assert_eq!(count_tile(&h, "Review"), 5);
    assert_eq!(count_tile(&h, "Include"), 2);
}

#[test]
fn run_lists_everything_moves_exactly_that_and_undo_per_module() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    let trash = plan_of(&h, FeatureId::TrashBins);
    let listed: Vec<PathBuf> = junk
        .selected()
        .chain(mail.selected())
        .map(|i| i.path.clone())
        .collect();
    let mail_size = selected_on_disk(&mail);
    let expect_freed = selected_on_disk(&junk) + mail_size;
    click(&mut h, "Review and Clean");
    for p in &listed {
        assert_eq!(count(&h, &m, p), 1, "{} not listed once", p.display());
    }
    let keep = m.env.home_path("Library/Mail Downloads/keep-c.doc");
    assert_eq!(count(&h, &m, &keep), 0, "unselected file listed");
    for i in &trash.items {
        assert_eq!(count(&h, &m, &i.path), 0, "Trash item listed");
    }
    let n: usize = junk.selected_count() + mail.selected_count();
    assert!(has(
        &h,
        &format!("Clean {n} items ({})?", human_size(expect_freed))
    ));
    click(&mut h, "Move to Trash");
    let all: Vec<PathBuf> = [&junk, &mail, &trash]
        .iter()
        .flat_map(|p| p.items.iter().map(|i| i.path.clone()))
        .collect();
    for p in &all {
        assert_eq!(p.exists(), !listed.contains(p), "{}", p.display());
    }
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect_freed))
    ));
    assert!(has(&h, &format!("2 moved, {}", human_size(mail_size))));
    // Undo only Mail (second result card).
    assert_eq!(h.query_all_by_label("Undo").count(), 2);
    click_nth(&mut h, "Undo", 1);
    for i in mail.selected() {
        assert!(i.path.exists(), "{} not put back", i.path.display());
    }
    for i in junk.selected() {
        assert!(!i.path.exists(), "junk came back");
    }
    assert!(has(&h, "Put back"));
    assert!(has_text(&h, "Mail Downloads: 2 items put back."));
    click(&mut h, "Done");
    assert!(has(&h, "Scan"));
}

#[test]
fn include_toggle_skips_a_module_and_is_saved() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    let l = tile_label(&h, "Include", 0);
    click(&mut h, &l);
    assert_eq!(h.state().settings.sweep_skip, vec![FeatureId::SystemJunk]);
    assert_eq!(
        Settings::load(&m.env).sweep_skip,
        vec![FeatureId::SystemJunk]
    );
    assert!(has(&h, &human_size(selected_on_disk(&mail))));
    click(&mut h, "Review and Clean");
    for i in junk.selected() {
        assert_eq!(count(&h, &m, &i.path), 0, "skipped module listed");
    }
    click(&mut h, "Move to Trash");
    for i in junk.items.iter() {
        assert!(i.path.exists(), "skipped module touched");
    }
    for i in mail.selected() {
        assert!(!i.path.exists());
    }
    // Turn it back on: fix7, the start screen's chip is the same choice, and a module left
    // out is not scanned (so it has no tile to tick).
    click(&mut h, "Done");
    assert!(has(&h, "Scan 4 of 5"));
    click(&mut h, "System Junk");
    assert!(h.state().settings.sweep_skip.is_empty());
    click(&mut h, "Scan");
}

#[test]
fn review_opens_the_module_with_its_plan() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let l = tile_label(&h, "Review", 0);
    click(&mut h, &l);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SystemJunk));
    let st = &h.state().plans[&FeatureId::SystemJunk];
    assert!(matches!(st.phase, plan_page::Phase::Results));
    assert_eq!(st.plan.as_ref().unwrap().id, junk.id);
    assert!(has(&h, "Review and Clean"));
    // The count tile opens its page.
    click(&mut h, "Quick Sweep");
    let l = tile_label(&h, "Review", 4);
    click(&mut h, &l);
    assert_eq!(h.state().page, Page::Feature(FeatureId::Updater));
}

#[test]
fn cancel_moves_nothing() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    click(&mut h, "Review and Clean");
    click(&mut h, "Cancel");
    assert!(junk.items.iter().all(|i| i.path.exists()));
    assert!(has(&h, "Review and Clean"));
}

#[test]
fn file_in_use_is_skipped_and_shown() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let b = m.env.home_path("Library/Mail Downloads/b.zip");
    m.env.fake_sys().unwrap().set_open(&b);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(b.exists());
    assert!(has(&h, "open in a running app"));
    assert!(has_text(&h, "1 moved, 1 skipped"));
}

#[test]
fn registered_modules_only() {
    let m = sweep_mac();
    let mut h = open(&m, false);
    click(&mut h, "Scan");
    let ids: Vec<FeatureId> = h
        .state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tiles
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, Sources::registered().ids());
    assert_eq!(count_tile(&h, "Review"), ids.len());
    // System Junk is always there; its number matches its plan on disk.
    let junk = plan_of(&h, FeatureId::SystemJunk);
    assert!(has(&h, &human_size(selected_on_disk(&junk))));
    assert!(matches!(
        h.state().sweep.result.as_ref().unwrap().tiles[0].data,
        TileData::Plan(_)
    ));
}
