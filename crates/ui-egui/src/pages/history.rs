//! History: every action CleanCraft took, newest first, with Undo.

use cleancraft_core::executor::{self, ActionKind, ActionRecord};
use cleancraft_core::fsutil::human_size;
use egui::{RichText, Ui};

use crate::app::CleanApp;
use crate::kit;
use crate::theme::{Weight, font};

#[derive(Default)]
pub struct State {
    pub records: Vec<ActionRecord>,
}

impl State {
    pub fn reload(&mut self, env: &cleancraft_core::Env) {
        let mut v = executor::history(env);
        v.reverse();
        self.records = v;
    }
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    kit::title(ui, &t, "History");
    kit::subtitle(
        ui,
        &t,
        "Everything CleanCraft moved or ran. Moved items can be put back while they are still in the Trash or quarantine.",
    );
    ui.add_space(8.0);
    if kit::secondary(ui, &t, "Refresh") {
        app.act();
        app.history.reload(&app.env);
    }
    if app.history.records.is_empty() {
        ui.add_space(20.0);
        ui.label("Nothing yet.");
        return;
    }
    let recs = app.history.records.clone();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for r in recs {
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let when = chrono::DateTime::from_timestamp(r.time, 0)
                        .map(|d| {
                            d.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default();
                    let feat = r.feature.map(|f| f.title()).unwrap_or("");
                    ui.vertical(|ui| {
                        match &r.kind {
                            ActionKind::Moved {
                                original, stored, ..
                            } => {
                                ui.label(
                                    RichText::new(app.env.display(original))
                                        .font(font(13.0, Weight::SemiBold)),
                                );
                                kit::dim(
                                    ui,
                                    &t,
                                    &format!(
                                        "{when} · {feat} · {} · now in {}",
                                        human_size(r.size),
                                        app.env.display(stored)
                                    ),
                                );
                            }
                            ActionKind::Command {
                                command,
                                ok,
                                output,
                            } => {
                                ui.label(RichText::new(command).font(font(13.0, Weight::SemiBold)));
                                kit::dim(
                                    ui,
                                    &t,
                                    &format!(
                                        "{when} · {feat} · {} {}",
                                        if *ok { "done" } else { "failed" },
                                        output.lines().next().unwrap_or("")
                                    ),
                                );
                            }
                            ActionKind::Toggled { what, .. } => {
                                ui.label(RichText::new(what).font(font(13.0, Weight::SemiBold)));
                                kit::dim(ui, &t, &format!("{when} · {feat}"));
                            }
                        }
                        if r.undone.is_some() {
                            ui.label(RichText::new("Put back").color(t.success));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if r.can_undo() && ui.button("Undo").clicked() {
                            app.act();
                            match executor::undo(&app.env, &r.id) {
                                Ok(_) => app.tell("Put back."),
                                Err(e) => app.warn(format!("Could not put it back: {e}")),
                            }
                            app.history.reload(&app.env);
                        }
                    });
                });
            });
        }
    });
}
