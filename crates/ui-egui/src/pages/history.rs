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
    /// The action log as it was when `records` were read.
    watch: JournalWatch,
}

impl State {
    pub fn reload(&mut self, env: &cleancraft_core::Env) {
        let mut v = executor::history(env);
        v.reverse();
        self.records = v;
        self.watch.prime(env);
    }
}

/// The size and time of the action log: it changes with every move, undo, switch and command,
/// whichever page (or agent) did it.
fn journal_stamp(env: &cleancraft_core::Env) -> Option<(u64, std::time::SystemTime)> {
    let md = std::fs::metadata(executor::journal_path(env)).ok()?;
    Some((md.len(), md.modified().ok()?))
}

/// Tells a page what changed on disk since it last looked, from CleanCraft's action log: items
/// moved away by any page (or an agent) and items put back. One file status per check, so a
/// page can ask every frame it is shown.
#[derive(Default)]
pub struct JournalWatch {
    stamp: Option<Option<(u64, std::time::SystemTime)>>,
    /// Record id -> undone, as last seen.
    seen: std::collections::HashMap<String, bool>,
}

impl JournalWatch {
    /// Remember the log as it is now (the page's data reflects it).
    pub fn prime(&mut self, env: &cleancraft_core::Env) {
        self.stamp = Some(journal_stamp(env));
        self.seen = executor::history(env)
            .into_iter()
            .map(|r| (r.id, r.undone.is_some()))
            .collect();
    }

    /// Records that are new or were undone since the last look; `None` when the log did not
    /// change (or the watch was never primed).
    pub fn changes(&mut self, env: &cleancraft_core::Env) -> Option<Vec<ActionRecord>> {
        let before = self.stamp?;
        let now = journal_stamp(env);
        if now == before {
            return None;
        }
        let recs = executor::history(env);
        let changed: Vec<ActionRecord> = recs
            .iter()
            .filter(|r| self.seen.get(&r.id) != Some(&r.undone.is_some()))
            .cloned()
            .collect();
        self.stamp = Some(now);
        self.seen = recs
            .into_iter()
            .map(|r| (r.id, r.undone.is_some()))
            .collect();
        Some(changed)
    }
}

/// Where moved items came from and went to: (original, stored, put back).
pub fn moved_paths(recs: &[ActionRecord]) -> Vec<(std::path::PathBuf, std::path::PathBuf, bool)> {
    recs.iter()
        .filter_map(|r| match &r.kind {
            ActionKind::Moved {
                original, stored, ..
            } => Some((original.clone(), stored.clone(), r.undone.is_some())),
            _ => None,
        })
        .collect()
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
    // A move or undo made since the list was read (another page, an agent): read it again.
    if app.history.watch.changes(&app.env).is_some() {
        app.history.reload(&app.env);
    }
    kit::header(
        ui,
        &t,
        Icon::History,
        "History",
        "Everything CleanCraft moved or ran, newest first. Moved items can be put back while they are still in the Trash or quarantine.",
    );
    let groups = group_records(&app.history.records);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(match groups.len() {
                0 => "No actions yet".to_string(),
                1 => "1 action".to_string(),
                n => format!("{n} actions"),
            })
            .font(font(size::TITLE2, Weight::Bold))
            .color(t.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::pill(ui, &t, "Refresh", kit::Kind::Secondary, None)
                .on_hover_text("Read the action log again")
                .clicked()
            {
                app.act();
                app.history.reload(&app.env);
            }
        });
    });
    ui.add_space(space::S);
    if groups.is_empty() {
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            kit::badge(ui, t.sec_a, Icon::History, 64.0);
            ui.add_space(space::M);
            ui.label(
                RichText::new("Nothing yet.")
                    .font(font(size::TITLE3, Weight::SemiBold))
                    .color(t.text),
            );
            ui.add_space(space::XS);
            kit::dim(
                ui,
                &t,
                "When CleanCraft moves files or runs a task, it is listed here, with Undo.",
            );
        });
        return;
    }
    let now = chrono::Local::now();
    let fade_out = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for g in groups {
                group_card(app, ui, &g, now);
                ui.add_space(space::S);
            }
        });
    kit::scroll_fade_page(ui, &fade_out, &app.t);
}

/// A moved item that is no longer where it was stored (the Trash was emptied): Undo cannot
/// bring it back, so none is offered.
fn gone(r: &ActionRecord) -> bool {
    match &r.kind {
        ActionKind::Moved { stored, .. } => {
            r.undone.is_none() && std::fs::symlink_metadata(stored).is_err()
        }
        _ => false,
    }
}

/// One action: the records of one plan (or one change) that follow each other in the log.
fn group_records(recs: &[ActionRecord]) -> Vec<Vec<ActionRecord>> {
    let mut out: Vec<Vec<ActionRecord>> = Vec::new();
    for r in recs {
        match out.last_mut() {
            Some(g) if g[0].plan_id == r.plan_id && !r.plan_id.is_empty() => g.push(r.clone()),
            _ => out.push(vec![r.clone()]),
        }
    }
    out
}

/// An action's card: what, when, how much, Undo All; then each item with its own Undo.
fn group_card(
    app: &mut CleanApp,
    ui: &mut Ui,
    g: &[ActionRecord],
    now: chrono::DateTime<chrono::Local>,
) {
    let t = app.t;
    let first = &g[0];
    let icon = match (&first.kind, first.feature) {
        (_, Some(f)) => kit::feature_look(&t, f).0,
        (ActionKind::Moved { original, .. }, None) => kit::kind_icon(original),
        (ActionKind::Command { .. }, None) => Icon::Tasks,
        (ActionKind::Toggled { .. }, None) => Icon::Startup,
    };
    let color = first
        .feature
        .map(|f| kit::feature_look(&t, f).1)
        .unwrap_or(t.sec_a);
    let moves = g
        .iter()
        .filter(|r| matches!(r.kind, ActionKind::Moved { .. }))
        .count();
    let size: u64 = g.iter().map(|r| r.size).sum();
    let undoable = g.iter().filter(|r| r.can_undo() && !gone(r)).count();
    let back = g.iter().filter(|r| r.undone.is_some()).count();
    let title = first
        .feature
        .map(|f| f.title().to_string())
        .unwrap_or_else(|| "CleanCraft".into());
    let mut what = Vec::new();
    if moves > 0 {
        what.push(format!(
            "{} moved, {}",
            cleancraft_core::fsutil::items(moves),
            human_size(size)
        ));
    }
    if back > 0 {
        what.push(format!("{back} put back"));
    }
    what.push(when_text(first.time, now));
    kit::card(&t)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (tile, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                kit::icon_tile(ui.painter(), tile, icon, color);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    kit::heading(ui, &title, font(size::BODY, Weight::SemiBold), t.text);
                    kit::dim(ui, &t, &what.join("  ·  "));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if undoable > 1 {
                        let r =
                            kit::pill(ui, &t, "Undo All", kit::Kind::Secondary, Some(Icon::Undo))
                                .on_hover_text(format!(
                                    "Put back the {undoable} items of this action"
                                ));
                        if r.clicked() {
                            app.act();
                            let (n, errs) = executor::undo_plan(&app.env, &first.plan_id);
                            app.files_back(&[]);
                            if errs.is_empty() {
                                app.tell(format!(
                                    "{} put back.",
                                    cleancraft_core::fsutil::items(n)
                                ));
                            } else {
                                app.warn(format!(
                                    "{} put back, {} could not be: {}",
                                    cleancraft_core::fsutil::items(n),
                                    errs.len(),
                                    errs[0].1
                                ));
                            }
                            app.history.reload(&app.env);
                        }
                    }
                });
            });
            ui.add_space(6.0);
            for (k, r) in g.iter().enumerate() {
                if k > 0 {
                    let y = ui.cursor().min.y;
                    ui.painter().hline(
                        egui::Rangef::new(ui.max_rect().left() + 40.0, ui.max_rect().right()),
                        y,
                        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(10)),
                    );
                }
                record_row(app, ui, r, now);
            }
        });
}

/// One record: its name and where it was (or the command), and Undo or "Put back".
fn record_row(
    app: &mut CleanApp,
    ui: &mut Ui,
    r: &ActionRecord,
    now: chrono::DateTime<chrono::Local>,
) {
    let t = app.t;
    let when = when_text(r.time, now);
    let feat = r.feature.map(|f| f.title()).unwrap_or("");
    ui.horizontal(|ui| {
        ui.set_min_height(44.0);
        ui.add_space(40.0);
        // Right side first, so long paths are shortened instead of pushing Undo away.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if r.can_undo() && !gone(r) {
                let b = kit::pill(ui, &t, "Undo", kit::Kind::Secondary, Some(Icon::Undo));
                if b.on_hover_text("Put this back where it was").clicked() {
                    app.act();
                    match executor::undo(&app.env, &r.id) {
                        Ok(_) => {
                            app.tell("Put back.");
                            app.files_back(&[]);
                        }
                        Err(e) => app.warn(format!("Could not put it back: {e}")),
                    }
                    app.history.reload(&app.env);
                }
            } else if r.undone.is_some() {
                kit::tag(ui, "Put back", t.success);
            } else if gone(r) {
                kit::tag(ui, "Emptied", t.text_dim).on_hover_text(
                    "It left the Trash or quarantine (emptied), so it cannot be put back.",
                );
            }
            if let ActionKind::Moved { .. } = &r.kind {
                ui.label(
                    RichText::new(human_size(r.size))
                        .font(font(size::CALLOUT, Weight::SemiBold))
                        .color(t.text),
                );
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    match &r.kind {
                        ActionKind::Moved {
                            original, stored, ..
                        } => {
                            let name = original
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| app.env.display(original));
                            // Shows the name; announces the whole original path.
                            let shown = app.env.display(original);
                            let nf = font(size::CALLOUT, Weight::Medium);
                            let room = ui.available_width().max(40.0);
                            let name_s = kit::truncate_middle(ui.ctx(), &name, &nf, room);
                            let nw = kit::galley(ui, &name_s, nf.clone(), t.text).size().x + 2.0;
                            let (nr, _) = ui.allocate_exact_size(
                                egui::vec2(nw.min(room), 20.0),
                                Sense::hover(),
                            );
                            kit::text_at(ui, nr, &name_s, &shown, nf, t.text, egui::Align::Min);
                            // Shows where it was; announces when, module, size and where it
                            // is now too (VoiceOver, tests).
                            let full = format!(
                                "{when} · {feat} · {} · now in {}",
                                human_size(r.size),
                                app.env.display(stored)
                            );
                            let fid = font(size::FOOTNOTE, Weight::Regular);
                            let w = ui.available_width().max(40.0);
                            let short = kit::truncate_middle(ui.ctx(), &shown, &fid, w);
                            let sw =
                                kit::galley(ui, &short, fid.clone(), t.text_faint).size().x + 2.0;
                            let (rr, _) =
                                ui.allocate_exact_size(egui::vec2(sw.min(w), 17.0), Sense::hover());
                            kit::text_at(
                                ui,
                                rr,
                                &short,
                                &full,
                                fid,
                                t.text_faint,
                                egui::Align::Min,
                            )
                            .on_hover_text(format!(
                                "Was {shown}\nNow in {}",
                                app.env.display(stored)
                            ));
                        }
                        ActionKind::Command {
                            command,
                            ok,
                            output,
                        } => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(command)
                                        .font(font(size::CALLOUT, Weight::Medium))
                                        .color(t.text),
                                )
                                .truncate(),
                            );
                            kit::dim(
                                ui,
                                &t,
                                &format!(
                                    "{when}  ·  {}{}",
                                    if *ok { "Done" } else { "Failed" },
                                    output
                                        .lines()
                                        .next()
                                        .map(|l| format!(": {l}"))
                                        .unwrap_or_default()
                                ),
                            );
                        }
                        ActionKind::Toggled { what, .. } => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(what)
                                        .font(font(size::CALLOUT, Weight::Medium))
                                        .color(t.text),
                                )
                                .truncate(),
                            );
                            kit::dim(ui, &t, &when);
                        }
                    }
                });
            });
        });
    });
}
