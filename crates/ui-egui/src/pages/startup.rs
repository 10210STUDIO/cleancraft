//! Startup Items: login items, launch agents and daemons (with program, signer and state),
//! and the apps using a lot of the Mac right now.
//!
//! Turning items off and on is immediate and undoable from History. Removing an agent's file
//! and quitting an app ask first and show exactly what will happen.

use cleancraft_core::executor::{self, Method, Report};
use cleancraft_core::features;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::startup::{self, ChangeError, Domain, LaunchItem, LoginItem, Proc};
use cleancraft_core::tasks;
use cleancraft_core::{Env, FeatureId, Plan};
use egui::{RichText, Ui};

use crate::app::CleanApp;
use crate::jobs::Job;
use crate::kit;
use crate::kit_list::{self, Filter, SortKey};
use crate::theme::{Weight, font, size, space};
use cleancraft_core::settings::ListPrefs;

/// Everything the page shows, read in one go.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub uid: Result<u32, String>,
    pub login: Result<Vec<LoginItem>, String>,
    /// Login items CleanCraft removed: (journal action id, item).
    pub removed_login: Vec<(String, LoginItem)>,
    pub launch: Vec<LaunchItem>,
    pub notes: Vec<String>,
    pub heavy: Result<Vec<Proc>, String>,
}

pub fn read(env: &Env) -> Snapshot {
    let uid = tasks::current_uid(env);
    let login = startup::list_login_items(env);
    let removed_login = startup::removed_login_items(env, login.as_deref().unwrap_or(&[]));
    let (launch, notes) = startup::list_launch_items(env, uid.as_ref().ok().copied());
    let heavy = match &uid {
        Ok(u) => startup::list_processes(env, *u).map(|v| startup::heavy_apps(&v)),
        Err(e) => Err(e.clone()),
    };
    Snapshot {
        uid,
        login,
        removed_login,
        launch,
        notes,
        heavy,
    }
}

/// What a click asks for.
#[derive(Clone, Debug)]
pub enum Action {
    Launch(LaunchItem, bool),
    RemoveLogin(LoginItem),
    RestoreLogin(String, String),
    Quit(Proc),
    Remove(Plan),
}

#[derive(Default)]
pub struct State {
    pub data: Option<Snapshot>,
    loading: Option<Job<Snapshot>>,
    busy: Option<Job<Result<String, String>>>,
    /// Quit waiting for confirmation.
    pub confirm_quit: Option<Proc>,
    /// Removal of an agent's file waiting for confirmation (a plan with that one item).
    pub confirm_remove: Option<Plan>,
    /// The action log as the snapshot reflects it (an uninstall moves agents too).
    watch: super::history::JournalWatch,
    /// The search over every startup item.
    pub search: String,
}

/// The list's name in the settings (filters and sort).
pub const LIST: &str = "startup";
/// Sort orders of every section's items.
pub const SORTS: [SortKey; 2] = [
    kit_list::BY_NAME,
    SortKey {
        key: "signer",
        label: "Sort by signer",
        first: "A to Z",
        second: "Z to A",
        descending: false,
    },
];

/// What the search and filters let through, for the sections.
struct View {
    q: String,
    prefs: ListPrefs,
    filters: Vec<Filter>,
}

impl View {
    fn active(&self) -> bool {
        !self.q.is_empty() || kit_list::filtering(&self.prefs, &self.filters)
    }

    fn launch(&self, env: &Env, it: &LaunchItem) -> bool {
        kit_list::matches(
            &self.q,
            &[
                &it.label,
                it.program.as_deref().unwrap_or(""),
                &owner_name(env, it).unwrap_or_default(),
                &it.signer.label(),
            ],
        ) && kit_list::passes(&self.prefs, &self.filters, |k| launch_has(it, k))
    }

    fn login(&self, it: &LoginItem, on: bool) -> bool {
        kit_list::matches(&self.q, &[&it.name, it.path.as_deref().unwrap_or("")])
            && kit_list::passes(&self.prefs, &self.filters, |k| login_has(on, k))
    }

    /// Does the Kind filter leave this kind of item in? (Sections of a kind the filters
    /// take out are not shown.)
    fn kind(&self, key: &str) -> bool {
        let kinds = ["login", "agents", "daemons"];
        let on: Vec<&str> = kinds
            .into_iter()
            .filter(|k| kit_list::on(&self.prefs, k))
            .collect();
        on.is_empty() || on.contains(&key)
    }
}

fn launch_has(it: &LaunchItem, key: &str) -> bool {
    match key {
        "agents" => matches!(it.domain, Domain::UserAgent | Domain::GlobalAgent),
        "daemons" => it.domain == Domain::Daemon,
        "on" => it.enabled,
        "off" => !it.enabled,
        "unsigned" => it.signer.is_weak(),
        _ => false,
    }
}

fn login_has(on: bool, key: &str) -> bool {
    match key {
        "login" => true,
        "on" => on,
        "off" => !on,
        _ => false,
    }
}

/// The app an agent belongs to, when its program is inside one ("Dropbox").
fn owner_name(env: &Env, it: &LaunchItem) -> Option<String> {
    let a = it
        .program
        .as_deref()
        .and_then(|p| cleancraft_core::sys::app_bundle_of(std::path::Path::new(p)))
        .map(|a| cleancraft_core::apps::resolve(env, &a.to_string_lossy()))?;
    std::path::Path::new(&a)
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
}

fn filters_for(data: &Snapshot) -> Vec<Filter> {
    let login_on = data.login.as_ref().map(|l| l.len()).unwrap_or(0);
    let login_off = data.removed_login.len();
    let n = |k: &str| data.launch.iter().filter(|i| launch_has(i, k)).count();
    vec![
        Filter::any("Kind", "login", "Login items", login_on + login_off),
        Filter::any("Kind", "agents", "Agents", n("agents")),
        Filter::any("Kind", "daemons", "Daemons", n("daemons")),
        Filter::any("State", "on", "On", n("on") + login_on),
        Filter::any("State", "off", "Off", n("off") + login_off),
        Filter::only(
            "Show only",
            "unsigned",
            "Not signed by a known developer",
            n("unsigned"),
        ),
    ]
}

/// Sort a section's agents by the chosen order.
fn sort_launch(env: &Env, prefs: &ListPrefs, items: &mut [&LaunchItem]) {
    let (key, rev) = kit_list::sort_of(prefs, &SORTS);
    let name = |it: &LaunchItem| {
        owner_name(env, it)
            .unwrap_or_else(|| it.label.clone())
            .to_lowercase()
    };
    items.sort_by(|a, b| {
        let o = match key.key {
            "signer" => a
                .signer
                .label()
                .to_lowercase()
                .cmp(&b.signer.label().to_lowercase())
                .then_with(|| name(a).cmp(&name(b))),
            _ => name(a).cmp(&name(b)),
        };
        if rev { o.reverse() } else { o }
    });
}

fn reload(app: &mut CleanApp) {
    app.startup.watch.prime(&app.env);
    let env = app.env.clone();
    app.startup.loading = Some(Job::start(app.sync, move |_| read(&env)));
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    if let Some(job) = app.startup.loading.as_mut() {
        if let Some(s) = job.poll() {
            app.startup.data = Some(s);
            app.startup.loading = None;
        } else if job.is_dead() {
            app.startup.loading = None;
        } else {
            busy = true;
        }
    }
    if let Some(job) = app.startup.busy.as_mut() {
        if let Some(r) = job.poll() {
            app.startup.busy = None;
            match r {
                Ok(m) => {
                    if m.starts_with("Moved ") {
                        crate::sound::play(&app.env, crate::sound::Cue::Trash);
                    }
                    app.tell(m)
                }
                Err(e) => app.warn(e),
            }
            reload(app);
            busy = true;
        } else if job.is_dead() {
            app.startup.busy = None;
            app.warn("The change stopped unexpectedly.");
        } else {
            busy = true;
        }
    }
    busy
}

fn change_text(e: ChangeError) -> String {
    plain_error(&e.to_string())
}

/// Tool errors in plain words: macOS's "not authorized to send Apple events" (-1743) means
/// CleanCraft lacks the Automation permission, and says where to turn it on.
pub fn plain_error(raw: &str) -> String {
    if raw.contains("-1743") || raw.contains("Not authorized to send Apple events") {
        return "CleanCraft needs permission to control System Events. Turn it on in System Settings > Privacy & Security > Automation, then try again.".into();
    }
    raw.to_string()
}

fn report_text(rep: &Report) -> Result<String, String> {
    if rep.skipped.is_empty() {
        Ok(format!(
            "Moved {} startup file{}. You can put it back from History.",
            rep.moved.len(),
            if rep.moved.len() == 1 { "" } else { "s" }
        ))
    } else {
        Err(rep
            .skipped
            .iter()
            .map(|(p, why)| {
                format!(
                    "Not moved: {} ({why})",
                    p.file_name().unwrap_or_default().to_string_lossy()
                )
            })
            .collect::<Vec<_>>()
            .join("; "))
    }
}

fn start(app: &mut CleanApp, a: Action) {
    let env = app.env.clone();
    let settings = app.settings.clone();
    let uid = app
        .startup
        .data
        .as_ref()
        .and_then(|d| d.uid.as_ref().ok().copied());
    app.startup.busy = Some(Job::start(app.sync, move |_| match a {
        Action::Launch(item, on) => {
            let uid = uid.ok_or("Could not read your user id.")?;
            startup::set_launch_item_enabled(&env, &item, uid, on)
                .map(|_| {
                    format!(
                        "{} {}. Undo it from History if you change your mind.",
                        if on { "Turned on" } else { "Turned off" },
                        item.label
                    )
                })
                .map_err(change_text)
        }
        Action::RemoveLogin(item) => startup::remove_login_item(&env, &item)
            .map(|_| format!("{} no longer opens at login.", item.name))
            .map_err(change_text),
        Action::RestoreLogin(id, name) => executor::undo(&env, &id)
            .map(|_| format!("{name} opens at login again."))
            .map_err(|e| format!("Could not add {name} back: {e}")),
        Action::Quit(p) => {
            let uid = uid.ok_or("Could not read your user id.")?;
            startup::quit_process(&env, &p, uid)
                .map(|_| format!("Asked {} to quit.", p.name))
                .map_err(change_text)
        }
        Action::Remove(plan) => {
            features::run_plan(&env, &settings, &plan, &plan.digest(), &mut |_, _| {})
                .map_err(|e| format!("{e:#}"))
                .and_then(|rep| report_text(&rep))
        }
    }));
}

/// [`row_button`], returning its response.
fn row_button_resp(ui: &mut Ui, text: &str, name: String, tip: &str) -> egui::Response {
    let t = crate::theme::Tokens::dark();
    let icon = (text == "Remove").then_some(crate::icons::Icon::Trash);
    let r = kit::ghost(ui, &t, text, icon);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
    r.on_hover_text(tip)
}

/// A small button whose accessible name says which item it acts on.
fn row_button(ui: &mut Ui, text: &str, name: String, tip: &str) -> bool {
    let t = crate::theme::Tokens::dark();
    let icon = (text == "Remove").then_some(crate::icons::Icon::Trash);
    let r = kit::ghost(ui, &t, text, icon);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
    r.on_hover_text(tip).clicked()
}

/// On or Off as one switch; its accessible name is the action a click takes ("Turn Off
/// Dropbox"). Returns true when clicked.
fn state_switch(ui: &mut Ui, t: &crate::theme::Tokens, on: bool, name: &str) -> bool {
    let action = if on { "Turn Off" } else { "Turn On" };
    kit::switch(ui, t, on, &format!("{action} {name}"))
        .on_hover_text(if on {
            "On: starts with your Mac. Click to turn off."
        } else {
            "Off. Click to turn on."
        })
        .clicked()
}

/// Lay out a row: `right` first (from the right edge), then `left` in what is left, so long
/// texts are cut short instead of running under the buttons.
fn split_row(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        right(ui);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            left(ui);
        });
    });
}

fn short(ui: &mut Ui, text: &str, size: f32, w: Weight, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(font(size, w)).color(color)).truncate());
}

/// A path line shortened in the middle, so the executable's name at the end stays readable
/// ("Runs /Applications/Example.app/...ExampleHelper"); the whole path is in the tooltip and
/// read by VoiceOver.
fn path_line(ui: &mut Ui, text: &str, color: egui::Color32) {
    let fid = font(size::FOOTNOTE, Weight::Regular);
    let shown = kit::truncate_middle(ui.ctx(), text, &fid, ui.available_width().max(40.0));
    let r = ui.add(
        egui::Label::new(RichText::new(shown).font(fid).color(color))
            .truncate()
            .selectable(false),
    );
    let full = text.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, full.clone()));
    r.on_hover_text(text);
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    kit::header(
        ui,
        &t,
        icon,
        f.title(),
        "What opens when you log in, what runs in the background, and the apps working hardest right now.",
    );
    if app.startup.data.is_none() && app.startup.loading.is_none() {
        reload(app);
    } else if app.startup.loading.is_none()
        && app.startup.busy.is_none()
        && app.startup.watch.changes(&app.env).is_some()
    {
        // Something changed elsewhere (an app removed with its agents, an undo in History).
        reload(app);
    }
    let Some(data) = app.startup.data.clone() else {
        kit::progress(ui, &t, f, "Reading startup items...", "", 0, false);
        return;
    };
    let busy = app.startup.busy.is_some() || app.startup.loading.is_some();
    // A change in progress keeps the list on screen (no full-page flash for a switch).
    if app.startup.busy.is_some() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            let time = ui.input(|i| i.time);
            kit::progress_ring(ui.painter(), r.center(), 16.0, &t, time);
            ui.label(
                RichText::new("Working... macOS may ask for your password.")
                    .font(font(size::CALLOUT, Weight::Medium))
                    .color(t.text),
            );
        });
        ui.ctx().request_repaint();
        ui.add_space(space::XS);
    }
    ui.horizontal(|ui| {
        let n_on = data.launch.iter().filter(|i| i.enabled).count()
            + data.login.as_ref().map(|l| l.len()).unwrap_or(0);
        ui.label(
            RichText::new(format!("{n_on} items start automatically"))
                .font(font(size::TITLE2, Weight::Bold))
                .color(t.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui
                .add_enabled_ui(!busy, |ui| {
                    kit::pill(ui, &t, "Scan Again", kit::Kind::Secondary, None)
                })
                .inner;
            if r.on_hover_text("Read the startup items again").clicked() && !busy {
                app.act();
                reload(app);
            }
        });
    });
    for n in &data.notes {
        kit::dim(ui, &t, n);
    }
    // Search, Filter, Sort over every startup item.
    ui.add_space(space::S);
    let filters = filters_for(&data);
    let mut prefs = kit_list::prefs(&app.settings, LIST);
    let mut changed = false;
    let mut search = std::mem::take(&mut app.startup.search);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S;
        let (_, ch) = kit_list::search(
            ui,
            &t,
            &mut search,
            "Search startup items",
            "Search startup items",
            240.0,
        );
        if ch {
            app.act();
        }
        changed |= kit_list::menus(ui, &t, LIST, &filters, &SORTS, &mut prefs);
    });
    let view = View {
        q: kit_list::query(&search),
        prefs: prefs.clone(),
        filters: filters.clone(),
    };
    let total = data.launch.len()
        + data.login.as_ref().map(|l| l.len()).unwrap_or(0)
        + data.removed_login.len();
    let shown = data
        .launch
        .iter()
        .filter(|i| view.launch(&app.env, i))
        .count()
        + data
            .login
            .as_ref()
            .map(|l| l.iter().filter(|i| view.login(i, true)).count())
            .unwrap_or(0)
        + data
            .removed_login
            .iter()
            .filter(|(_, i)| view.login(i, false))
            .count();
    let st = kit_list::status(
        ui,
        &t,
        shown,
        total,
        ("item", "items"),
        &filters,
        &mut prefs,
        &mut search,
    );
    changed |= st.prefs;
    app.startup.search = search;
    if changed || st.search {
        app.act();
    }
    if changed && kit_list::store(&mut app.settings, LIST, prefs.clone()) {
        app.save_settings();
    }
    let view = View {
        q: kit_list::query(&app.startup.search),
        prefs,
        filters,
    };
    let fade_out = egui::ScrollArea::vertical()
        .id_salt("startup")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled_ui(app.startup.busy.is_none(), |ui| {
                if view.kind("login") {
                    login_section(app, ui, &data, &view);
                }
                let kinds = [
                    (Domain::UserAgent, "agents"),
                    (Domain::GlobalAgent, "agents"),
                    (Domain::Daemon, "daemons"),
                ];
                for (d, kind) in kinds {
                    if view.kind(kind) {
                        launch_section(app, ui, &data, d, &view);
                    }
                }
                // The busiest apps are not startup items: shown when nothing narrows the
                // list.
                if !view.active() {
                    heavy_section(app, ui, &data);
                }
                ui.add_space(space::M);
            });
        });
    kit::scroll_fade_page(ui, &fade_out, &t);
    if app.startup.confirm_quit.is_some() {
        confirm_quit(app, ui);
    }
    if app.startup.confirm_remove.is_some() {
        confirm_remove(app, ui);
    }
}

fn section(ui: &mut Ui, t: &crate::theme::Tokens, title: &str, desc: &str) {
    ui.add_space(14.0);
    ui.label(
        RichText::new(title)
            .font(font(size::TITLE3, Weight::Bold))
            .color(t.text),
    );
    kit::dim(ui, t, desc);
    ui.add_space(6.0);
}

fn login_section(app: &mut CleanApp, ui: &mut Ui, data: &Snapshot, view: &View) {
    let t = app.t;
    let mut on_items: Vec<&LoginItem> = data
        .login
        .as_ref()
        .map(|l| l.iter().filter(|i| view.login(i, true)).collect())
        .unwrap_or_default();
    let mut off_items: Vec<&(String, LoginItem)> = data
        .removed_login
        .iter()
        .filter(|(_, i)| view.login(i, false))
        .collect();
    let (_, rev) = kit_list::sort_of(&view.prefs, &SORTS);
    let by_name = |a: &str, b: &str| {
        let o = a.to_lowercase().cmp(&b.to_lowercase());
        if rev { o.reverse() } else { o }
    };
    on_items.sort_by(|a, b| by_name(&a.name, &b.name));
    off_items.sort_by(|a, b| by_name(&a.1.name, &b.1.name));
    if view.active() && on_items.is_empty() && off_items.is_empty() && data.login.is_ok() {
        return;
    }
    section(
        ui,
        &t,
        "Login items",
        "Apps that open when you log in. Turn one off and it no longer opens at login; turn it on again any time.",
    );
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match &data.login {
            Err(e) => {
                kit::strip(ui, crate::icons::Icon::Warning, t.warning, e);
            }
            Ok(list) if list.is_empty() && data.removed_login.is_empty() => {
                kit::dim(ui, &t, "No app opens when you log in.");
            }
            Ok(_) => {
                for it in on_items {
                    let icon = it
                        .path
                        .as_deref()
                        .map(|p| cleancraft_core::apps::resolve(&app.env, p));
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        split_row(
                            ui,
                            |ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(crate::theme::icon::APP_ROW),
                                    egui::Sense::hover(),
                                );
                                match &icon {
                                    Some(p) => {
                                        crate::images::paint_app_icon(ui, r, p);
                                    }
                                    None => kit::initial_tile(
                                        ui.painter(),
                                        r,
                                        &it.name,
                                        kit::name_color(&it.name),
                                    ),
                                }
                                short(ui, &it.name, size::BODY, Weight::SemiBold, t.text);
                                short(
                                    ui,
                                    it.path.as_deref().unwrap_or("location unknown"),
                                    size::FOOTNOTE,
                                    Weight::Regular,
                                    t.text_faint,
                                );
                            },
                            |ui| {
                                if state_switch(ui, &t, true, &it.name) {
                                    app.act();
                                    start(app, Action::RemoveLogin(it.clone()));
                                }
                            },
                        );
                    });
                }
                for (id, it) in off_items {
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        split_row(
                            ui,
                            |ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(crate::theme::icon::APP_ROW),
                                    egui::Sense::hover(),
                                );
                                kit::initial_tile(
                                    ui.painter(),
                                    r,
                                    &it.name,
                                    egui::Color32::from_gray(90),
                                );
                                short(ui, &it.name, size::BODY, Weight::SemiBold, t.text_dim);
                                short(
                                    ui,
                                    it.path.as_deref().unwrap_or(""),
                                    size::FOOTNOTE,
                                    Weight::Regular,
                                    t.text_faint,
                                );
                            },
                            |ui| {
                                if state_switch(ui, &t, false, &it.name) {
                                    app.act();
                                    start(app, Action::RestoreLogin(id.clone(), it.name.clone()));
                                }
                            },
                        );
                    });
                }
            }
        }
    });
}

fn launch_section(app: &mut CleanApp, ui: &mut Ui, data: &Snapshot, d: Domain, view: &View) {
    let t = app.t;
    let mut items: Vec<&LaunchItem> = data
        .launch
        .iter()
        .filter(|i| i.domain == d && view.launch(&app.env, i))
        .collect();
    // A section the search or filters empty is left out.
    if items.is_empty() && view.active() {
        return;
    }
    sort_launch(&app.env, &view.prefs, &mut items);
    let desc = match d {
        Domain::UserAgent => {
            "Background helpers that your apps installed for you. They start when you log in."
        }
        other => other.description(),
    };
    section(ui, &t, d.title(), desc);
    let removable = app.env.can_modify(&d.path(&app.env));
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if items.is_empty() {
            kit::dim(ui, &t, "None on this Mac.");
        }
        let n = items.len();
        for (k, it) in items.into_iter().enumerate() {
            // The app that owns the agent, when its program is inside one.
            let owner = it
                .program
                .as_deref()
                .and_then(|p| cleancraft_core::sys::app_bundle_of(std::path::Path::new(p)))
                .map(|a| cleancraft_core::apps::resolve(&app.env, &a.to_string_lossy()));
            // Remove shows while the row is hovered or the keyboard is on it, like the row
            // actions of every other list (REVIEW-C C27); it is always there for VoiceOver.
            let hov_id = egui::Id::new(("startup-row-hover", &it.label));
            let row_hot: bool = ui.data(|d| d.get_temp(hov_id)).unwrap_or(false);
            let row = ui.horizontal(|ui| {
                split_row(
                    ui,
                    |ui| {
                        let tile = crate::theme::icon::APP_ROW;
                        match &owner {
                            Some(a) => {
                                crate::images::app_icon(ui, a, tile);
                            }
                            None => {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(tile),
                                    egui::Sense::hover(),
                                );
                                kit::icon_tile(
                                    ui.painter(),
                                    r,
                                    crate::icons::Icon::Gear,
                                    egui::Color32::from_gray(96),
                                );
                            }
                        }
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            // The app it belongs to, when known, reads better than an id.
                            let app_name = owner.as_ref().and_then(|a| {
                                std::path::Path::new(a)
                                    .file_stem()
                                    .map(|n| n.to_string_lossy().into_owned())
                            });
                            match &app_name {
                                Some(n) => {
                                    ui.horizontal(|ui| {
                                        short(ui, n, size::BODY, Weight::SemiBold, t.text);
                                        short(
                                            ui,
                                            &it.label,
                                            size::FOOTNOTE,
                                            Weight::Regular,
                                            t.text_dim,
                                        );
                                    });
                                }
                                None => short(ui, &it.label, size::BODY, Weight::SemiBold, t.text),
                            }
                            let prog = it.program.as_deref().unwrap_or("no program named");
                            path_line(ui, &format!("Runs {prog}"), t.text_faint);
                            let (sig, col) = if !it.program_exists && it.program.is_some() {
                                (
                                    "Program missing: this item does nothing".to_string(),
                                    t.warning,
                                )
                            } else if it.signer.is_weak() {
                                (it.signer.label(), t.danger)
                            } else {
                                (it.signer.label(), t.text_dim)
                            };
                            short(ui, &sig, size::FOOTNOTE, Weight::Medium, col);
                            if let Some(p) = &it.problem {
                                short(ui, p, size::FOOTNOTE, Weight::Medium, t.warning);
                            }
                        });
                    },
                    |ui| {
                        if it.apple {
                            kit::tag(ui, "Part of macOS", t.text_dim);
                            return;
                        }
                        if state_switch(ui, &t, it.enabled, &it.label) {
                            app.act();
                            start(app, Action::Launch(it.clone(), !it.enabled));
                        }
                        ui.add_space(space::XS);
                        // Removing moves the file: a quieter, separate action with the
                        // Trash glyph, never next to the switch's own spot.
                        if removable {
                            let r = ui
                                .scope(|ui| {
                                    if !row_hot {
                                        ui.multiply_opacity(0.0);
                                    }
                                    row_button_resp(
                                        ui,
                                        "Remove",
                                        format!("Remove {}", it.label),
                                        "Move its file to the Trash (asks first). Turn Off keeps it.",
                                    )
                                })
                                .inner;
                            let focused = r.has_focus();
                            ui.data_mut(|d| {
                                d.insert_temp(
                                    egui::Id::new(("startup-remove-focus", &it.label)),
                                    focused,
                                )
                            });
                            if r.clicked() {
                                app.act();
                                open_remove(app, it);
                            }
                        }
                        if d.admin() {
                            kit::dim(ui, &t, "needs password");
                        }
                    },
                );
            });
            let focus_on_remove: bool = ui
                .data(|d| d.get_temp(egui::Id::new(("startup-remove-focus", &it.label))))
                .unwrap_or(false);
            let hot = ui.rect_contains_pointer(row.response.rect.expand2(egui::vec2(0.0, 3.0)))
                || focus_on_remove;
            if hot != row_hot {
                ui.data_mut(|d| d.insert_temp(hov_id, hot));
                ui.ctx().request_repaint();
            }
            if k + 1 < n {
                ui.add_space(2.0);
                let y = ui.cursor().min.y;
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(12)),
                );
                ui.add_space(4.0);
            }
        }
    });
}

fn open_remove(app: &mut CleanApp, it: &LaunchItem) {
    match features::scan(&app.env, &app.settings, FeatureId::LoginItems) {
        Ok(mut plan) => {
            let ids: Vec<usize> = plan.items.iter().map(|i| i.id).collect();
            for id in ids {
                let on = plan.items[id].path == it.plist;
                plan.set_selected(id, on);
            }
            if plan.selected_count() == 1 {
                app.startup.confirm_remove = Some(plan);
            } else {
                app.warn(format!(
                    "{} cannot be removed by CleanCraft (it needs administrator rights or is protected).",
                    it.label
                ));
            }
        }
        Err(e) => app.warn(format!("Could not prepare the removal: {e:#}")),
    }
}

fn heavy_section(app: &mut CleanApp, ui: &mut Ui, data: &Snapshot) {
    let t = app.t;
    section(
        ui,
        &t,
        "Heavy apps",
        &format!(
            "Your apps using at least {:.0}% of a processor or {:.0}% of memory. macOS parts and CleanCraft are never listed.",
            startup::HEAVY_CPU,
            startup::HEAVY_MEM
        ),
    );
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match &data.heavy {
            Err(e) => {
                kit::strip(ui, crate::icons::Icon::Warning, t.warning, e);
            }
            Ok(v) if v.is_empty() => {
                kit::dim(ui, &t, "No app is working hard right now.");
            }
            Ok(v) => {
                for p in v {
                    let icon = p
                        .app
                        .as_deref()
                        .map(|a| cleancraft_core::apps::resolve(&app.env, a));
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        split_row(
                            ui,
                            |ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(crate::theme::icon::APP_ROW),
                                    egui::Sense::hover(),
                                );
                                match &icon {
                                    Some(a) => {
                                        crate::images::paint_app_icon(ui, r, a);
                                    }
                                    None => kit::initial_tile(
                                        ui.painter(),
                                        r,
                                        &p.name,
                                        kit::name_color(&p.name),
                                    ),
                                }
                                short(ui, &p.name, size::BODY, Weight::SemiBold, t.text);
                                ui.label(
                                    RichText::new(format!(
                                        "CPU {:.1}%  Memory {:.1}%",
                                        p.cpu, p.mem
                                    ))
                                    .font(font(size::FOOTNOTE, Weight::Medium))
                                    .color(t.warning),
                                );
                                kit::dim(ui, &t, &format!("pid {}", p.pid));
                            },
                            |ui| {
                                if row_button(
                                    ui,
                                    "Quit",
                                    format!("Quit {}", p.name),
                                    "Ask the app to quit (asks first)",
                                ) {
                                    app.act();
                                    app.startup.confirm_quit = Some(p.clone());
                                }
                            },
                        );
                    });
                }
            }
        }
    });
}

fn confirm_quit(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(p) = app.startup.confirm_quit.clone() else {
        return;
    };
    let (prog, args) = startup::quit_command(&p);
    let cmd = std::iter::once(prog)
        .chain(args.iter().map(|a| tasks::sh_quote_if_needed(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut go = false;
    let mut close = false;
    let dlg = kit::modal(egui::Id::new("startup-quit"), &t).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        kit::dialog_head(
            ui,
            &t,
            crate::icons::Icon::Stop,
            t.warning,
            &format!("Quit {}?", p.name),
            if p.bundle_id.is_some() {
                "The app is asked to quit, like choosing Quit in its menu. It may ask you to save your work."
            } else {
                "The program is asked to stop. Unsaved work in it can be lost."
            },
        );
        ui.add_space(10.0);
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            kit::eyebrow(ui, &t, "Command");
            ui.label(
                RichText::new(cmd)
                    .font(font(size::FOOTNOTE, Weight::Medium))
                    .color(t.text),
            );
        });
        ui.add_space(14.0);
        let (g, c) = kit::dialog_actions(ui, &t, "Quit App", kit::Kind::Danger);
        go = g;
        close = c;
    });
    if dlg.should_close() {
        close = true;
    }
    if go {
        app.act();
        app.startup.confirm_quit = None;
        start(app, Action::Quit(p));
    } else if close {
        app.act();
        app.startup.confirm_quit = None;
    }
}

fn confirm_remove(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(plan) = app.startup.confirm_remove.clone() else {
        return;
    };
    let (dest, label) = match app.settings.method {
        Method::Trash => ("the Trash", "Move to Trash"),
        Method::Quarantine | Method::AdminQuarantine => {
            ("the CleanCraft quarantine", "Move to Quarantine")
        }
    };
    let mut go = false;
    let mut close = false;
    let dlg = kit::modal(egui::Id::new("startup-remove"), &t).show(ui.ctx(), |ui| {
        ui.set_width(600.0);
        kit::dialog_head(
            ui,
            &t,
            crate::icons::Icon::Trash,
            t.sec_a,
            &format!("Move this startup file to {dest}?"),
            "The agent is stopped first, then its file moves. You can put it back from History; it starts again at the next login.",
        );
        ui.add_space(10.0);
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for it in plan.selected() {
                ui.label(
                    RichText::new(app.env.display(&it.path))
                        .font(font(size::CALLOUT, Weight::Medium))
                        .color(t.text),
                );
                ui.label(
                    RichText::new(format!("{}  {}", human_size(it.size), it.reason))
                        .color(t.text_dim),
                );
            }
        });
        ui.add_space(14.0);
        let (g, c) = kit::dialog_actions(ui, &t, label, kit::Kind::Danger);
        go = g;
        close = c;
    });
    if dlg.should_close() {
        close = true;
    }
    if go {
        app.act();
        app.startup.confirm_remove = None;
        start(app, Action::Remove(plan));
    } else if close {
        app.act();
        app.startup.confirm_remove = None;
    }
}
