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
use egui::{Align, Layout, RichText, Ui};

use crate::app::CleanApp;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font};

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

pub fn start_scan(app: &mut CleanApp, f: FeatureId) {
    let env = app.env.clone();
    let settings = app.settings.clone();
    let sync = app.sync;
    let job = Job::start(sync, move |status| {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let Some(feat) = features::get(f) else {
            return Err(format!("{} has no scan yet", f.title()));
        };
        let ctx = features::ScanCtx {
            env: &env,
            settings: &settings,
            cancel: &cancel,
            progress: &|s| status(s),
        };
        feat.scan(&ctx).map_err(|e| format!("{e:#}"))
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
            } else {
                !job.is_dead()
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
            kit::subtitle(ui, &t, blurb(f));
        });
    });
    ui.add_space(10.0);
    if let Some(e) = app.plan_state(f).error.clone() {
        ui.label(RichText::new(e).color(t.danger));
    }
    let phase = match &app.plan_state(f).phase {
        Phase::Idle => 0,
        Phase::Scanning(_) => 1,
        Phase::Results => 2,
        Phase::Confirm => 3,
        Phase::Cleaning(_) => 4,
        Phase::Done(_) => 5,
    };
    match phase {
        0 => idle(app, ui, f),
        1 => {
            let status = match &app.plan_state(f).phase {
                Phase::Scanning(j) => j.status(),
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Looking in {status}"));
            });
        }
        2 => results(app, ui, f),
        3 => {
            results(app, ui, f);
            confirm(app, ui, f);
        }
        4 => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Moving the selected items...");
            });
        }
        _ => done(app, ui, f),
    }
}

fn idle(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        super::extras::before_scan(app, ui, f);
        kit::dim(
            ui,
            &t,
            "Scanning only looks. Nothing is removed until you review and confirm the list.",
        );
        ui.add_space(8.0);
        if kit::primary(ui, &t, "Scan") {
            app.act();
            start_scan(app, f);
        }
    });
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

fn results(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let Some(plan) = app.plan_state(f).plan.clone() else {
        return;
    };
    // Summary line.
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(human_size(plan.total_size()))
                .font(font(30.0, Weight::Bold))
                .color(t.accent),
        );
        ui.label(RichText::new(format!("found in {} items", plan.items.len())).color(t.text_dim));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::secondary(ui, &t, "Scan Again") {
                app.act();
                start_scan(app, f);
            }
        });
    });
    for note in &plan.notes {
        kit::dim(ui, &t, note);
    }
    if plan.items.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("Nothing to clean here.").font(font(16.0, Weight::SemiBold)));
        return;
    }
    // Bottom bar first, so it always has its place at the bottom of the window.
    egui::Panel::bottom(egui::Id::new(("plan-bottom", f.as_str())))
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
        .show(ui, |ui| {
            let sel_size = app
                .plan_state(f)
                .plan
                .as_ref()
                .map(|p| p.selected_size())
                .unwrap_or(0);
            let sel_count = app
                .plan_state(f)
                .plan
                .as_ref()
                .map(|p| p.selected_count())
                .unwrap_or(0);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{sel_count} selected, {}", human_size(sel_size)))
                        .font(font(14.0, Weight::SemiBold)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if sel_count > 0 && kit::primary(ui, &t, "Review and Clean") {
                        app.act();
                        app.plan_state(f).phase = Phase::Confirm;
                    }
                });
            });
        });
    ui.add_space(6.0);
    let avail_h = ui.available_height() - 8.0;
    let body =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), avail_h));
    let left = egui::Rect::from_min_size(body.min, egui::vec2(250.0, avail_h));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + 12.0, body.top()), body.max);
    ui.allocate_rect(body, egui::Sense::hover());
    {
        // Categories.
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
            ui.set_clip_rect(left.intersect(ui.clip_rect()));
            kit::card(&t).show(ui, |ui| {
                ui.set_min_size(egui::vec2(222.0, avail_h - 30.0));
                ui.set_max_width(222.0);
                egui::ScrollArea::vertical().id_salt("cats").show(ui, |ui| {
                    let all_sel = app.plan_state(f).category.is_none();
                    if ui
                        .selectable_label(
                            all_sel,
                            format!("All items  {}", human_size(plan.total_size())),
                        )
                        .clicked()
                    {
                        app.act();
                        app.plan_state(f).category = None;
                    }
                    for c in &plan.categories {
                        let count = plan.category_count(&c.id);
                        if count == 0 {
                            continue;
                        }
                        let sel_items = plan
                            .items
                            .iter()
                            .filter(|i| i.category == c.id && i.locked.is_none())
                            .collect::<Vec<_>>();
                        let all_on = !sel_items.is_empty() && sel_items.iter().all(|i| i.selected);
                        ui.horizontal(|ui| {
                            let mut on = all_on;
                            let cb = ui.add_enabled(
                                !sel_items.is_empty(),
                                egui::Checkbox::without_text(&mut on),
                            );
                            cb.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Checkbox,
                                    !sel_items.is_empty(),
                                    all_on,
                                    format!("Select {}", c.title),
                                )
                            });
                            if cb.changed() {
                                app.act();
                                if let Some(p) = app.plan_state(f).plan.as_mut() {
                                    p.set_category_selected(&c.id, on);
                                }
                                keep_a_copy(app, f);
                            }
                            let current =
                                app.plan_state(f).category.as_deref() == Some(c.id.as_str());
                            if ui
                                .selectable_label(
                                    current,
                                    format!(
                                        "{}  {}",
                                        c.title,
                                        human_size(plan.category_size(&c.id))
                                    ),
                                )
                                .on_hover_text(&c.description)
                                .clicked()
                            {
                                app.act();
                                app.plan_state(f).category = Some(c.id.clone());
                            }
                        });
                    }
                });
            });
        });
        // Items.
        ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
            ui.set_clip_rect(right.intersect(ui.clip_rect()));
            ui.horizontal(|ui| {
                let st = app.plan_state(f);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.search)
                        .hint_text("Search")
                        .desired_width(220.0),
                );
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search")
                });
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
                    });
                if sort != app.plan_state(f).sort {
                    app.act();
                    app.plan_state(f).sort = sort;
                }
                if ui.button("Select All").clicked() {
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
                if ui.button("Select None").clicked() {
                    app.act();
                    let ids = visible_items(app.plan_state(f));
                    if let Some(p) = app.plan_state(f).plan.as_mut() {
                        for id in ids {
                            p.set_selected(id, false);
                        }
                    }
                }
            });
            let rows = rows(app.plan_state(f));
            kit::card(&t).show(ui, |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), avail_h - 76.0));
                egui::ScrollArea::vertical()
                    .id_salt("items")
                    .max_height(avail_h - 76.0)
                    .show_rows(ui, 44.0, rows.len(), |ui, range| {
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
        _ => format!("{g}: {n} items, {} selected", human_size(freed)),
    };
    ui.horizontal(|ui| {
        ui.set_height(40.0);
        ui.add_space(2.0);
        ui.label(
            RichText::new(text)
                .font(font(12.5, Weight::SemiBold))
                .color(t.text_dim),
        );
    });
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
    ui.horizontal(|ui| {
        ui.set_height(40.0);
        let mut on = selected_now;
        let cb = ui.add_enabled(it.locked.is_none(), egui::Checkbox::new(&mut on, &name));
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
        ui.vertical(|ui| {
            kit::dim(ui, &t, &app.env.display(&it.path));
            let why = match &it.locked {
                Some(l) => format!("{} (cannot be selected: {l})", it.reason),
                None => it.reason.clone(),
            };
            ui.label(
                RichText::new(why)
                    .font(font(11.0, Weight::Regular))
                    .color(kit::risk_color(&t, it.risk)),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("Ignore")
                .on_hover_text("Never propose this again (Settings > Ignore list)")
                .clicked()
            {
                app.act();
                app.settings.ignore.push(it.path.clone());
                app.save_settings();
                if let Some(p) = app.plan_state(f).plan.as_mut() {
                    p.set_selected(id, false);
                    p.items[id].locked = Some("on your ignore list".into());
                }
            }
            if ui
                .small_button("Reveal")
                .on_hover_text("Show in Finder")
                .clicked()
            {
                app.act();
                let path = it.path.to_string_lossy().into_owned();
                if let Err(e) = app.env.sys.run("/usr/bin/open", &["-R", &path]) {
                    app.warn(format!("Could not open Finder: {e}"));
                }
            }
            ui.label(RichText::new(human_size(it.size)).font(font(13.0, Weight::SemiBold)));
        });
    });
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
        executor::Method::Quarantine => "the CleanCraft quarantine",
    };
    let mut close = false;
    let mut go = false;
    egui::Modal::new(egui::Id::new(("confirm", f.as_str()))).show(ui.ctx(), |ui| {
        ui.set_width(640.0);
        ui.label(
            RichText::new(format!(
                "Move {} items ({}) to {dest}?",
                plan.selected_count(),
                human_size(plan.selected_size())
            ))
            .font(font(18.0, Weight::Bold)),
        );
        kit::dim(
            ui,
            &t,
            "These exact files will be moved. You can put them back from History.",
        );
        egui::ScrollArea::both()
            .max_width(640.0)
            .max_height(360.0)
            .show(ui, |ui| {
                egui::Grid::new("confirm-list")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for it in plan.selected() {
                            ui.label(app.env.display(&it.path));
                            ui.label(human_size(it.size));
                            ui.label(RichText::new(&it.reason).color(t.text_dim));
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = match method {
                executor::Method::Trash => "Move to Trash",
                executor::Method::Quarantine => "Move to Quarantine",
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

fn done(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let Phase::Done(rep) = &app.plan_state(f).phase else {
        return;
    };
    let rep = rep.clone();
    ui.label(
        RichText::new(format!("{} freed", human_size(rep.freed)))
            .font(font(30.0, Weight::Bold))
            .color(t.accent),
    );
    ui.label(format!(
        "{} items moved. {} skipped.",
        rep.moved.len(),
        rep.skipped.len()
    ));
    if !rep.skipped.is_empty() {
        kit::card(&t).show(ui, |ui| {
            ui.label(RichText::new("Not moved").font(font(14.0, Weight::SemiBold)));
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (p, why) in &rep.skipped {
                        ui.horizontal(|ui| {
                            ui.label(app.env.display(p));
                            ui.label(RichText::new(why.to_string()).color(t.warning));
                        });
                    }
                });
        });
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if !rep.moved.is_empty() && kit::secondary(ui, &t, "Undo") {
            app.act();
            let (n, errs) = executor::undo_plan(&app.env, &rep.plan_id);
            if errs.is_empty() {
                app.tell(format!("{n} items put back."));
            } else {
                app.warn(format!(
                    "{n} items put back, {} could not be: {}",
                    errs.len(),
                    errs[0].1
                ));
            }
            start_scan(app, f);
        }
        if kit::secondary(ui, &t, "Show History") {
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
