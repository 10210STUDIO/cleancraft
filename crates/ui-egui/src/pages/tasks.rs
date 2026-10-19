//! Tune-up Tasks: a checklist of maintenance commands. Each shows what it does, the exact
//! command, whether macOS will ask for the administrator password and when it last ran.
//! "Run Selected" opens a confirmation with the exact commands; the results show per task,
//! with the output log and, for purgeable space, free space before and after.

use std::collections::{BTreeMap, BTreeSet};

use cleancraft_core::fsutil::human_size;
use cleancraft_core::tasks::{self, Outcome, Prepared, RunResult, TASKS};
use cleancraft_core::{Env, FeatureId};
use egui::{RichText, Ui};

use crate::app::CleanApp;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font};

/// What the availability check found for one task.
pub type Ready = Result<Prepared, String>;

#[derive(Default)]
pub struct State {
    pub loaded: bool,
    loading: Option<Job<BTreeMap<String, Ready>>>,
    /// Availability per task id.
    pub ready: BTreeMap<String, Ready>,
    pub selected: BTreeSet<String>,
    /// Last run per task, from the journal.
    pub last: BTreeMap<String, (i64, bool)>,
    /// The confirmation: what will run, and the tasks that cannot run now with why.
    pub confirm: Option<(Vec<Prepared>, Vec<(String, String)>)>,
    running: Option<Job<Vec<RunResult>>>,
    pub results: BTreeMap<String, RunResult>,
    pub show_log: BTreeSet<String>,
}

impl State {
    pub fn is_busy(&self) -> bool {
        self.loading.is_some() || self.running.is_some()
    }
}

fn check_all(env: &Env) -> BTreeMap<String, Ready> {
    TASKS
        .iter()
        .map(|t| (t.id.to_string(), tasks::prepare(env, t.id)))
        .collect()
}

fn start_loading(app: &mut CleanApp) {
    let env = app.env.clone();
    app.tasks.loading = Some(Job::start(app.sync, move |_| check_all(&env)));
    app.tasks.last = tasks::last_runs(&app.env);
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    if let Some(job) = app.tasks.loading.as_mut() {
        if let Some(r) = job.poll() {
            let st = &mut app.tasks;
            st.selected
                .retain(|id| r.get(id).is_some_and(|x| x.is_ok()));
            st.ready = r;
            st.loaded = true;
            st.loading = None;
        } else if job.is_dead() {
            app.tasks.loading = None;
            app.tasks.loaded = true;
        } else {
            busy = true;
        }
    }
    if let Some(job) = app.tasks.running.as_mut() {
        if let Some(results) = job.poll() {
            app.tasks.running = None;
            let cancelled = results
                .iter()
                .filter(|r| r.outcome == Outcome::Cancelled)
                .count();
            let failed = results
                .iter()
                .filter(|r| r.outcome == Outcome::Failed)
                .count();
            let done = results.len() - cancelled - failed;
            for r in results {
                app.tasks.results.insert(r.id.clone(), r);
            }
            let msg = format!("{done} done, {failed} failed, {cancelled} cancelled.");
            if failed > 0 {
                app.warn(msg);
            } else {
                app.tell(msg);
            }
            start_loading(app);
            busy = true;
        } else if job.is_dead() {
            app.tasks.running = None;
            app.warn("The tasks stopped unexpectedly.");
        } else {
            busy = true;
        }
    }
    busy
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    kit::header(
        ui,
        &t,
        icon,
        f.title(),
        "Housekeeping commands that come with macOS. You see each exact command before it runs; commands cannot be undone, and History records them.",
    );
    if !app.tasks.loaded && app.tasks.loading.is_none() {
        start_loading(app);
    }
    if let Some(job) = &app.tasks.running {
        let status = job.status();
        let secs = job.elapsed_secs();
        kit::progress(ui, &t, icon, &format!("Running {status}"), "", secs, false);
        return;
    }
    if !app.tasks.loaded {
        kit::progress(
            ui,
            &t,
            icon,
            "Checking which tasks can run on this Mac...",
            "",
            0,
            false,
        );
        return;
    }
    let n = app.tasks.selected.len();
    ui.horizontal(|ui| {
        if n > 0 {
            if kit::primary(ui, &t, "Run Selected") {
                app.act();
                open_confirm(app);
            }
            ui.label(
                RichText::new(format!("{n} selected"))
                    .font(font(13.5, Weight::Medium))
                    .color(t.text_dim),
            );
        } else {
            kit::dim(ui, &t, "Tick the tasks you want to run.");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::secondary(ui, &t, "Check Again") {
                app.act();
                start_loading(app);
            }
        });
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt("tasks")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for info in TASKS {
                task_card(app, ui, info);
                ui.add_space(crate::theme::space::S);
            }
        });
    if app.tasks.confirm.is_some() {
        confirm(app, ui);
    }
}

fn task_card(app: &mut CleanApp, ui: &mut Ui, info: &tasks::TaskInfo) {
    let t = app.t;
    let ready = app.tasks.ready.get(info.id).cloned();
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let can = matches!(ready, Some(Ok(_)));
            let mut on = app.tasks.selected.contains(info.id);
            let cb = ui
                .add_enabled_ui(can, |ui| {
                    kit::check_text_font(
                        ui,
                        &t,
                        &mut on,
                        info.title,
                        font(14.5, Weight::SemiBold),
                        true,
                    )
                })
                .inner;
            if cb.changed() {
                app.act();
                if on {
                    app.tasks.selected.insert(info.id.to_string());
                } else {
                    app.tasks.selected.remove(info.id);
                }
            }
            if info.admin {
                kit::tag(ui, "Administrator password", t.warning);
            }
            kit::tag(
                ui,
                &format!("{} risk", info.risk.label()),
                kit::risk_color(&t, info.risk),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(info.description).color(t.text_dim));
        let command = match &ready {
            Some(Ok(p)) => p.commands().join("\n"),
            _ => info.command.to_string(),
        };
        kit::well(&t)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Command: {command}"))
                        .monospace()
                        .size(11.5)
                        .color(t.text_faint),
                );
            });
        if let Some(Err(why)) = &ready {
            ui.label(RichText::new(why).color(t.warning));
        }
        match app.tasks.last.get(info.id) {
            Some((ts, ok)) => kit::dim(
                ui,
                &t,
                &format!(
                    "Last run {} ({})",
                    tasks::local_time(*ts),
                    if *ok { "done" } else { "failed" }
                ),
            ),
            None => kit::dim(ui, &t, "Never run by CleanCraft"),
        }
        // Same rule as Quick Sweep's tune-up count.
        if let Some(why) = tasks::suggestion(
            info.id,
            &app.tasks.last,
            matches!(ready, Some(Ok(_))),
            chrono::Utc::now().timestamp(),
        ) {
            ui.label(
                RichText::new(why)
                    .font(font(11.5, Weight::Medium))
                    .color(t.sec_a),
            );
        }
        if let Some(r) = app.tasks.results.get(info.id).cloned() {
            let color = match r.outcome {
                Outcome::Done => t.success,
                Outcome::Failed => t.danger,
                Outcome::Cancelled => t.warning,
            };
            ui.label(
                RichText::new(&r.message)
                    .font(font(13.0, Weight::SemiBold))
                    .color(color),
            );
            if let (Some(b), Some(a)) = (r.free_before, r.free_after) {
                ui.label(format!(
                    "Free space before {}, after {}",
                    human_size(b),
                    human_size(a)
                ));
            }
            if let (Some(b), Some(a)) = (r.file_before, r.file_after) {
                ui.label(format!(
                    "Mail index before {}, after {}",
                    human_size(b),
                    human_size(a)
                ));
            }
            if !r.output.is_empty() {
                let open = app.tasks.show_log.contains(info.id);
                let label = if open {
                    format!("Hide log of {}", info.title)
                } else {
                    format!("Show log of {}", info.title)
                };
                if kit::ghost(ui, &t, &label, None).clicked() {
                    app.act();
                    if open {
                        app.tasks.show_log.remove(info.id);
                    } else {
                        app.tasks.show_log.insert(info.id.to_string());
                    }
                }
                if open {
                    kit::well(&t).show(ui, |ui| {
                        ui.label(RichText::new(&r.output).monospace().size(11.5));
                    });
                }
            }
        }
            });
        });
    });
}

fn open_confirm(app: &mut CleanApp) {
    let mut ok = Vec::new();
    let mut not = Vec::new();
    for info in TASKS {
        if !app.tasks.selected.contains(info.id) {
            continue;
        }
        // Checked again now: the Mac may have changed since the list was drawn.
        match tasks::prepare(&app.env, info.id) {
            Ok(p) => ok.push(p),
            Err(e) => not.push((info.title.to_string(), e)),
        }
    }
    app.tasks.confirm = Some((ok, not));
}

fn confirm(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some((ok, not)) = app.tasks.confirm.clone() else {
        return;
    };
    let mut go = false;
    let mut close = false;
    let max_list = (ui.ctx().content_rect().height() - 300.0).clamp(120.0, 380.0);
    kit::modal(egui::Id::new("tasks-confirm"), &t).show(ui.ctx(), |ui| {
        ui.set_width((ui.ctx().content_rect().width() - 120.0).clamp(420.0, 640.0));
        kit::dialog_head(
            ui,
            &t,
            crate::icons::Icon::Tasks,
            t.sec_a,
            &format!("Run {} tasks?", ok.len()),
            "These exact commands will run, in this order. They cannot be undone.",
        );
        ui.add_space(12.0);
        egui::ScrollArea::vertical()
            .max_height(max_list)
            .show(ui, |ui| {
                for p in &ok {
                    let Some(info) = tasks::info(&p.id) else {
                        continue;
                    };
                    ui.add_space(4.0);
                    ui.label(RichText::new(info.title).font(font(14.0, Weight::SemiBold)));
                    ui.label(RichText::new(info.description).color(t.text_dim));
                    for c in p.commands() {
                        ui.label(RichText::new(c).monospace().size(11.5));
                    }
                    if p.needs_admin() {
                        ui.label(
                            RichText::new("macOS will ask for the administrator password.")
                                .color(t.warning),
                        );
                    }
                }
                for (title, why) in &not {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("{title} will not run: {why}")).color(t.warning),
                    );
                }
            });
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !ok.is_empty() && kit::primary(ui, &t, "Run") {
                go = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
    if go {
        app.act();
        app.tasks.confirm = None;
        for p in &ok {
            app.tasks.results.remove(&p.id);
        }
        let env = app.env.clone();
        app.tasks.running = Some(Job::start(app.sync, move |status| {
            ok.iter()
                .map(|p| {
                    status(tasks::info(&p.id).map(|i| i.title).unwrap_or(""));
                    tasks::run(&env, p)
                })
                .collect()
        }));
    } else if close {
        app.act();
        app.tasks.confirm = None;
    }
}
