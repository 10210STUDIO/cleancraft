//! Pages. Plan-based modules share [`plan_page`]; the others have their own file.

pub mod apps;
pub mod disk_map;
pub mod extras;
pub mod extras_cleanup;
pub mod extras_files;
pub mod history;
pub mod monitor;
pub mod plan_page;
pub mod settings;
pub mod startup;
pub mod sweep;
pub mod tasks;

use cleancraft_core::FeatureId;
use egui::Ui;

use crate::app::CleanApp;

pub fn feature_page(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    match f {
        FeatureId::SmartCare => sweep::ui(app, ui, f),
        FeatureId::Uninstaller | FeatureId::Updater => apps::ui(app, ui, f),
        FeatureId::LoginItems => startup::ui(app, ui, f),
        FeatureId::Maintenance => tasks::ui(app, ui, f),
        FeatureId::SpaceLens => disk_map::ui(app, ui, f),
        FeatureId::Monitor => monitor::ui(app, ui, f),
        _ => plan_page::ui(app, ui, f),
    }
}
