//! Extra controls for the file modules (Large & Old Files, Duplicates, Similar Images): where
//! to look, before the scan, and the "Select Copies" menu above duplicate and look-alike sets.
//!
//! The chosen location lives in core ([`ScanOptions`], `<data>/files.json`) because the scan
//! reads it; the text being typed and its error message live in egui memory.

use cleancraft_core::FeatureId;
use cleancraft_core::features::duplicates::{KeepRule, apply_keep_rule};
use cleancraft_core::features::large_old::{Location, ScanOptions, parse_folder};
use cleancraft_core::fsutil::human_size;
use egui::{Id, RichText, Ui};

use crate::app::CleanApp;
use crate::kit;

#[derive(Clone, Default)]
struct Picker {
    /// "Another folder" is chosen but not confirmed yet.
    other: bool,
    text: String,
    error: Option<String>,
}

fn picker_id(f: FeatureId) -> Id {
    Id::new(("files-location", f.as_str()))
}

fn save_location(app: &mut CleanApp, f: FeatureId, loc: Location) -> bool {
    let mut o = ScanOptions::load(&app.env);
    o.set_location(f, loc);
    match o.save(&app.env) {
        Ok(()) => true,
        Err(e) => {
            app.warn(format!("The location could not be saved: {e}"));
            false
        }
    }
}

pub fn before_scan(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    match f {
        FeatureId::LargeOldFiles | FeatureId::Duplicates => location_picker(app, ui, f),
        FeatureId::SimilarPhotos => {
            let t = app.t;
            kit::dim(
                ui,
                &t,
                "Looks at pictures over 100 KB in Pictures and Downloads. Photos libraries are never opened.",
            );
            ui.add_space(6.0);
        }
        _ => {}
    }
}

fn location_picker(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let id = picker_id(f);
    let current = ScanOptions::load(&app.env).location(f).clone();
    let mut p: Picker = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    if let Location::Folder(dir) = &current
        && !p.other
        && p.text.is_empty()
    {
        p.text = app.env.display(dir);
    }
    kit::card(&t).show(ui, |ui| {
        ui.set_max_width(520.0);
        ui.label(
            RichText::new("Where to look")
                .font(crate::theme::font(crate::theme::size::TITLE3, crate::theme::Weight::SemiBold))
                .color(t.text),
        );
        let mut choices: Vec<Location> = Location::PRESETS.to_vec();
        if f == FeatureId::LargeOldFiles {
            choices.push(Location::DownloadsBySource);
        }
        ui.horizontal_wrapped(|ui| {
            for loc in choices {
                let on = !p.other && current == loc;
                if kit::radio(ui, &t, on, &loc.label(&app.env)).clicked() {
                    app.act();
                    p.other = false;
                    p.error = None;
                    save_location(app, f, loc);
                }
            }
            let folder_on = p.other || matches!(current, Location::Folder(_));
            if kit::radio(ui, &t, folder_on, "Another folder").clicked() {
                app.act();
                p.other = true;
                p.error = None;
            }
        });
        if p.other || matches!(current, Location::Folder(_)) {
            let mut apply = false;
            ui.horizontal(|ui| {
                let r = kit::text_field(ui, &t, &mut p.text, "~/Projects", 300.0);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Folder path")
                });
                if p.other && !r.has_focus() && p.text.is_empty() && p.error.is_none() {
                    r.request_focus();
                }
                if r.changed() {
                    app.act();
                    p.error = None;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    app.act();
                    p.error = None;
                    if !matches!(current, Location::Folder(_)) {
                        p.other = false;
                        p.text.clear();
                    }
                }
                let can = !p.text.trim().is_empty();
                if ui
                    .add_enabled_ui(can, |ui| kit::secondary(ui, &t, "Use Folder"))
                    .inner
                {
                    apply = true;
                }
            });
            if apply {
                app.act();
                match parse_folder(&app.env, &p.text) {
                    Ok(dir) => {
                        if save_location(app, f, Location::Folder(dir.clone())) {
                            p.other = false;
                            p.error = None;
                            p.text = app.env.display(&dir);
                        }
                    }
                    Err(e) => p.error = Some(e),
                }
            }
            if let Some(e) = &p.error {
                ui.label(RichText::new(e).color(t.danger));
            }
        }
        let now = ScanOptions::load(&app.env).location(f).clone();
        kit::dim(
            ui,
            &t,
            &format!(
                "Looking in: {}",
                match &now {
                    Location::Folder(d) => app.env.display(d),
                    other => other.label(&app.env),
                }
            ),
        );
        if f == FeatureId::LargeOldFiles && now != Location::DownloadsBySource {
            kit::dim(
                ui,
                &t,
                &format!(
                    "Large means {} or more; old means not opened for {} days (Settings).",
                    human_size(app.settings.large_min_bytes),
                    app.settings.old_days
                ),
            );
        }
        kit::dim(
            ui,
            &t,
            "Library, Documents, Desktop, iCloud, Photos and Music libraries and apps are left out.",
        );
    });
    ui.data_mut(|d| d.insert_temp(id, p));
    ui.add_space(8.0);
}

pub fn results_toolbar(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    if !matches!(f, FeatureId::Duplicates | FeatureId::SimilarPhotos) {
        return;
    }
    let mut rules = vec![
        ("Keep Suggested", KeepRule::Smart),
        ("Keep Newest", KeepRule::Newest),
        ("Keep Oldest", KeepRule::Oldest),
    ];
    if f == FeatureId::SimilarPhotos {
        rules.push(("Keep Largest File", KeepRule::Largest));
    }
    let mut chosen = None;
    let r = ui.menu_button("Select Copies", |ui| {
        for (label, rule) in &rules {
            if ui.button(*label).clicked() {
                chosen = Some(*rule);
                ui.close();
            }
        }
    });
    if r.response.clicked() {
        app.act();
    }
    if let Some(rule) = chosen {
        app.act();
        let env = app.env.clone();
        if let Some(p) = app.plan_state(f).plan.as_mut() {
            apply_keep_rule(&env, p, rule);
        }
    }
}
