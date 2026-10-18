//! Page owned by a builder (see docs/agents). Not built yet.

use egui::Ui;

use crate::app::CleanApp;

#[derive(Default)]
pub struct State {}

/// Finish background work. Returns true while something runs.
pub fn poll(_app: &mut CleanApp) -> bool {
    false
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: cleancraft_core::FeatureId) {
    let t = app.t;
    crate::kit::title(ui, &t, f.title());
}
