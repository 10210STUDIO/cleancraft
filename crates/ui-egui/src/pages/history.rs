//! History: every action CleanCraft took, newest first, with Undo.

use cleancraft_core::executor::{self, ActionKind, ActionRecord};
use cleancraft_core::fsutil::human_size;
use egui::{RichText, Sense, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::kit;
use crate::theme::{Weight, font, space};

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
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            kit::header(
                ui,
                &t,
                Icon::History,
                "History",
                "Everything CleanCraft moved or ran. Moved items can be put back while they are still in the Trash or quarantine.",
            );
        });
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(match app.history.records.len() {
                1 => "1 action".to_string(),
                n => format!("{n} actions"),
            })
            .font(font(18.0, Weight::Bold))
            .color(t.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::secondary(ui, &t, "Refresh") {
                app.act();
                app.history.reload(&app.env);
            }
        });
    });
    ui.add_space(space::S);
    if app.history.records.is_empty() {
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            kit::badge(ui, t.sec_a, Icon::History, 56.0);
            ui.add_space(space::M);
            ui.label(RichText::new("Nothing yet.").font(font(16.0, Weight::SemiBold)));
        });
        return;
    }
    let recs = app.history.records.clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in recs {
                let icon = match &r.kind {
                    ActionKind::Moved { original, .. } => kit::kind_icon(original),
                    ActionKind::Command { .. } => Icon::Tasks,
                    ActionKind::Toggled { .. } => Icon::Startup,
                };
                let color = r
                    .feature
                    .map(|f| kit::feature_look(&t, f).1)
                    .unwrap_or(t.sec_a);
                kit::card(&t)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (tile, _) =
                                ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                            kit::icon_tile(ui.painter(), tile, icon, color);
                            ui.add_space(4.0);
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
                                        ui.label(
                                            RichText::new(command)
                                                .font(font(13.0, Weight::SemiBold)),
                                        );
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
                                        ui.label(
                                            RichText::new(what).font(font(13.0, Weight::SemiBold)),
                                        );
                                        kit::dim(ui, &t, &format!("{when} · {feat}"));
                                    }
                                }
                                if r.undone.is_some() {
                                    kit::tag(ui, "Put back", t.success);
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if r.can_undo()
                                        && kit::pill(
                                            ui,
                                            &t,
                                            "Undo",
                                            kit::Kind::Secondary,
                                            Some(Icon::Undo),
                                        )
                                        .clicked()
                                    {
                                        app.act();
                                        match executor::undo(&app.env, &r.id) {
                                            Ok(_) => app.tell("Put back."),
                                            Err(e) => {
                                                app.warn(format!("Could not put it back: {e}"))
                                            }
                                        }
                                        app.history.reload(&app.env);
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(6.0);
            }
        });
}
