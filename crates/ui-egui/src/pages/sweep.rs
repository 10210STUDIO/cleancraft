//! Quick Sweep: one Scan across the cleanup, protection and app modules, one tile per module
//! (only modules that exist get a tile), Review opens a module with its plan already loaded,
//! Run carries out every included module's preselection after ONE confirmation that lists
//! every file, and each module's cleanup can be undone on its own.

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::sweep::{self, ModuleRun, Sources, SweepResult, TileData};
use cleancraft_core::{FeatureId, Plan};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::app::{CleanApp, Page};
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::pages::plan_page;
use crate::theme::{Weight, font, space};

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
    /// Set to stop the running scan.
    pub cancel: Arc<AtomicBool>,
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
    let cancel = Arc::new(AtomicBool::new(false));
    app.sweep.cancel = cancel.clone();
    let job = Job::start(app.sync, move |status| {
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

const SUBTITLE: &str = "One scan across junk, mail downloads, the Trash, threats and your apps.";

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    if !matches!(app.sweep.phase, Phase::Idle) {
        kit::header(ui, &t, icon, f.title(), SUBTITLE);
    }
    match &app.sweep.phase {
        Phase::Idle => idle(app, ui),
        Phase::Scanning(j) => {
            let s = j.status();
            let secs = j.elapsed_secs();
            let place = if s.is_empty() {
                "Getting ready...".to_string()
            } else {
                format!("Looking at {s}")
            };
            if kit::progress(ui, &t, icon, "Sweeping your Mac", &place, secs, true) {
                app.act();
                app.sweep.cancel.store(true, Ordering::Relaxed);
                app.sweep.phase = Phase::Idle;
                app.tell("Quick Sweep stopped. Nothing changed.");
            }
        }
        Phase::Results => results(app, ui),
        Phase::Confirm => {
            results(app, ui);
            confirm(app, ui);
        }
        Phase::Running(j) => {
            let s = j.status();
            let secs = j.elapsed_secs();
            kit::progress(
                ui,
                &t,
                Icon::Trash,
                "Cleaning",
                &format!("Cleaning {s}"),
                secs,
                false,
            );
        }
        Phase::Done(_) => done(app, ui),
    }
}

fn idle(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if kit::hero(
        ui,
        &t,
        Icon::Sweep,
        FeatureId::SmartCare.title(),
        SUBTITLE,
        "Scan",
        "Scanning only looks. You see every file before anything moves.",
        |_| {},
    ) {
        app.act();
        start_scan(app);
    }
}

fn results(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(res) = app.sweep.result.clone() else {
        return;
    };
    let skip = app.settings.sweep_skip.clone();
    let ready = res.ready_bytes(&skip);
    ui.horizontal(|ui| {
        kit::display(ui, &t, &human_size(ready));
        ui.vertical(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new("ready to clean")
                    .font(font(15.0, Weight::Medium))
                    .color(t.text_dim),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ready > 0 && kit::primary(ui, &t, "Run") {
                app.act();
                app.sweep.phase = Phase::Confirm;
            }
            if kit::secondary(ui, &t, "Scan Again") {
                app.act();
                start_scan(app);
            }
        });
    });
    if res.tiles.is_empty() {
        kit::dim(ui, &t, "No module is available to scan.");
        return;
    }
    ui.add_space(space::M);
    let gap = space::M;
    egui::ScrollArea::vertical()
        .id_salt("sweep-tiles")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let w = ui.available_width();
            let per_row = (((w + gap) / (250.0 + gap)).floor() as usize).clamp(1, 4);
            let tile_w = (w - gap * (per_row as f32 - 1.0)) / per_row as f32;
            for chunk in res.tiles.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for tile in chunk {
                        tile_ui(app, ui, tile, tile_w);
                    }
                });
                ui.add_space(gap);
            }
        });
}

const TILE_H: f32 = 170.0;

fn tile_ui(app: &mut CleanApp, ui: &mut Ui, tile: &sweep::Tile, w: f32) {
    let t = app.t;
    let skipped = app.settings.sweep_skip.contains(&tile.id);
    let mut review = false;
    let mut include = !skipped;
    let (icon, color) = kit::feature_look(&t, tile.id);
    let (rect, _) = ui.allocate_exact_size(vec2(w, TILE_H), Sense::hover());
    let p = ui.painter().clone();
    // Card with a glow of the module's colour in its corner.
    p.add(egui::Shape::rect_filled(
        rect.translate(vec2(0.0, 6.0)),
        16.0,
        Color32::from_black_alpha(50),
    ));
    p.rect_filled(rect, 16.0, t.glass);
    icons::glow(
        &p.with_clip_rect(rect.shrink(1.0)),
        pos2(rect.right() - 30.0, rect.top() + 20.0),
        w * 0.55,
        color.gamma_multiply(if skipped { 0.05 } else { 0.16 }),
        0.8,
    );
    p.rect_stroke(
        rect,
        16.0,
        egui::Stroke::new(1.0, t.glass_stroke),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(16.0);
    kit::place(ui, inner, |ui| {
        if skipped {
            ui.multiply_opacity(0.5);
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            kit::badge(ui, color, icon, 34.0);
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new(tile.id.title())
                        .font(font(14.5, Weight::SemiBold))
                        .color(t.text),
                )
                .truncate(),
            );
        });
        ui.add_space(8.0);
        let (big, small, big_color) = match &tile.data {
            TileData::Plan(p) if !tile.runnable() => (
                human_size(p.total_size()),
                format!(
                    "{} in the Trash. Emptied from its own page.",
                    cleancraft_core::fsutil::items(p.items.len())
                ),
                t.text,
            ),
            TileData::Plan(p) => (
                human_size(p.selected_size()),
                format!(
                    "{} of {} selected",
                    p.selected_count(),
                    cleancraft_core::fsutil::items(p.items.len())
                ),
                t.text,
            ),
            TileData::Count { count, noun } => (
                cleancraft_core::fsutil::count(*count, noun.trim_end_matches('s'), noun),
                "Open the page to act on them.".to_string(),
                t.text,
            ),
            TileData::Failed(e) => ("Could not scan".to_string(), e.clone(), t.danger),
        };
        ui.label(
            RichText::new(big)
                .font(font(24.0, Weight::Bold))
                .color(big_color),
        );
        ui.add(
            egui::Label::new(
                RichText::new(small)
                    .font(font(11.5, Weight::Regular))
                    .color(t.text_faint),
            )
            .truncate(),
        );
    });
    // Controls on the bottom edge.
    let controls = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 32.0), inner.max);
    kit::place_with(
        ui,
        controls,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if kit::secondary(ui, &t, "Review") {
                review = true;
            }
            if tile.runnable() {
                ui.add_space(4.0);
                kit::check_text(ui, &t, &mut include, "Include");
            }
        },
    );
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
    let width = (ui.ctx().content_rect().width() - 120.0).clamp(420.0, 680.0);
    let max_list = (ui.ctx().content_rect().height() - 330.0).clamp(120.0, 380.0);
    kit::modal(egui::Id::new("sweep-confirm"), &t).show(ui.ctx(), |ui| {
        ui.set_width(width);
        kit::dialog_head(
            ui,
            &t,
            Icon::Sweep,
            t.sec_a,
            &format!(
                "Clean {} ({})?",
                cleancraft_core::fsutil::items(count),
                human_size(size)
            ),
            "These exact files will be moved. Threats go to the CleanCraft quarantine. You can put everything back from History.",
        );
        ui.add_space(space::M);
        let admin: usize = plans
            .iter()
            .map(cleancraft_core::features::admin_count)
            .sum();
        if let Some(note) = super::plan_page::admin_note(admin) {
            kit::strip(ui, Icon::Warning, t.warning, &note);
            ui.add_space(space::S);
        }
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(max_list)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for p in &plans {
                        let Some(fid) = p.feature else { continue };
                        let (icon, color) = kit::feature_look(&t, fid);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            kit::badge(ui, color, icon, 22.0);
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}, {}",
                                    fid.title(),
                                    cleancraft_core::fsutil::items(p.selected_count()),
                                    human_size(p.selected_size())
                                ))
                                .font(font(13.5, Weight::SemiBold))
                                .color(t.text),
                            );
                        });
                        ui.add_space(2.0);
                        for it in p.selected() {
                            kit::confirm_line(
                                ui,
                                &t,
                                &it.path,
                                &app.env.display(&it.path),
                                &it.reason,
                                &human_size(it.size),
                            );
                        }
                    }
                });
        });
        ui.add_space(space::L);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
    let key: Vec<String> = runs
        .iter()
        .filter_map(|r| r.result.as_ref().ok().map(|x| x.plan_id.clone()))
        .collect();
    let started = kit::first_seen(ui, ("sweep-done", key));
    let mut undo: Option<(FeatureId, String)> = None;
    ui.vertical_centered(|ui| {
        let d = if ui.available_height() < 480.0 {
            72.0
        } else {
            96.0
        };
        kit::done_check(ui, &t, d, started);
        ui.add_space(space::S);
        ui.label(
            RichText::new(headline)
                .font(font(28.0, Weight::Bold))
                .color(t.text),
        );
        if runs
            .iter()
            .filter_map(|r| r.result.as_ref().ok())
            .any(|r| !r.moved.is_empty())
        {
            kit::dim(ui, &t, cleancraft_core::features::SPACE_NOTE);
        }
        ui.add_space(space::M);
        kit::centered_row(ui, egui::Id::new("sweep-done-buttons"), |ui| {
            if kit::pill(
                ui,
                &t,
                "Show History",
                kit::Kind::Secondary,
                Some(Icon::History),
            )
            .clicked()
            {
                app.act();
                app.navigate("history");
            }
            if kit::primary(ui, &t, "Done") {
                app.act();
                app.sweep.phase = Phase::Idle;
                app.sweep.result = None;
            }
        });
    });
    ui.add_space(space::M);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in &runs {
                let (icon, color) = kit::feature_look(&t, r.id);
                kit::card(&t)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            kit::badge(ui, color, icon, 30.0);
                            ui.label(
                                RichText::new(r.id.title())
                                    .font(font(14.5, Weight::SemiBold))
                                    .color(t.text),
                            );
                            match &r.result {
                                Ok(rep) => {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} moved, {} skipped, {}",
                                            rep.moved.len(),
                                            rep.skipped.len(),
                                            human_size(rep.freed)
                                        ))
                                        .color(t.text_dim),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if app.sweep.undone.contains(&r.id) {
                                                kit::tag(ui, "Put back", t.success);
                                            } else if !rep.moved.is_empty()
                                                && kit::pill(
                                                    ui,
                                                    &t,
                                                    "Undo",
                                                    kit::Kind::Secondary,
                                                    Some(Icon::Undo),
                                                )
                                                .clicked()
                                            {
                                                undo = Some((r.id, rep.plan_id.clone()));
                                            }
                                        },
                                    );
                                }
                                Err(e) => {
                                    ui.label(RichText::new(e).color(t.danger));
                                }
                            }
                        });
                        if let Ok(rep) = &r.result {
                            for (p, why) in &rep.skipped {
                                ui.horizontal(|ui| {
                                    ui.add_space(38.0);
                                    ui.label(RichText::new(app.env.display(p)).color(t.text_dim));
                                    ui.label(RichText::new(why.to_string()).color(t.warning));
                                });
                            }
                        }
                    });
                ui.add_space(space::S);
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
}
