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
use crate::theme::{Weight, font};

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
}

fn reload(app: &mut CleanApp) {
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
                Ok(m) => app.tell(m),
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
    e.to_string()
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

/// A small button whose accessible name says which item it acts on.
fn row_button(ui: &mut Ui, text: &str, name: String) -> bool {
    let r = ui.small_button(text);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
    r.clicked()
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
                "What opens when you log in, what runs in the background, and the apps working hardest right now.",
            );
        });
    });
    ui.add_space(8.0);
    if app.startup.data.is_none() && app.startup.loading.is_none() {
        reload(app);
    }
    if app.startup.busy.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Working... macOS may ask for your password.");
        });
        return;
    }
    let Some(data) = app.startup.data.clone() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Reading startup items...");
        });
        return;
    };
    ui.horizontal(|ui| {
        let n_on = data.launch.iter().filter(|i| i.enabled).count()
            + data.login.as_ref().map(|l| l.len()).unwrap_or(0);
        ui.label(
            RichText::new(format!("{n_on} items start automatically"))
                .font(font(15.0, Weight::SemiBold)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::secondary(ui, &t, "Refresh") {
                app.act();
                reload(app);
            }
        });
    });
    for n in &data.notes {
        kit::dim(ui, &t, n);
    }
    egui::ScrollArea::vertical()
        .id_salt("startup")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            login_section(app, ui, &data);
            for d in Domain::ALL {
                launch_section(app, ui, &data, d);
            }
            heavy_section(app, ui, &data);
        });
    if app.startup.confirm_quit.is_some() {
        confirm_quit(app, ui);
    }
    if app.startup.confirm_remove.is_some() {
        confirm_remove(app, ui);
    }
}

fn section(ui: &mut Ui, t: &crate::theme::Tokens, title: &str, desc: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(font(16.0, Weight::Bold)));
    kit::dim(ui, t, desc);
    ui.add_space(4.0);
}

fn login_section(app: &mut CleanApp, ui: &mut Ui, data: &Snapshot) {
    let t = app.t;
    section(
        ui,
        &t,
        "Login items",
        "Apps that open when you log in. Turning one off removes it from the list; Turn On adds it back.",
    );
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match &data.login {
            Err(e) => {
                ui.label(RichText::new(e).color(t.warning));
            }
            Ok(list) if list.is_empty() && data.removed_login.is_empty() => {
                ui.label("No login items.");
            }
            Ok(list) => {
                for it in list {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&it.name).font(font(13.5, Weight::SemiBold)));
                        kit::dim(ui, &t, it.path.as_deref().unwrap_or("location unknown"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if row_button(ui, "Turn Off", format!("Turn Off {}", it.name)) {
                                app.act();
                                start(app, Action::RemoveLogin(it.clone()));
                            }
                            ui.label(RichText::new("On").color(t.success));
                        });
                    });
                }
                for (id, it) in &data.removed_login {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&it.name).font(font(13.5, Weight::SemiBold)));
                        kit::dim(ui, &t, it.path.as_deref().unwrap_or(""));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if row_button(ui, "Turn On", format!("Turn On {}", it.name)) {
                                app.act();
                                start(app, Action::RestoreLogin(id.clone(), it.name.clone()));
                            }
                            ui.label(RichText::new("Off").color(t.text_faint));
                        });
                    });
                }
            }
        }
    });
}

fn launch_section(app: &mut CleanApp, ui: &mut Ui, data: &Snapshot, d: Domain) {
    let t = app.t;
    let items: Vec<&LaunchItem> = data.launch.iter().filter(|i| i.domain == d).collect();
    section(ui, &t, d.title(), d.description());
    let removable = app.env.can_modify(&d.path(&app.env));
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if items.is_empty() {
            ui.label("None.");
        }
        for it in items {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&it.label).font(font(13.5, Weight::SemiBold)));
                    let prog = it.program.as_deref().unwrap_or("no program named");
                    kit::dim(ui, &t, &format!("Runs {prog}"));
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
                    ui.label(RichText::new(sig).size(11.5).color(col));
                    if let Some(p) = &it.problem {
                        ui.label(RichText::new(p).size(11.5).color(t.warning));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if it.apple {
                        kit::dim(ui, &t, "Part of macOS");
                        return;
                    }
                    if removable && row_button(ui, "Remove", format!("Remove {}", it.label)) {
                        app.act();
                        open_remove(app, it);
                    }
                    let (text, on) = if it.enabled {
                        ("Turn Off", false)
                    } else {
                        ("Turn On", true)
                    };
                    if row_button(ui, text, format!("{text} {}", it.label)) {
                        app.act();
                        start(app, Action::Launch(it.clone(), on));
                    }
                    if it.enabled {
                        ui.label(RichText::new("On").color(t.success));
                    } else {
                        ui.label(RichText::new("Off").color(t.text_faint));
                    }
                    if d.admin() {
                        kit::dim(ui, &t, "needs password");
                    }
                });
            });
            ui.separator();
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
                ui.label(RichText::new(e).color(t.warning));
            }
            Ok(v) if v.is_empty() => {
                ui.label("No app is working hard right now.");
            }
            Ok(v) => {
                for p in v {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&p.name).font(font(13.5, Weight::SemiBold)));
                        ui.label(format!("CPU {:.1}%  Memory {:.1}%", p.cpu, p.mem));
                        kit::dim(ui, &t, &format!("pid {}", p.pid));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if row_button(ui, "Quit", format!("Quit {}", p.name)) {
                                app.act();
                                app.startup.confirm_quit = Some(p.clone());
                            }
                        });
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
    egui::Modal::new(egui::Id::new("startup-quit")).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new(format!("Quit {}?", p.name)).font(font(18.0, Weight::Bold)));
        kit::dim(
            ui,
            &t,
            if p.bundle_id.is_some() {
                "The app is asked to quit, like choosing Quit in its menu. It may ask you to save your work."
            } else {
                "The program is asked to stop. Unsaved work in it can be lost."
            },
        );
        ui.label(RichText::new(cmd).monospace().size(11.5));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if kit::danger(ui, &t, "Quit App") {
                go = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
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
    egui::Modal::new(egui::Id::new("startup-remove")).show(ui.ctx(), |ui| {
        ui.set_width(620.0);
        ui.label(
            RichText::new(format!("Move this startup file to {dest}?"))
                .font(font(18.0, Weight::Bold)),
        );
        kit::dim(
            ui,
            &t,
            "The agent is stopped first, then its file moves. You can put it back from History; it starts again at the next login.",
        );
        for it in plan.selected() {
            ui.label(RichText::new(app.env.display(&it.path)).monospace().size(11.5));
            ui.label(format!("{}  {}", human_size(it.size), it.reason));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
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
        app.startup.confirm_remove = None;
        start(app, Action::Remove(plan));
    } else if close {
        app.act();
        app.startup.confirm_remove = None;
    }
}
