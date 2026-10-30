//! Quick Sweep: one Scan across the cleanup, protection and app modules, one tile per module
//! (only modules that exist get a tile), Review opens a module with its plan already loaded,
//! Review and Clean carries out every included module's preselection after ONE confirmation that lists
//! every file, and each module's cleanup can be undone on its own.

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::sweep::{self, ModuleRun, Sources, SweepResult, TileData};
use cleancraft_core::{FeatureId, Plan};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

use crate::app::{CleanApp, Page};
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::pages::plan_page;
use crate::theme::{Weight, font, icon as isz, size, space};

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
    /// Index (in `Sources::ids`) of the module the running scan is in. Modules scan one
    /// after the other, so the ones before it are done.
    pub scan_at: usize,
    /// Files came back since the results were made (an Undo): scan again when shown.
    pub stale: bool,
    /// Tiles to scan again on their own (the Trash after files moved into it, a tile that
    /// could not scan): REVIEW-U N2, U23.
    pub refresh: Vec<FeatureId>,
    /// Those single-tile scans while they run.
    pub tile_jobs: Vec<(FeatureId, Job<Option<TileData>>)>,
    /// Tiles emptied by a cleanup made elsewhere since the scan (REVIEW-U N1).
    pub cleaned: Vec<FeatureId>,
}

impl State {
    /// Ask for the tile of `id` to be scanned again (when there are results showing it).
    pub fn refresh_tile(&mut self, id: FeatureId) {
        if self.result.as_ref().is_some_and(|r| r.tile(id).is_some()) && !self.refresh.contains(&id)
        {
            self.refresh.push(id);
        }
    }

    /// Is the tile of `id` being scanned again?
    pub fn refreshing(&self, id: FeatureId) -> bool {
        self.refresh.contains(&id) || self.tile_jobs.iter().any(|(f, _)| *f == id)
    }
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
    app.sweep.scan_at = 0;
    let job = Job::start_metered(app.sync, move |meter| {
        sweep::scan_metered(
            &env,
            &settings,
            &src,
            &cancel,
            &|s| meter.set_step(s),
            meter,
        )
    });
    app.sweep.phase = Phase::Scanning(job);
    crate::sound::play(&app.env, crate::sound::Cue::ScanStart);
}

/// Start the scan when the page shows nothing yet (a route such as "scan:quick_sweep" from a
/// scheduled scan's notification). Results on screen and a running scan stay as they are.
pub fn start_scan_if_idle(app: &mut CleanApp) {
    if matches!(app.sweep.phase, Phase::Idle) {
        start_scan(app);
    }
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
    let job = Job::start_metered(app.sync, move |meter| {
        sweep::run_metered(
            &env,
            &settings,
            &src,
            &confirmed,
            &|s| meter.set_step(s),
            meter,
        )
    });
    app.sweep.undone.clear();
    app.sweep.phase = Phase::Running(job);
}

/// Single-tile scans: start the ones asked for, put finished ones in their tile.
fn poll_tiles(app: &mut CleanApp) -> bool {
    let idle = matches!(
        app.sweep.phase,
        Phase::Results | Phase::Done(_) | Phase::Idle
    );
    if app.sweep.result.is_none() {
        app.sweep.refresh.clear();
        app.sweep.tile_jobs.clear();
        return false;
    }
    if idle && !app.sweep.refresh.is_empty() {
        let src = sources(app);
        for id in std::mem::take(&mut app.sweep.refresh) {
            if app.sweep.tile_jobs.iter().any(|(f, _)| *f == id) {
                continue;
            }
            let env = app.env.clone();
            let settings = app.settings.clone();
            let src = src.clone();
            let job = Job::start(app.sync, move |status| {
                sweep::scan_one(&env, &settings, &src, id, &AtomicBool::new(false), status)
            });
            app.sweep.tile_jobs.push((id, job));
        }
    }
    let mut busy = false;
    let mut finished = Vec::new();
    app.sweep.tile_jobs.retain_mut(|(id, job)| {
        if let Some(data) = job.poll() {
            finished.push((*id, data));
            false
        } else if job.is_dead() {
            false
        } else {
            busy = true;
            true
        }
    });
    for (id, data) in finished {
        let Some(data) = data else { continue };
        app.sweep.cleaned.retain(|f| *f != id);
        if let Some(res) = app.sweep.result.as_mut()
            && let Some(tile) = res.tiles.iter_mut().find(|t| t.id == id)
        {
            tile.data = data;
        }
    }
    busy
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let tiles_busy = poll_tiles(app);
    tiles_busy | poll_phase(app)
}

fn poll_phase(app: &mut CleanApp) -> bool {
    let st = &mut app.sweep;
    match &mut st.phase {
        Phase::Scanning(job) => {
            if let Some(r) = job.poll() {
                st.result = Some(r);
                st.cleaned.clear();
                st.refresh.clear();
                st.tile_jobs.clear();
                st.phase = Phase::Results;
                crate::sound::play(&app.env, crate::sound::Cue::ScanDone);
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
                // Module pages (and every other list) drop what was just moved.
                let moved: Vec<std::path::PathBuf> = runs
                    .iter()
                    .filter_map(|r| r.result.as_ref().ok())
                    .flat_map(CleanApp::moved_paths)
                    .collect();
                app.files_moved(&moved);
                let ok = runs.iter().any(|r| r.result.is_ok());
                app.sweep.phase = Phase::Done(runs);
                crate::sound::play(
                    &app.env,
                    if ok {
                        crate::sound::Cue::CleanDone
                    } else {
                        crate::sound::Cue::Error
                    },
                );
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

const SUBTITLE: &str = "One scan across junk, mail and duplicate downloads, the Trash, threats, developer clutter and your apps.";

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    // Files came back since the results were made: they are out of date.
    if app.sweep.stale {
        match app.sweep.phase {
            Phase::Results => {
                app.sweep.stale = false;
                start_scan(app);
            }
            Phase::Idle => {
                app.sweep.stale = false;
                app.sweep.result = None;
            }
            _ => {}
        }
    }
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    // One title row in every phase, at the same place (REVIEW-A 18, REVIEW-C C15): the start
    // screen's large headline says what the scan does instead of repeating the name.
    kit::header(ui, &t, icon, f.title(), "");
    match &app.sweep.phase {
        Phase::Idle => idle(app, ui),
        Phase::Scanning(j) => {
            let s = j.status();
            let meter = j.meter.clone();
            // Which module the scan is in: its title is the status when it starts (a module
            // then reports its own places, which leave the index where it is).
            let skip = app.settings.sweep_skip.clone();
            let ids: Vec<FeatureId> = sources(app)
                .ids()
                .into_iter()
                .filter(|id| !skip.contains(id))
                .collect();
            if let Some(i) = ids.iter().position(|id| sweep::tile_title(*id) == s)
                && i > app.sweep.scan_at
            {
                app.sweep.scan_at = i;
            }
            // Each module fills an equal slice of the bar, so the share says which modules
            // are done too.
            if let Some(f) = meter.fraction() {
                let i = (f * ids.len() as f32).floor() as usize;
                if i > app.sweep.scan_at {
                    app.sweep.scan_at = i;
                }
            }
            let at = app.sweep.scan_at.min(ids.len().saturating_sub(1));
            let place = if s.is_empty() {
                "Getting ready...".to_string()
            } else {
                format!("Looking at {s}")
            };
            let looks: Vec<(Icon, Color32, &str)> = ids
                .iter()
                .map(|id| {
                    let (i, c) = kit::feature_look(&t, *id);
                    (i, c, sweep::tile_title(*id))
                })
                .collect();
            let states: Vec<kit::ChipState> = (0..ids.len())
                .map(|i| match i.cmp(&at) {
                    std::cmp::Ordering::Less => kit::ChipState::Done,
                    std::cmp::Ordering::Equal => kit::ChipState::Working,
                    std::cmp::Ordering::Greater => kit::ChipState::Waiting,
                })
                .collect();
            if kit::progress_with(
                ui,
                &t,
                FeatureId::SmartCare,
                "Sweeping your Mac",
                &place,
                &meter,
                true,
                |ui| {
                    ui.add_space(space::M);
                    kit::chips_with_state(ui, &t, &looks, &states, 760.0);
                },
            ) {
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
            kit::progress(
                ui,
                &t,
                FeatureId::SmartCare,
                "Cleaning",
                &s,
                &j.meter.clone(),
                false,
            );
        }
        Phase::Done(_) => done(app, ui),
    }
}

fn idle(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut health_clicked = false;
    let mut toggled: Option<FeatureId> = None;
    let ids = sources(app).ids();
    let looks: Vec<(Icon, Color32, &str)> = ids
        .iter()
        .map(|id| {
            let (i, c) = kit::feature_look(&t, *id);
            (i, c, sweep::tile_title(*id))
        })
        .collect();
    // The chips choose what the scan includes (Settings::sweep_skip, the same choice as the
    // Include box on each result tile).
    let on: Vec<bool> = ids
        .iter()
        .map(|id| !app.settings.sweep_skip.contains(id))
        .collect();
    let picked = on.iter().filter(|o| **o).count();
    let action = if picked == ids.len() {
        "Scan".to_string()
    } else {
        format!("Scan {picked} of {}", ids.len())
    };
    let note = if picked == 0 {
        "Choose at least one module above to scan."
    } else {
        "Scanning only looks. You see every file before anything moves. Click a module to leave it out."
    };
    if kit::hero(
        ui,
        &t,
        FeatureId::SmartCare,
        kit::HeroText {
            title: "One scan for your whole Mac",
            desc: SUBTITLE,
            covers: &[],
            action: &action,
            note,
            enabled: picked > 0,
        },
        |ui| {
            if let Some(i) = kit::chip_toggles(ui, &t, &looks, &on, 760.0) {
                toggled = ids.get(i).copied();
            }
            ui.add_space(space::S);
            // Mac Health from the first page (U7-4): its state when it was read, as a link.
            let line = health_line(app);
            if kit::link(ui, &t, &line).clicked() {
                health_clicked = true;
            }
            ui.add_space(space::XS);
        },
    ) {
        app.act();
        start_scan(app);
    }
    if let Some(id) = toggled {
        app.act();
        if app.settings.sweep_skip.contains(&id) {
            app.settings.sweep_skip.retain(|x| *x != id);
        } else {
            app.settings.sweep_skip.push(id);
        }
        app.save_settings();
    }
    if health_clicked {
        app.act();
        app.open_route(&cleancraft_core::route::AppRoute::Health);
    }
}

/// "Mac Health: Needs attention (6)", "Mac Health: Good", or before it was read "Mac Health:
/// see how your Mac is doing".
pub fn health_line(app: &CleanApp) -> String {
    if app.health.facts.is_none() {
        return "Mac Health: see how your Mac is doing".into();
    }
    let (state, why) = cleancraft_core::health::summary(&app.health.cards);
    match state {
        cleancraft_core::health::State::Good => "Mac Health: Good".into(),
        cleancraft_core::health::State::NeedsAttention => {
            format!("Mac Health: Needs attention ({})", why.len())
        }
    }
}

fn results(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(res) = app.sweep.result.clone() else {
        return;
    };
    let skip = app.settings.sweep_skip.clone();
    let ready = res.ready_bytes(&skip);
    ui.horizontal_top(|ui| {
        kit::display_count(ui, &t, "sweep-ready", ready);
        ui.vertical(|ui| {
            ui.add_space(kit::baseline_offset(size::DISPLAY, size::TITLE3));
            ui.label(
                crate::rich("ready to clean")
                    .font(font(size::TITLE3, Weight::Medium))
                    .color(t.text_dim),
            );
        });
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), kit::DISPLAY_ROW_H),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if ready > 0 && kit::primary(ui, &t, "Review and Clean") {
                    app.act();
                    app.sweep.phase = Phase::Confirm;
                }
                if kit::secondary(ui, &t, "Scan Again") {
                    app.act();
                    start_scan(app);
                }
            },
        );
    });
    if res.tiles.is_empty() {
        kit::dim(ui, &t, "No module is available to scan.");
        return;
    }
    ui.add_space(space::L);
    let gap = space::GUTTER;
    let out = egui::ScrollArea::vertical()
        .id_salt("sweep-tiles")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Room on the right for the scroll bar when the tiles need one, so it never sits
            // on them.
            let full = ui.available_width();
            let per_full = (((full + gap) / (236.0 + gap)).floor() as usize).clamp(1, 4);
            let rows_full = res.tiles.len().div_ceil(per_full) as f32;
            let scrolls = rows_full * TILE_MIN_H + gap * (rows_full - 1.0) > ui.available_height();
            let w = if scrolls { full - 14.0 } else { full };
            let fit = (((w + gap) / (236.0 + gap)).floor() as usize).clamp(1, 4);
            // As few rows as fit, with the tiles shared evenly between them (5 tiles: 3 + 2,
            // never 4 + 1); a shorter last row widens its tiles, so no cell stays empty.
            let rows = res.tiles.len().div_ceil(fit).max(1);
            let per_row = res.tiles.len().div_ceil(rows).max(1);
            // Tiles share the height of the page, so the grid fills it instead of leaving
            // an empty lower half.
            let n_rows = res.tiles.len().div_ceil(per_row) as f32;
            let tile_h = ((ui.available_height() - gap * (n_rows - 1.0) - 4.0) / n_rows)
                .clamp(TILE_MIN_H, TILE_MAX_H)
                .floor();
            ui.spacing_mut().item_spacing.y = 0.0;
            // Tiles fade in one after the other each time new results arrive.
            let key = res.tiles.iter().fold(String::new(), |mut k, tile| {
                if let TileData::Plan(p) = &tile.data {
                    k.push_str(&p.id);
                }
                k
            });
            for (i, chunk) in res.tiles.chunks(per_row).enumerate() {
                if i > 0 {
                    ui.add_space(gap);
                }
                let tile_w = (w - gap * (chunk.len() as f32 - 1.0)) / chunk.len() as f32;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (j, tile) in chunk.iter().enumerate() {
                        let (k, _) = kit::appear(ui, ("sweep-tiles", &key), i * per_row + j);
                        ui.scope(|ui| {
                            // Present from the first frame, fading up (REVIEW-C C10).
                            ui.multiply_opacity(0.3 + 0.7 * k);
                            tile_ui(app, ui, tile, tile_w, tile_h);
                        });
                    }
                });
            }
        });
    kit::scroll_fade_page(ui, &out, &t);
}

/// The line under a runnable tile's number: "2 of 3 items selected, 740 KB found" ("0 of 3
/// items selected, 740 KB found" when nothing is preselected, as in Mail Downloads).
pub fn sweep_tile_line(p: &Plan) -> String {
    format!(
        "{} of {} selected, {} found",
        p.selected_count(),
        cleancraft_core::fsutil::items(p.items.len()),
        human_size(p.total_size())
    )
}

const TILE_MIN_H: f32 = 184.0;
const TILE_MAX_H: f32 = 260.0;

fn tile_ui(app: &mut CleanApp, ui: &mut Ui, tile: &sweep::Tile, w: f32, tile_h: f32) {
    let t = app.t;
    let skipped = app.settings.sweep_skip.contains(&tile.id);
    let mut review = false;
    let mut retry = false;
    let mut include = !skipped;
    let (icon, color) = kit::feature_look(&t, tile.id);
    let refreshing = app.sweep.refreshing(tile.id);
    let cleaned = app.sweep.cleaned.contains(&tile.id);
    let is_trash = tile.id == FeatureId::TrashBins;
    // A plan tile with nothing left in it (never the Trash, which shows its size).
    let empty = !is_trash && matches!(&tile.data, TileData::Plan(p) if p.items.is_empty());
    let trash_full = is_trash
        && matches!(&tile.data, TileData::Plan(p) if p.items.iter().any(|i| i.category != "mail_trash" && i.size > 0));
    let mut rescan = false;
    let (rect, _) = ui.allocate_exact_size(vec2(w, tile_h), Sense::hover());
    let p = ui.painter().clone();
    // Card with a glow of the module's colour in its corner.
    p.add(egui::Shape::rect_filled(
        rect.translate(vec2(0.0, 6.0)),
        16.0,
        Color32::from_black_alpha(50),
    ));
    p.rect_filled(rect, 16.0, t.glass);
    icons::tile_glow(
        &p,
        rect.shrink(0.5),
        16.0,
        pos2(rect.right() - 36.0, rect.top() + 28.0),
        w * 0.6,
        color.gamma_multiply(if skipped { 0.05 } else { 0.16 }),
    );
    p.rect_stroke(
        rect,
        16.0,
        egui::Stroke::new(1.0, t.glass_stroke),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(18.0);
    let mut share: Option<f32> = None;
    kit::place(ui, inner, |ui| {
        if skipped {
            ui.multiply_opacity(0.5);
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            kit::badge(ui, color, icon, isz::CARD);
            ui.add_space(space::XS);
            ui.add(
                egui::Label::new(
                    crate::rich(sweep::tile_title(tile.id))
                        .font(font(size::TITLE3, Weight::SemiBold))
                        .color(t.text),
                )
                .truncate(),
            );
        });
        ui.add_space(space::M);
        let (big, small, big_color) = match &tile.data {
            // The Trash is never part of the run: the tile says what it holds and offers
            // Finder's Empty Trash (REVIEW-U U4, N2).
            TileData::Plan(p) if tile.id == FeatureId::TrashBins => (
                human_size(p.total_size()),
                if p.items.is_empty() || p.total_size() == 0 {
                    "The Trash is empty.".to_string()
                } else {
                    "Waiting in the Trash. Empty it to get the space back.".to_string()
                },
                t.text,
            ),
            // Nothing left: cleaned from its own page since the scan, or nothing found
            // (REVIEW-U N1).
            TileData::Plan(p) if p.items.is_empty() => (
                "All clean".to_string(),
                if cleaned {
                    "Cleaned just now.".to_string()
                } else {
                    "Nothing to clean right now.".to_string()
                },
                t.text,
            ),
            TileData::Plan(p) if !tile.runnable() => (
                human_size(p.total_size()),
                format!(
                    "{} found. Open the page to act on them.",
                    cleancraft_core::fsutil::items(p.items.len())
                ),
                t.text,
            ),
            // Developer Clutter: what was found, and the module's own preselection under it.
            TileData::Plan(p) if sweep::shows_found_size(tile.id) => {
                share = Some(p.selected_size() as f32 / p.total_size().max(1) as f32);
                (
                    human_size(p.total_size()),
                    format!(
                        "found in {}, {} selected",
                        cleancraft_core::fsutil::items(p.items.len()),
                        human_size(p.selected_size())
                    ),
                    t.text,
                )
            }
            // Duplicate Downloads: the extra copies; one of each set always stays.
            TileData::Plan(p) if tile.id == FeatureId::Duplicates => {
                let (bytes, copies) = plan_page::duplicate_copies(p);
                share = Some(p.selected_size() as f32 / bytes.max(1) as f32);
                (
                    human_size(p.selected_size()),
                    format!(
                        "{} of {} extra {} selected. One of each set stays.",
                        p.selected_count(),
                        copies,
                        if copies == 1 { "copy" } else { "copies" }
                    ),
                    t.text,
                )
            }
            // The big number is what Run would move (the selection); what was found is said
            // under it, so a module with nothing preselected (Mail Downloads) still shows it.
            TileData::Plan(p) => {
                share = Some(p.selected_size() as f32 / p.total_size().max(1) as f32);
                (human_size(p.selected_size()), sweep_tile_line(p), t.text)
            }
            TileData::Count { count, noun } => (
                cleancraft_core::fsutil::count(*count, noun.trim_end_matches('s'), noun),
                "Open the page to act on them.".to_string(),
                t.text,
            ),
            TileData::Failed(e) => (
                "Could not scan".to_string(),
                plan_page::sentence(e),
                t.warning,
            ),
        };
        let small = if refreshing {
            "Checking again...".to_string()
        } else {
            small
        };
        let failed = matches!(tile.data, TileData::Failed(_));
        ui.horizontal(|ui| {
            if failed {
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Warning, big_color);
            } else if empty {
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Check, t.success);
            }
            ui.label(
                crate::rich(big)
                    .font(font(
                        if failed || empty {
                            size::TITLE2
                        } else {
                            size::NUMBER
                        },
                        Weight::Bold,
                    ))
                    .color(if failed { t.text } else { big_color }),
            );
        });
        ui.add(
            egui::Label::new(
                crate::rich(small)
                    .font(font(size::CALLOUT, Weight::Regular))
                    .color(t.text_dim),
            )
            .wrap(),
        );
    });
    // How much of what was found is selected, just above the controls.
    if let Some(f) = share {
        let bar = Rect::from_min_size(
            pos2(inner.left(), inner.bottom() - 32.0 - 20.0),
            vec2(inner.width(), 6.0),
        );
        if bar.top() > inner.top() + 120.0 {
            let mut q = p.clone();
            if skipped {
                q.set_opacity(0.5);
            }
            kit::paint_bar(&q, bar, &t, f, color);
        }
    }
    // Controls on the bottom edge.
    let controls = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 32.0), inner.max);
    kit::place_with(
        ui,
        controls,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if empty && cleaned {
                // Cleaned since the scan: look at this one module again (REVIEW-U N1).
                let r = kit::secondary_resp(ui, &t, "Scan Again");
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        !refreshing,
                        crate::tr(format!("Scan {} again", sweep::tile_title(tile.id))),
                    )
                });
                if r.clicked() && !refreshing {
                    rescan = true;
                }
                return;
            }
            // Named per tile for VoiceOver ("Review Duplicate Downloads") (U7-12).
            let title = sweep::tile_title(tile.id);
            let r = kit::secondary_resp(ui, &t, "Review");
            r.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    crate::tr(format!("Review {title}")),
                )
            });
            if r.clicked() {
                review = true;
            }
            // Include only where there is something to clean.
            if tile.runnable() && !empty {
                ui.add_space(4.0);
                let r = kit::check_text(ui, &t, &mut include, "Include");
                let on = include;
                r.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        true,
                        on,
                        crate::tr(format!("Include {title}")),
                    )
                });
            }
            if trash_full {
                ui.add_space(4.0);
                super::extras_cleanup::empty_trash(app, ui, "Empty Trash...");
            }
            // A module that could not scan offers to scan again (REVIEW-U U23).
            if matches!(tile.data, TileData::Failed(_)) {
                let r = kit::secondary_resp(ui, &t, "Try Again");
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        crate::tr(format!("Scan {} again", sweep::tile_title(tile.id))),
                    )
                });
                if r.clicked() {
                    retry = true;
                }
            }
        },
    );
    if retry || rescan {
        // Only this module scans again; the other tiles keep their results.
        app.act();
        app.sweep.refresh_tile(tile.id);
        return;
    }
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
    let width = kit::dialog_width(ui.ctx());
    let max_list = (ui.ctx().content_rect().height() - 350.0).clamp(120.0, 380.0);
    let dlg = kit::modal(egui::Id::new("sweep-confirm"), &t).show(ui.ctx(), |ui| {
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
            if plans.iter().any(|p| p.feature == Some(FeatureId::Malware)) {
                "These exact files will be moved. Threats go to the CleanCraft quarantine. You can put everything back from History."
            } else {
                "These exact files will be moved. You can put everything back from History."
            },
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
            // Height known from the first frame, so the dialog opens in its final place.
            let est: f32 = plans
                .iter()
                .map(|p| 32.0 + p.selected_count() as f32 * crate::theme::hit::CONFIRM_LINE)
                .sum();
            // A little room for the module rows' own height (taller than the badge), so a
            // short list never scrolls (U7-11).
            let list_h = (est + 16.0).min(max_list);
            let out = egui::ScrollArea::vertical()
                .min_scrolled_height(list_h)
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for p in &plans {
                        let Some(fid) = p.feature else { continue };
                        let (icon, color) = kit::feature_look(&t, fid);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            kit::badge(ui, color, icon, 24.0);
                            ui.label(
                                crate::rich(format!(
                                    "{}: {}, {}",
                                    sweep::tile_title(fid),
                                    cleancraft_core::fsutil::items(p.selected_count()),
                                    human_size(p.selected_size())
                                ))
                                .font(font(size::BODY, Weight::SemiBold))
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
                                &plan_page::confirm_reason(&app.env, p, it),
                                &human_size(it.size),
                            );
                        }
                    }
                });
            kit::list_more(ui, &t, &out, count);
        });
        ui.add_space(space::L);
        let label = match app.settings.method {
            Method::Trash => "Move to Trash",
            Method::Quarantine | Method::AdminQuarantine => "Move to Quarantine",
        };
        let (ok, cancel) = kit::dialog_actions(ui, &t, label, kit::Kind::Danger);
        go |= ok;
        close |= cancel;
    });
    if dlg.should_close() {
        close = true;
    }
    if go {
        app.act();
        start_run(app);
    } else if close {
        app.act();
        app.sweep.phase = Phase::Results;
    }
}

/// The files of a run that did not move and can be tried again (not the ones that are gone).
fn retryable(rep: &executor::Report) -> Vec<std::path::PathBuf> {
    rep.skipped
        .iter()
        .filter(|(_, w)| !matches!(w, executor::SkipReason::Missing))
        .map(|(p, _)| p.clone())
        .collect()
}

/// Did the runs move anything to the Trash, to the quarantine?
fn moved_to(runs: &[ModuleRun]) -> (bool, bool) {
    let (mut trash, mut quarantine) = (false, false);
    for rep in runs.iter().filter_map(|r| r.result.as_ref().ok()) {
        for m in &rep.moved {
            if let executor::ActionKind::Moved { method, .. } = &m.kind {
                match method {
                    Method::Trash => trash = true,
                    Method::Quarantine | Method::AdminQuarantine => quarantine = true,
                }
            }
        }
    }
    (trash, quarantine)
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
    let mut retry: Option<Vec<std::path::PathBuf>> = None;
    // One centred composition like every module's Done screen: the object, the headline,
    // the buttons, then a card per module, all centred in the page (scrolls when long).
    egui::ScrollArea::vertical()
        .id_salt("sweep-done")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            kit::centered_block(ui, egui::Id::new("sweep-done-block"), |ui| {
                let d = if ui.available_height() < 480.0 {
                    120.0
                } else {
                    160.0
                };
                kit::done_hero(ui, &t, FeatureId::SmartCare, d, started);
                ui.add_space(space::M);
                ui.label(
                    crate::rich(headline)
                        .font(font(size::LARGE_TITLE, Weight::Bold))
                        .color(t.text),
                );
                // Where the space comes back, by where the items went; Finder's Empty
                // Trash right here when something went to the Trash (REVIEW-U U4).
                let (to_trash, to_quarantine) = moved_to(&runs);
                let note = match (to_trash, to_quarantine) {
                    (true, false) => Some("The space comes back when the Trash is emptied."),
                    (false, true) => Some(
                        "The space comes back when the quarantine is moved to the Trash (Settings) and the Trash is emptied.",
                    ),
                    (true, true) => Some(
                        "The space comes back when the Trash is emptied. Threats wait in the quarantine (Settings).",
                    ),
                    (false, false) => None,
                };
                if let Some(note) = note {
                    kit::dim(ui, &t, note);
                }
                ui.add_space(space::L);
                kit::centered_row(ui, egui::Id::new("sweep-done-buttons"), |ui| {
                    if to_trash {
                        super::extras_cleanup::empty_trash(app, ui, "Empty Trash...");
                    }
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
                ui.add_space(space::XL);
                // A centred column, not full width: short rows read better.
                let col = ui.available_width().min(760.0);
                for r in &runs {
                    let (icon, color) = kit::feature_look(&t, r.id);
                    ui.allocate_ui_with_layout(
                        vec2(col, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            kit::card(&t)
                                .inner_margin(egui::Margin::symmetric(16, 12))
                                .show(ui, |ui| {
                                    ui.set_width(col - 34.0);
                                    ui.horizontal(|ui| {
                                        kit::badge(ui, color, icon, isz::CARD);
                                        ui.add_space(space::XS);
                                        ui.label(
                                            crate::rich(sweep::tile_title(r.id))
                                                .font(font(size::TITLE3, Weight::SemiBold))
                                                .color(t.text),
                                        );
                                        match &r.result {
                                            Ok(rep) => {
                                                ui.label(
                                                    crate::rich(if rep.skipped.is_empty() {
                                                        format!(
                                                            "{} moved, {}",
                                                            rep.moved.len(),
                                                            human_size(rep.freed)
                                                        )
                                                    } else {
                                                        format!(
                                                            "{} moved, {} skipped, {}",
                                                            rep.moved.len(),
                                                            rep.skipped.len(),
                                                            human_size(rep.freed)
                                                        )
                                                    })
                                                    .font(font(size::CALLOUT, Weight::Regular))
                                                    .color(t.text_dim),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
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
                                                            undo =
                                                                Some((r.id, rep.plan_id.clone()));
                                                        }
                                                    },
                                                );
                                            }
                                            Err(e) => {
                                                ui.label(crate::rich(e).color(t.danger));
                                            }
                                        }
                                    });
                                    if let Ok(rep) = &r.result {
                                        // Each file that stayed: why, and the next step, as
                                        // on every module's Done screen.
                                        for (p, why) in &rep.skipped {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.add_space(44.0);
                                                ui.label(
                                                    crate::rich(app.env.display(p))
                                                        .color(t.text_dim),
                                                );
                                                ui.label(
                                                    crate::rich(why.to_string()).color(t.warning),
                                                );
                                                if let Some(h) = plan_page::skip_hint(why) {
                                                    ui.label(
                                                        crate::rich(h)
                                                            .font(font(
                                                                size::FOOTNOTE,
                                                                Weight::Regular,
                                                            ))
                                                            .color(t.text_dim),
                                                    );
                                                }
                                            });
                                        }
                                        let again = retryable(rep);
                                        if !again.is_empty() && !app.sweep.undone.contains(&r.id) {
                                            ui.horizontal(|ui| {
                                                ui.add_space(44.0);
                                                let b = kit::secondary_resp(ui, &t, "Try Again");
                                                let title = sweep::tile_title(r.id);
                                                b.widget_info(|| {
                                                    egui::WidgetInfo::labeled(
                                                        egui::WidgetType::Button,
                                                        true,
                                                        crate::tr(format!("Try Again {title}")),
                                                    )
                                                });
                                                if b.clicked() {
                                                    retry = Some(again.clone());
                                                }
                                            });
                                        }
                                    }
                                });
                        },
                    );
                    ui.add_space(space::S);
                }
            });
        });
    if let Some(again) = retry {
        // Only what did not move, with the same confirmation listing it.
        app.act();
        if let Some(res) = app.sweep.result.as_mut() {
            for tile in &mut res.tiles {
                if let TileData::Plan(plan) = &mut tile.data {
                    for i in 0..plan.items.len() {
                        let on = again.contains(&plan.items[i].path);
                        plan.set_selected(i, on);
                    }
                }
            }
            if !res.to_run(&app.settings.sweep_skip).is_empty() {
                app.sweep.phase = Phase::Confirm;
            } else {
                app.warn(
                    "Those files are no longer in Quick Sweep's list. Scan Again to look for them.",
                );
            }
        }
        return;
    }
    if let Some((id, plan_id)) = undo {
        app.act();
        let (n, errs) = executor::undo_plan(&app.env, &plan_id);
        app.files_back(&[]);
        app.sweep.undone.push(id);
        if errs.is_empty() {
            app.tell(format!(
                "{}: {} put back.",
                sweep::tile_title(id),
                cleancraft_core::fsutil::items(n)
            ));
        } else {
            app.warn(format!(
                "{}: {} put back, {} could not be: {}",
                sweep::tile_title(id),
                cleancraft_core::fsutil::items(n),
                errs.len(),
                errs[0].1
            ));
        }
    }
}
