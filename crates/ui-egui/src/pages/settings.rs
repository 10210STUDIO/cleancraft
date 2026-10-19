//! Settings: where removed items go, the ignore list, thresholds, agents, menu bar.

use cleancraft_core::executor::Method;
use egui::{RichText, Ui};

use crate::app::CleanApp;
use crate::kit;
use crate::theme::{Weight, font};

#[derive(Default)]
pub struct State {
    /// Text box for adding an ignore-list path.
    pub new_ignore: String,
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    kit::title(ui, &t, "Settings");
    ui.add_space(8.0);
    let mut changed = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        kit::card(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Removed items go to").font(font(14.0, Weight::SemiBold)));
            let mut m = app.settings.method;
            ui.radio_value(
                &mut m,
                Method::Trash,
                "The Trash (empty it yourself when you are sure)",
            );
            ui.radio_value(
                &mut m,
                Method::Quarantine,
                "CleanCraft quarantine (~/Library/Application Support/CleanCraft/Quarantine)",
            );
            if m != app.settings.method {
                app.settings.method = m;
                changed = true;
            }
        });
        ui.add_space(8.0);
        kit::card(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Ignore list").font(font(14.0, Weight::SemiBold)));
            kit::dim(
                ui,
                &t,
                "Scans never propose these paths or anything inside them.",
            );
            let list = app.settings.ignore.clone();
            for (i, p) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    let shown = app.env.display(p);
                    ui.label(&shown);
                    let r = ui.small_button("Remove");
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
                let r = ui.add(
                    egui::TextEdit::singleline(&mut app.settings_page.new_ignore)
                        .hint_text("Path to ignore")
                        .desired_width(380.0),
                );
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Path to ignore")
                });
                if r.changed() {
                    app.act();
                }
                if ui.button("Add to Ignore List").clicked() {
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
        ui.add_space(8.0);
        kit::card(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Thresholds").font(font(14.0, Weight::SemiBold)));
            let mut mb = (app.settings.large_min_bytes / 1_000_000) as u32;
            if ui
                .add(egui::Slider::new(&mut mb, 10..=5000).text("Large file minimum (MB)"))
                .changed()
            {
                app.settings.large_min_bytes = mb as u64 * 1_000_000;
                changed = true;
            }
            let mut days = app.settings.old_days as u32;
            if ui
                .add(
                    egui::Slider::new(&mut days, 30..=1825).text("Old means not opened for (days)"),
                )
                .changed()
            {
                app.settings.old_days = days as u64;
                changed = true;
            }
            let mut dl = app.settings.downloads_days as u32;
            if ui
                .add(egui::Slider::new(&mut dl, 7..=730).text("Unused disk images after (days)"))
                .changed()
            {
                app.settings.downloads_days = dl as u64;
                changed = true;
            }
        });
        ui.add_space(8.0);
        kit::card(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Agents and menu bar").font(font(14.0, Weight::SemiBold)));
            if ui
                .checkbox(
                    &mut app.settings.agents_may_clean,
                    "Let AI agents (MCP) carry out confirmed cleanups",
                )
                .changed()
            {
                changed = true;
            }
            if ui
                .checkbox(
                    &mut app.settings.menu_bar,
                    "Show the live monitor in the menu bar",
                )
                .changed()
            {
                changed = true;
            }
        });
    });
    if changed {
        app.act();
        app.save_settings();
    }
}
