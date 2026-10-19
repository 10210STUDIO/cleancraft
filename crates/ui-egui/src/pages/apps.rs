//! App Remover and App Updates.
//!
//! App Remover: the app list (search, sort, groups), an app's detail with every related file
//! and a checkbox per file, Uninstall and Reset with a confirmation listing the exact files,
//! then a Done screen with Undo. The groups Leftovers, Installers and Extensions show the App
//! Remover's own scan with Remove Selected.
//!
//! App Updates: Check, then per app "installed -> newest", release notes, and buttons that
//! hand off to the app's own updater, its download page or the App Store; macOS updates open
//! Software Update. CleanCraft never installs software itself.
//!
//! Keys: Enter in the search box opens the first app shown; Esc leaves an app's detail or
//! cancels the confirmation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cleancraft_core::apps::{self, AppInfo, Store};
use cleancraft_core::executor::{self, Method, Report};
use cleancraft_core::extensions::CAT_EXTENSIONS;
use cleancraft_core::features::{self, leftovers, uninstaller};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::updater::{self, Source, UpdateInfo, UpdatesReport};
use cleancraft_core::{FeatureId, Plan};
use egui::{Align, Key, Layout, RichText, Ui};

use crate::app::CleanApp;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Group {
    #[default]
    All,
    Unused,
    AppStore,
    Setapp,
    Other,
    Leftovers,
    Installers,
    Extensions,
}

impl Group {
    pub const LIST: [Group; 8] = [
        Group::All,
        Group::Unused,
        Group::AppStore,
        Group::Setapp,
        Group::Other,
        Group::Leftovers,
        Group::Installers,
        Group::Extensions,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::All => "All Apps",
            Group::Unused => "Unused",
            Group::AppStore => "From App Store",
            Group::Setapp => "From Setapp",
            Group::Other => "From Elsewhere",
            Group::Leftovers => "Leftovers",
            Group::Installers => "Installers",
            Group::Extensions => "Extensions",
        }
    }

    /// The App Remover scan category this group shows, for file groups.
    pub fn category(self) -> Option<&'static str> {
        match self {
            Group::Leftovers => Some(leftovers::CAT_ORPHANS),
            Group::Installers => Some(leftovers::CAT_INSTALLERS),
            Group::Extensions => Some(CAT_EXTENSIONS),
            _ => None,
        }
    }

    fn has(self, a: &AppInfo, now: i64) -> bool {
        match self {
            Group::All => true,
            Group::Unused => a.unused(now) && !a.apple,
            Group::AppStore => a.store == Store::AppStore,
            Group::Setapp => a.store == Store::Setapp,
            Group::Other => a.store == Store::Other,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Size,
    Name,
    LastOpened,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            SortBy::Size => "Sort by size",
            SortBy::Name => "Sort by name",
            SortBy::LastOpened => "Sort by last opened",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Uninstall,
    Reset,
    RemoveFiles,
}

#[derive(Default)]
pub enum Phase {
    #[default]
    Browse,
    Confirm(Action),
    Working(Job<Result<Report, String>>, Action),
    Done(Report, Action),
}

type Loaded = (Vec<AppInfo>, Result<Plan, String>);

#[derive(Default)]
pub struct State {
    /// The app list, once looked at.
    pub list: Option<Vec<AppInfo>>,
    /// Leftovers of removed apps, installers and extensions.
    pub extras: Option<Plan>,
    load: Option<Job<Loaded>>,
    pub group: Group,
    pub search: String,
    pub sort: SortBy,
    /// The app whose detail is open.
    pub selected: Option<PathBuf>,
    /// Its uninstall plan (checkboxes).
    pub detail: Option<Plan>,
    /// The plan shown in the confirmation and carried out.
    pub pending: Option<Plan>,
    pub phase: Phase,
    pub error: Option<String>,
    pub updates: Option<UpdatesReport>,
    upd_job: Option<Job<UpdatesReport>>,
    pub notes_open: BTreeSet<PathBuf>,
}

impl State {
    pub fn is_loading(&self) -> bool {
        self.load.is_some()
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------- work ----------

/// Look at the apps again (size, last opened) and the App Remover's own scan.
pub fn start_load(app: &mut CleanApp) {
    let env = app.env.clone();
    let settings = app.settings.clone();
    let job = Job::start(app.sync, move |status| {
        let list = apps::inventory(&env, true, &|n| status(n));
        status("leftovers and extensions");
        let extras =
            features::scan(&env, &settings, FeatureId::Uninstaller).map_err(|e| format!("{e:#}"));
        (list, extras)
    });
    app.apps.load = Some(job);
}

fn start_updates(app: &mut CleanApp) {
    let env = app.env.clone();
    let job = Job::start(app.sync, move |status| {
        let list = apps::inventory(&env, false, &|_| {});
        updater::check_all(&env, &list, &|n| status(n))
    });
    app.apps.upd_job = Some(job);
}

/// Open an app's detail with a fresh plan.
pub fn open_app(app: &mut CleanApp, path: &Path) {
    app.apps.selected = Some(path.to_path_buf());
    app.apps.error = None;
    rebuild_detail(app, None);
}

/// Recompute the selected app's plan, keeping the user's checkboxes from `keep`.
fn rebuild_detail(app: &mut CleanApp, keep: Option<&Plan>) {
    let Some(path) = app.apps.selected.clone() else {
        return;
    };
    if !path.exists() {
        app.apps.selected = None;
        app.apps.detail = None;
        return;
    }
    match uninstaller::uninstall_plan(&app.env, &app.settings, &path, false) {
        Ok(mut p) => {
            if let Some(old) = keep {
                copy_selection(old, &mut p);
            }
            app.apps.detail = Some(p);
        }
        Err(e) => {
            app.apps.detail = None;
            app.apps.error = Some(format!("{e:#}"));
        }
    }
}

/// Carry checkbox choices over by path (leftovers only; the app item follows the action).
fn copy_selection(from: &Plan, to: &mut Plan) {
    for i in 0..to.items.len() {
        if to.items[i].category == uninstaller::CAT_APP {
            continue;
        }
        if let Some(o) = from.items.iter().find(|o| o.path == to.items[i].path) {
            to.set_selected(i, o.selected);
        }
    }
}

fn start_action(app: &mut CleanApp, action: Action) {
    let Some(plan) = app.apps.pending.clone() else {
        app.apps.phase = Phase::Browse;
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
    app.apps.phase = Phase::Working(job, action);
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    let st = &mut app.apps;
    if let Some(job) = st.load.as_mut() {
        if let Some((list, extras)) = job.poll() {
            st.load = None;
            st.list = Some(list);
            match extras {
                Ok(p) => st.extras = Some(p),
                Err(e) => {
                    st.extras = None;
                    st.error = Some(e);
                }
            }
        } else if job.is_dead() {
            st.load = None;
            st.error = Some("Looking at the apps stopped unexpectedly.".into());
        } else {
            busy = true;
        }
    }
    if let Some(job) = st.upd_job.as_mut() {
        if let Some(r) = job.poll() {
            st.upd_job = None;
            st.updates = Some(r);
        } else if job.is_dead() {
            st.upd_job = None;
            st.error = Some("The update check stopped unexpectedly.".into());
        } else {
            busy = true;
        }
    }
    let mut finished = None;
    if let Phase::Working(job, action) = &mut st.phase {
        if let Some(r) = job.poll() {
            finished = Some((r, *action));
        } else if job.is_dead() {
            finished = Some((Err("The removal stopped unexpectedly.".into()), *action));
        } else {
            busy = true;
        }
    }
    if let Some((r, action)) = finished {
        match r {
            Ok(rep) => {
                app.apps.phase = Phase::Done(rep, action);
                app.apps.pending = None;
            }
            Err(e) => {
                app.apps.phase = Phase::Browse;
                app.apps.error = Some(e);
                app.apps.pending = None;
                let old = app.apps.detail.clone();
                rebuild_detail(app, old.as_ref());
            }
        }
        // The list changed on disk: look again.
        start_load(app);
        busy = true;
    }
    busy
}

// ---------- page ----------

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
                if f == FeatureId::Updater {
                    "Newer versions of your apps and of macOS. CleanCraft opens the right place to install them."
                } else {
                    "Remove apps with the files they leave behind, or reset an app to a fresh start."
                },
            );
        });
    });
    ui.add_space(10.0);
    if let Some(e) = app.apps.error.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&e).color(t.danger));
            if ui.small_button("Hide Message").clicked() {
                app.act();
                app.apps.error = None;
            }
        });
    }
    if f == FeatureId::Updater {
        updates_page(app, ui);
    } else {
        remover_page(app, ui);
    }
}

fn remover_page(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if app.apps.list.is_none() && app.apps.load.is_none() && app.apps.error.is_none() {
        start_load(app);
        poll(app);
    }
    match &app.apps.phase {
        Phase::Working(job, _) => {
            let s = job.status();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Moving to the Trash... {s}"));
            });
            return;
        }
        Phase::Done(..) => {
            done(app, ui);
            return;
        }
        _ => {}
    }
    if app.apps.list.is_none() {
        if let Some(job) = &app.apps.load {
            let s = job.status();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Looking at your apps: {s}"));
            });
        } else if kit::primary(ui, &t, "Look Again") {
            app.act();
            app.apps.error = None;
            start_load(app);
        }
        return;
    }
    if app.apps.selected.is_some() {
        detail(app, ui);
    } else {
        browse(app, ui);
    }
    if let Phase::Confirm(action) = app.apps.phase {
        confirm(app, ui, action);
    }
}

/// The apps listed: in the chosen group and matching the search, never an app on the ignore
/// list (Settings): CleanCraft does not offer to remove what the user asked it to leave alone.
fn visible_apps(st: &State, settings: &cleancraft_core::Settings) -> Vec<AppInfo> {
    let now = now();
    let q = st.search.trim().to_lowercase();
    let mut v: Vec<AppInfo> = st
        .list
        .iter()
        .flatten()
        .filter(|a| st.group.has(a, now))
        .filter(|a| !settings.is_ignored(&a.path))
        .filter(|a| {
            q.is_empty()
                || a.name.to_lowercase().contains(&q)
                || a.bundle_id.to_lowercase().contains(&q)
                || a.vendor.to_lowercase().contains(&q)
        })
        .cloned()
        .collect();
    match st.sort {
        SortBy::Size => v.sort_by_key(|a| std::cmp::Reverse(a.size)),
        SortBy::Name => v.sort_by_key(|a| a.name.to_lowercase()),
        // Longest unopened first; unknown dates last.
        SortBy::LastOpened => v.sort_by_key(|a| a.last_used.unwrap_or(i64::MAX)),
    }
    v
}

fn visible_files(st: &State) -> Vec<usize> {
    let (Some(plan), Some(cat)) = (&st.extras, st.group.category()) else {
        return Vec::new();
    };
    let q = st.search.trim().to_lowercase();
    let mut v: Vec<usize> = plan
        .items
        .iter()
        .filter(|i| i.category == cat)
        .filter(|i| {
            q.is_empty()
                || i.path.to_string_lossy().to_lowercase().contains(&q)
                || i.reason.to_lowercase().contains(&q)
        })
        .map(|i| i.id)
        .collect();
    match st.sort {
        SortBy::Name => v.sort_by_key(|i| {
            plan.items[*i]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        }),
        _ => v.sort_by(|a, b| plan.items[*b].size.cmp(&plan.items[*a].size)),
    }
    v
}

fn group_count(st: &State, g: Group) -> (usize, u64) {
    if let Some(cat) = g.category() {
        return st
            .extras
            .as_ref()
            .map(|p| (p.category_count(cat), p.category_size(cat)))
            .unwrap_or((0, 0));
    }
    let now = now();
    st.list
        .iter()
        .flatten()
        .filter(|a| g.has(a, now))
        .fold((0, 0), |(n, s), a| (n + 1, s + a.size))
}

fn browse(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    // Toolbar.
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut app.apps.search)
                .hint_text("Search apps")
                .desired_width(240.0),
        );
        r.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search apps")
        });
        if r.changed() {
            app.act();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            app.act();
            if app.apps.group.category().is_none()
                && let Some(first) = visible_apps(&app.apps, &app.settings).first()
            {
                let p = first.path.clone();
                open_app(app, &p);
                return;
            }
        }
        let mut sort = app.apps.sort;
        egui::ComboBox::from_id_salt("apps-sort")
            .selected_text(sort.label())
            .show_ui(ui, |ui| {
                for s in [SortBy::Size, SortBy::Name, SortBy::LastOpened] {
                    ui.selectable_value(&mut sort, s, s.label());
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Sort"));
        if sort != app.apps.sort {
            app.act();
            app.apps.sort = sort;
        }
        let loading = app.apps.load.is_some();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if loading {
                ui.spinner();
            } else if kit::secondary(ui, &t, "Look Again") {
                app.act();
                start_load(app);
            }
        });
    });
    if app.apps.selected.is_some() {
        return;
    }
    ui.add_space(8.0);
    let file_group = app.apps.group.category().is_some();
    if file_group {
        files_bottom_bar(app, ui);
    }
    let avail_h = ui.available_height() - 8.0;
    let body =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), avail_h));
    let left = egui::Rect::from_min_size(body.min, egui::vec2(250.0, avail_h));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + 12.0, body.top()), body.max);
    ui.allocate_rect(body, egui::Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.set_clip_rect(left.intersect(ui.clip_rect()));
        kit::card(&t).show(ui, |ui| {
            ui.set_min_size(egui::vec2(222.0, avail_h - 30.0));
            ui.set_max_width(222.0);
            for g in Group::LIST {
                if g == Group::Leftovers {
                    ui.add_space(6.0);
                    ui.separator();
                }
                let (n, size) = group_count(&app.apps, g);
                let text = if size > 0 {
                    format!("{}  {n}  ·  {}", g.title(), human_size(size))
                } else {
                    format!("{}  {n}", g.title())
                };
                let on = app.apps.group == g;
                if ui.selectable_label(on, text).clicked() {
                    app.act();
                    app.apps.group = g;
                }
            }
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.set_clip_rect(right.intersect(ui.clip_rect()));
        kit::card(&t).show(ui, |ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), avail_h - 30.0));
            if file_group {
                files_list(app, ui, avail_h - 30.0);
            } else {
                apps_list(app, ui, avail_h - 30.0);
            }
        });
    });
}

fn apps_list(app: &mut CleanApp, ui: &mut Ui, h: f32) {
    let t = app.t;
    let rows = visible_apps(&app.apps, &app.settings);
    if rows.is_empty() {
        ui.label(RichText::new("No apps here.").color(t.text_dim));
        return;
    }
    let now = now();
    let mut open = None;
    egui::ScrollArea::vertical()
        .id_salt("apps-list")
        .max_height(h)
        .show_rows(ui, 40.0, rows.len(), |ui, range| {
            for a in &rows[range] {
                ui.horizontal(|ui| {
                    ui.set_height(36.0);
                    let r = ui.add(
                        egui::Button::new(
                            RichText::new(&a.name).font(font(14.0, Weight::SemiBold)),
                        )
                        .frame(false),
                    );
                    if r.clicked() {
                        open = Some(a.path.clone());
                    }
                    let mut info = vec![];
                    if !a.version.is_empty() {
                        info.push(a.version.clone());
                    }
                    info.push(a.vendor.clone());
                    info.push(a.store.label().to_string());
                    info.push(format!(
                        "Opened: {}",
                        apps::last_used_text(a.last_used, now)
                    ));
                    kit::dim(ui, &t, &info.join("  ·  "));
                    if a.apple {
                        ui.label(
                            RichText::new("Part of macOS")
                                .font(font(11.0, Weight::Medium))
                                .color(t.warning),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(human_size(a.size)).font(font(13.0, Weight::SemiBold)),
                        );
                    });
                });
            }
        });
    if let Some(p) = open {
        app.act();
        open_app(app, &p);
    }
}

fn files_list(app: &mut CleanApp, ui: &mut Ui, h: f32) {
    let t = app.t;
    let ids = visible_files(&app.apps);
    if ids.is_empty() {
        ui.label(RichText::new("Nothing here.").color(t.text_dim));
    }
    if let Some(notes) = app.apps.extras.as_ref().map(|p| p.notes.clone()) {
        for n in notes {
            kit::dim(ui, &t, &n);
        }
    }
    let Some(plan) = app.apps.extras.clone() else {
        return;
    };
    let mut toggles = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("apps-files")
        .max_height(h - 20.0)
        .show_rows(ui, 44.0, ids.len(), |ui, range| {
            for &id in &ids[range] {
                if let Some(on) = file_row(app, ui, &plan.items[id]) {
                    toggles.push((id, on));
                }
            }
        });
    for (id, on) in toggles {
        app.act();
        if let Some(p) = app.apps.extras.as_mut() {
            p.set_selected(id, on);
        }
    }
}

/// One file with its checkbox; returns the new state when the user toggled it.
fn file_row(app: &CleanApp, ui: &mut Ui, it: &cleancraft_core::Item) -> Option<bool> {
    // The app itself is always part of an uninstall; Reset is the way to keep it.
    let toggle = it.locked.is_none() && it.category != uninstaller::CAT_APP;
    let t = app.t;
    let mut changed = None;
    ui.horizontal(|ui| {
        ui.set_height(40.0);
        let mut on = it.selected;
        let shown = app.env.display(&it.path);
        let cb = ui.add_enabled(toggle, egui::Checkbox::new(&mut on, &shown));
        if cb.changed() {
            changed = Some(on);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(human_size(it.size)).font(font(13.0, Weight::SemiBold)));
            let why = match &it.locked {
                Some(l) => format!("{} ({l})", it.reason),
                None => super::plan_page::reason_text(it),
            };
            ui.label(
                RichText::new(why)
                    .font(font(11.0, Weight::Regular))
                    .color(kit::risk_color(&t, it.risk)),
            );
        });
    });
    changed
}

fn files_bottom_bar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let (n, size) = app
        .apps
        .extras
        .as_ref()
        .map(|p| (p.selected_count(), p.selected_size()))
        .unwrap_or((0, 0));
    egui::Panel::bottom(egui::Id::new("apps-files-bottom"))
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{n} selected, {}", human_size(size)))
                        .font(font(14.0, Weight::SemiBold)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if n > 0 && kit::danger(ui, &t, "Remove Selected") {
                        app.act();
                        app.apps.pending = app.apps.extras.clone();
                        app.apps.phase = Phase::Confirm(Action::RemoveFiles);
                    }
                    if n > 0 && kit::secondary(ui, &t, "Clear Selection") {
                        app.act();
                        if let Some(p) = app.apps.extras.as_mut() {
                            for i in 0..p.items.len() {
                                p.set_selected(i, false);
                            }
                        }
                    }
                });
            });
        });
}

fn detail(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(path) = app.apps.selected.clone() else {
        return;
    };
    let info = app
        .apps
        .list
        .iter()
        .flatten()
        .find(|a| a.path == path)
        .cloned();
    let esc = matches!(app.apps.phase, Phase::Browse) && ui.input(|i| i.key_pressed(Key::Escape));
    if esc || kit::secondary(ui, &t, "Back to Apps") {
        app.act();
        app.apps.selected = None;
        app.apps.detail = None;
        return;
    }
    let Some(plan) = app.apps.detail.clone() else {
        ui.label(RichText::new("This app could not be read.").color(t.danger));
        return;
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.add_space(6.0);
    ui.label(
        RichText::new(&name)
            .font(font(20.0, Weight::Bold))
            .color(t.text),
    );
    if let Some(a) = &info {
        kit::dim(
            ui,
            &t,
            &format!(
                "Version {}  ·  {}  ·  {}  ·  {}  ·  Opened: {}",
                if a.version.is_empty() {
                    "?"
                } else {
                    &a.version
                },
                a.bundle_id,
                a.vendor,
                a.store.label(),
                apps::last_used_text(a.last_used, now())
            ),
        );
    }
    for n in &plan.notes {
        ui.label(RichText::new(n).color(t.warning));
    }
    let bundle_locked = plan
        .items
        .iter()
        .find(|i| i.category == uninstaller::CAT_APP)
        .map(|i| i.locked.is_some())
        .unwrap_or(true);
    let app_blocked = !plan.notes.is_empty();
    let files_sel = plan
        .selected()
        .filter(|i| i.category != uninstaller::CAT_APP)
        .count();
    // Bottom bar.
    egui::Panel::bottom(egui::Id::new("apps-detail-bottom"))
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} selected, {}",
                        plan.selected_count(),
                        human_size(plan.selected_size())
                    ))
                    .font(font(14.0, Weight::SemiBold)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let un =
                        ui.add_enabled_ui(!bundle_locked, |ui| kit::danger(ui, &t, "Uninstall"));
                    if un.inner {
                        app.act();
                        app.apps.pending = app.apps.detail.clone();
                        app.apps.phase = Phase::Confirm(Action::Uninstall);
                    }
                    let rs = ui.add_enabled_ui(!app_blocked && files_sel > 0, |ui| {
                        kit::secondary(ui, &t, "Reset")
                    });
                    rs.response
                        .on_hover_text("Remove the app's settings and data, keep the app");
                    if rs.inner {
                        app.act();
                        match uninstaller::uninstall_plan(&app.env, &app.settings, &path, true) {
                            Ok(mut p) => {
                                copy_selection(&plan, &mut p);
                                app.apps.pending = Some(p);
                                app.apps.phase = Phase::Confirm(Action::Reset);
                            }
                            Err(e) => app.apps.error = Some(format!("{e:#}")),
                        }
                    }
                    if kit::secondary(ui, &t, "Check Again") {
                        app.act();
                        rebuild_detail(app, Some(&plan));
                    }
                    if !app.settings.is_ignored(&path) {
                        let ig = ui.add(egui::Button::new("Ignore"));
                        if ig
                            .on_hover_text(
                                "Never offer this app for removal (Settings > Ignore list)",
                            )
                            .clicked()
                        {
                            app.act();
                            app.settings.ignore.push(path.clone());
                            app.save_settings();
                            app.apps.selected = None;
                            app.apps.detail = None;
                        }
                    }
                });
            });
            if !app_blocked && files_sel == 0 {
                kit::dim(ui, &t, "Select files to reset the app.");
            }
        });
    ui.add_space(6.0);
    let h = ui.available_height() - 8.0;
    let mut toggles = Vec::new();
    kit::card(&t).show(ui, |ui| {
        ui.set_min_size(egui::vec2(ui.available_width(), h - 30.0));
        egui::ScrollArea::vertical()
            .id_salt("apps-detail")
            .max_height(h - 30.0)
            .show(ui, |ui| {
                for c in &plan.categories {
                    let items: Vec<_> = plan.items.iter().filter(|i| i.category == c.id).collect();
                    if items.is_empty() {
                        continue;
                    }
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {}",
                            c.title,
                            human_size(plan.category_size(&c.id))
                        ))
                        .font(font(13.0, Weight::SemiBold)),
                    )
                    .on_hover_text(&c.description);
                    for it in items {
                        if let Some(on) = file_row(app, ui, it) {
                            toggles.push((it.id, on));
                        }
                    }
                    ui.add_space(6.0);
                }
            });
    });
    for (id, on) in toggles {
        app.act();
        if let Some(p) = app.apps.detail.as_mut() {
            p.set_selected(id, on);
        }
    }
}

fn confirm(app: &mut CleanApp, ui: &mut Ui, action: Action) {
    let t = app.t;
    let Some(plan) = app.apps.pending.clone() else {
        app.apps.phase = Phase::Browse;
        return;
    };
    let dest = match app.settings.method {
        Method::Trash => "the Trash",
        Method::Quarantine | Method::AdminQuarantine => "the CleanCraft quarantine",
    };
    let name = app
        .apps
        .selected
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = cleancraft_core::fsutil::items(plan.selected_count());
    let size = human_size(plan.selected_size());
    let verb = if plan.selected_count() == 1 {
        "moves"
    } else {
        "move"
    };
    let headline = match action {
        Action::Uninstall => format!("Uninstall {name}? {n} ({size}) {verb} to {dest}."),
        Action::Reset => format!("Reset {name}? {n} ({size}) {verb} to {dest}. The app stays."),
        Action::RemoveFiles => format!("Move {n} ({size}) to {dest}?"),
    };
    let mut go = false;
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::new("apps-confirm")).show(ui.ctx(), |ui| {
        ui.set_width(660.0);
        ui.label(RichText::new(&headline).font(font(17.0, Weight::Bold)));
        kit::dim(
            ui,
            &t,
            "These exact files will be moved. You can put them back from History.",
        );
        if let Some(note) =
            super::plan_page::admin_note(cleancraft_core::features::admin_count(&plan))
        {
            ui.label(RichText::new(note).color(t.warning));
        }
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                egui::Grid::new("apps-confirm-list")
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
    if resp.should_close() {
        close = true;
    }
    if go {
        app.act();
        start_action(app, action);
        poll(app);
    } else if close {
        app.act();
        app.apps.pending = None;
        app.apps.phase = Phase::Browse;
    }
}

fn done(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Phase::Done(rep, action) = &app.apps.phase else {
        return;
    };
    let (rep, action) = (rep.clone(), *action);
    let what = match action {
        Action::Uninstall => "Uninstalled. ",
        Action::Reset => "Reset done. ",
        Action::RemoveFiles => "",
    };
    ui.label(
        RichText::new(cleancraft_core::features::moved_text([&rep]))
            .font(font(30.0, Weight::Bold))
            .color(t.accent),
    );
    if !rep.moved.is_empty() {
        kit::dim(ui, &t, cleancraft_core::features::SPACE_NOTE);
    }
    ui.label(format!(
        "{what}{} moved. {} skipped.",
        cleancraft_core::fsutil::items(rep.moved.len()),
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
                app.tell(format!("{} put back.", cleancraft_core::fsutil::items(n)));
            } else {
                app.warn(format!(
                    "{} put back, {} could not be: {}",
                    cleancraft_core::fsutil::items(n),
                    errs.len(),
                    errs[0].1
                ));
            }
            app.apps.phase = Phase::Browse;
            let old = app.apps.detail.clone();
            rebuild_detail(app, old.as_ref());
            start_load(app);
        }
        if kit::secondary(ui, &t, "Show History") {
            app.act();
            app.apps.phase = Phase::Browse;
            app.navigate("history");
        }
        if kit::primary(ui, &t, "Done") {
            app.act();
            app.apps.phase = Phase::Browse;
            let old = app.apps.detail.clone();
            rebuild_detail(app, old.as_ref());
        }
    });
}

// ---------- updates ----------

fn updates_page(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if let Some(job) = &app.apps.upd_job {
        let s = job.status();
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Checking {s}..."));
        });
        return;
    }
    let Some(r) = app.apps.updates.clone() else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            kit::dim(
                ui,
                &t,
                "CleanCraft asks each app's update feed and the App Store, then hands the update to the app or the App Store.",
            );
            ui.add_space(8.0);
            if kit::primary(ui, &t, "Check for Updates") {
                app.act();
                start_updates(app);
                poll(app);
            }
        });
        return;
    };
    let avail: Vec<UpdateInfo> = r.available().cloned().collect();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(match avail.len() {
                0 => "All checked apps are up to date".to_string(),
                1 => "1 update available".to_string(),
                n => format!("{n} updates available"),
            })
            .font(font(22.0, Weight::Bold))
            .color(t.accent),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::secondary(ui, &t, "Check Again") {
                app.act();
                start_updates(app);
                poll(app);
            }
        });
    });
    let h = ui.available_height() - 8.0;
    egui::ScrollArea::vertical()
        .id_salt("updates")
        .max_height(h)
        .show(ui, |ui| {
            for u in &avail {
                update_row(app, ui, u);
            }
            let failed: Vec<&UpdateInfo> = r.apps.iter().filter(|u| u.error.is_some()).collect();
            if !failed.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Could not check").font(font(14.0, Weight::SemiBold)));
                for u in failed {
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            u.name,
                            u.error.clone().unwrap_or_default()
                        ))
                        .color(t.warning),
                    );
                }
            }
            let current: Vec<&UpdateInfo> = r
                .apps
                .iter()
                .filter(|u| !u.available && u.error.is_none())
                .collect();
            if !current.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Up to date").font(font(14.0, Weight::SemiBold)));
                for u in current {
                    kit::dim(
                        ui,
                        &t,
                        &format!("{}  {}  ({})", u.name, u.installed, u.source.label()),
                    );
                }
            }
            if r.not_checked > 0 {
                kit::dim(
                    ui,
                    &t,
                    &format!(
                        "{} apps have no update feed CleanCraft can read; they update themselves.",
                        r.not_checked
                    ),
                );
            }
            ui.add_space(10.0);
            ui.label(RichText::new("macOS").font(font(14.0, Weight::SemiBold)));
            if let Some(e) = &r.macos_error {
                ui.label(RichText::new(e).color(t.warning));
            } else if r.macos.is_empty() {
                kit::dim(ui, &t, "macOS is up to date.");
            }
            for m in &r.macos {
                let mut line = format!("{}  {}", m.title, m.version);
                if let Some(b) = m.size_bytes {
                    line.push_str(&format!("  ·  {}", human_size(b)));
                }
                if m.restart {
                    line.push_str("  ·  needs a restart");
                }
                ui.label(line);
            }
            if (!r.macos.is_empty() || r.macos_error.is_some())
                && kit::secondary(ui, &t, "Open Software Update")
            {
                app.act();
                if let Err(e) = updater::open_software_update(&app.env) {
                    app.warn(format!("Could not open System Settings: {e}"));
                }
            }
        });
}

fn update_row(app: &mut CleanApp, ui: &mut Ui, u: &UpdateInfo) {
    let t = app.t;
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&u.name).font(font(15.0, Weight::SemiBold)));
            ui.label(
                RichText::new(format!(
                    "{} -> {}",
                    u.installed,
                    u.latest.clone().unwrap_or_default()
                ))
                .color(t.accent),
            );
            kit::dim(ui, &t, u.source.label());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = ui.add(egui::Button::new("Update"));
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Update {}", u.name),
                    )
                });
                if r.clicked() {
                    app.act();
                    match updater::start_update(&app.env, u) {
                        Ok(msg) => app.tell(msg),
                        Err(e) => app.warn(format!("Could not open {}: {e}", u.name)),
                    }
                }
                if u.source == Source::Sparkle && u.download_url.is_some() {
                    let r = ui.add(egui::Button::new("Download"));
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Download {}", u.name),
                        )
                    });
                    if r.clicked() {
                        app.act();
                        match updater::open_download(&app.env, u) {
                            Ok(()) => app.tell(format!(
                                "The download of {} {} opened in your browser.",
                                u.name,
                                u.latest.clone().unwrap_or_default()
                            )),
                            Err(e) => app.warn(format!("Could not open the download: {e}")),
                        }
                    }
                }
                if !u.notes.is_empty() {
                    let open = app.apps.notes_open.contains(&u.app);
                    let r = ui.add(egui::Button::new(if open {
                        "Hide Notes"
                    } else {
                        "What's New"
                    }));
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("What's new in {}", u.name),
                        )
                    });
                    if r.clicked() {
                        app.act();
                        if open {
                            app.apps.notes_open.remove(&u.app);
                        } else {
                            app.apps.notes_open.insert(u.app.clone());
                        }
                    }
                }
            });
        });
        if app.apps.notes_open.contains(&u.app) {
            ui.label(RichText::new(&u.notes).color(t.text_dim));
        }
    });
    ui.add_space(6.0);
}
