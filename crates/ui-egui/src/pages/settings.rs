//! Settings: where removed items go, the ignore list, thresholds, agents, menu bar.

use cleancraft_core::executor::Method;
use egui::{RichText, Sense, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::kit;
use crate::theme::{Weight, font, size, space};

/// A card's heading with its icon.
fn heading(ui: &mut Ui, t: &crate::theme::Tokens, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        kit::badge(ui, t.sec_a, icon, 28.0);
        ui.add_space(2.0);
        ui.label(
            RichText::new(text)
                .font(font(size::TITLE3, Weight::SemiBold))
                .color(t.text),
        );
    });
    ui.add_space(2.0);
}

/// A form row: the label in a leading column, the slider and its value with the unit after
/// it. The slider keeps `name` as its accessible name.
/// A form row: label in the right-aligned label column, a switch after it, and `text`
/// beside the switch. Returns true when clicked. The switch is announced as `text`.
fn switch_row(ui: &mut Ui, t: &crate::theme::Tokens, on: bool, text: &str, label: &str) -> bool {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
        ui.painter().text(
            r.right_center() - vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            font(size::BODY, Weight::Regular),
            t.text_dim,
        );
        let c = kit::switch(ui, t, on, text).clicked();
        // Painted: the switch already carries the words for VoiceOver.
        kit::heading(ui, text, font(size::CALLOUT, Weight::Medium), t.text);
        c
    })
    .inner
}

fn slider_row(
    ui: &mut Ui,
    t: &crate::theme::Tokens,
    name: &str,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    unit: &str,
) -> bool {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
        ui.painter().text(
            r.right_center() - vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            font(size::BODY, Weight::Regular),
            t.text_dim,
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 140.0).clamp(160.0, 360.0);
        let r = kit::slider(
            ui,
            egui::Slider::new(value, range)
                .suffix(unit)
                .trailing_fill(true),
        );
        let v = *value as f64;
        let name = name.to_string();
        r.widget_info(|| egui::WidgetInfo::slider(true, v, name.clone()));
        r.changed()
    })
    .inner
}

#[derive(Default)]
pub struct State {
    /// Text box for adding an ignore-list path.
    pub new_ignore: String,
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    kit::header(
        ui,
        &t,
        Icon::Settings,
        "Settings",
        "Where removed items go, what scans leave alone, how big and old files are judged, and sounds.",
    );
    let mut changed = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Trash, "Removed items go to");
                let mut m = app.settings.method;
                for (value, text) in [
                    (Method::Trash, "The Trash (empty it yourself when you are sure)"),
                    (
                        Method::Quarantine,
                        "CleanCraft quarantine (~/Library/Application Support/CleanCraft/Quarantine)",
                    ),
                ] {
                    if kit::radio(ui, &t, m == value, text).clicked() {
                        m = value;
                    }
                }
                if m != app.settings.method {
                    app.settings.method = m;
                    changed = true;
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Shield, "Ignore list");
                kit::dim(
                    ui,
                    &t,
                    "Scans never propose these paths or anything inside them.",
                );
                let list = app.settings.ignore.clone();
                for (i, p) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let shown = app.env.display(p);
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Folder, t.text_dim);
                        ui.label(RichText::new(&shown).color(t.text));
                        let r = kit::ghost(ui, &t, "Remove", Some(Icon::Close));
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Remove {shown}"),
                            )
                        });
                        if r.clicked() {
                            app.settings.ignore.remove(i);
                            changed = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    let r = kit::text_field(
                        ui,
                        &t,
                        &mut app.settings_page.new_ignore,
                        "Path to ignore",
                        380.0,
                    );
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            true,
                            "Path to ignore",
                        )
                    });
                    if r.changed() {
                        app.act();
                    }
                    if kit::secondary(ui, &t, "Add to Ignore List") {
                        let raw = app.settings_page.new_ignore.trim().to_string();
                        if raw.is_empty() {
                            app.act();
                            app.warn("Type a path first.");
                        } else {
                            let p = if let Some(rest) = raw.strip_prefix("~/") {
                                app.env.home_path(rest)
                            } else {
                                app.env.sys_path(&raw)
                            };
                            app.settings.ignore.push(p);
                            app.settings_page.new_ignore.clear();
                            changed = true;
                        }
                    }
                });
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::LargeOld, "Thresholds");
                let mut mb = (app.settings.large_min_bytes / 1_000_000) as u32;
                if slider_row(ui, &t, "Large file minimum (MB)", "Large files start at", &mut mb, 10..=5000, " MB") {
                    app.settings.large_min_bytes = mb as u64 * 1_000_000;
                    changed = true;
                }
                let mut days = app.settings.old_days as u32;
                if slider_row(ui, &t, "Old means not opened for (days)", "Old means not opened for", &mut days, 30..=1825, " days") {
                    app.settings.old_days = days as u64;
                    changed = true;
                }
                let mut dl = app.settings.downloads_days as u32;
                if slider_row(ui, &t, "Unused disk images after (days)", "Disk images count as unused after", &mut dl, 7..=730, " days") {
                    app.settings.downloads_days = dl as u64;
                    changed = true;
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Sweep, "AI agents");
                if kit::check_text(
                    ui,
                    &t,
                    &mut app.settings.agents_may_clean,
                    "Let AI agents (MCP) carry out confirmed cleanups",
                )
                .changed()
                {
                    changed = true;
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Monitor, "Sounds");
                if switch_row(ui, &t, app.settings.sounds, "Play sounds", "Sounds") {
                    app.settings.sounds = !app.settings.sounds;
                    changed = true;
                }
                ui.add_enabled_ui(app.settings.sounds, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
                        ui.painter().text(
                            r.right_center() - vec2(12.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            "Volume",
                            font(size::BODY, Weight::Regular),
                            t.text_dim,
                        );
                        ui.spacing_mut().slider_width =
                            (ui.available_width() - 160.0).clamp(160.0, 300.0);
                        let mut v = app.settings.sound_volume;
                        let r = kit::slider(
                            ui,
                            egui::Slider::new(&mut v, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        let shown = v as f64;
                        r.widget_info(|| egui::WidgetInfo::slider(true, shown, "Volume"));
                        if r.changed() {
                            app.settings.sound_volume = v;
                            changed = true;
                        }
                        let p = kit::pill(ui, &t, "Preview", kit::Kind::Secondary, None);
                        p.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Preview sound")
                        });
                        if p.clicked() {
                            app.act();
                            crate::sound::preview(&app.env, app.settings.sound_volume);
                        }
                    });
                    if switch_row(
                        ui,
                        &t,
                        app.settings.sound_ticks,
                        "Click sounds on switches",
                        "Clicks",
                    ) {
                        app.settings.sound_ticks = !app.settings.sound_ticks;
                        changed = true;
                    }
                });
            });
        });
    if changed {
        app.act();
        app.save_settings();
    }
}
