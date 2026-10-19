//! History: every action CleanCraft took, newest first, with Undo.

use cleancraft_core::executor::{self, ActionKind, ActionRecord};
use cleancraft_core::fsutil::human_size;
use egui::{RichText, Sense, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::kit;
use crate::theme::{Weight, font, size, space};

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

/// "Today, 13:26", "Yesterday, 09:02", "Mon 12 Oct, 18:40", or with the year when it is
/// not this year.
pub fn when_text(ts: i64, now: chrono::DateTime<chrono::Local>) -> String {
    let Some(d) = chrono::DateTime::from_timestamp(ts, 0) else {
        return String::new();
    };
    let d = d.with_timezone(&chrono::Local);
    let days = (now.date_naive() - d.date_naive()).num_days();
    let time = d.format("%H:%M");
    match days {
        0 => format!("Today, {time}"),
        1 => format!("Yesterday, {time}"),
        _ if d.format("%Y").to_string() == now.format("%Y").to_string() => {
            format!("{}, {time}", d.format("%a %-d %b"))
        }
        _ => format!("{}, {time}", d.format("%-d %b %Y")),
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
            .font(font(size::TITLE2, Weight::Bold))
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
            kit::badge(ui, t.sec_a, Icon::History, 64.0);
            ui.add_space(space::M);
            ui.label(RichText::new("Nothing yet.").font(font(size::TITLE3, Weight::SemiBold)));
        });
        return;
    }
    let recs = app.history.records.clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in recs {
                let icon = match (&r.kind, r.feature) {
                    (_, Some(f)) => kit::feature_look(&t, f).0,
                    (ActionKind::Moved { original, .. }, None) => kit::kind_icon(original),
                    (ActionKind::Command { .. }, None) => Icon::Tasks,
                    (ActionKind::Toggled { .. }, None) => Icon::Startup,
                };
                let color = r
                    .feature
                    .map(|f| kit::feature_look(&t, f).1)
                    .unwrap_or(t.sec_a);
                kit::card(&t)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (tile, _) =
                                ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                            kit::icon_tile(ui.painter(), tile, icon, color);
                            ui.add_space(4.0);
                            let when = when_text(r.time, chrono::Local::now());
                            let feat = r.feature.map(|f| f.title()).unwrap_or("");
                            ui.vertical(|ui| {
                                match &r.kind {
                                    ActionKind::Moved {
                                        original, stored, ..
                                    } => {
                                        let name = original
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| app.env.display(original));
                                        ui.label(
                                            RichText::new(name)
                                                .font(font(size::BODY, Weight::SemiBold)),
                                        );
                                        // Shows when, module and size; announces where it
                                        // is now too (VoiceOver, tests).
                                        let shown =
                                            format!("{when}  ·  {feat}  ·  {}", human_size(r.size));
                                        let full = format!(
                                            "{when} · {feat} · {} · now in {}",
                                            human_size(r.size),
                                            app.env.display(stored)
                                        );
                                        let fid = font(size::FOOTNOTE, Weight::Medium);
                                        let w = kit::galley(ui, &shown, fid.clone(), t.text_dim)
                                            .size()
                                            .x
                                            + 2.0;
                                        let (rr, _) = ui.allocate_exact_size(
                                            egui::vec2(w, 17.0),
                                            Sense::hover(),
                                        );
                                        kit::text_at(
                                            ui,
                                            rr,
                                            &shown,
                                            &full,
                                            fid,
                                            t.text_dim,
                                            egui::Align::Min,
                                        );
                                        ui.scope(|ui| {
                                            ui.set_max_width(ui.available_width() - 120.0);
                                            kit::path_label(ui, &t, &app.env.display(original))
                                                .on_hover_text(format!(
                                                    "Now in {}",
                                                    app.env.display(stored)
                                                ));
                                        });
                                    }
                                    ActionKind::Command {
                                        command,
                                        ok,
                                        output,
                                    } => {
                                        ui.label(
                                            RichText::new(command)
                                                .font(font(size::BODY, Weight::SemiBold)),
                                        );
                                        kit::dim(
                                            ui,
                                            &t,
                                            &format!(
                                                "{when}  ·  {feat}  ·  {} {}",
                                                if *ok { "done" } else { "failed" },
                                                output.lines().next().unwrap_or("")
                                            ),
                                        );
                                    }
                                    ActionKind::Toggled { what, .. } => {
                                        ui.label(
                                            RichText::new(what)
                                                .font(font(size::BODY, Weight::SemiBold)),
                                        );
                                        kit::dim(ui, &t, &format!("{when}  ·  {feat}"));
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
