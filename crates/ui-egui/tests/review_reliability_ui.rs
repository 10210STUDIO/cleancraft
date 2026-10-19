//! Reliability review (reviewer "reliability"): the window under real-world failures. Big
//! plans, background jobs while the user switches pages or clicks twice, a dead cleanup job,
//! damaged settings, restart.
//!
//! Ignored tests prove defects (ID in the ignore reason); the others protect behaviour that was
//! checked and found right. Run: `cargo test -p cleancraft-ui-egui --test review_reliability_ui`
//! and the defects with `-- --ignored`.

mod common;

use std::time::Duration;

use cleancraft_core::executor::{self, ActionKind, Report};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Settings};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::plan_page::Phase;
use common::*;
use egui_kittest::kittest::Queryable;

/// The app with real background threads (as in the shipped app), not inline work.
fn async_harness(mac: &FakeMac) -> H {
    let app = CleanApp::new(mac.env.clone());
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(W, HGT))
        .build_ui_state(
            |ui, app: &mut CleanApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app,
        );
    steps(&mut h);
    h
}

/// Step frames until `done` holds (background thread finished), at most ~10 s.
fn wait_until(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..1000 {
        h.step();
        if done(h) {
            steps(h);
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn phase_is(h: &H, f: FeatureId, want: &str) -> bool {
    let Some(st) = h.state().plans.get(&f) else {
        return false;
    };
    let now = match st.phase {
        Phase::Idle => "idle",
        Phase::Scanning(_) => "scanning",
        Phase::Results => "results",
        Phase::Confirm => "confirm",
        Phase::Cleaning(_) => "cleaning",
        Phase::Done(_) => "done",
    };
    now == want
}

/// `n` app cache folders, each with one small file: System Junk proposes and selects each.
fn many_caches(n: usize) -> FakeMac {
    let mac = FakeMac::new();
    for i in 0..n {
        mac.home_file(&format!("Library/Caches/com.example.app{i:05}/c.bin"), 600);
    }
    mac
}

fn cache_rows(h: &H, mac: &FakeMac) -> usize {
    let prefix = mac
        .env
        .display(&mac.env.home_path("Library/Caches/com.example.app"));
    h.query_all_by_label_contains(&prefix).count()
}

// ---------------------------------------------------------------------------------------------
// Defects
// ---------------------------------------------------------------------------------------------

/// REL-7. The confirmation dialog builds one row per selected item every frame (a Grid in a
/// ScrollArea, not `show_rows`). With thousands of items (a big cache folder, Large & Old Files on
/// a full disk) the dialog lays out every row on every frame and the window stalls; the item list
/// behind it is virtualised and stays light.
#[test]
fn confirm_dialog_with_thousands_of_items_lays_out_only_visible_rows() {
    let mac = many_caches(3000);
    let mut h = harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    assert_eq!(
        h.state().plans[&FeatureId::SystemJunk]
            .plan
            .as_ref()
            .unwrap()
            .selected_count(),
        3000
    );
    let list = cache_rows(&h, &mac);
    assert!(list < 100, "item list builds {list} rows");
    click(&mut h, "Review and Clean");
    let dialog = cache_rows(&h, &mac) - list;
    assert!(
        dialog < 200,
        "the confirmation builds {dialog} rows per frame for 3000 items"
    );
}

/// REL-8. A damaged settings.json (truncated by a crash or full disk) is read as "no settings":
/// no warning, and the ignore list is gone. The next scan proposes and preselects what the user
/// had put on the ignore list, and the next settings save overwrites the old file for good.
#[test]
fn damaged_settings_are_reported_and_the_ignore_list_survives() {
    let mac = FakeMac::sample();
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    let s = Settings {
        ignore: vec![editor.clone()],
        ..Settings::default()
    };
    s.save(&mac.env).unwrap();
    let full = std::fs::read(Settings::path(&mac.env)).unwrap();
    std::fs::write(Settings::path(&mac.env), &full[..full.len() - 4]).unwrap();
    let mut h = harness(&mac);
    // A warning in the notice bar that names the settings file.
    let warned = h
        .state()
        .notice
        .as_ref()
        .is_some_and(|(m, _)| m.to_lowercase().contains("settings"));
    assert!(warned, "no warning about the damaged settings file");
    let copy = Settings::path(&mac.env).with_extension("damaged.json");
    assert_eq!(
        std::fs::read(&copy).ok().as_deref(),
        Some(&full[..full.len() - 4]),
        "the damaged file is not kept as a copy"
    );
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let proposed = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .items
        .iter()
        .any(|i| i.path == editor && i.selected);
    assert!(
        warned || !proposed,
        "no word about the damaged settings, and the ignored {} is proposed and selected again",
        mac.env.display(&editor)
    );
}

/// REL-10. When the cleanup job ends without a result (the worker thread panicked), the page
/// keeps showing "Moving the selected items..." with a spinner forever: no error, no way back to
/// the list. Scans handle this case ("The scan stopped unexpectedly."), cleanups do not.
#[test]
fn dead_cleanup_job_is_reported() {
    let mac = FakeMac::sample();
    let mut h = async_harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "results"
    )));
    // The worker dies the way any panic in run_plan or a feature's prepare would end it.
    h.state_mut()
        .plans
        .get_mut(&FeatureId::SystemJunk)
        .unwrap()
        .phase = Phase::Cleaning(Job::start(false, |_| -> Result<Report, String> {
        panic!("worker died")
    }));
    let left = wait_until(&mut h, |h| !has_text(h, "Moving the selected items"));
    assert!(
        left,
        "still 'Moving the selected items...' after the job died"
    );
}

// ---------------------------------------------------------------------------------------------
// Checked and found right
// ---------------------------------------------------------------------------------------------

#[test]
fn item_list_of_thousands_lays_out_only_visible_rows() {
    let mac = many_caches(3000);
    let mut h = harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let rows = cache_rows(&h, &mac);
    assert!(rows > 0 && rows < 100, "{rows} rows built");
    let total: u64 = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| tree_size(&i.path).0)
        .sum();
    assert!(has(&h, &human_size(total)));
}

#[test]
fn switching_pages_while_scanning_keeps_the_result() {
    let mac = FakeMac::sample();
    let mut h = async_harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    click(&mut h, "History");
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "results"
    )));
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    let plan = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .clone()
        .unwrap();
    let total: u64 = plan.items.iter().map(|i| tree_size(&i.path).0).sum();
    assert!(has(&h, &human_size(total)));
    assert!(has(&h, "Review and Clean"));
}

#[test]
fn move_to_trash_cannot_be_clicked_twice() {
    let mac = FakeMac::sample();
    let mut h = async_harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "results"
    )));
    let selected = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .selected_count();
    click(&mut h, "Review and Clean");
    h.get_by_label("Move to Trash").click();
    h.step();
    h.step();
    // The second click of a double click finds no button.
    assert!(
        h.query_by_label("Move to Trash").is_none(),
        "the button is still there"
    );
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "done"
    )));
    let hist = executor::history(&mac.env);
    assert_eq!(
        hist.len(),
        selected,
        "one journal line per item, no repeats"
    );
    let mut ids: Vec<_> = hist.iter().map(|r| r.id.clone()).collect();
    ids.dedup();
    assert_eq!(ids.len(), selected);
}

#[test]
fn scan_button_is_gone_while_scanning_and_undo_runs_once() {
    let mac = FakeMac::sample();
    let mut h = async_harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    h.get_by_label("Scan").click();
    h.step();
    h.step();
    assert!(h.query_by_label("Scan").is_none());
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "results"
    )));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(wait_until(&mut h, |h| phase_is(
        h,
        FeatureId::SystemJunk,
        "done"
    )));
    let moved = executor::history(&mac.env).len();
    h.get_by_label("Undo").click();
    h.step();
    h.step();
    assert!(h.query_by_label("Undo").is_none(), "Undo still offered");
    assert!(has_text(&h, &format!("{moved} items put back")));
}

#[test]
fn history_undo_after_trash_emptied_says_why() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    // Finder empties the Trash.
    for r in executor::history(&mac.env) {
        if let ActionKind::Moved { stored, .. } = r.kind {
            let _ = std::fs::remove_dir_all(&stored);
            let _ = std::fs::remove_file(&stored);
        }
    }
    click(&mut h, "History");
    click_nth(&mut h, "Undo", 0);
    assert!(has_text(&h, "Trash emptied"));
}

#[test]
fn ignore_and_history_survive_a_restart() {
    let mac = FakeMac::sample();
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    {
        let mut h = harness(&mac);
        // The tool sits in the "Clean" section of the sidebar.
        click(&mut h, "Clean");
        click(&mut h, "System Junk");
        click(&mut h, "Scan");
        type_into(&mut h, "Search", "example.editor");
        click(&mut h, "Ignore");
        click(&mut h, "Review and Clean");
        click(&mut h, "Move to Trash");
        assert!(editor.exists());
    }
    // A new window on the same Mac.
    let mut h = harness(&mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let plan = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .clone()
        .unwrap();
    assert!(!plan.items.iter().any(|i| i.path == editor));
    click(&mut h, "History");
    let moved = executor::history(&mac.env);
    assert!(!moved.is_empty());
    click_nth(&mut h, "Undo", 0);
    assert!(has(&h, "Put back."));
    let undone = executor::history(&mac.env)
        .into_iter()
        .filter(|r| r.undone.is_some())
        .count();
    assert_eq!(undone, 1);
}
