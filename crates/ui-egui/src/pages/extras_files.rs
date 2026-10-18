//! Extra controls for this builder's plan modules (see pages/extras.rs).

use cleancraft_core::FeatureId;
use egui::Ui;

use crate::app::CleanApp;

pub fn before_scan(_app: &mut CleanApp, _ui: &mut Ui, _f: FeatureId) {}

pub fn results_toolbar(_app: &mut CleanApp, _ui: &mut Ui, _f: FeatureId) {}
