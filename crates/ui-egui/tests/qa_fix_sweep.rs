//! FIX-4 by clicks: Quick Sweep counts tune-up suggestions and app updates on the real
//! registry. Each tile's number is checked against its page: the Tune-up Tasks page shows one
//! "Suggested" line per counted task, and App Updates finds the same number of updates.
//! Running a suggested task lowers the count; a failed update check is "Could not scan", not 0.

mod common;

use cleancraft_core::FeatureId;
use cleancraft_core::sweep::TileData;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::updater;
use cleancraft_ui_egui::app::Page;
use common::*;
use egui_kittest::kittest::Queryable;

const CAST: &str = "https://example.com/editor/cast.xml";

fn mac() -> FakeMac {
    let mac = FakeMac::new();
    let fs = mac.env.fake_sys().unwrap();
    fs.answer("id -u", "501\n");
    fs.answer("sw_vers -productVersion", "14.6.1\n");
    fs.answer(
        "/usr/sbin/diskutil verifyVolume /",
        "The volume appears to be OK\n",
    );
    mac.app_spec(&AppSpec {
        build: "200",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Notes Pro", "com.maker.NotesPro", "3.1")
    });
    mac.web(
        CAST,
        &appcast(&[("210", "2.1", "<ul><li>Faster</li></ul>")]),
    );
    mac.web(
        &updater::lookup_url("com.maker.NotesPro"),
        r#"{"resultCount":1,"results":[{"version":"3.2","releaseNotes":"Sync fixes"}]}"#,
    );
    fs.answer(
        "/usr/sbin/softwareupdate --list",
        "No new software available.\n",
    );
    mac
}

fn tile_count(h: &H, id: FeatureId) -> Option<usize> {
    match &h.state().sweep.result.as_ref()?.tile(id)?.data {
        TileData::Count { count, .. } => Some(*count),
        _ => None,
    }
}

/// Click the Review button of a tile (tiles are in result order), scrolled into view first.
fn review(h: &mut H, id: FeatureId) {
    let n = h
        .state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tiles
        .iter()
        .position(|t| t.id == id)
        .expect("tile");
    let node = h.get_all_by_label("Review").nth(n).expect("Review button");
    node.scroll_to_me();
    steps(h);
    let node = h.get_all_by_label("Review").nth(n).unwrap();
    let r = node.rect();
    assert!(r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5 && r.min.y >= 0.0);
    node.click();
    steps(h);
}

/// Scroll a control into view, then click it like a user.
fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

#[test]
fn tune_up_suggestions_are_counted_and_match_the_tasks_page() {
    let m = mac();
    let mut h = harness(&m);
    click(&mut h, "Scan");
    // macOS 14 without Mail: verify disk and Launch Services are due (never run); periodic
    // scripts do not apply, Mail has no index.
    assert_eq!(tile_count(&h, FeatureId::Maintenance), Some(2));
    assert!(has(&h, "2 suggestions"));
    review(&mut h, FeatureId::Maintenance);
    assert_eq!(h.state().page, Page::Feature(FeatureId::Maintenance));
    assert_eq!(
        h.query_all_by_label("Suggested: never run by CleanCraft")
            .count(),
        2
    );
    // Run one suggested task: it is no longer suggested, and the sweep counts one less.
    reach(&mut h, "Verify startup disk");
    reach(&mut h, "Run Selected");
    click(&mut h, "Run");
    assert!(has_text(&h, "1 done, 0 failed, 0 cancelled."));
    assert_eq!(
        h.query_all_by_label("Suggested: never run by CleanCraft")
            .count(),
        1
    );
    click(&mut h, "Quick Sweep");
    click(&mut h, "Scan Again");
    assert_eq!(tile_count(&h, FeatureId::Maintenance), Some(1));
    assert!(has(&h, "1 suggestions"));
}

#[test]
fn app_updates_are_counted_and_match_the_updates_page() {
    let m = mac();
    let mut h = harness(&m);
    click(&mut h, "Scan");
    assert_eq!(tile_count(&h, FeatureId::Updater), Some(2));
    assert!(has(&h, "2 updates"));
    review(&mut h, FeatureId::Updater);
    assert_eq!(h.state().page, Page::Feature(FeatureId::Updater));
    click(&mut h, "Check for Updates");
    assert!(has(&h, "2 updates available"));
}

#[test]
fn update_check_failing_everywhere_is_not_a_zero() {
    let m = mac();
    m.web_fails(CAST, 6);
    m.web_fails(&updater::lookup_url("com.maker.NotesPro"), 6);
    let mut h = harness(&m);
    click(&mut h, "Scan");
    let tile = h
        .state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tile(FeatureId::Updater)
        .unwrap()
        .data
        .clone();
    assert!(
        matches!(&tile, TileData::Failed(e) if e.contains("no app could be checked")),
        "{tile:?}"
    );
    assert!(has(&h, "Could not scan"));
}
