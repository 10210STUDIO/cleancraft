//! Extra controls for the cleanup modules (see pages/extras.rs).
//!
//! - Trash Bins: "Empty Trash in Finder". CleanCraft never deletes for good, so the Trash items
//!   are locked in the list; this button asks Finder to empty the Trash after a confirmation
//!   that says it cannot be undone, logs the command and scans again.
//! - Trash Bins, Mail: each Mail account's trash has "Erase in Mail..." in its header. After
//!   a confirmation that says Mail erases those messages for good with its own command and
//!   CleanCraft cannot undo it, CleanCraft chooses Mailbox > Erase Deleted Items > <account>
//!   in Mail ([`trash::erase_in_mail`]), then measures again (now and a few seconds later,
//!   after Mail's own confirmation). Without the Accessibility permission the toolbar offers
//!   "Open Accessibility Settings" and "Open Mail", and the notice gives the menu path.
//!   A Trash that cannot be read offers Full Disk Access in the bottom bar.
//! - Privacy Traces: "Wi-Fi Settings" opens System Settings at Wi-Fi (macOS removes known
//!   networks there), "Reset Permissions" lists the granted app permissions with checkboxes and
//!   takes the checked ones back with `tccutil reset`, each logged, then scans again.

use std::sync::{Arc, Mutex};

use cleancraft_core::FeatureId;
use cleancraft_core::features::{privacy, trash};
use cleancraft_core::fsutil::human_size;
use egui::{Id, Ui};

use crate::app::CleanApp;
use crate::kit;
use crate::pages::plan_page;

pub const EMPTY_BUTTON: &str = "Empty Trash in Finder";
pub const EMPTY_CONFIRM: &str = "Empty Trash";
pub const WIFI_BUTTON: &str = "Wi-Fi Settings";
pub const RESET_BUTTON: &str = "Reset Permissions";
pub const RESET_CONFIRM: &str = "Reset Selected";
pub const ERASE_BUTTON: &str = "Erase in Mail...";
pub const ERASE_CONFIRM: &str = "Erase in Mail";
pub const ACCESS_BUTTON: &str = "Open Accessibility Settings";
pub const OPEN_MAIL_BUTTON: &str = "Open Mail";
pub const FDA_BUTTON: &str = "Open Full Disk Access";
/// Seconds after Erase in Mail before the list is measured again (Mail's own confirmation).
pub const RECHECK_SECS: f64 = 8.0;
/// Under the list in the Empty Trash and Erase in Mail confirmations (CHK7-3).
pub const ERASE_NOTE: &str = "Mail and Finder erase these for good; CleanCraft cannot undo it.";
/// Shown when the Trash changed between the list and the click: nothing was erased.
pub const CHANGED_NOTE: &str =
    "The Trash changed since this list was made. Look at the new list, then confirm again.";
/// Height of one row of the confirmation lists.
const ROW_H: f64 = 22.0;

/// The Erase in Mail confirmation: account for the menu, messages, bytes, and each mailbox
/// Mail erases (name, messages, bytes).
#[derive(Clone, Debug, Default)]
struct EraseAsk {
    account: Option<String>,
    messages: usize,
    bytes: u64,
    mailboxes: Vec<(String, usize, u64)>,
}

/// One entry Finder will erase: its name, which Trash it is in, its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashEntry {
    pub path: std::path::PathBuf,
    pub name: String,
    pub trash: String,
    pub size: u64,
}

/// Everything Finder's Empty Trash erases now: each entry of every Finder Trash (the startup
/// disk's and other disks'), with its size; Mail's trash is not part of it.
pub fn finder_trash_entries(env: &cleancraft_core::Env) -> Vec<TrashEntry> {
    let mut v = Vec::new();
    for t in trash::trashes(env).into_iter().filter(|t| !t.mail) {
        for p in cleancraft_core::fsutil::children(&t.path) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            v.push(TrashEntry {
                size: cleancraft_core::fsutil::tree_size(&p).0,
                path: p,
                name,
                trash: t.label.clone(),
            });
        }
    }
    v
}

/// A scrollable list for a confirmation: (name, detail, size) rows.
pub fn erase_list(ui: &mut Ui, t: &crate::theme::Tokens, rows: &[(String, String, u64)]) {
    kit::well(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .auto_shrink([false, true])
            .show_rows(ui, ROW_H as f32, rows.len(), |ui, range| {
                for (name, detail, size) in &rows[range] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(ROW_H as f32);
                        ui.label(egui::RichText::new(name).color(t.text));
                        if !detail.is_empty() {
                            ui.label(egui::RichText::new(detail).color(t.text_dim));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(human_size(*size)).color(t.text_dim));
                        });
                    });
                }
            });
    });
}

type Shared<T> = Arc<Mutex<Option<T>>>;

#[derive(Clone, Default)]
struct State {
    /// The confirmation is open: what Finder's Trashes held when it opened (CHK7-3).
    confirm_empty: Option<Vec<TrashEntry>>,
    /// The Trash changed between the list and the click: the list was made again.
    empty_changed: bool,
    /// Finder is emptying the Trash on a worker thread.
    empty_job: Option<Shared<trash::EmptyOutcome>>,
    /// The Reset Permissions dialog: (service, client, label, checked).
    reset: Option<Vec<(String, String, String, bool)>>,
    /// The Erase in Mail confirmation, while open.
    erase: Option<EraseAsk>,
    /// Erase in Mail could not use Mail's menu: show the settings and Open Mail.
    erase_help: bool,
    /// When (egui time) to measure the Trash again after Erase in Mail.
    recheck_at: Option<f64>,
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
        // Trash Bins' "Empty Trash in Finder" sits in the bottom bar (trash_bar); after Erase
        // in Mail the help buttons sit here.
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

/// A Finder Trash in the Trash Bins list holds something (the startup Trash row is listed
/// even when empty or unreadable, with size 0).
fn finder_trash_full(app: &mut CleanApp) -> bool {
    app.plan_state(FeatureId::TrashBins)
        .plan
        .as_ref()
        .is_some_and(|p| {
            p.items.iter().any(|i| {
                matches!(i.category.as_str(), "startup_trash" | "external_trash") && i.size > 0
            })
        })
}

/// Trash Bins' bottom bar action: Empty Trash when a Finder Trash holds something, Full Disk
/// Access when the Trash could not be read.
pub fn trash_bar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let unreadable = app
        .plan_state(FeatureId::TrashBins)
        .plan
        .as_ref()
        .is_some_and(|p| {
            p.items
                .iter()
                .any(|i| i.locked.as_deref() == Some(trash::LOCK_NEEDS_ACCESS))
        });
    if unreadable && kit::secondary(ui, &t, FDA_BUTTON) {
        app.act();
        match cleancraft_core::permissions::open_fda_settings(&app.env) {
            Ok(()) => app.tell(
                "System Settings is open at Full Disk Access. Turn CleanCraft on, then Scan Again.",
            ),
            Err(e) => app.warn(format!("Could not open System Settings: {e}")),
        }
    }
    let busy = load(ui).empty_job.is_some();
    if busy || finder_trash_full(app) {
        empty_trash(app, ui, EMPTY_BUTTON);
    }
}

/// The bottom bar's line on Trash Bins: what to do next for what is listed.
pub fn trash_hint(plan: &cleancraft_core::Plan) -> String {
    let finder = plan
        .items
        .iter()
        .any(|i| i.category != "mail_trash" && i.size > 0);
    let mail = plan.items.iter().any(|i| i.category == "mail_trash");
    match (finder, mail) {
        (true, true) => {
            "Finder empties the Trash; Mail erases deleted mail. Neither can be undone.".into()
        }
        (true, false) => "Finder empties the Trash. CleanCraft cannot put those items back.".into(),
        (false, true) => {
            "Use Erase in Mail to remove deleted messages. Only Mail can erase them.".into()
        }
        (false, false) => "Nothing is waiting in the Trash.".into(),
    }
}

/// "Erase in Mail..." for one Mail account's trash (its group header). When Mail's name for
/// the account is not known, "Open Mail" instead: the person picks the account in Mail's own
/// menu (CleanCraft never uses "In All Accounts", which would erase more than is shown).
pub fn erase_button(app: &mut CleanApp, ui: &mut Ui, group: &str) {
    let t = app.t;
    let env = app.env.clone();
    let named = app
        .plan_state(FeatureId::TrashBins)
        .plan
        .as_ref()
        .and_then(|p| trash::mail_group(&env, p, group))
        .is_some_and(|g| g.0.is_some());
    if !named {
        let r = kit::secondary_resp(ui, &t, OPEN_MAIL_BUTTON)
            .on_hover_text("In Mail: Mailbox > Erase Deleted Items, then the account");
        r.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{OPEN_MAIL_BUTTON} {group}"),
            )
        });
        if r.clicked() {
            app.act();
            match trash::open_mail(&app.env) {
                Ok(()) => app.tell(
                    "Mail is open. Choose Mailbox > Erase Deleted Items and pick this account, then Scan Again here.",
                ),
                Err(e) => app.warn(format!("Could not open Mail: {e}")),
            }
        }
        return;
    }
    let r = kit::secondary_resp(ui, &t, ERASE_BUTTON)
        .on_hover_text("Mail erases this account's deleted messages for good");
    r.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{ERASE_BUTTON} {group}"),
        )
    });
    if r.clicked() {
        app.act();
        let env = app.env.clone();
        let ask = app
            .plan_state(FeatureId::TrashBins)
            .plan
            .as_ref()
            .and_then(|p| trash::mail_group(&env, p, group));
        if let Some((account, messages, bytes)) = ask {
            let mailboxes = app
                .plan_state(FeatureId::TrashBins)
                .plan
                .as_ref()
                .map(|p| mailboxes_of(&env, p, group))
                .unwrap_or_default();
            let mut st = load(ui);
            st.erase = Some(EraseAsk {
                account,
                messages,
                bytes,
                mailboxes,
            });
            store(ui, st);
        }
    }
}

/// Each mailbox of one Mail account's trash group: (mailbox, messages, bytes), what Erase in
/// Mail erases.
fn mailboxes_of(
    env: &cleancraft_core::Env,
    plan: &cleancraft_core::Plan,
    group: &str,
) -> Vec<(String, usize, u64)> {
    let infos = trash::trashes(env);
    plan.items
        .iter()
        .filter(|i| i.category == "mail_trash" && i.group.as_deref() == Some(group))
        .map(|i| {
            let info = infos.iter().find(|t| t.path == i.path);
            let name = info
                .map(|t| {
                    if t.mailbox.is_empty() {
                        t.label.clone()
                    } else {
                        t.mailbox.clone()
                    }
                })
                .unwrap_or_else(|| env.display(&i.path));
            (name, info.map(|t| t.entries).unwrap_or(0), i.size)
        })
        .collect()
}

/// Trash Bins toolbar: the help buttons after Erase in Mail could not use Mail's menu.
fn trash_toolbar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let st = load(ui);
    if st.erase_help {
        if kit::secondary(ui, &t, ACCESS_BUTTON) {
            app.act();
            match app.env.sys.open_url(trash::ACCESSIBILITY_URL) {
                Ok(()) => app.tell(
                    "System Settings is open at Accessibility. Turn CleanCraft on, then use Erase in Mail again.",
                ),
                Err(e) => app.warn(format!("Could not open System Settings: {e}")),
            }
        }
        if kit::secondary(ui, &t, OPEN_MAIL_BUTTON) {
            app.act();
            match trash::open_mail(&app.env) {
                Ok(()) => app.tell(
                    "Mail is open. Choose Mailbox > Erase Deleted Items, then Scan Again here.",
                ),
                Err(e) => app.warn(format!("Could not open Mail: {e}")),
            }
        }
    }
}

/// Every frame of Trash Bins' results: the Erase in Mail confirmation and the re-measure a
/// few seconds after it.
pub fn trash_frame(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut st = load(ui);
    let now = ui.input(|i| i.time);
    if let Some(at) = st.recheck_at {
        if now >= at {
            st.recheck_at = None;
            store(ui, st.clone());
            plan_page::start_scan(app, FeatureId::TrashBins);
            app.sweep.refresh_tile(FeatureId::TrashBins);
            return;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    if let Some(ask) = st.erase.clone() {
        let mut go = false;
        let mut cancel = false;
        let name = ask.account.clone().unwrap_or_default();
        let whose = format!("of {name}");
        let menu = format!("Mailbox > Erase Deleted Items > {name}");
        let dlg = kit::modal(Id::new("confirm-erase-mail"), &t).show(ui.ctx(), |ui| {
            ui.set_width(540.0);
            kit::dialog_head(
                ui,
                &t,
                crate::icons::Icon::Trash,
                t.danger,
                &format!(
                    "Erase {} in Mail ({})?",
                    cleancraft_core::fsutil::count(ask.messages, "deleted message", "deleted messages"),
                    human_size(ask.bytes)
                ),
                &format!(
                    "Mail permanently erases the deleted messages {whose} with its own command ({menu}); on a mail server account they are erased on the server too. CleanCraft cannot undo this, and the messages cannot be put back. CleanCraft opens Mail and chooses the command for you, which needs the Accessibility permission; Mail may ask you to confirm."
                ),
            );
            ui.add_space(12.0);
            let rows: Vec<(String, String, u64)> = ask
                .mailboxes
                .iter()
                .map(|(mailbox, n, bytes)| {
                    (
                        format!("{mailbox} of {name}"),
                        cleancraft_core::fsutil::count(*n, "message", "messages"),
                        *bytes,
                    )
                })
                .collect();
            erase_list(ui, &t, &rows);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(ERASE_NOTE).color(t.danger));
            ui.add_space(12.0);
            let (ok, no) = kit::dialog_actions(ui, &t, ERASE_CONFIRM, kit::Kind::Danger);
            go |= ok;
            cancel |= no;
        });
        if dlg.should_close() {
            cancel = true;
        }
        if cancel {
            app.act();
            st.erase = None;
        } else if go {
            app.act();
            st.erase = None;
            let out = trash::erase_in_mail(&app.env, ask.account.as_deref());
            st.erase_help = out.needs_accessibility || out.needs_automation;
            if out.ok {
                app.tell(out.message);
                st.recheck_at = Some(now + RECHECK_SECS);
            } else {
                app.warn(out.message);
            }
            // Measured again now (and, after Mail's own confirmation, a little later).
            store(ui, st);
            plan_page::start_scan(app, FeatureId::TrashBins);
            app.sweep.refresh_tile(FeatureId::TrashBins);
            return;
        }
    }
    store(ui, st);
}

/// "Empty Trash" with Finder's own emptying, after a confirmation that says it cannot be
/// undone: on the Trash Bins toolbar and on every Done screen that moved items to the Trash
/// (REVIEW-U U4: free the space without leaving CleanCraft).
pub fn empty_trash(app: &mut CleanApp, ui: &mut Ui, label: &str) {
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
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 14.0, &t, time);
                ui.label("Finder is emptying the Trash...");
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
                store(ui, st);
                return;
            }
        }
    }
    if kit::secondary_resp(ui, &t, label)
        .on_hover_text("Finder removes what is in the Trash for good")
        .clicked()
    {
        app.act();
        st.confirm_empty = Some(finder_trash_entries(&app.env));
        st.empty_changed = false;
    }
    if let Some(list) = st.confirm_empty.clone() {
        let mut go = false;
        let mut cancel = false;
        let size: u64 = list.iter().map(|e| e.size).sum();
        let changed = st.empty_changed;
        let dlg = kit::modal(Id::new("confirm-empty-trash"), &t).show(ui.ctx(), |ui| {
            ui.set_width(560.0);
            kit::dialog_head(
                ui,
                &t,
                crate::icons::Icon::Trash,
                t.danger,
                &format!("Empty the Trash ({})?", human_size(size)),
                &format!(
                    "Finder will remove these {} from the Trash of every disk for good. Mail's deleted messages are not affected.",
                    cleancraft_core::fsutil::items(list.len())
                ),
            );
            ui.add_space(12.0);
            let rows: Vec<(String, String, u64)> = list
                .iter()
                .map(|e| (e.name.clone(), e.trash.clone(), e.size))
                .collect();
            erase_list(ui, &t, &rows);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(ERASE_NOTE).color(t.danger));
            if changed {
                ui.label(egui::RichText::new(CHANGED_NOTE).color(t.warning));
            }
            ui.add_space(12.0);
            let (ok, no) = kit::dialog_actions(ui, &t, EMPTY_CONFIRM, kit::Kind::Danger);
            go |= ok;
            cancel |= no;
        });
        if dlg.should_close() {
            cancel = true;
        }
        if cancel {
            app.act();
            st.confirm_empty = None;
        } else if go && finder_trash_entries(&app.env) != list {
            // Something came into (or left) the Trash after the list was shown: Finder would
            // erase more than the person saw. Show the new list instead.
            app.act();
            st.confirm_empty = Some(finder_trash_entries(&app.env));
            st.empty_changed = true;
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
    app.sweep.refresh_tile(FeatureId::TrashBins);
}

fn privacy_toolbar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut st = load(ui);
    if plan_has(app, FeatureId::Privacy, &["wifi"])
        && kit::secondary_resp(ui, &t, WIFI_BUTTON)
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
        && kit::secondary_resp(ui, &t, RESET_BUTTON)
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
        let dlg = kit::modal(Id::new("reset-permissions"), &t).show(ui.ctx(), |ui| {
            ui.set_width(560.0);
            kit::dialog_head(
                ui,
                &t,
                crate::icons::Icon::Privacy,
                t.sec_a,
                "Reset app permissions",
                "macOS takes the access back; each app asks again the next time it needs it. An app that is open may need to be restarted.",
            );
            ui.add_space(12.0);
            kit::well(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (_, _, label, on) in list.iter_mut() {
                            if kit::check_text(ui, &t, on, label.as_str()).changed() {
                                toggled = true;
                            }
                        }
                    });
            });
            ui.add_space(14.0);
            let n = list.iter().filter(|e| e.3).count();
            if n > 0 {
                let (ok, no) = kit::dialog_actions(ui, &t, RESET_CONFIRM, kit::Kind::Danger);
                go |= ok;
                cancel |= no;
            } else {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::secondary(ui, &t, "Cancel") {
                        cancel = true;
                    }
                });
            }
        });
        if dlg.should_close() {
            cancel = true;
        }
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
