//! Extra controls for the cleanup modules (see pages/extras.rs).
//!
//! - Trash Bins: "Empty Trash in Finder". CleanCraft never deletes for good, so the Trash items
//!   are locked in the list; this button asks Finder to empty the Trash after a confirmation
//!   that says it cannot be undone, logs the command and scans again.
//! - Privacy Traces: "Wi-Fi Settings" opens System Settings at Wi-Fi (macOS removes known
//!   networks there), "Reset Permissions" lists the granted app permissions with checkboxes and
//!   takes the checked ones back with `tccutil reset`, each logged, then scans again.

use std::sync::{Arc, Mutex};

use cleancraft_core::FeatureId;
use cleancraft_core::features::{privacy, trash};
use cleancraft_core::fsutil::human_size;
use egui::{Id, RichText, Ui};

use crate::app::CleanApp;
use crate::kit;
use crate::pages::plan_page;
use crate::theme::{Weight, font};

pub const EMPTY_BUTTON: &str = "Empty Trash in Finder";
pub const EMPTY_CONFIRM: &str = "Empty Trash";
pub const WIFI_BUTTON: &str = "Wi-Fi Settings";
pub const RESET_BUTTON: &str = "Reset Permissions";
pub const RESET_CONFIRM: &str = "Reset Selected";

type Shared<T> = Arc<Mutex<Option<T>>>;

#[derive(Clone, Default)]
struct State {
    /// The confirmation is open; the bytes Finder's Trashes held when it opened.
    confirm_empty: Option<u64>,
    /// Finder is emptying the Trash on a worker thread.
    empty_job: Option<Shared<trash::EmptyOutcome>>,
    /// The Reset Permissions dialog: (service, client, label, checked).
    reset: Option<Vec<(String, String, String, bool)>>,
}

fn state_id() -> Id {
    Id::new("extras-cleanup-state")
}

fn load(ui: &Ui) -> State {
    ui.ctx()
        .data_mut(|d| d.get_temp::<State>(state_id()))
        .unwrap_or_default()
}

fn store(ui: &Ui, s: State) {
    ui.ctx().data_mut(|d| d.insert_temp(state_id(), s));
}

pub fn before_scan(_app: &mut CleanApp, _ui: &mut Ui, _f: FeatureId) {}

pub fn results_toolbar(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    match f {
        FeatureId::TrashBins => trash_toolbar(app, ui),
        FeatureId::Privacy => privacy_toolbar(app, ui),
        _ => {}
    }
}

fn plan_has(app: &mut CleanApp, f: FeatureId, cats: &[&str]) -> bool {
    app.plan_state(f)
        .plan
        .as_ref()
        .is_some_and(|p| p.items.iter().any(|i| cats.contains(&i.category.as_str())))
}

fn trash_toolbar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut st = load(ui);
    // A finished background empty: report, scan again.
    if let Some(job) = st.empty_job.clone() {
        let done = job.lock().ok().and_then(|mut g| g.take());
        match done {
            Some(out) => {
                st.empty_job = None;
                finish_empty(app, out);
            }
            None => {
                ui.spinner();
                ui.label("Finder is emptying the Trash...");
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
                store(ui, st);
                return;
            }
        }
    }
    if !plan_has(app, FeatureId::TrashBins, &["startup_trash", "external_trash"]) {
        store(ui, st);
        return;
    }
    if ui
        .button(EMPTY_BUTTON)
        .on_hover_text("Finder removes what is in the Trash for good")
        .clicked()
    {
        app.act();
        st.confirm_empty = Some(trash::finder_trash_size(&app.env));
    }
    if let Some(size) = st.confirm_empty {
        let mut go = false;
        let mut cancel = false;
        egui::Modal::new(Id::new("confirm-empty-trash")).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.label(
                RichText::new(format!("Empty the Trash ({})?", human_size(size)))
                    .font(font(18.0, Weight::Bold)),
            );
            kit::dim(
                ui,
                &t,
                "Finder will remove everything in the Trash of every disk for good. This cannot be undone, and CleanCraft cannot put these items back. Mail's deleted messages are not affected.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if kit::danger(ui, &t, EMPTY_CONFIRM) {
                    go = true;
                }
                if kit::secondary(ui, &t, "Cancel") {
                    cancel = true;
                }
            });
        });
        if cancel {
            app.act();
            st.confirm_empty = None;
        } else if go {
            app.act();
            st.confirm_empty = None;
            let env = app.env.clone();
            if app.sync {
                let out = trash::empty_trash_in_finder(&env);
                finish_empty(app, out);
            } else {
                let slot: Shared<trash::EmptyOutcome> = Arc::default();
                let s2 = slot.clone();
                std::thread::spawn(move || {
                    let out = trash::empty_trash_in_finder(&env);
                    if let Ok(mut g) = s2.lock() {
                        *g = Some(out);
                    }
                });
                st.empty_job = Some(slot);
            }
        }
    }
    store(ui, st);
}

fn finish_empty(app: &mut CleanApp, out: trash::EmptyOutcome) {
    if out.ok {
        app.tell(out.message);
    } else {
        app.warn(out.message);
    }
    plan_page::start_scan(app, FeatureId::TrashBins);
}

fn privacy_toolbar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut st = load(ui);
    if plan_has(app, FeatureId::Privacy, &["wifi"])
        && ui
            .button(WIFI_BUTTON)
            .on_hover_text("Open System Settings at Wi-Fi to remove known networks")
            .clicked()
    {
        app.act();
        match app.env.sys.open_url(privacy::WIFI_SETTINGS_URL) {
            Ok(()) => app.tell(
                "System Settings is open at Wi-Fi. Remove networks under Known Networks, then scan again.",
            ),
            Err(e) => app.warn(format!("Could not open System Settings: {e}")),
        }
    }
    if plan_has(app, FeatureId::Privacy, &["permissions"])
        && ui
            .button(RESET_BUTTON)
            .on_hover_text("Take back access you gave to apps")
            .clicked()
    {
        app.act();
        let env = app.env.clone();
        let list = app
            .plan_state(FeatureId::Privacy)
            .plan
            .as_ref()
            .map(|p| {
                p.items
                    .iter()
                    .filter(|i| i.category == "permissions")
                    .filter_map(|i| {
                        let (svc, client) = privacy::permission_of(&env, &i.path)?;
                        let label = format!("{}: {client}", privacy::service_title(&svc));
                        Some((svc, client, label, false))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        st.reset = Some(list);
    }
    if let Some(mut list) = st.reset.take() {
        let mut go = false;
        let mut cancel = false;
        let mut toggled = false;
        egui::Modal::new(Id::new("reset-permissions")).show(ui.ctx(), |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Reset app permissions").font(font(18.0, Weight::Bold)));
            kit::dim(
                ui,
                &t,
                "macOS takes the access back; each app asks again the next time it needs it. An app that is open may need to be restarted.",
            );
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (_, _, label, on) in list.iter_mut() {
                        if ui.checkbox(on, label.as_str()).changed() {
                            toggled = true;
                        }
                    }
                });
            ui.add_space(8.0);
            let n = list.iter().filter(|e| e.3).count();
            ui.horizontal(|ui| {
                if n > 0 && kit::danger(ui, &t, RESET_CONFIRM) {
                    go = true;
                }
                if kit::secondary(ui, &t, "Cancel") {
                    cancel = true;
                }
            });
        });
        if toggled {
            app.act();
        }
        if cancel {
            app.act();
        } else if go {
            app.act();
            let mut ok = 0;
            let mut errors = Vec::new();
            for (svc, client, _, on) in &list {
                if !*on {
                    continue;
                }
                match privacy::reset_permission(&app.env, svc, client) {
                    Ok(_) => ok += 1,
                    Err(e) => errors.push(e),
                }
            }
            if errors.is_empty() {
                app.tell(format!(
                    "{ok} permissions reset. Each app asks again next time."
                ));
            } else {
                app.warn(format!(
                    "{ok} permissions reset, {} failed: {}",
                    errors.len(),
                    errors[0]
                ));
            }
            plan_page::start_scan(app, FeatureId::Privacy);
        } else {
            st.reset = Some(list);
        }
    }
    store(ui, st);
}
