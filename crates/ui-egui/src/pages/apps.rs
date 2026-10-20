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

use cleancraft_core::app_quit;
use cleancraft_core::apps::{self, AppInfo, Store};
use cleancraft_core::executor::{self, Method, Report};
use cleancraft_core::extensions::CAT_EXTENSIONS;
use cleancraft_core::features::{self, leftovers, uninstaller};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::updater::{self, Source, UpdateInfo, UpdatesReport};
use cleancraft_core::{FeatureId, Plan};
use egui::{Align, Color32, Key, Layout, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

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

    /// The name shown in the narrow group column (the full title is announced).
    pub fn short_title(self) -> &'static str {
        match self {
            Group::AppStore => "App Store",
            Group::Setapp => "Setapp",
            Group::Other => "Other Sources",
            g => g.title(),
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
    /// The open app is running (its files are locked until it quits).
    pub running: bool,
    /// What of it runs: the app, its helpers, agents and services (apps4's app_quit).
    pub parts: Option<app_quit::RunningState>,
    /// Quitting needs the administrator password (a system service): asking first.
    pub confirm_admin: bool,
    /// Quitting the open app from its banner.
    pub quit: Quit,
    /// The Force Quit confirmation is open.
    pub confirm_force: bool,
    /// Background check whether the open app still runs (rows unlock when it quits).
    still_running: Option<Job<app_quit::RunningState>>,
    checked_running: Option<std::time::Instant>,
    /// What the list reflects of CleanCraft's action log (moves made by any page).
    watch: super::history::JournalWatch,
}

/// Quitting the app whose detail is open.
#[derive(Default)]
pub enum Quit {
    #[default]
    Idle,
    /// Asked to quit (or forced); the job waits a few seconds for it to be gone.
    Asking(Job<Result<QuitOutcome, String>>),
    /// Parts of it still run after waiting (a save dialog, or they refused): what to say.
    StillOpen(String),
    Failed(String),
}

/// What a quit or force quit left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuitOutcome {
    /// Nothing of the app runs any more.
    pub done: bool,
    /// One or two sentences for the banner.
    pub message: String,
}

/// How often the open, running app is checked again.
const RUNNING_CHECK: std::time::Duration = std::time::Duration::from_millis(1500);

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
    app.apps.watch.prime(&app.env);
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
    app.apps.quit = Quit::Idle;
    app.apps.confirm_force = false;
    rebuild_detail(app, None);
}

/// Leave the app's detail for the list.
fn close_detail(app: &mut CleanApp) {
    app.apps.selected = None;
    app.apps.detail = None;
    app.apps.running = false;
    app.apps.parts = None;
    app.apps.quit = Quit::Idle;
    app.apps.confirm_force = false;
    app.apps.confirm_admin = false;
    app.apps.still_running = None;
}

/// Recompute the selected app's plan, keeping the user's checkboxes from `keep` (unless
/// `keep` was locked as a whole: then nothing in it was the user's choice).
fn rebuild_detail(app: &mut CleanApp, keep: Option<&Plan>) {
    let Some(path) = app.apps.selected.clone() else {
        return;
    };
    if !path.exists() {
        close_detail(app);
        return;
    }
    app.apps.running = uninstaller::is_running(&app.env, &path);
    app.apps.parts = app
        .apps
        .running
        .then(|| app_quit::running_parts(&app.env, &path));
    app.apps.checked_running = Some(std::time::Instant::now());
    match uninstaller::uninstall_plan(&app.env, &app.settings, &path, false) {
        Ok(mut p) => {
            if let Some(old) = keep.filter(|o| o.notes.is_empty()) {
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

/// Ask the open app to quit (or force it), then wait a little for it to be gone.
/// `allow_admin`: the person agreed to the password dialog for system services.
fn start_quit(app: &mut CleanApp, path: &Path, force: bool, allow_admin: bool) {
    let env = app.env.clone();
    let p = path.to_path_buf();
    app.apps.quit = Quit::Asking(Job::start(app.sync, move |_| {
        quit_now(&env, &p, force, allow_admin)
    }));
}

/// Quit the app at `path` with everything it runs (apps4's core flow), and say how it went.
fn quit_now(
    env: &cleancraft_core::Env,
    path: &Path,
    force: bool,
    allow_admin: bool,
) -> Result<QuitOutcome, String> {
    // Tests (FakeMac) do not wait: the fake processes stop at once or never.
    let fast = env.is_fake();
    let rep = if force {
        let mut o = app_quit::ForceOptions {
            allow_admin,
            ..Default::default()
        };
        if fast {
            o.grace = std::time::Duration::ZERO;
            o.wait = std::time::Duration::ZERO;
            o.poll = std::time::Duration::from_millis(1);
        }
        app_quit::force_quit(env, path, o)?
    } else {
        let mut o = app_quit::QuitOptions {
            allow_admin,
            ..Default::default()
        };
        if fast {
            o.wait = std::time::Duration::ZERO;
            o.poll = std::time::Duration::from_millis(1);
        }
        app_quit::quit_app(env, path, o)?
    };
    Ok(QuitOutcome {
        done: rep.done(),
        message: rep.message(),
    })
}

/// Forget what moved away (any page, or an agent): apps whose bundle is gone leave the list
/// and their groups, files gone leave the Leftovers, Installers and Extensions groups, so
/// every count is right at once, before the list is looked at again.
pub fn forget_moved(st: &mut State) {
    if let Some(list) = st.list.as_mut() {
        list.retain(|a| a.path.exists());
    }
    if let Some(p) = st.extras.as_mut() {
        p.items
            .retain(|i| std::fs::symlink_metadata(&i.path).is_ok());
        for (k, it) in p.items.iter_mut().enumerate() {
            it.id = k;
        }
    }
    // The Done screen keeps the app for Undo; Done itself returns to the list.
    if matches!(st.phase, Phase::Browse) && st.selected.as_ref().is_some_and(|p| !p.exists()) {
        st.selected = None;
        st.detail = None;
        st.running = false;
        st.quit = Quit::Idle;
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
            // Anything moved while it looked.
            forget_moved(st);
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
                // What moved leaves the lists and counts now; the slow look below confirms.
                forget_moved(&mut app.apps);
                if let Phase::Done(rep, _) = &app.apps.phase {
                    let m = CleanApp::moved_paths(rep);
                    app.files_moved(&m);
                }
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
    busy |= poll_quit(app);
    busy |= poll_running(app);
    // Moves and undos made elsewhere (another page, History, an agent).
    if app.apps.list.is_some()
        && app.apps.load.is_none()
        && !matches!(app.apps.phase, Phase::Working(..))
        && let Some(changes) = app.apps.watch.changes(&app.env)
    {
        let moved = super::history::moved_paths(&changes);
        if !moved.is_empty() {
            forget_moved(&mut app.apps);
            if moved.iter().any(|m| m.2) {
                // Something came back: only a new look finds it.
                start_load(app);
                busy = true;
            }
            if app.apps.selected.is_some() && matches!(app.apps.phase, Phase::Browse) {
                let old = app.apps.detail.clone();
                rebuild_detail(app, old.as_ref());
            }
        }
    }
    busy
}

fn poll_quit(app: &mut CleanApp) -> bool {
    let Quit::Asking(job) = &mut app.apps.quit else {
        return false;
    };
    let r = if let Some(r) = job.poll() {
        r
    } else if job.is_dead() {
        Err("Quitting stopped unexpectedly.".into())
    } else {
        return true;
    };
    match r {
        Ok(o) if o.done => {
            app.apps.quit = Quit::Idle;
            app.tell(o.message);
        }
        Ok(o) => app.apps.quit = Quit::StillOpen(o.message),
        Err(e) => app.apps.quit = Quit::Failed(e),
    }
    // Fresh defaults: nothing was selectable while it ran.
    rebuild_detail(app, None);
    false
}

/// While the open app runs, look again every moment: when the person quits it themselves
/// (or it quits after a save dialog), its files unlock without a click.
fn poll_running(app: &mut CleanApp) -> bool {
    let st = &mut app.apps;
    if !st.running || st.selected.is_none() || matches!(st.quit, Quit::Asking(_)) {
        st.still_running = None;
        return false;
    }
    if let Some(job) = st.still_running.as_mut() {
        match job.poll() {
            Some(state) if state.locked() => {
                st.still_running = None;
                st.parts = Some(state);
                st.checked_running = Some(std::time::Instant::now());
            }
            Some(_) => {
                st.still_running = None;
                st.quit = Quit::Idle;
                rebuild_detail(app, None);
                let name = app
                    .apps
                    .selected
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                app.tell(format!("{name} has quit. You can uninstall it now."));
            }
            None if job.is_dead() => st.still_running = None,
            None => {}
        }
        return true;
    }
    let due = st
        .checked_running
        .is_none_or(|t| t.elapsed() >= RUNNING_CHECK);
    if due && let Some(path) = st.selected.clone() {
        let env = app.env.clone();
        st.still_running = Some(Job::start(app.sync, move |_| {
            app_quit::running_parts(&env, &path)
        }));
    }
    // Keep frames coming so the next check happens.
    true
}

// ---------- page ----------

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    let sub = if f == FeatureId::Updater {
        "Newer versions of your apps and of macOS. CleanCraft opens the right place to install them."
    } else {
        "Remove apps with the files they leave behind, or reset an app to a fresh start."
    };
    // The updates start screen has its own large title.
    let hero = f == FeatureId::Updater && app.apps.updates.is_none() && app.apps.upd_job.is_none();
    if !hero {
        kit::header(ui, &t, icon, f.title(), sub);
    }
    if let Some(e) = app.apps.error.clone() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Warning, t.danger);
            ui.label(RichText::new(&e).color(t.danger));
            if kit::ghost(ui, &t, "Hide Message", None).clicked() {
                app.act();
                app.apps.error = None;
            }
        });
        ui.add_space(space::S);
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
            let secs = job.elapsed_secs();
            kit::progress(
                ui,
                &t,
                FeatureId::Uninstaller,
                "Moving to the Trash...",
                &s,
                secs,
                false,
            );
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
            let secs = job.elapsed_secs();
            kit::progress(
                ui,
                &t,
                FeatureId::Uninstaller,
                "Looking at your apps",
                &s,
                secs,
                false,
            );
        } else if kit::primary(ui, &t, "Scan Again") {
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
        let r = kit::search_field(ui, &t, &mut app.apps.search, "Search apps", 240.0);
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
        let choices: Vec<(SortBy, &str)> = [SortBy::Size, SortBy::Name, SortBy::LastOpened]
            .into_iter()
            .map(|s| (s, s.label()))
            .collect();
        kit::sort_menu(ui, "apps-sort", &mut sort, &choices);
        if sort != app.apps.sort {
            app.act();
            app.apps.sort = sort;
        }
        let loading = app.apps.load.is_some();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if loading {
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 16.0, &t, time);
                ui.ctx().request_repaint();
            } else if kit::secondary(ui, &t, "Scan Again") {
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
    let left_w = if body.width() < 760.0 { 236.0 } else { 272.0 };
    let left = egui::Rect::from_min_size(body.min, egui::vec2(left_w, avail_h));
    let right = egui::Rect::from_min_max(
        egui::pos2(left.right() + space::GUTTER, body.top()),
        body.max,
    );
    ui.allocate_rect(body, egui::Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.set_clip_rect(left.expand(24.0).intersect(ui.clip_rect()));
        kit::card(&t)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(left_w - 18.0, avail_h - 18.0));
                ui.set_max_width(left_w - 18.0);
                egui::ScrollArea::vertical()
                    .id_salt("app-groups")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        let total = group_count(&app.apps, Group::All).1.max(1);
                        for (k, g) in Group::LIST.into_iter().enumerate() {
                            if g == Group::Leftovers {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    kit::eyebrow(ui, &t, "Left behind");
                                });
                                ui.add_space(2.0);
                            }
                            let (n, size) = group_count(&app.apps, g);
                            let text = if size > 0 {
                                format!("{}  {n}  ·  {}", g.title(), human_size(size))
                            } else {
                                format!("{}  {n}", g.title())
                            };
                            let on = app.apps.group == g;
                            let count = if g.category().is_some() {
                                cleancraft_core::fsutil::items(n)
                            } else if n == 1 {
                                "1 app".to_string()
                            } else {
                                format!("{n} apps")
                            };
                            let size_text = if size > 0 {
                                human_size(size)
                            } else {
                                String::new()
                            };
                            // Empty groups stay listed (so the list does not jump) but step back.
                            let r = ui
                                .scope(|ui| {
                                    if n == 0 {
                                        ui.multiply_opacity(0.45);
                                    }
                                    super::plan_page::category_row(
                                        ui,
                                        &t,
                                        &text,
                                        g.short_title(),
                                        &size_text,
                                        &count,
                                        size as f32 / total as f32,
                                        kit::series_color(&t, k),
                                        on,
                                        None,
                                    )
                                })
                                .inner;
                            if r.row_clicked {
                                app.act();
                                app.apps.group = g;
                            }
                        }
                    });
            });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.set_clip_rect(right.expand(24.0).intersect(ui.clip_rect()));
        kit::card(&t)
            .inner_margin(egui::Margin::symmetric(6, 8))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), avail_h - 18.0));
                if file_group {
                    files_list(app, ui, avail_h - 18.0);
                } else {
                    apps_list(app, ui, avail_h - 18.0);
                }
            });
    });
}

/// Height of an app in the list.
const APP_ROW_H: f32 = 58.0;

fn apps_list(app: &mut CleanApp, ui: &mut Ui, h: f32) {
    let t = app.t;
    let rows = visible_apps(&app.apps, &app.settings);
    if rows.is_empty() {
        let searching = !app.apps.search.trim().is_empty();
        empty_state(
            ui,
            &t,
            Icon::Search,
            if searching {
                "No app matches your search."
            } else {
                "No apps here."
            },
            if searching {
                "Search looks at app names, developers and bundle ids."
            } else if app.apps.group == Group::Unused {
                "Every app was opened in the last six months."
            } else {
                "Apps in this group show here once they are installed."
            },
        );
        return;
    }
    let now = now();
    let mut open = None;
    let fade_out = egui::ScrollArea::vertical()
        .id_salt("apps-list")
        .max_height(h)
        .auto_shrink([false, false])
        .show_rows(ui, APP_ROW_H, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for a in &rows[range] {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, APP_ROW_H), Sense::click());
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &a.name)
                });
                if resp.clicked() {
                    open = Some(a.path.clone());
                }
                let feel = kit::feel_ring(ui, &t, &resp, r.shrink(2.0), 10.0);
                let hov = feel.hov;
                let p = ui.painter().clone();
                if hov > 0.0 || feel.down > 0.0 {
                    p.rect_filled(
                        r.shrink(2.0),
                        10.0,
                        Color32::from_white_alpha((10.0 * hov + 8.0 * feel.down) as u8),
                    );
                }
                let tile = Rect::from_center_size(
                    pos2(r.left() + 30.0, r.center().y),
                    egui::Vec2::splat(crate::theme::icon::APP_ROW + 4.0),
                );
                kit::app_icon(ui, tile, &a.name, Some(&a.path));
                let mut info = vec![];
                if !a.version.is_empty() {
                    info.push(a.version.clone());
                }
                info.push(a.vendor.clone());
                info.push(a.store.label().to_string());
                info.push(apps::opened_text(a, now));
                let info = info.join("  ·  ");
                let size_text = human_size(a.size);
                let size_font = font(size::BODY, Weight::SemiBold);
                let size_w = kit::galley(ui, &size_text, size_font.clone(), t.text)
                    .size()
                    .x;
                kit::text_at(
                    ui,
                    Rect::from_min_max(
                        pos2(r.right() - 30.0 - size_w, r.top()),
                        pos2(r.right() - 24.0, r.bottom()),
                    ),
                    &size_text,
                    &size_text,
                    size_font,
                    t.text,
                    Align::Max,
                );
                let chev =
                    Rect::from_center_size(pos2(r.right() - 12.0, r.center().y), vec2(12.0, 12.0));
                icons::paint(
                    &p,
                    chev,
                    Icon::Chevron,
                    icons::lerp(t.text_faint, t.text, hov),
                );
                let x0 = tile.right() + 12.0;
                let text_w = (r.right() - 40.0 - size_w - x0).max(60.0);
                let name_font = font(size::BODY, Weight::SemiBold);
                let name_s = kit::truncate_middle(ui.ctx(), &a.name, &name_font, text_w - 120.0);
                let name_w = kit::galley(ui, &name_s, name_font.clone(), t.text).size().x;
                // Painted only: the row button carries the name.
                p.text(
                    pos2(x0, r.top() + 20.0),
                    egui::Align2::LEFT_CENTER,
                    name_s,
                    name_font,
                    t.text,
                );
                if a.apple {
                    let tr = Rect::from_min_size(
                        pos2(x0 + name_w + 10.0, r.top() + 10.0),
                        vec2(120.0, 20.0),
                    );
                    kit::place(ui, tr, |ui| {
                        kit::tag(ui, "Part of macOS", t.warning);
                    });
                }
                let info_font = font(size::FOOTNOTE, Weight::Regular);
                let info_s = kit::truncate_middle(ui.ctx(), &info, &info_font, text_w);
                kit::text_at(
                    ui,
                    Rect::from_min_size(pos2(x0, r.top() + 31.0), vec2(text_w, 18.0)),
                    &info_s,
                    &info,
                    info_font,
                    t.text_faint,
                    Align::Min,
                );
            }
        });
    kit::scroll_fade(ui, &fade_out, t.glass_solid());
    if let Some(p) = open {
        app.act();
        open_app(app, &p);
    }
}

fn files_list(app: &mut CleanApp, ui: &mut Ui, h: f32) {
    let t = app.t;
    let ids = visible_files(&app.apps);
    // Notes of the scan (a list that could not be read) belong with the extensions.
    if app.apps.group == Group::Extensions
        && let Some(notes) = app.apps.extras.as_ref().map(|p| p.notes.clone())
    {
        for n in notes {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Warning, t.warning);
                kit::dim(ui, &t, &n);
            });
        }
    }
    if ids.is_empty() {
        let searching = !app.apps.search.trim().is_empty();
        empty_state(
            ui,
            &t,
            if searching { Icon::Search } else { Icon::Check },
            if searching {
                "No file matches your search."
            } else {
                "Nothing here."
            },
            if searching {
                "Try another word, or clear the search box."
            } else {
                match app.apps.group {
                    Group::Leftovers => "No files of removed apps were found.",
                    Group::Installers => "No app installers were found in your folders.",
                    _ => "No extensions were found.",
                }
            },
        );
    }
    let Some(plan) = app.apps.extras.clone() else {
        return;
    };
    let mut toggles = Vec::new();
    let fade_out = egui::ScrollArea::vertical()
        .id_salt("apps-files")
        .max_height(h - 20.0)
        .auto_shrink([false, false])
        .show_rows(ui, FILE_ROW_H, ids.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &id in &ids[range] {
                if let Some(on) = file_row(app, ui, &plan.items[id], None, false) {
                    toggles.push((id, on));
                }
            }
        });
    kit::scroll_fade(ui, &fade_out, t.glass_solid());
    for (id, on) in toggles {
        app.act();
        if let Some(p) = app.apps.extras.as_mut() {
            p.set_selected(id, on);
        }
    }
}

/// A centred empty state: a badge, a line, and what to do about it.
fn empty_state(ui: &mut Ui, t: &crate::theme::Tokens, icon: Icon, title: &str, hint: &str) {
    ui.add_space(space::XXL);
    ui.vertical_centered(|ui| {
        kit::badge(ui, t.sec_a, icon, 56.0);
        ui.add_space(space::M);
        ui.label(
            RichText::new(title)
                .font(font(size::TITLE3, Weight::SemiBold))
                .color(t.text),
        );
        ui.add_space(space::XS);
        kit::dim(ui, t, hint);
    });
    ui.add_space(space::S);
}

/// Height of a file in the App Remover lists.
const FILE_ROW_H: f32 = 50.0;

/// "~/Library/Caches/com.example.App" -> ("~/Library/Caches", "com.example.App").
fn split_shown(shown: &str) -> (&str, &str) {
    match shown.trim_end_matches('/').rsplit_once('/') {
        Some(("", name)) => ("/", name),
        Some((folder, name)) => (folder, name),
        None => ("", shown),
    }
}

/// The kind part of a reason ("Cache: named after com.example.App" -> "Cache"); the group
/// heading above already says the rest.
fn reason_kind(reason: &str) -> &str {
    reason
        .split_once(": ")
        .map(|(k, _)| k)
        .unwrap_or(reason)
        .trim()
}

/// One file with its checkbox: the name prominent, its folder under it (dimmed, shortened in
/// the middle), why it belongs to the app, and its size. A click anywhere on the row toggles
/// it. `shared_lock` is the lock every row of the plan carries (a running app): the banner
/// says it once, so the rows show only a padlock. Returns the new state when toggled.
fn file_row(
    app: &CleanApp,
    ui: &mut Ui,
    it: &cleancraft_core::Item,
    shared_lock: Option<&str>,
    short_reason: bool,
) -> Option<bool> {
    let t = app.t;
    let is_app = it.category == uninstaller::CAT_APP;
    let locked = it.locked.is_some();
    // The app itself is always part of an uninstall; Reset is the way to keep it.
    let toggle = !locked && !is_app;
    let w = ui.available_width();
    let sense = if toggle {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (r, row) = ui.allocate_exact_size(vec2(w, FILE_ROW_H), sense);
    let shown = app.env.display(&it.path);
    let card = r.shrink2(vec2(2.0, 2.0));
    // Pointing hand, eased hover and press, keyboard focus ring (kit).
    let feel = kit::feel_ring(ui, &t, &row, card, 10.0);
    let hov = if toggle { feel.hov } else { 0.0 };
    let p = ui.painter().clone();
    if it.selected && !is_app {
        p.rect_filled(
            card,
            10.0,
            t.sec_a.gamma_multiply(0.06 + 0.04 * hov + 0.04 * feel.down),
        );
    } else if hov > 0.0 {
        p.rect_filled(
            card,
            10.0,
            Color32::from_white_alpha((9.0 * hov + 6.0 * feel.down) as u8),
        );
    }
    // Checkbox, named after the whole path (VoiceOver, tests); a padlock when locked.
    let mut on = it.selected;
    let cb_rect = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(22.0, 22.0));
    let cb = kit::place(ui, cb_rect, |ui| {
        if locked {
            ui.multiply_opacity(0.0);
        }
        ui.add_enabled_ui(toggle, |ui| kit::check(ui, &t, &mut on, &shown))
            .inner
    });
    let cb = if is_app && !locked {
        cb.on_disabled_hover_text("The app is always part of an uninstall. Reset keeps it.")
    } else {
        cb
    };
    if locked {
        icons::paint(
            &p,
            Rect::from_center_size(cb_rect.center(), egui::Vec2::splat(16.0)),
            Icon::Lock,
            t.text_faint,
        );
    }
    let mut changed = cb.changed().then_some(on);
    if changed.is_none() && toggle && row.clicked() {
        changed = Some(!it.selected);
    }
    // Icon: the app's own for the bundle, the file's kind otherwise.
    let ir = Rect::from_center_size(pos2(r.left() + 54.0, r.center().y), egui::Vec2::splat(26.0));
    let (folder, name) = split_shown(&shown);
    // Your home folder reads as "~", like Finder's path bar.
    let home = app.env.display(app.env.home());
    let folder = match folder.strip_prefix(home.as_str()) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => folder.to_string(),
    };
    let folder = folder.as_str();
    if is_app {
        kit::app_icon(ui, ir, name.trim_end_matches(".app"), Some(&it.path));
    } else {
        crate::images::paint_file_icon(
            ui,
            ir.shrink(2.0),
            &it.path,
            crate::file_icons::FileKind::of(&it.path),
        );
    }
    let ink = if locked { t.text_dim } else { t.text };
    // Size, right aligned in a fixed column.
    let size_text = human_size(it.size);
    let size_font = font(size::BODY, Weight::SemiBold);
    let size_w = kit::galley(ui, &size_text, size_font.clone(), t.text)
        .size()
        .x
        .max(64.0);
    let size_r = Rect::from_min_max(
        pos2(r.right() - 12.0 - size_w, r.top()),
        pos2(r.right() - 10.0, r.bottom()),
    );
    kit::text_at(
        ui,
        size_r,
        &size_text,
        &size_text,
        size_font,
        ink,
        Align::Max,
    );
    // Why it is listed. A lock that is not the plan's shared one is the most useful thing
    // to say, so it takes the place of the reason.
    let own_lock = it
        .locked
        .as_deref()
        .filter(|l| shared_lock != Some(*l) && !l.is_empty());
    let why_full = match own_lock {
        Some(l) => format!("{} ({l})", it.reason),
        None => super::plan_page::reason_text(it),
    };
    let why_shown = match own_lock {
        Some(l) => l.to_string(),
        // The app's name and version are the page's heading already.
        None if is_app => String::new(),
        None if short_reason && !is_app => {
            let mut s = reason_kind(&it.reason).to_string();
            if it.needs_admin {
                s.push_str(", needs password");
            }
            s
        }
        None => why_full.clone(),
    };
    let x0 = r.left() + 80.0;
    let right_end = size_r.left() - 16.0;
    let avail = (right_end - x0).max(80.0);
    let wf = font(size::FOOTNOTE, Weight::Medium);
    let why_cap = if short_reason {
        (avail * 0.34).min(240.0)
    } else {
        (avail * 0.45).min(360.0)
    };
    let why_w = kit::galley(ui, &why_shown, wf.clone(), t.text_dim)
        .size()
        .x
        .min(why_cap);
    let why_s = kit::truncate_end(ui.ctx(), &why_shown, &wf, why_w + 1.0);
    kit::text_at(
        ui,
        Rect::from_min_max(
            pos2(right_end - why_w - 2.0, r.top()),
            pos2(right_end, r.bottom()),
        ),
        &why_s,
        &why_full,
        wf,
        if own_lock.is_some() {
            t.warning
        } else {
            t.text_dim
        },
        Align::Max,
    );
    // Name (prominent) and folder (dim, shortened in the middle). Painted only: the checkbox
    // already carries the whole path.
    let text_w = (avail - why_w - 20.0).max(60.0);
    let nf = font(size::BODY, Weight::Medium);
    let name_s = kit::truncate_middle(ui.ctx(), name, &nf, text_w);
    p.text(
        pos2(x0, r.top() + 17.0),
        egui::Align2::LEFT_CENTER,
        &name_s,
        nf,
        ink,
    );
    let ff = font(size::FOOTNOTE, Weight::Regular);
    let folder_s = kit::truncate_middle(ui.ctx(), folder, &ff, text_w);
    p.text(
        pos2(x0, r.top() + 35.0),
        egui::Align2::LEFT_CENTER,
        &folder_s,
        ff,
        t.text_faint,
    );
    // The whole path when either line had to be shortened.
    if name_s != name || folder_s != folder {
        row.on_hover_text(&shown);
    }
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
    kit::bottom_bar(ui, &t, egui::Id::new("apps-files-bottom"), |ui| {
        ui.label(
            RichText::new(human_size(size))
                .font(font(size::TITLE2, Weight::Bold))
                .color(t.text),
        );
        kit::quiet_label(
            ui,
            &t,
            &format!("{} selected", cleancraft_core::fsutil::items(n)),
            &format!("{n} selected, {}", human_size(size)),
        );
        if n == 0 {
            ui.add_space(space::S);
            kit::dim(ui, &t, "Tick files to remove them.");
        }
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
}

/// Why the open app cannot be uninstalled right now.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    /// The app is running: Quit it here.
    Running,
    /// Part of macOS.
    System(String),
    /// On the ignore list.
    Ignored(String),
}

fn block_of(app: &CleanApp, plan: &Plan, path: &Path) -> Option<Block> {
    if plan.notes.is_empty() {
        return None;
    }
    if app.settings.is_ignored(path) {
        return Some(Block::Ignored(plan.notes[0].clone()));
    }
    if app.apps.running {
        return Some(Block::Running);
    }
    Some(Block::System(plan.notes[0].clone()))
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
    let esc = matches!(app.apps.phase, Phase::Browse)
        && !app.apps.confirm_force
        && !app.apps.confirm_admin
        && ui.input(|i| i.key_pressed(Key::Escape));
    let back = kit::pill(ui, &t, "Back to Apps", kit::Kind::Ghost, Some(Icon::Back)).clicked();
    if esc || back {
        app.act();
        close_detail(app);
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
    ui.add_space(space::S);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(
            egui::Vec2::splat(crate::theme::icon::APP_LARGE),
            Sense::hover(),
        );
        kit::app_icon(ui, r, &name, Some(&path));
        ui.add_space(space::S);
        let total = human_size(plan.total_size());
        let total_font = font(size::NUMBER, Weight::Bold);
        let total_w = kit::galley(ui, &total, total_font.clone(), t.text).size().x + 8.0;
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - total_w - 16.0).max(120.0));
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                egui::Label::new(
                    RichText::new(&name)
                        .font(font(size::TITLE, Weight::Bold))
                        .color(t.text),
                )
                .truncate(),
            );
            if let Some(a) = &info {
                let mut parts = vec![if a.version.is_empty() {
                    "Version unknown".to_string()
                } else {
                    format!("Version {}", a.version)
                }];
                parts.push(a.vendor.clone());
                parts.push(a.store.label().to_string());
                parts.push(apps::opened_text(a, now()));
                let line = parts.join("  ·  ");
                ui.add(
                    egui::Label::new(
                        RichText::new(&line)
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .truncate(),
                )
                .on_hover_text(format!("{line}\n{}", a.bundle_id));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(total).font(total_font).color(icons::lerp(
                t.sec_a,
                Color32::WHITE,
                0.15,
            )));
        });
    });
    ui.add_space(space::S);
    let block = block_of(app, &plan, &path);
    if let Some(b) = &block {
        block_banner(app, ui, b, &name, &path);
        ui.add_space(space::S);
    }
    let shared_lock = plan.notes.first().cloned();
    let files_sel = plan
        .selected()
        .filter(|i| i.category != uninstaller::CAT_APP)
        .count();
    kit::bottom_bar(ui, &t, egui::Id::new("apps-detail-bottom"), |ui| {
        let hint = block.is_none() && files_sel == 0;
        ui.allocate_ui_with_layout(
            vec2(320.0, 44.0),
            egui::Layout::top_down(Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                // Centred on the bar's line, with or without the hint under it.
                let two = hint || block.is_some();
                ui.add_space(if two { 2.0 } else { 12.0 });
                ui.label(
                    RichText::new(format!(
                        "{} selected, {}",
                        plan.selected_count(),
                        human_size(plan.selected_size())
                    ))
                    .font(font(size::BODY, Weight::SemiBold))
                    .color(if block.is_some() { t.text_dim } else { t.text }),
                );
                match &block {
                    Some(Block::Running) => {
                        kit::dim(ui, &t, &format!("Quit {name} to choose what to remove."))
                    }
                    Some(Block::System(_)) => kit::dim(ui, &t, "macOS keeps this app."),
                    Some(Block::Ignored(_)) => kit::dim(ui, &t, "On your ignore list."),
                    None if hint => kit::dim(ui, &t, "Select files to reset the app."),
                    None => {}
                }
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // A blocked action is a quiet, disabled button (a pale red one reads as broken).
            let can_uninstall = block.is_none()
                && plan
                    .items
                    .iter()
                    .any(|i| i.category == uninstaller::CAT_APP && i.locked.is_none());
            let kind = if can_uninstall {
                kit::Kind::Danger
            } else {
                kit::Kind::Secondary
            };
            let un = ui
                .add_enabled_ui(can_uninstall, |ui| {
                    kit::pill(ui, &t, "Uninstall", kind, None)
                })
                .inner;
            let why_not = match &block {
                Some(Block::Running) => format!("Quit {name} first."),
                Some(Block::System(s) | Block::Ignored(s)) => s.clone(),
                None => String::new(),
            };
            let un = un.on_disabled_hover_text(&why_not);
            if un.clicked() && can_uninstall {
                app.act();
                app.apps.pending = app.apps.detail.clone();
                app.apps.phase = Phase::Confirm(Action::Uninstall);
            }
            let can_reset = block.is_none() && files_sel > 0;
            let rs = ui.add_enabled_ui(can_reset, |ui| {
                kit::pill(ui, &t, "Reset", kit::Kind::Secondary, None)
            });
            let rs = rs
                .inner
                .on_hover_text("Remove the app's settings and data, keep the app")
                .on_disabled_hover_text(if block.is_some() {
                    why_not.as_str()
                } else {
                    "Select the files to remove first. The app stays."
                });
            if rs.clicked() && can_reset {
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
            if kit::pill(ui, &t, "Check Again", kit::Kind::Secondary, None)
                .on_hover_text("Look at the app's files again")
                .clicked()
            {
                app.act();
                rebuild_detail(app, Some(&plan));
            }
            if !app.settings.is_ignored(&path) && !matches!(block, Some(Block::System(_))) {
                let ig = kit::secondary_resp(ui, &t, "Ignore");
                if ig
                    .on_hover_text("Never offer this app for removal (Settings > Ignore list)")
                    .clicked()
                {
                    app.act();
                    app.settings.ignore.push(path.clone());
                    app.save_settings();
                    close_detail(app);
                }
            }
        });
    });
    ui.add_space(space::XS);
    let h = ui.available_height() - 4.0;
    let mut toggles = Vec::new();
    kit::card(&t)
        .inner_margin(egui::Margin::symmetric(8, 8))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), h - 18.0));
            let fade_out = egui::ScrollArea::vertical()
                .id_salt("apps-detail")
                .max_height(h - 18.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (k, c) in plan.categories.iter().enumerate() {
                        let items: Vec<_> =
                            plan.items.iter().filter(|i| i.category == c.id).collect();
                        if items.is_empty() {
                            continue;
                        }
                        ui.add_space(if k == 0 { 4.0 } else { 12.0 });
                        category_head(app, ui, &plan, c, k, items.len());
                        for it in items {
                            if let Some(on) = file_row(app, ui, it, shared_lock.as_deref(), true) {
                                toggles.push((it.id, on));
                            }
                        }
                    }
                    ui.add_space(4.0);
                });
            kit::scroll_fade(ui, &fade_out, t.glass_solid());
        });
    for (id, on) in toggles {
        app.act();
        if let Some(p) = app.apps.detail.as_mut() {
            p.set_selected(id, on);
        }
    }
    if app.apps.confirm_force {
        confirm_force(app, ui, &name);
    }
    if app.apps.confirm_admin {
        confirm_admin(app, ui, &name);
    }
}

/// A group of the app's files: a dot in the group's colour, its title, size and count, and
/// what the group means (no tooltip: it would cover the first row).
fn category_head(
    app: &CleanApp,
    ui: &mut Ui,
    plan: &Plan,
    c: &cleancraft_core::plan::Category,
    k: usize,
    n: usize,
) {
    let t = app.t;
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 4.0, kit::series_color(&t, k));
        ui.add_space(4.0);
        ui.label(
            RichText::new(&c.title)
                .font(font(size::BODY, Weight::SemiBold))
                .color(t.text),
        );
        let count = if c.id == uninstaller::CAT_APP {
            String::new()
        } else {
            format!("  ·  {}", cleancraft_core::fsutil::items(n))
        };
        ui.label(
            RichText::new(format!("{}{count}", human_size(plan.category_size(&c.id))))
                .font(font(size::CALLOUT, Weight::Medium))
                .color(t.text_dim),
        );
    });
    if c.id != uninstaller::CAT_APP && !c.description.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(32.0);
            ui.add(
                egui::Label::new(
                    RichText::new(&c.description)
                        .font(font(size::FOOTNOTE, Weight::Regular))
                        .color(t.text_faint),
                )
                .truncate(),
            );
        });
    }
    ui.add_space(2.0);
}

/// The reason the app cannot be removed now, with what to do about it.
fn block_banner(app: &mut CleanApp, ui: &mut Ui, b: &Block, name: &str, path: &Path) {
    let t = app.t;
    let parts = app.apps.parts.clone();
    let (color, icon) = match b {
        Block::Running => (t.warning, Icon::Warning),
        Block::System(_) | Block::Ignored(_) => (t.text_dim, Icon::Lock),
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(egui::CornerRadius::same(crate::theme::radius::M))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
                icons::paint(ui.painter(), r, icon, color);
                ui.add_space(space::XS);
                // Actions first (from the right), so long names are cut instead of pushing
                // the buttons out.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = match b {
                        Block::Running => {
                            quit_actions(app, ui, name, path);
                            running_text(app, name)
                        }
                        Block::Ignored(why) => {
                            if kit::pill(
                                ui,
                                &t,
                                "Remove from Ignore List",
                                kit::Kind::Secondary,
                                None,
                            )
                            .clicked()
                            {
                                app.act();
                                app.settings.ignore.retain(|p| p != path);
                                app.save_settings();
                                rebuild_detail(app, None);
                            }
                            (
                                why.clone(),
                                "Remove it from the list to uninstall it.".into(),
                            )
                        }
                        Block::System(why) => (
                            why.clone(),
                            "macOS needs it. Updates of macOS keep it current.".into(),
                        ),
                    };
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&text.0)
                                        .font(font(size::BODY, Weight::SemiBold))
                                        .color(icons::lerp(color, Color32::WHITE, 0.55)),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&text.1)
                                        .font(font(size::CALLOUT, Weight::Regular))
                                        .color(t.text_dim),
                                )
                                .wrap(),
                            );
                            // What runs, when it is more than the app itself (helpers,
                            // agents, services): "name · role".
                            if let (Block::Running, Some(st)) = (b, &parts) {
                                // The headline names the app; list what else runs.
                                let lines: Vec<String> = st
                                    .running()
                                    .filter(|p| p.kind != app_quit::PartKind::App)
                                    .map(|p| format!("{}  ·  {}", p.name, p.role()))
                                    .collect();
                                if !lines.is_empty() {
                                    ui.add_space(2.0);
                                    for l in lines.iter().take(4) {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(l)
                                                    .font(font(size::FOOTNOTE, Weight::Medium))
                                                    .color(t.text_dim),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    if lines.len() > 4 {
                                        kit::dim(ui, &t, &format!("and {} more", lines.len() - 4));
                                    }
                                }
                            }
                        });
                    });
                });
            });
        });
}

/// The banner's two lines while the app runs, by how quitting goes.
fn running_text(app: &CleanApp, name: &str) -> (String, String) {
    match &app.apps.quit {
        Quit::Idle => (
            app.apps
                .parts
                .as_ref()
                .map(|s| s.headline())
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| format!("{name} is open")),
            "Quit it to uninstall or reset it. The files unlock as soon as it closes.".to_string(),
        ),
        Quit::Asking(job) => (
            format!("Waiting for {name} to quit..."),
            match job.elapsed_secs() {
                0..=2 => format!("{name} may ask you to save your work first."),
                s => format!(
                    "{s} seconds. If {name} shows a dialog, answer it, or force it to quit."
                ),
            },
        ),
        Quit::StillOpen(msg) => (format!("{name} did not quit"), msg.clone()),
        Quit::Failed(e) => (format!("{name} could not be quit"), e.clone()),
    }
}

/// Quit, Try Again and Force Quit, by how quitting goes (laid out right to left).
fn quit_actions(app: &mut CleanApp, ui: &mut Ui, name: &str, path: &Path) {
    let t = app.t;
    let asking = matches!(app.apps.quit, Quit::Asking(_));
    let late = matches!(&app.apps.quit, Quit::Asking(j) if j.elapsed_secs() >= 3);
    let offer_force = late || matches!(app.apps.quit, Quit::StillOpen(_) | Quit::Failed(_));
    if asking {
        let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
        let time = ui.input(|i| i.time);
        kit::progress_ring(ui.painter(), r.center(), 18.0, &t, time);
        ui.ctx().request_repaint();
    } else {
        let label = if matches!(app.apps.quit, Quit::Idle) {
            format!("Quit {name}")
        } else {
            "Try Again".to_string()
        };
        if kit::pill(ui, &t, &label, kit::Kind::Primary, None).clicked() {
            app.act();
            if app.apps.parts.as_ref().is_some_and(|s| s.needs_admin()) {
                // A system service of the app: say so before the password dialog.
                app.apps.confirm_admin = true;
            } else {
                start_quit(app, path, false, false);
            }
        }
    }
    if offer_force {
        ui.add_space(space::XS);
        let f = kit::pill(ui, &t, "Force Quit", kit::Kind::Secondary, None);
        if f.on_hover_text(format!("Stop {name} at once")).clicked() {
            app.act();
            app.apps.confirm_force = true;
        }
    }
}

/// A system service of the app runs as root: stopping it asks for the password. The person
/// chooses: quit everything (password), or quit what does not need it.
fn confirm_admin(app: &mut CleanApp, ui: &mut Ui, name: &str) {
    let t = app.t;
    let Some(path) = app.apps.selected.clone() else {
        app.apps.confirm_admin = false;
        return;
    };
    let question = app
        .apps
        .parts
        .as_ref()
        .and_then(|s| s.admin_question())
        .unwrap_or_default();
    let (mut all, mut rest, mut close) = (false, false, false);
    let resp = kit::modal(egui::Id::new("apps-quit-admin"), &t).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        crate::images::dialog_head_app(ui, &t, &path, &format!("Quit {name}?"), &question);
        ui.add_space(space::L);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::primary(ui, &t, "Quit and Enter Password") {
                all = true;
            }
            if kit::secondary(ui, &t, "Quit the Rest") {
                rest = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
    if resp.should_close() {
        close = true;
    }
    if all || rest || close {
        app.act();
        app.apps.confirm_admin = false;
    }
    if all {
        start_quit(app, &path, false, true);
    } else if rest {
        start_quit(app, &path, false, false);
    }
}

fn confirm_force(app: &mut CleanApp, ui: &mut Ui, name: &str) {
    let t = app.t;
    let Some(path) = app.apps.selected.clone() else {
        app.apps.confirm_force = false;
        return;
    };
    let mut go = false;
    let mut close = false;
    let resp = kit::modal(egui::Id::new("apps-force-quit"), &t).show(ui.ctx(), |ui| {
        ui.set_width(480.0);
        crate::images::dialog_head_app(
            ui,
            &t,
            &path,
            &format!("Force quit {name}?"),
            "It stops at once, without asking to save. Work you have not saved in it is lost.",
        );
        ui.add_space(space::L);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::danger(ui, &t, "Force Quit") {
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
        app.apps.confirm_force = false;
        let admin = app.apps.parts.as_ref().is_some_and(|s| s.needs_admin());
        start_quit(app, &path, true, admin);
    } else if close {
        app.act();
        app.apps.confirm_force = false;
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
    let selected = app.apps.selected.clone();
    let name = selected
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
    let width = (ui.ctx().content_rect().width() - 120.0).clamp(420.0, 660.0);
    let max_list = (ui.ctx().content_rect().height() - 330.0).clamp(120.0, 360.0);
    let resp = kit::modal(egui::Id::new("apps-confirm"), &t).show(ui.ctx(), |ui| {
        ui.set_width(width);
        let sub = "These exact files will be moved. You can put them back from History.";
        match (&selected, action) {
            (Some(p), Action::Uninstall | Action::Reset) => {
                crate::images::dialog_head_app(ui, &t, p, &headline, sub)
            }
            _ => kit::dialog_head(
                ui,
                &t,
                if action == Action::RemoveFiles {
                    Icon::Trash
                } else {
                    Icon::Remover
                },
                t.sec_a,
                &headline,
                sub,
            ),
        }
        ui.add_space(space::M);
        if let Some(note) =
            super::plan_page::admin_note(cleancraft_core::features::admin_count(&plan))
        {
            kit::strip(ui, Icon::Warning, t.warning, &note);
            ui.add_space(space::S);
        }
        let rows: Vec<&cleancraft_core::Item> = plan.selected().collect();
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("apps-confirm-list")
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
    let Phase::Done(rep, action) = &app.apps.phase else {
        return;
    };
    let (rep, action) = (rep.clone(), *action);
    let what = match action {
        Action::Uninstall => "Uninstalled. ",
        Action::Reset => "Reset done. ",
        Action::RemoveFiles => "",
    };
    let summary = format!(
        "{what}{} moved. {} skipped.",
        cleancraft_core::fsutil::items(rep.moved.len()),
        rep.skipped.len()
    );
    super::plan_page::done_view(
        app,
        ui,
        FeatureId::Uninstaller,
        &rep,
        &summary,
        |app, ui| {
            let t = app.t;
            if !rep.moved.is_empty()
                && kit::pill(ui, &t, "Undo", kit::Kind::Secondary, Some(Icon::Undo)).clicked()
            {
                app.act();
                let (n, errs) = executor::undo_plan(&app.env, &rep.plan_id);
                app.files_back(&CleanApp::moved_paths(&rep));
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
                app.apps.phase = Phase::Browse;
                app.navigate("history");
            }
            if kit::primary(ui, &t, "Done") {
                app.act();
                app.apps.phase = Phase::Browse;
                let old = app.apps.detail.clone();
                rebuild_detail(app, old.as_ref());
            }
        },
    );
}

// ---------- updates ----------

fn updates_page(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if let Some(job) = &app.apps.upd_job {
        let s = job.status();
        let secs = job.elapsed_secs();
        kit::progress(
            ui,
            &t,
            FeatureId::Updater,
            "Checking for updates",
            &format!("Checking {s}..."),
            secs,
            false,
        );
        return;
    }
    let Some(r) = app.apps.updates.clone() else {
        if kit::hero(
            ui,
            &t,
            FeatureId::Updater,
            kit::HeroText {
                title: FeatureId::Updater.title(),
                desc: "Newer versions of your apps and of macOS. CleanCraft opens the right place to install them.",
                covers: super::covers(FeatureId::Updater),
                action: "Check for Updates",
                note: "CleanCraft asks each app's update feed and the App Store, then hands the update to the app or the App Store.",
            },
            |_| {},
        ) {
            app.act();
            start_updates(app);
            poll(app);
        }
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
            .font(font(size::TITLE2, Weight::Bold))
            .color(t.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::secondary(ui, &t, "Check Again") {
                app.act();
                start_updates(app);
                poll(app);
            }
        });
    });
    ui.add_space(space::S);
    let h = ui.available_height() - 8.0;
    let fade_out = egui::ScrollArea::vertical()
        .id_salt("updates")
        .max_height(h)
        .show(ui, |ui| {
            for u in &avail {
                update_row(app, ui, u);
            }
            let failed: Vec<&UpdateInfo> = r.apps.iter().filter(|u| u.error.is_some()).collect();
            let current: Vec<&UpdateInfo> = r
                .apps
                .iter()
                .filter(|u| !u.available && u.error.is_none())
                .collect();
            ui.add_space(space::S);
            // Two cards side by side: your apps that need nothing, and macOS.
            ui.columns(2, |cols| {
                // Columns are justified; text in a card is not.
                cols[0].with_layout(egui::Layout::top_down(Align::Min), |ui| {
                kit::card(&t).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::card_head(ui, &t, Icon::Check, t.success, "Up to date");
                    ui.add_space(space::XS);
                    if current.is_empty() {
                        kit::dim(ui, &t, "No other app was checked.");
                    }
                    for u in &current {
                        kit::dim(
                            ui,
                            &t,
                            &format!("{}  {}  ({})", u.name, u.installed, u.source.label()),
                        );
                    }
                    if r.not_checked > 0 {
                        ui.add_space(space::XS);
                        kit::dim(
                            ui,
                            &t,
                            &format!(
                                "{} apps have no update feed CleanCraft can read; they update themselves.",
                                r.not_checked
                            ),
                        );
                    }
                    if !failed.is_empty() {
                        ui.add_space(space::S);
                        ui.label(
                            RichText::new("Could not check")
                                .font(font(size::BODY, Weight::SemiBold)),
                        );
                        for u in &failed {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    u.name,
                                    u.error.clone().unwrap_or_default()
                                ))
                                .font(font(size::CALLOUT, Weight::Regular))
                                .color(t.warning),
                            );
                        }
                    }
                });
                });
                // Columns are justified; text in a card is not.
                cols[1].with_layout(egui::Layout::top_down(Align::Min), |ui| {
                kit::card(&t).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::card_head(ui, &t, Icon::Gear, t.sec_a, "macOS");
                    ui.add_space(space::XS);
                    if let Some(e) = &r.macos_error {
                        ui.label(RichText::new(e).color(t.warning));
                    } else if r.macos.is_empty() {
                        kit::dim(ui, &t, "macOS is up to date.");
                    }
                    for m in &r.macos {
                        // Shown: the title (with the version once) and the size, then a
                        // restart tag. Announced: the whole line.
                        let mut line = format!("{}  {}", m.title, m.version);
                        let mut shown = if m.title.contains(&m.version) {
                            m.title.clone()
                        } else {
                            format!("{}  {}", m.title, m.version)
                        };
                        if let Some(b) = m.size_bytes {
                            line.push_str(&format!("  ·  {}", human_size(b)));
                            shown.push_str(&format!("  ·  {}", human_size(b)));
                        }
                        if m.restart {
                            line.push_str("  ·  needs a restart");
                        }
                        let fid = font(size::BODY, Weight::Regular);
                        let w = kit::galley(ui, &shown, fid.clone(), t.text).size().x + 2.0;
                        let (rr, _) = ui.allocate_exact_size(
                            vec2(w.min(ui.available_width()), 20.0),
                            Sense::hover(),
                        );
                        kit::text_at(ui, rr, &shown, &line, fid, t.text, Align::Min);
                        if m.restart {
                            kit::tag(ui, "Restart required", t.warning);
                        }
                    }
                    if !r.macos.is_empty() || r.macos_error.is_some() {
                        ui.add_space(space::XS);
                        if kit::secondary(ui, &t, "Open Software Update") {
                            app.act();
                            if let Err(e) = updater::open_software_update(&app.env) {
                                app.warn(format!("Could not open System Settings: {e}"));
                            }
                        }
                    }
                });
                });
            });
        });
    kit::scroll_fade_page(ui, &fade_out, &t);
}

fn update_row(app: &mut CleanApp, ui: &mut Ui, u: &UpdateInfo) {
    let t = app.t;
    kit::card(&t)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(
                    egui::Vec2::splat(crate::theme::icon::APP_ROW + 8.0),
                    Sense::hover(),
                );
                kit::app_icon(ui, r, &u.name, Some(&u.app));
                ui.add_space(space::XS);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(&u.name)
                            .font(font(size::TITLE3, Weight::SemiBold))
                            .color(t.text),
                    );
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} -> {}",
                                u.installed,
                                u.latest.clone().unwrap_or_default()
                            ))
                            .font(font(size::FOOTNOTE, Weight::Medium))
                            .color(icons::lerp(
                                t.sec_a,
                                Color32::WHITE,
                                0.2,
                            )),
                        );
                        kit::dim(ui, &t, u.source.label());
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let r = kit::pill(ui, &t, "Update", kit::Kind::Primary, None).on_hover_text(
                        match u.source {
                            Source::Sparkle => "Open the app so its own updater installs the new version",
                            _ => "Open the App Store page to install the new version",
                        },
                    );
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
                        let r = kit::secondary_resp(ui, &t, "Download Only").on_hover_text(
                            "Open the new version's download in your browser; you install it yourself",
                        );
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
                        let r = kit::secondary_resp(
                            ui,
                            &t,
                            if open { "Hide Notes" } else { "What's New" },
                        );
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
    ui.add_space(space::S);
}
