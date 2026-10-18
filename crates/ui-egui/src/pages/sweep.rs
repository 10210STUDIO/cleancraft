//! Quick Sweep: one Scan across the cleanup, protection and app modules, one tile per module
//! (only modules that exist get a tile), Review opens a module with its plan already loaded,
//! Run carries out every included module's preselection after ONE confirmation that lists
//! every file, and each module's cleanup can be undone on its own.

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::sweep::{self, ModuleRun, Sources, SweepResult, TileData};
use cleancraft_core::{FeatureId, Plan};
use egui::{RichText, Ui};

use crate::app::{CleanApp, Page};
use crate::jobs::Job;
use crate::kit;
use crate::pages::plan_page;
use crate::theme::{Weight, font};

#[derive(Default)]
pub enum Phase {
    #[default]
    Idle,
    Scanning(Job<SweepResult>),
    Results,
    Confirm,
    Running(Job<Vec<ModuleRun>>),
    Done(Vec<ModuleRun>),
}

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    pub result: Option<SweepResult>,
    /// Modules to use instead of the registered ones (tests).
    pub sources: Option<Sources>,
    /// Modules whose cleanup was put back on the Done screen.
    pub undone: Vec<FeatureId>,
}

fn sources(app: &CleanApp) -> Sources {
    app.sweep
        .sources
        .clone()
        .unwrap_or_else(Sources::registered)
}

fn start_scan(app: &mut CleanApp) {
    let env = app.env.clone();
    let settings = app.settings.clone();
    let src = sources(app);
    let job = Job::start(app.sync, move |status| {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        sweep::scan(&env, &settings, &src, &cancel, status)
    });
    app.sweep.phase = Phase::Scanning(job);
}

fn start_run(app: &mut CleanApp) {
    let Some(res) = &app.sweep.result else {
        return;
    };
    // The digests of exactly what the dialog listed.
    let confirmed: Vec<(Plan, String)> = res
        .to_run(&app.settings.sweep_skip)
        .into_iter()
        .map(|p| (p.clone(), p.digest()))
        .collect();
    let env = app.env.clone();
    let settings = app.settings.clone();
    let src = sources(app);
    let job = Job::start(app.sync, move |status| {
        sweep::run(&env, &settings, &src, &confirmed, status)
    });
    app.sweep.undone.clear();
    app.sweep.phase = Phase::Running(job);
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let st = &mut app.sweep;
    match &mut st.phase {
        Phase::Scanning(job) => {
            if let Some(r) = job.poll() {
                st.result = Some(r);
                st.phase = Phase::Results;
                false
            } else if job.is_dead() {
                st.phase = Phase::Idle;
                app.warn("The scan stopped unexpectedly.");
                false
            } else {
                true
            }
        }
        Phase::Running(job) => {
            if let Some(runs) = job.poll() {
                // Module pages must not show lists that were just cleaned.
                for r in &runs {
                    let ps = app.plan_state(r.id);
                    if !ps.is_busy() {
                        ps.phase = plan_page::Phase::Idle;
                        ps.plan = None;
                    }
                }
                app.sweep.phase = Phase::Done(runs);
                false
            } else if job.is_dead() {
                st.phase = Phase::Results;
                app.warn("The cleanup stopped unexpectedly. Check History for what moved.");
                false
            } else {
                true
            }
        }
        _ => false,
    }
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (glyph, color) = kit::feature_look(&t, f);
    ui.horizontal(|ui| {
        kit::badge(ui, color, glyph, 44.0);
        ui.vertical(|ui| {
            kit::title(ui, &t, f.title());
            kit::subtitle(
                ui,
                &t,
                "One scan across junk, mail downloads, the Trash, threats and your apps.",
            );
        });
    });
    ui.add_space(12.0);
    match &app.sweep.phase {
        Phase::Idle => idle(app, ui),
        Phase::Scanning(j) => {
            let s = j.status();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Looking at {s}"));
            });
        }
        Phase::Results => results(app, ui),
        Phase::Confirm => {
            results(app, ui);
            confirm(app, ui);
        }
        Phase::Running(j) => {
            let s = j.status();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Cleaning {s}"));
            });
        }
        Phase::Done(_) => done(app, ui),
    }
}

fn idle(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        kit::dim(
            ui,
            &t,
            "Scanning only looks. You see every file before anything moves.",
        );
        ui.add_space(10.0);
        let b = egui::Button::new(
            RichText::new("Scan")
                .color(t.on_accent)
                .font(font(20.0, Weight::Bold)),
        )
        .fill(t.accent)
        .corner_radius(60)
        .min_size(egui::vec2(120.0, 120.0));
        if ui.add(b).clicked() {
            app.act();
            start_scan(app);
        }
    });
}

fn results(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(res) = app.sweep.result.clone() else {
        return;
    };
    let skip = app.settings.sweep_skip.clone();
    let ready = res.ready_bytes(&skip);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(human_size(ready))
                .font(font(30.0, Weight::Bold))
                .color(t.accent),
        );
        ui.label(RichText::new("ready to clean").color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::secondary(ui, &t, "Scan Again") {
                app.act();
                start_scan(app);
            }
            if ready > 0 && kit::primary(ui, &t, "Run") {
                app.act();
                app.sweep.phase = Phase::Confirm;
            }
        });
    });
    if res.tiles.is_empty() {
        kit::dim(ui, &t, "No module is available to scan.");
        return;
    }
    ui.add_space(10.0);
    let per_row = 3;
    for chunk in res.tiles.chunks(per_row) {
        ui.columns(per_row, |cols| {
            for (i, tile) in chunk.iter().enumerate() {
                tile_ui(app, &mut cols[i], tile);
            }
        });
        ui.add_space(10.0);
    }
}

fn tile_ui(app: &mut CleanApp, ui: &mut Ui, tile: &sweep::Tile) {
    let t = app.t;
    let skipped = app.settings.sweep_skip.contains(&tile.id);
    let mut review = false;
    let mut include = !skipped;
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if skipped {
            ui.multiply_opacity(0.45);
        }
        let (glyph, color) = kit::feature_look(&t, tile.id);
        ui.horizontal(|ui| {
            kit::badge(ui, color, glyph, 30.0);
            ui.label(
                RichText::new(tile.id.title())
                    .font(font(15.0, Weight::SemiBold))
                    .color(t.text),
            );
        });
        ui.add_space(4.0);
        match &tile.data {
            TileData::Plan(p) if !tile.runnable() => {
                ui.label(
                    RichText::new(human_size(p.total_size()))
                        .font(font(22.0, Weight::Bold))
                        .color(t.text),
                );
                kit::dim(
                    ui,
                    &t,
                    &format!(
                        "{} in the Trash. Emptied from its own page.",
                        cleancraft_core::fsutil::items(p.items.len())
                    ),
                );
            }
            TileData::Plan(p) => {
                ui.label(
                    RichText::new(human_size(p.selected_size()))
                        .font(font(22.0, Weight::Bold))
                        .color(t.text),
                );
                kit::dim(
                    ui,
                    &t,
                    &format!(
                        "{} of {} selected",
                        p.selected_count(),
                        cleancraft_core::fsutil::items(p.items.len())
                    ),
                );
            }
            TileData::Count { count, noun } => {
                ui.label(
                    RichText::new(cleancraft_core::fsutil::count(
                        *count,
                        noun.trim_end_matches('s'),
                        noun,
                    ))
                    .font(font(22.0, Weight::Bold))
                    .color(t.text),
                );
                kit::dim(ui, &t, "Open the page to act on them.");
            }
            TileData::Failed(e) => {
                ui.label(
                    RichText::new("Could not scan")
                        .font(font(16.0, Weight::SemiBold))
                        .color(t.danger),
                );
                kit::dim(ui, &t, e);
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if kit::secondary(ui, &t, "Review") {
                review = true;
            }
            if tile.runnable() {
                ui.checkbox(&mut include, "Include");
            }
        });
    });
    if review {
        app.act();
        if let TileData::Plan(p) = &tile.data {
            let st = app.plan_state(tile.id);
            st.plan = Some(p.clone());
            st.phase = plan_page::Phase::Results;
            st.category = None;
            st.error = None;
        }
        app.page = Page::Feature(tile.id);
    }
    if include == skipped {
        app.act();
        if include {
            app.settings.sweep_skip.retain(|x| *x != tile.id);
        } else {
            app.settings.sweep_skip.push(tile.id);
        }
        app.save_settings();
    }
}

fn confirm(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(res) = app.sweep.result.clone() else {
        return;
    };
    let plans: Vec<Plan> = res
        .to_run(&app.settings.sweep_skip)
        .into_iter()
        .cloned()
        .collect();
    let count: usize = plans.iter().map(|p| p.selected_count()).sum();
    let size: u64 = plans.iter().map(|p| p.selected_size()).sum();
    let mut go = false;
    let mut close = false;
    egui::Modal::new(egui::Id::new("sweep-confirm")).show(ui.ctx(), |ui| {
        ui.set_width(680.0);
        ui.label(
            RichText::new(format!(
                "Clean {} ({})?",
                cleancraft_core::fsutil::items(count),
                human_size(size)
            ))
                .font(font(18.0, Weight::Bold)),
        );
        kit::dim(
            ui,
            &t,
            "These exact files will be moved. Threats go to the CleanCraft quarantine. You can put everything back from History.",
        );
        let admin: usize = plans
            .iter()
            .map(cleancraft_core::features::admin_count)
            .sum();
        if let Some(note) = super::plan_page::admin_note(admin) {
            ui.label(RichText::new(note).color(t.warning));
        }
        egui::ScrollArea::vertical()
            .max_height(380.0)
            .show(ui, |ui| {
                for p in &plans {
                    let Some(fid) = p.feature else { continue };
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "{}: {}, {}",
                            fid.title(),
                            cleancraft_core::fsutil::items(p.selected_count()),
                            human_size(p.selected_size())
                        ))
                        .font(font(14.0, Weight::SemiBold)),
                    );
                    egui::Grid::new(("sweep-list", fid.as_str()))
                        .striped(true)
                        .num_columns(3)
                        .show(ui, |ui| {
                            for it in p.selected() {
                                ui.label(app.env.display(&it.path));
                                ui.label(human_size(it.size));
                                ui.label(RichText::new(&it.reason).color(t.text_dim));
                                ui.end_row();
                            }
                        });
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = match app.settings.method {
                Method::Trash => "Move to Trash",
                Method::Quarantine | Method::AdminQuarantine => "Move to Quarantine",
            };
            if kit::danger(ui, &t, label) {
                go = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
    if go {
        app.act();
        start_run(app);
    } else if close {
        app.act();
        app.sweep.phase = Phase::Results;
    }
}

fn done(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Phase::Done(runs) = &app.sweep.phase else {
        return;
    };
    let runs = runs.clone();
    let headline =
        cleancraft_core::features::moved_text(runs.iter().filter_map(|r| r.result.as_ref().ok()));
    ui.label(
        RichText::new(headline)
            .font(font(30.0, Weight::Bold))
            .color(t.accent),
    );
    if runs
        .iter()
        .filter_map(|r| r.result.as_ref().ok())
        .any(|r| !r.moved.is_empty())
    {
        kit::dim(ui, &t, cleancraft_core::features::SPACE_NOTE);
    }
    ui.add_space(8.0);
    let mut undo: Option<(FeatureId, String)> = None;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            for r in &runs {
                kit::card(&t).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(r.id.title()).font(font(15.0, Weight::SemiBold)));
                        match &r.result {
                            Ok(rep) => {
                                ui.label(format!(
                                    "{} moved, {} skipped, {}",
                                    rep.moved.len(),
                                    rep.skipped.len(),
                                    human_size(rep.freed)
                                ));
                                if app.sweep.undone.contains(&r.id) {
                                    ui.label(RichText::new("Put back").color(t.text_dim));
                                } else if !rep.moved.is_empty() && kit::secondary(ui, &t, "Undo") {
                                    undo = Some((r.id, rep.plan_id.clone()));
                                }
                            }
                            Err(e) => {
                                ui.label(RichText::new(e).color(t.danger));
                            }
                        }
                    });
                    if let Ok(rep) = &r.result {
                        for (p, why) in &rep.skipped {
                            ui.horizontal(|ui| {
                                ui.label(app.env.display(p));
                                ui.label(RichText::new(why.to_string()).color(t.warning));
                            });
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });
    if let Some((id, plan_id)) = undo {
        app.act();
        let (n, errs) = executor::undo_plan(&app.env, &plan_id);
        app.sweep.undone.push(id);
        if errs.is_empty() {
            app.tell(format!(
                "{}: {} put back.",
                id.title(),
                cleancraft_core::fsutil::items(n)
            ));
        } else {
            app.warn(format!(
                "{}: {} put back, {} could not be: {}",
                id.title(),
                cleancraft_core::fsutil::items(n),
                errs.len(),
                errs[0].1
            ));
        }
    }
    ui.horizontal(|ui| {
        if kit::secondary(ui, &t, "Show History") {
            app.act();
            app.navigate("history");
        }
        if kit::primary(ui, &t, "Done") {
            app.act();
            app.sweep.phase = Phase::Idle;
            app.sweep.result = None;
        }
    });
}
