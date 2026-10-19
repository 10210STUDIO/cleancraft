//! The page every cleanup module shares: Scan, review the exact list, confirm, move, undo.
//!
//! Flow: Idle -> Scanning -> Results (categories, items, search, sort, select) -> Confirm (a
//! dialog with every selected file, its size and reason) -> Cleaning -> Done (freed, skipped
//! with reasons, Undo).

use cleancraft_core::executor::{self, Report};
use cleancraft_core::features;
use cleancraft_core::features::duplicates;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::{FeatureId, Plan};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Align, Color32, Layout, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

#[derive(Default)]
pub enum Phase {
    #[default]
    Idle,
    Scanning(Job<Result<Plan, String>>),
    Results,
    Confirm,
    Cleaning(Job<Result<Report, String>>),
    Done(Report),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Size,
    Name,
}

#[derive(Default)]
pub struct PlanState {
    pub phase: Phase,
    pub plan: Option<Plan>,
    /// The category shown on the right; None = all.
    pub category: Option<String>,
    pub search: String,
    pub sort: Sort,
    pub error: Option<String>,
    /// Set to stop the running scan.
    pub cancel: Arc<AtomicBool>,
}

impl PlanState {
    pub fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Scanning(_) | Phase::Cleaning(_))
    }
}

/// Short text about each module, shown before the first scan.
pub fn blurb(f: FeatureId) -> &'static str {
    match f {
        FeatureId::SystemJunk => {
            "Caches, logs and leftovers that macOS and your apps rebuild on their own."
        }
        FeatureId::MailAttachments => {
            "Copies of attachments Mail keeps on disk. The originals stay on your mail server."
        }
        FeatureId::TrashBins => "What is waiting in the Trash of every disk.",
        FeatureId::LargeOldFiles => {
            "Big files and files you have not opened for a long time. Nothing is selected for you."
        }
        FeatureId::Duplicates => "Files with exactly the same content in more than one place.",
        FeatureId::SimilarPhotos => "Pictures that look alike: bursts, edits, resized copies.",
        FeatureId::Malware => {
            "Startup programs and apps that are unsigned, tampered with or known to be unwanted."
        }
        FeatureId::Privacy => "Browser history, cookies and recent-item lists.",
        _ => "",
    }
}

/// Scans that look at `ScanCtx::cancel` while they work: these get a Stop button.
pub fn can_stop(f: FeatureId) -> bool {
    matches!(
        f,
        FeatureId::SystemJunk
            | FeatureId::MailAttachments
            | FeatureId::LargeOldFiles
            | FeatureId::Duplicates
            | FeatureId::SimilarPhotos
            | FeatureId::Privacy
            | FeatureId::Malware
    )
}

pub fn start_scan(app: &mut CleanApp, f: FeatureId) {
    let env = app.env.clone();
    let settings = app.settings.clone();
    let sync = app.sync;
    let flag = Arc::new(AtomicBool::new(false));
    app.plan_state(f).cancel = flag.clone();
    let job = Job::start(sync, move |status| {
        let cancel = flag;
        let Some(feat) = features::get(f) else {
            return Err(format!("{} has no scan yet", f.title()));
        };
        let ctx = features::ScanCtx {
            env: &env,
            settings: &settings,
            cancel: &cancel,
            progress: &|s| status(s),
        };
        features::scan_with(feat.as_ref(), &ctx).map_err(|e| format!("{e:#}"))
    });
    let st = app.plan_state(f);
    st.phase = Phase::Scanning(job);
    st.error = None;
}

fn start_clean(app: &mut CleanApp, f: FeatureId) {
    let Some(plan) = app.plan_state(f).plan.clone() else {
        return;
    };
    let env = app.env.clone();
    let settings = app.settings.clone();
    let digest = plan.digest();
    let job = Job::start(app.sync, move |status| {
        features::run_plan(&env, &settings, &plan, &digest, &mut |n, total| {
            status(&format!("{n} of {total}"))
        })
        .map_err(|e| format!("{e:#}"))
    });
    app.plan_state(f).phase = Phase::Cleaning(job);
}

/// Finish jobs. Returns true while something is running.
pub fn poll(app: &mut CleanApp, f: FeatureId) -> bool {
    let st = app.plan_state(f);
    match &mut st.phase {
        Phase::Scanning(job) => {
            if let Some(r) = job.poll() {
                match r {
                    Ok(plan) => {
                        st.plan = Some(plan);
                        st.category = None;
                        st.phase = Phase::Results;
                    }
                    Err(e) => {
                        st.error = Some(e);
                        st.phase = Phase::Idle;
                    }
                }
                false
            } else if job.is_dead() {
                st.error = Some("The scan stopped unexpectedly.".into());
                st.phase = Phase::Idle;
                false
            } else {
                true
            }
        }
        Phase::Cleaning(job) => {
            if let Some(r) = job.poll() {
                match r {
                    Ok(rep) => st.phase = Phase::Done(rep),
                    Err(e) => {
                        st.error = Some(e);
                        st.phase = Phase::Results;
                    }
                }
                false
            } else if job.is_dead() {
                st.error = Some(
                    "The cleanup stopped unexpectedly. History shows what was moved before it stopped."
                        .into(),
                );
                st.phase = Phase::Results;
                false
            } else {
                true
            }
        }
        _ => false,
    }
}

/// Stop the running scan: the scan is told to stop and its result is dropped.
pub fn stop_scan(app: &mut CleanApp, f: FeatureId) {
    let st = app.plan_state(f);
    st.cancel.store(true, Ordering::Relaxed);
    st.phase = Phase::Idle;
    app.tell(format!("{} scan stopped. Nothing changed.", f.title()));
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    let phase = match &app.plan_state(f).phase {
        Phase::Idle => 0,
        Phase::Scanning(_) => 1,
        Phase::Results => 2,
        Phase::Confirm => 3,
        Phase::Cleaning(_) => 4,
        Phase::Done(_) => 5,
    };
    if phase != 0 {
        kit::header(ui, &t, icon, f.title(), blurb(f));
    }
    if let Some(e) = app.plan_state(f).error.clone() {
        kit::strip(ui, Icon::Warning, t.danger, &e);
        ui.add_space(space::S);
    }
    match phase {
        0 => idle(app, ui, f),
        1 => {
            let (status, secs) = match &app.plan_state(f).phase {
                Phase::Scanning(j) => (j.status(), j.elapsed_secs()),
                _ => (String::new(), 0),
            };
            let place = if status.is_empty() {
                "Getting ready...".to_string()
            } else {
                format!("Looking in {status}")
            };
            if kit::progress(
                ui,
                &t,
                icon,
                &format!("Scanning {}", f.title()),
                &place,
                secs,
                can_stop(f),
            ) {
                app.act();
                stop_scan(app, f);
            }
        }
        2 => results(app, ui, f),
        3 => {
            results(app, ui, f);
            confirm(app, ui, f);
        }
        4 => {
            let (status, secs) = match &app.plan_state(f).phase {
                Phase::Cleaning(j) => (j.status(), j.elapsed_secs()),
                _ => (String::new(), 0),
            };
            kit::progress(
                ui,
                &t,
                Icon::Trash,
                "Moving the selected items...",
                &status,
                secs,
                false,
            );
        }
        _ => done(app, ui, f),
    }
}

fn idle(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    let go = kit::hero(
        ui,
        &t,
        icon,
        f.title(),
        blurb(f),
        "Scan",
        "Scanning only looks. Nothing is removed until you review and confirm the list.",
        |ui| super::extras::before_scan(app, ui, f),
    );
    if go {
        app.act();
        start_scan(app, f);
    }
}

fn visible_items(st: &PlanState) -> Vec<usize> {
    let Some(plan) = &st.plan else {
        return Vec::new();
    };
    let q = st.search.to_lowercase();
    let mut v: Vec<usize> = plan
        .items
        .iter()
        .filter(|i| st.category.as_ref().is_none_or(|c| &i.category == c))
        .filter(|i| {
            q.is_empty()
                || i.path.to_string_lossy().to_lowercase().contains(&q)
                || i.reason.to_lowercase().contains(&q)
        })
        .map(|i| i.id)
        .collect();
    match st.sort {
        Sort::Size => v.sort_by(|a, b| plan.items[*b].size.cmp(&plan.items[*a].size)),
        Sort::Name => v.sort_by(|a, b| {
            let n = |i: usize| {
                plan.items[i]
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            };
            n(*a).cmp(&n(*b))
        }),
    }
    v
}

/// Height of a line in the item list.
const ROW_H: f32 = 52.0;

fn results(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let Some(plan) = app.plan_state(f).plan.clone() else {
        return;
    };
    // Summary: the big total and the sentence under it.
    ui.horizontal(|ui| {
        kit::display(ui, &t, &human_size(plan.total_size()));
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!(
                    "found in {}",
                    cleancraft_core::fsutil::items(plan.items.len())
                ))
                .font(font(15.0, Weight::Medium))
                .color(t.text_dim),
            );
            for note in &plan.notes {
                kit::dim(ui, &t, note);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::secondary(ui, &t, "Scan Again") {
                app.act();
                start_scan(app, f);
            }
        });
    });
    if plan.items.is_empty() {
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            kit::badge(ui, t.success, Icon::Check, 64.0);
            ui.add_space(space::M);
            ui.label(
                RichText::new("Nothing to clean here.")
                    .font(font(18.0, Weight::SemiBold))
                    .color(t.text),
            );
        });
        return;
    }
    // Bottom bar first, so it always has its place at the bottom of the window.
    let (sel_count, sel_size) = app
        .plan_state(f)
        .plan
        .as_ref()
        .map(|p| (p.selected_count(), p.selected_size()))
        .unwrap_or((0, 0));
    egui::Panel::bottom(egui::Id::new(("plan-bottom", f.as_str())))
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 10,
            bottom: 0,
        }))
        .show(ui, |ui| {
            kit::bar_frame(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(38.0);
                    ui.label(
                        RichText::new(human_size(sel_size))
                            .font(font(22.0, Weight::Bold))
                            .color(t.text),
                    );
                    ui.label(
                        RichText::new(format!("{sel_count} selected, {}", human_size(sel_size)))
                            .font(font(12.5, Weight::Medium))
                            .color(t.text_dim),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if sel_count > 0 && kit::primary(ui, &t, "Review and Clean") {
                            app.act();
                            app.plan_state(f).phase = Phase::Confirm;
                        } else if sel_count == 0 {
                            kit::dim(ui, &t, "Tick the items to clean.");
                        }
                    });
                });
            });
        });
    ui.add_space(space::S);
    let avail_h = ui.available_height() - 4.0;
    let body =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), avail_h));
    let left_w = if body.width() < 760.0 { 220.0 } else { 252.0 };
    let left = egui::Rect::from_min_size(body.min, egui::vec2(left_w, avail_h));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + space::M, body.top()), body.max);
    ui.allocate_rect(body, egui::Sense::hover());
    // Categories.
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.set_clip_rect(left.expand(24.0).intersect(ui.clip_rect()));
        kit::card(&t)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(left_w - 18.0, avail_h - 18.0));
                ui.set_max_width(left_w - 18.0);
                egui::ScrollArea::vertical()
                    .id_salt("cats")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        categories(app, ui, f, &plan);
                    });
            });
    });
    // Items.
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.set_clip_rect(right.expand(24.0).intersect(ui.clip_rect()));
        // Wraps when a module adds its own buttons, so nothing is pushed out of the window.
        ui.horizontal_wrapped(|ui| {
            let st = app.plan_state(f);
            let r = ui.add(
                egui::TextEdit::singleline(&mut st.search)
                    .hint_text("Search")
                    .desired_width(200.0)
                    .margin(egui::Margin::symmetric(10, 7)),
            );
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search"));
            if r.changed() {
                app.act();
            }
            let st = app.plan_state(f);
            let mut sort = st.sort;
            egui::ComboBox::from_id_salt("sort")
                .selected_text(match sort {
                    Sort::Size => "Sort by size",
                    Sort::Name => "Sort by name",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sort, Sort::Size, "Sort by size");
                    ui.selectable_value(&mut sort, Sort::Name, "Sort by name");
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Sort")
                });
            if sort != app.plan_state(f).sort {
                app.act();
                app.plan_state(f).sort = sort;
            }
            if kit::secondary(ui, &t, "Select All") {
                app.act();
                let ids = visible_items(app.plan_state(f));
                if let Some(p) = app.plan_state(f).plan.as_mut() {
                    for id in ids {
                        p.set_selected(id, true);
                    }
                }
                keep_a_copy(app, f);
            }
            super::extras::results_toolbar(app, ui, f);
            if kit::secondary(ui, &t, "Select None") {
                app.act();
                let ids = visible_items(app.plan_state(f));
                if let Some(p) = app.plan_state(f).plan.as_mut() {
                    for id in ids {
                        p.set_selected(id, false);
                    }
                }
            }
        });
        ui.add_space(4.0);
        let rows = rows(app.plan_state(f));
        let list_h = (ui.available_height() - 18.0).max(60.0);
        kit::card(&t)
            .inner_margin(egui::Margin::symmetric(6, 8))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), list_h));
                if rows.is_empty() {
                    ui.add_space(space::XL);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No items match.").color(t.text_dim));
                    });
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("items")
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row in &rows[range] {
                            match row {
                                Row::Item(id) => item_row(app, ui, f, &plan, *id),
                                Row::Group(g) => group_row(app, ui, f, g),
                            }
                        }
                    });
            });
    });
}

/// The category column: "All items" and one card per category with a share bar.
fn categories(app: &mut CleanApp, ui: &mut Ui, f: FeatureId, plan: &Plan) {
    let t = app.t;
    let total = plan.total_size().max(1);
    let all_sel = app.plan_state(f).category.is_none();
    let all_label = format!("All items  {}", human_size(plan.total_size()));
    let r = category_row(
        ui,
        &t,
        &all_label,
        "All items",
        &human_size(plan.total_size()),
        &cleancraft_core::fsutil::items(plan.items.len()),
        1.0,
        t.sec_a,
        all_sel,
        None,
    );
    if r.row_clicked {
        app.act();
        app.plan_state(f).category = None;
    }
    ui.add_space(2.0);
    let mut k = 0;
    for c in &plan.categories {
        let count = plan.category_count(&c.id);
        if count == 0 {
            continue;
        }
        let color = kit::series_color(&t, k + 1);
        k += 1;
        let sel_items = plan
            .items
            .iter()
            .filter(|i| i.category == c.id && i.locked.is_none())
            .collect::<Vec<_>>();
        let all_on = !sel_items.is_empty() && sel_items.iter().all(|i| i.selected);
        let size = plan.category_size(&c.id);
        let current = app.plan_state(f).category.as_deref() == Some(c.id.as_str());
        let label = format!("{}  {}", c.title, human_size(size));
        let r = category_row(
            ui,
            &t,
            &label,
            &c.title,
            &human_size(size),
            &cleancraft_core::fsutil::items(count),
            size as f32 / total as f32,
            color,
            current,
            Some((!sel_items.is_empty(), all_on, format!("Select {}", c.title))),
        );
        r.row.on_hover_text(&c.description);
        if let Some(on) = r.check_changed {
            app.act();
            if let Some(p) = app.plan_state(f).plan.as_mut() {
                p.set_category_selected(&c.id, on);
            }
            keep_a_copy(app, f);
        }
        if r.row_clicked {
            app.act();
            app.plan_state(f).category = Some(c.id.clone());
        }
    }
}

pub struct CatResp {
    pub row: egui::Response,
    pub row_clicked: bool,
    pub check_changed: Option<bool>,
}

/// One category card: an optional checkbox, the title and size, the item count and a bar
/// with the category's share of the total. The row's accessible name is `label`.
pub fn category_row(
    ui: &mut Ui,
    t: &crate::theme::Tokens,
    label: &str,
    title: &str,
    size_text: &str,
    count_text: &str,
    share: f32,
    color: Color32,
    current: bool,
    check: Option<(bool, bool, String)>,
) -> CatResp {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 48.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, current, label)
    });
    let hov = kit::anim(
        ui.ctx(),
        resp.id.with("hov"),
        resp.hovered(),
        crate::theme::motion::HOVER,
    );
    let p = ui.painter().clone();
    if current {
        p.rect_filled(r, 12.0, color.gamma_multiply(0.16));
        p.rect_stroke(
            r,
            12.0,
            egui::Stroke::new(1.0, color.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
    } else if hov > 0.0 {
        p.rect_filled(r, 12.0, Color32::from_white_alpha((10.0 * hov) as u8));
    }
    let mut x = r.left() + 10.0;
    let mut check_changed = None;
    if let Some((enabled, on, name)) = check {
        let cb_rect = Rect::from_center_size(pos2(x + 9.0, r.top() + 16.0), vec2(20.0, 20.0));
        let mut v = on;
        let cb = kit::place(ui, cb_rect, |ui| {
            ui.add_enabled_ui(enabled, |ui| kit::check(ui, t, &mut v, &name))
                .inner
        });
        if cb.changed() {
            check_changed = Some(v);
        }
        x += 26.0;
    } else {
        p.circle_filled(pos2(x + 9.0, r.top() + 16.0), 5.0, color);
        x += 26.0;
    }
    let size_g = kit::galley(ui, size_text, font(13.0, Weight::SemiBold), t.text);
    let size_w = size_g.size().x;
    p.galley(
        pos2(
            r.right() - 10.0 - size_w,
            r.top() + 16.0 - size_g.size().y / 2.0,
        ),
        size_g,
        t.text,
    );
    let title_w = r.right() - 18.0 - size_w - x;
    let title_s = kit::truncate_middle(ui.ctx(), title, &font(13.0, Weight::SemiBold), title_w);
    p.text(
        pos2(x, r.top() + 16.0),
        egui::Align2::LEFT_CENTER,
        title_s,
        font(13.0, Weight::SemiBold),
        if current {
            t.text
        } else {
            icons::lerp(t.text_dim, t.text, 0.5 + 0.5 * hov)
        },
    );
    p.text(
        pos2(x, r.top() + 33.0),
        egui::Align2::LEFT_CENTER,
        count_text,
        font(10.5, Weight::Regular),
        t.text_faint,
    );
    let bar = Rect::from_min_max(
        pos2(x + 62.0, r.top() + 31.0),
        pos2(r.right() - 10.0, r.top() + 35.0),
    );
    kit::paint_bar(&p, bar, t, share, color);
    CatResp {
        row_clicked: resp.clicked(),
        row: resp,
        check_changed,
    }
}

/// A line of the item list: a set header (duplicates, look-alike pictures) or an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Group(String),
    Item(usize),
}

/// The visible items, with a header above each set when items belong to sets. Sets keep the
/// order of their first item in the sorted list; items without a set come last.
pub fn rows(st: &PlanState) -> Vec<Row> {
    let ids = visible_items(st);
    let Some(plan) = &st.plan else {
        return Vec::new();
    };
    if !ids.iter().any(|i| plan.items[*i].group.is_some()) {
        return ids.into_iter().map(Row::Item).collect();
    }
    let mut order: Vec<String> = Vec::new();
    let mut sets: std::collections::HashMap<String, Vec<usize>> = Default::default();
    let mut loose = Vec::new();
    for id in ids {
        match &plan.items[id].group {
            Some(g) => {
                if !sets.contains_key(g) {
                    order.push(g.clone());
                }
                sets.entry(g.clone()).or_default().push(id);
            }
            None => loose.push(id),
        }
    }
    // Sets of copies always get a header; other groupings only when it groups something.
    let copies = plan.feature.is_some_and(keeps_a_copy);
    let mut out = Vec::new();
    for g in order {
        let ids = sets.remove(&g).unwrap_or_default();
        if copies || ids.len() > 1 {
            out.push(Row::Group(g.clone()));
        }
        out.extend(ids.into_iter().map(Row::Item));
    }
    out.extend(loose.into_iter().map(Row::Item));
    out
}

/// Features whose sets are copies of one another: one item per set must always stay.
/// Other features may use `group` only to show headers (e.g. one set per app).
pub fn keeps_a_copy(f: FeatureId) -> bool {
    matches!(f, FeatureId::Duplicates | FeatureId::SimilarPhotos)
}

/// After a bulk selection: every set keeps one copy (the suggested one).
fn keep_a_copy(app: &mut CleanApp, f: FeatureId) {
    if !keeps_a_copy(f) {
        return;
    }
    let env = app.env.clone();
    let fixed = app
        .plan_state(f)
        .plan
        .as_mut()
        .map(|p| duplicates::keep_one_per_group(&env, p))
        .unwrap_or(0);
    if fixed > 0 {
        app.tell(format!(
            "One file in each set stays unselected, so a copy is always kept ({fixed} sets)."
        ));
    }
}

fn group_row(app: &mut CleanApp, ui: &mut Ui, f: FeatureId, g: &str) {
    let t = app.t;
    let Some(plan) = app.plan_state(f).plan.as_ref() else {
        return;
    };
    let members: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.group.as_deref() == Some(g))
        .collect();
    let n = members.len();
    let kept = members
        .iter()
        .filter(|i| !(i.selected && i.locked.is_none()))
        .count();
    let freed: u64 = members
        .iter()
        .filter(|i| i.selected && i.locked.is_none())
        .map(|i| i.size)
        .sum();
    let text = match f {
        FeatureId::SimilarPhotos => {
            format!(
                "{n} similar pictures: {kept} kept, {} to remove",
                human_size(freed)
            )
        }
        FeatureId::Duplicates => {
            format!(
                "{n} identical files: {kept} kept, {} to remove",
                human_size(freed)
            )
        }
        _ => format!(
            "{g}: {}, {} selected",
            cleancraft_core::fsutil::items(n),
            human_size(freed)
        ),
    };
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
    let line = Rect::from_min_max(
        pos2(r.left() + 6.0, r.bottom() - 10.0),
        pos2(r.right() - 6.0, r.bottom() - 9.0),
    );
    ui.painter()
        .rect_filled(line, 0.0, t.sec_a.gamma_multiply(0.25));
    let ir = Rect::from_center_size(
        pos2(r.left() + 18.0, r.center().y + 2.0),
        egui::Vec2::splat(16.0),
    );
    icons::paint(ui.painter(), ir, Icon::Duplicates, t.sec_a);
    kit::text_at(
        ui,
        Rect::from_min_max(
            pos2(r.left() + 34.0, r.top() + 8.0),
            pos2(r.right() - 8.0, r.bottom() - 10.0),
        ),
        &text,
        &text,
        font(12.5, Weight::SemiBold),
        t.text_dim,
        Align::Min,
    );
}

fn item_row(app: &mut CleanApp, ui: &mut Ui, f: FeatureId, plan: &Plan, id: usize) {
    let t = app.t;
    let it = &plan.items[id];
    let selected_now = app
        .plan_state(f)
        .plan
        .as_ref()
        .map(|p| p.items[id].selected)
        .unwrap_or(false);
    let name = it
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
    let hov = kit::anim(
        ui.ctx(),
        egui::Id::new(("row-hov", f.as_str(), id)),
        resp.contains_pointer(),
        crate::theme::motion::HOVER,
    );
    let p = ui.painter().clone();
    let card = r.shrink2(vec2(2.0, 2.0));
    if selected_now {
        p.rect_filled(card, 10.0, t.sec_a.gamma_multiply(0.07 + 0.04 * hov));
    } else if hov > 0.0 {
        p.rect_filled(card, 10.0, Color32::from_white_alpha((9.0 * hov) as u8));
    }
    let locked = it.locked.is_some();
    // Checkbox, named after the file.
    let cb_rect = Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(20.0, 20.0));
    let mut on = selected_now;
    let cb = kit::place(ui, cb_rect, |ui| {
        ui.add_enabled_ui(!locked, |ui| kit::check(ui, &t, &mut on, &name))
            .inner
    });
    if cb.changed() {
        app.act();
        let every_copy = on
            && keeps_a_copy(f)
            && app
                .plan_state(f)
                .plan
                .as_ref()
                .is_some_and(|p| duplicates::would_select_every_copy(p, id));
        if every_copy {
            app.warn(
                "Keep at least one file in each set. Deselect another copy first, then select this one.",
            );
        } else if let Some(p) = app.plan_state(f).plan.as_mut() {
            p.set_selected(id, on);
        }
    }
    // Kind tile.
    let tile = Rect::from_center_size(pos2(r.left() + 50.0, r.center().y), vec2(32.0, 32.0));
    p.rect_filled(tile, 9.0, Color32::from_white_alpha(12));
    let kind = kit::kind_icon(&it.path);
    icons::paint(
        &p,
        tile.shrink(7.0),
        kind,
        icons::lerp(t.sec_a, t.text, 0.25),
    );
    // Actions and size on the right.
    let right_w = 190.0;
    let actions = Rect::from_min_max(
        pos2(r.right() - right_w, r.top()),
        pos2(r.right() - 6.0, r.bottom()),
    );
    kit::place_with(ui, actions, Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let ig = kit::ghost(ui, &t, "Ignore", None)
            .on_hover_text("Never propose this again (Settings > Ignore list)");
        if ig.clicked() {
            app.act();
            app.settings.ignore.push(it.path.clone());
            app.save_settings();
            if let Some(p) = app.plan_state(f).plan.as_mut() {
                p.set_selected(id, false);
                p.items[id].locked = Some("on your ignore list".into());
            }
        }
        // Only files and folders on disk can be shown in Finder (Wi-Fi networks and
        // permissions are listed under a label, not a file).
        if std::fs::symlink_metadata(&it.path).is_ok()
            && kit::ghost(ui, &t, "Reveal", None)
                .on_hover_text("Show in Finder")
                .clicked()
        {
            app.act();
            let path = it.path.to_string_lossy().into_owned();
            match app.env.sys.run("/usr/bin/open", &["-R", &path]) {
                Ok(out) if out.ok() => {}
                Ok(out) => app.warn(format!("Could not open Finder: {}", out.stderr.trim())),
                Err(e) => app.warn(format!("Could not open Finder: {e}")),
            }
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(human_size(it.size))
                .font(font(13.5, Weight::SemiBold))
                .color(if locked { t.text_faint } else { t.text }),
        );
    });
    // Name, then the reason as a tag and the dimmed path.
    let x0 = r.left() + 76.0;
    let text_w = (r.right() - right_w - 8.0 - x0).max(60.0);
    let name_font = font(13.5, Weight::SemiBold);
    let name_s = kit::truncate_middle(ui.ctx(), &name, &name_font, text_w);
    // Painted only: the checkbox already carries the name for VoiceOver.
    p.text(
        pos2(x0, r.top() + 16.0),
        egui::Align2::LEFT_CENTER,
        name_s,
        name_font,
        if locked { t.text_dim } else { t.text },
    );
    let why = match &it.locked {
        Some(l) => format!("{} (cannot be selected: {l})", it.reason),
        None => reason_text(it),
    };
    let tag_font = font(10.5, Weight::Medium);
    let tag_max = (text_w * 0.5).max(60.0);
    let tag_s = kit::truncate_middle(ui.ctx(), &why, &tag_font, tag_max - 14.0);
    let color = kit::risk_color(&t, it.risk);
    let tg = kit::galley(ui, &tag_s, tag_font.clone(), color);
    let tag_r = Rect::from_min_size(pos2(x0, r.top() + 29.0), tg.size() + vec2(14.0, 5.0));
    p.rect_filled(tag_r, tag_r.height() / 2.0, color.gamma_multiply(0.15));
    kit::text_at(ui, tag_r, &tag_s, &why, tag_font, color, Align::Center);
    let shown = app.env.display(&it.path);
    let path_x = tag_r.right() + 8.0;
    let path_w = (x0 + text_w - path_x).max(24.0);
    let pf = font(11.0, Weight::Regular);
    let path_s = kit::truncate_middle(ui.ctx(), &shown, &pf, path_w);
    kit::text_at(
        ui,
        Rect::from_min_size(
            pos2(path_x, r.top() + 28.0),
            vec2(path_w, tag_r.height() + 2.0),
        ),
        &path_s,
        &shown,
        pf,
        t.text_faint,
        Align::Min,
    );
}

/// The confirmation line for items that need the administrator password, when there are any.
pub fn admin_note(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some(
            "1 item is in a folder only an administrator can change. macOS will ask for the administrator password, and it goes to the CleanCraft quarantine."
                .into(),
        ),
        n => Some(format!(
            "{n} items are in folders only an administrator can change. macOS will ask for the administrator password, and they go to the CleanCraft quarantine."
        )),
    }
}

/// The reason shown for an item, with the administrator note.
pub fn reason_text(it: &cleancraft_core::Item) -> String {
    let mut s = it.reason.clone();
    if it.needs_admin && it.locked.is_none() {
        s.push_str(" (needs the administrator password)");
    }
    s
}

fn confirm(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let Some(plan) = app.plan_state(f).plan.clone() else {
        return;
    };
    // A feature can insist on the quarantine (Threat Check); say where items really go.
    let method = features::get(f)
        .and_then(|x| x.method_override())
        .unwrap_or(app.settings.method);
    let dest = match method {
        executor::Method::Trash => "the Trash",
        executor::Method::Quarantine | executor::Method::AdminQuarantine => {
            "the CleanCraft quarantine"
        }
    };
    let mut close = false;
    let mut go = false;
    let width = (ui.ctx().content_rect().width() - 120.0).clamp(420.0, 640.0);
    let max_list = (ui.ctx().content_rect().height() - 330.0).clamp(120.0, 360.0);
    kit::modal(egui::Id::new(("confirm", f.as_str())), &t).show(ui.ctx(), |ui| {
        ui.set_width(width);
        kit::dialog_head(
            ui,
            &t,
            Icon::Trash,
            t.sec_a,
            &format!(
                "Move {} ({}) to {dest}?",
                cleancraft_core::fsutil::items(plan.selected_count()),
                human_size(plan.selected_size())
            ),
            "These exact files will be moved. You can put them back from History.",
        );
        ui.add_space(space::M);
        if let Some(note) = admin_note(features::admin_count(&plan)) {
            kit::strip(ui, Icon::Warning, t.warning, &note);
            ui.add_space(space::S);
        }
        // Only the visible rows are laid out: plans can hold thousands of items.
        let rows: Vec<&cleancraft_core::Item> = plan.selected().collect();
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("confirm-list")
                .max_height(max_list)
                .show_rows(ui, 26.0, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for it in &rows[range] {
                        kit::confirm_line(
                            ui,
                            &t,
                            &it.path,
                            &app.env.display(&it.path),
                            &it.reason,
                            &human_size(it.size),
                        );
                    }
                });
        });
        ui.add_space(space::L);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = match method {
                executor::Method::Trash => "Move to Trash",
                executor::Method::Quarantine | executor::Method::AdminQuarantine => {
                    "Move to Quarantine"
                }
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
        start_clean(app, f);
    } else if close {
        app.act();
        app.plan_state(f).phase = Phase::Results;
    }
}

/// The list of what did not move, with the reason for each.
pub fn skipped_card(
    ui: &mut Ui,
    t: &crate::theme::Tokens,
    env: &cleancraft_core::Env,
    skipped: &[(std::path::PathBuf, impl std::fmt::Display)],
    max_h: f32,
) {
    if skipped.is_empty() {
        return;
    }
    kit::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width().min(720.0));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Warning, t.warning);
            ui.label(
                RichText::new("Not moved")
                    .font(font(size::H2, Weight::SemiBold))
                    .color(t.text),
            );
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(max_h.max(60.0))
            .show(ui, |ui| {
                for (p, why) in skipped {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(env.display(p)).color(t.text));
                        ui.label(RichText::new(why.to_string()).color(t.warning));
                    });
                }
            });
    });
}

/// The Done screen every cleanup shares: the check, how much moved and where, the summary
/// line, the buttons (from the caller) and what did not move.
pub fn done_view(
    app: &mut CleanApp,
    ui: &mut Ui,
    rep: &Report,
    summary: &str,
    buttons: impl FnOnce(&mut CleanApp, &mut Ui),
) {
    let t = app.t;
    let started = kit::first_seen(ui, ("done", &rep.plan_id));
    let id = egui::Id::new(("done-block", &rep.plan_id));
    kit::centered_block(ui, id, |ui| {
        let d = if ui.available_height() < 420.0 {
            84.0
        } else {
            116.0
        };
        kit::done_check(ui, &t, d, started);
        ui.add_space(space::M);
        ui.label(
            RichText::new(features::moved_text([rep]))
                .font(font(30.0, Weight::Bold))
                .color(t.text),
        );
        if !rep.moved.is_empty() {
            kit::dim(ui, &t, cleancraft_core::features::SPACE_NOTE);
        }
        ui.label(RichText::new(summary).color(t.text_dim));
        ui.add_space(space::L);
        kit::centered_row(ui, id.with("buttons"), |ui| buttons(app, ui));
        ui.add_space(space::L);
        let h = ui.available_height() - 80.0;
        skipped_card(ui, &t, &app.env, &rep.skipped, h);
    });
}

fn done(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let Phase::Done(rep) = &app.plan_state(f).phase else {
        return;
    };
    let rep = rep.clone();
    let summary = format!(
        "{} moved. {} skipped.",
        cleancraft_core::fsutil::items(rep.moved.len()),
        rep.skipped.len()
    );
    done_view(app, ui, &rep, &summary, |app, ui| {
        let t = app.t;
        if !rep.moved.is_empty()
            && kit::pill(ui, &t, "Undo", kit::Kind::Secondary, Some(Icon::Undo)).clicked()
        {
            app.act();
            let (n, errs) = executor::undo_plan(&app.env, &rep.plan_id);
            if errs.is_empty() {
                app.tell(format!("{} put back.", cleancraft_core::fsutil::items(n)));
            } else {
                app.warn(format!(
                    "{} put back, {} could not be: {}",
                    cleancraft_core::fsutil::items(n),
                    errs.len(),
                    errs[0].1
                ));
            }
            start_scan(app, f);
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
            let st = app.plan_state(f);
            st.phase = Phase::Idle;
            st.plan = None;
        }
    });
}
