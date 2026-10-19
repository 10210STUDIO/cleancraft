//! The redesigned screens keep working at the smallest window (1060 x 680): the start screen's
//! big Scan button, the results' bottom bar, the confirmation and Done with Undo are all inside
//! the window and act; scans that can stop show Stop, and Stop leaves nothing changed.

use std::sync::atomic::Ordering;

use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::{plan_page, sweep};
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

const W: f32 = 1060.0;
const HGT: f32 = 680.0;

fn harness(mac: &FakeMac) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = Access::Granted;
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

fn steps(h: &mut H) {
    for _ in 0..5 {
        h.step();
    }
}

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn has_text(h: &H, needle: &str) -> bool {
    h.query_all_by_label_contains(needle).next().is_some()
}

/// Click a control by label; it must lie inside the small window.
fn click(h: &mut H, label: &str) {
    let node = h.get_by_label(label);
    let r = node.rect();
    assert!(
        r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5,
        "'{label}' is outside the {W}x{HGT} window: {r:?}"
    );
    node.click();
    steps(h);
}

#[test]
fn system_junk_flow_fits_the_smallest_window() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    let before = h.state().action_count();
    click(&mut h, "Scan");
    assert!(h.state().action_count() > before);
    assert!(has_text(&h, "found in"));
    let plan = h
        .state_mut()
        .plan_state(FeatureId::SystemJunk)
        .plan
        .clone()
        .expect("a plan");
    let chosen: Vec<_> = plan.selected().map(|i| i.path.clone()).collect();
    assert!(!chosen.is_empty());
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(has_text(&h, "moved to the Trash"));
    assert!(chosen.iter().all(|p| !p.exists()), "selected items moved");
    click(&mut h, "Undo");
    assert!(chosen.iter().all(|p| p.exists()), "Undo put them back");
}

#[test]
fn stop_ends_a_plan_scan_and_changes_nothing() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    h.state_mut().plan_state(FeatureId::SystemJunk).phase =
        plan_page::Phase::Scanning(Job::waiting("~/Library/Caches"));
    steps(&mut h);
    assert!(has_text(&h, "Scanning System Junk"));
    assert!(has_text(&h, "Looking in ~/Library/Caches"));
    click(&mut h, "Stop");
    let st = h.state_mut().plan_state(FeatureId::SystemJunk);
    assert!(matches!(st.phase, plan_page::Phase::Idle));
    assert!(
        st.cancel.load(Ordering::Relaxed),
        "the scan was told to stop"
    );
    assert!(st.plan.is_none());
    assert!(has_text(&h, "System Junk scan stopped. Nothing changed."));
    assert!(has(&h, "Scan"));
}

#[test]
fn scans_that_cannot_stop_show_no_stop_button() {
    let mac = testkit_cleanup::trash_mac();
    let mut h = harness(&mac);
    click(&mut h, "Trash Bins");
    assert!(!plan_page::can_stop(FeatureId::TrashBins));
    h.state_mut().plan_state(FeatureId::TrashBins).phase =
        plan_page::Phase::Scanning(Job::waiting("Trash"));
    steps(&mut h);
    assert!(has_text(&h, "Scanning Trash Bins"));
    assert!(!has(&h, "Stop"));
}

#[test]
fn stop_ends_a_quick_sweep_scan() {
    let mac = testkit_monitor::sweep_mac();
    let mut h = harness(&mac);
    h.state_mut().sweep.phase = sweep::Phase::Scanning(Job::waiting("System Junk"));
    steps(&mut h);
    assert!(has_text(&h, "Sweeping your Mac"));
    click(&mut h, "Stop");
    assert!(matches!(h.state().sweep.phase, sweep::Phase::Idle));
    assert!(h.state().sweep.cancel.load(Ordering::Relaxed));
    assert!(has_text(&h, "Quick Sweep stopped. Nothing changed."));
}

#[test]
fn quick_sweep_flow_fits_the_smallest_window() {
    let mac = testkit_monitor::sweep_mac();
    let mut h = harness(&mac);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    click(&mut h, "Scan");
    assert!(has_text(&h, "ready to clean"));
    click(&mut h, "Run");
    click(&mut h, "Move to Trash");
    assert!(has_text(&h, "moved to the Trash"));
    click(&mut h, "Done");
    assert!(has(&h, "Scan"));
}
