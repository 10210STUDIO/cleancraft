//! Shared helpers for UI tests: a CleanApp on a fake Mac, driven by clicks and typing.
#![allow(dead_code)]

use cleancraft_core::testkit::FakeMac;
use cleancraft_ui_egui::CleanApp;
use egui_kittest::kittest::Queryable;

pub type H = egui_kittest::Harness<'static, CleanApp>;

pub const W: f32 = 1280.0;
pub const HGT: f32 = 820.0;

/// The app on `mac`, work running inline. Keep `mac` alive as long as the harness.
pub fn harness(mac: &FakeMac) -> H {
    let app = CleanApp::new(mac.env.clone()).with_sync();
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

pub fn steps(h: &mut H) {
    for _ in 0..4 {
        h.step();
    }
}

pub fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

pub fn has_text(h: &H, needle: &str) -> bool {
    h.query_all_by_label_contains(needle).next().is_some()
}

/// Click a widget by its label. Fails if the widget lies outside the window: a control the
/// user cannot reach counts as a dead control.
pub fn click(h: &mut H, label: &str) {
    let node = h.get_by_label(label);
    let r = node.rect();
    assert!(
        r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5,
        "'{label}' is outside the window: {r:?}"
    );
    node.click();
    steps(h);
}

/// Click the n-th widget with this label (labels repeat, e.g. "Reveal").
pub fn click_nth(h: &mut H, label: &str, n: usize) {
    let node = h
        .get_all_by_label(label)
        .nth(n)
        .unwrap_or_else(|| panic!("no {n}th '{label}'"));
    node.click();
    steps(h);
}

pub fn type_into(h: &mut H, label: &str, text: &str) {
    let node = h.get_by_label(label);
    node.focus();
    node.type_text(text);
    steps(h);
}

/// The n-th control named "<what> <tile>" (Quick Sweep's tiles name their Review and Include
/// after the module since fix7 U7-12), in tile order.
pub fn tile_label(h: &H, what: &str, n: usize) -> String {
    use egui_kittest::kittest::NodeT;
    let prefix = format!("{what} ");
    h.root()
        .children_recursive()
        .filter_map(|x| x.accesskit_node().label())
        .filter(|l| is_tile_label(l, &prefix))
        .nth(n)
        .unwrap_or_else(|| panic!("no control #{n} starting with {prefix}"))
}

/// How many controls are named "<what> <tile>".
pub fn count_tile(h: &H, what: &str) -> usize {
    use egui_kittest::kittest::NodeT;
    let prefix = format!("{what} ");
    let kinds = [
        egui::accesskit::Role::Button,
        egui::accesskit::Role::CheckBox,
    ];
    h.root()
        .children_recursive()
        .filter(|x| kinds.contains(&x.accesskit_node().role()))
        .filter_map(|x| x.accesskit_node().label())
        .filter(|l| is_tile_label(l, &prefix))
        .count()
}

fn is_tile_label(l: &str, prefix: &str) -> bool {
    l.strip_prefix(prefix).is_some_and(|rest| {
        cleancraft_core::FeatureId::ALL
            .iter()
            .any(|f| cleancraft_core::sweep::tile_title(*f) == rest)
    })
}
