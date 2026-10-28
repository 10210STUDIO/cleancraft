//! Full control for agents: everything the app can do, through the same safety core.
//!
//! - `select_items` sets a saved plan's selection to exactly what the agent asks (any item of
//!   the plan that is not locked, also ones unticked by default), saves it and returns the
//!   preview with the NEW digest. `execute_plan` then needs that digest, so the digest stays
//!   the proof that what runs is exactly the list the user was shown.
//! - Every tool that changes the Mac is refused while the CleanCraft setting
//!   "Let AI assistants clean after you confirm" is off ([`crate::gate`]). No tool here can
//!   turn that setting on; `agent_permissions` only says how the user does it.
//! - Actions that are not plans (quitting apps, the Trash, permissions, settings, the menu bar
//!   agent, updates) each have a look-only step that shows what will happen, and are logged
//!   in History; the ones that can be undone answer with the id `undo` takes.
//! - Administrator items (the system's launch agents and daemons, apps installed by a
//!   package) use the existing administrator path: macOS shows its password or Touch ID
//!   dialog on the Mac. No password ever passes through MCP. A cancelled dialog skips those
//!   items with the reason; the rest of the run goes on.

use std::path::PathBuf;

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use cleancraft_core::app_quit;
use cleancraft_core::apps;
use cleancraft_core::executor::{self, Method};
use cleancraft_core::features::{self, privacy, trash, uninstaller};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::login_agent;
use cleancraft_core::plans;
use cleancraft_core::schedule::ScanEvery;
use cleancraft_core::self_update;
use cleancraft_core::startup;
use cleancraft_core::sweep::{self, Sources};
use cleancraft_core::tasks;
use cleancraft_core::{Env, FeatureId, Plan, Settings};

use crate::{CleanCraftMcp, ToolResult, gate, ok, plan_view};

/// What the agent tells the user before a run that needs the administrator password.
pub const ADMIN_PROMPT: &str = "Some items need administrator rights. macOS shows its password or Touch ID dialog on the Mac: the user must approve it there. CleanCraft never asks for the password itself. If the user cancels, those items are skipped and the rest still runs.";

/// What every preview says the agent must do next.
pub const CONFIRM_STEP: &str = "Show this list to the user (path, size and reason of each item, the totals, and whether the administrator password is needed) and ask for a clear yes. Only after that, call execute_plan with this plan_id and this digest.";

// ---------------------------------------------------------------------------------------------
// Selection

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SelectArgs {
    pub plan_id: String,
    /// Item ids of the plan to select (from scan or get_plan).
    pub item_ids: Option<Vec<usize>>,
    /// Item paths to select, as the plan shows them ("/Users/me/Library/..." or "~/Library/...").
    pub paths: Option<Vec<String>>,
    /// Select every item of the plan that is not locked.
    pub select_all: Option<bool>,
    /// Start from an empty selection. Alone it clears the selection.
    pub clear: Option<bool>,
}

/// An item path the agent sent, as an env path.
pub fn resolve_path(env: &Env, s: &str) -> PathBuf {
    let s = s.trim();
    if s == "~" {
        return env.home().to_path_buf();
    }
    match s.strip_prefix("~/") {
        Some(rest) => env.home_path(rest),
        None => env.sys_path(s),
    }
}

/// Set the selection of `plan` to exactly what `a` asks: every unlocked item when
/// `select_all`, plus the items named by id or path. Unknown or locked items are refused and
/// nothing changes.
pub fn apply_selection(env: &Env, plan: &mut Plan, a: &SelectArgs) -> Result<(), String> {
    let all = a.select_all.unwrap_or(false);
    let clear = a.clear.unwrap_or(false);
    if !all && !clear && a.item_ids.is_none() && a.paths.is_none() {
        return Err("Say what to select: item_ids, paths, select_all or clear.".into());
    }
    let mut want: Vec<usize> = Vec::new();
    for id in a.item_ids.iter().flatten() {
        let Some(n) = plan.items.iter().position(|i| i.id == *id) else {
            return Err(format!(
                "Refused: there is no item {id} in this plan (it has {} items).",
                plan.items.len()
            ));
        };
        want.push(n);
    }
    for p in a.paths.iter().flatten() {
        let path = resolve_path(env, p);
        let found = plan
            .items
            .iter()
            .position(|i| i.path == path || env.display(&i.path) == p.trim());
        match found {
            Some(n) => want.push(n),
            None => return Err(format!("Refused: {p} is not an item of this plan.")),
        }
    }
    for &n in &want {
        if let Some(why) = &plan.items[n].locked {
            return Err(format!(
                "Refused: {} cannot be selected: {why}",
                env.display(&plan.items[n].path)
            ));
        }
    }
    for (n, it) in plan.items.iter_mut().enumerate() {
        it.selected = it.locked.is_none() && (all || want.contains(&n));
    }
    Ok(())
}

/// Where the selected items of `plan` go.
fn destination(plan: &Plan, settings: &Settings) -> &'static str {
    let method = plan
        .feature
        .and_then(features::get)
        .and_then(|f| f.method_override())
        .unwrap_or(settings.method);
    match method {
        Method::Trash => "the Trash",
        Method::Quarantine | Method::AdminQuarantine => "the CleanCraft quarantine",
    }
}

/// The full preview of a plan's selection: what the user must see before saying yes.
pub fn selection_view(env: &Env, settings: &Settings, plan: &Plan) -> Value {
    let admin = features::admin_count(plan);
    let items: Vec<Value> = plan
        .selected()
        .map(|i| {
            json!({
                "id": i.id,
                "path": env.display(&i.path),
                "size": human_size(i.size),
                "bytes": i.size,
                "category": i.category,
                "reason": i.reason,
                "risk": i.risk,
                "needs_admin": admin > 0 && i.needs_admin,
            })
        })
        .collect();
    let mut v = json!({
        "plan_id": plan.id,
        "feature": plan.feature,
        "digest": plan.digest(),
        "selected_count": plan.selected_count(),
        "selected": human_size(plan.selected_size()),
        "selected_bytes": plan.selected_size(),
        "item_count": plan.items.len(),
        "needs_admin_count": admin,
        "moves_to": destination(plan, settings),
        "items": items,
        "agents_may_clean": settings.agents_may_clean,
        "next": CONFIRM_STEP,
    });
    if admin > 0 {
        v["admin_note"] = json!(format!(
            "{admin} of these move to the CleanCraft quarantine with the administrator password. {ADMIN_PROMPT}"
        ));
    }
    if plan.feature == Some(FeatureId::LoginItems) {
        v["startup_note"] = json!(
            "Each agent or daemon is stopped (launchctl bootout) right after its file moved; an item whose file did not move is left running and untouched. Undo puts the file back; it starts again at the next login or restart."
        );
    }
    if !settings.agents_may_clean {
        v["gate_note"] = json!(crate::REFUSED);
    }
    v
}

pub fn select_items(env: &Env, a: &SelectArgs) -> Result<Value, String> {
    let (settings, _) = Settings::load_checked(env);
    let mut plan = plans::load(env, &a.plan_id).map_err(|e| e.to_string())?;
    apply_selection(env, &mut plan, a)?;
    plans::save(env, &plan).map_err(|e| e.to_string())?;
    Ok(selection_view(env, &settings, &plan))
}

// ---------------------------------------------------------------------------------------------
// What agents may do

/// One thing an agent can do.
#[derive(Clone, Debug, Serialize)]
pub struct ActionInfo {
    pub tool: &'static str,
    pub area: &'static str,
    pub what: &'static str,
    /// Changes the Mac: refused while the setting is off.
    pub changes: bool,
    /// The look-only step to show the user first.
    pub preview: &'static str,
    pub undo: &'static str,
}

const fn a(
    tool: &'static str,
    area: &'static str,
    what: &'static str,
    changes: bool,
    preview: &'static str,
    undo: &'static str,
) -> ActionInfo {
    ActionInfo {
        tool,
        area,
        what,
        changes,
        preview,
        undo,
    }
}

/// Every action an agent can take, with what each needs. `list_actions` returns it.
pub const ACTIONS: &[ActionInfo] = &[
    a(
        "list_features",
        "Cleanup",
        "The modules and their ids.",
        false,
        "",
        "",
    ),
    a(
        "get_plan",
        "Cleanup",
        "Read a saved plan, page by page.",
        false,
        "",
        "",
    ),
    a(
        "scan",
        "Cleanup",
        "Scan one module into a plan (every cleanup module: junk, large and old files, duplicates, similar images, Disk Map folders, App Remover, Threat Check, Startup Items, Mail attachments, Trash bins, Privacy, Developer Clutter).",
        false,
        "",
        "",
    ),
    a(
        "select_items",
        "Cleanup",
        "Choose exactly which items of a plan to clean, also ones not ticked by default. Saves the plan and returns the preview with the new digest.",
        false,
        "",
        "",
    ),
    a(
        "execute_plan",
        "Cleanup",
        "Move the confirmed selection of a plan to the Trash or the quarantine.",
        true,
        "scan, get_plan or select_items (the digest)",
        "undo with undo_with (the plan id) or one action id",
    ),
    a(
        "sweep_preview",
        "Quick Sweep",
        "Scan the Quick Sweep modules (all, or the ones named) into one plan each.",
        false,
        "",
        "",
    ),
    a(
        "sweep_run",
        "Quick Sweep",
        "Run several confirmed Quick Sweep plans in one go, each with its digest.",
        true,
        "sweep_preview, then get_plan or select_items per module",
        "undo each plan id",
    ),
    a(
        "uninstall_preview",
        "Apps",
        "Preview uninstalling or resetting one app, with its leftovers.",
        false,
        "",
        "",
    ),
    a(
        "uninstall_apps_preview",
        "Apps",
        "Preview uninstalling several apps at once, with their leftovers.",
        false,
        "",
        "",
    ),
    a(
        "app_running",
        "Apps",
        "What of an app runs now (app, helpers, agents, system services) and what quitting it needs.",
        false,
        "",
        "",
    ),
    a(
        "quit_app",
        "Apps",
        "Quit an app and its background helpers before uninstalling it; force=true stops what ignored the polite quit.",
        true,
        "app_running",
        "stopped agents can be started again from History (undo)",
    ),
    a(
        "toggle_startup_item",
        "Startup Items",
        "Turn a launch agent, daemon or login item off or on (reversible).",
        true,
        "list_startup_items",
        "undo with undo_with",
    ),
    a(
        "quit_process",
        "Startup Items",
        "Quit a heavy app listed by list_startup_items.",
        true,
        "list_startup_items",
        "none (open the app again)",
    ),
    a(
        "run_task",
        "Tune-up",
        "Run one Tune-up Task.",
        true,
        "list_tasks (the exact command)",
        "none (maintenance commands)",
    ),
    a(
        "trash_status",
        "Trash",
        "The Trash bins (Finder and Mail), their sizes and the token for empty_trash.",
        false,
        "",
        "",
    ),
    a(
        "empty_trash",
        "Trash",
        "Ask Finder to empty the Trash. This one is permanent.",
        true,
        "trash_status (the token)",
        "none: emptied items are gone",
    ),
    a(
        "erase_mail_deleted",
        "Trash",
        "Ask Mail to erase the deleted messages of one account (Mail may ask again).",
        true,
        "trash_status (the account)",
        "none",
    ),
    a(
        "reset_app_permissions",
        "Privacy",
        "Take back camera, microphone, screen and other access given to apps (tccutil reset).",
        true,
        "scan privacy (the permission items)",
        "none: each app asks again",
    ),
    a(
        "quarantine_status",
        "Quarantine",
        "What the CleanCraft quarantine holds.",
        false,
        "",
        "",
    ),
    a(
        "quarantine_to_trash",
        "Quarantine",
        "Move items of the quarantine to the Trash.",
        true,
        "quarantine_status",
        "undo with undo_with",
    ),
    a(
        "get_settings",
        "Settings",
        "Read CleanCraft's settings, the ignore list, schedules and the menu bar agent.",
        false,
        "",
        "",
    ),
    a(
        "update_settings",
        "Settings",
        "Change settings: schedules, where removed items go, sizes and ages, notifications, update checks. Never the AI assistants switch.",
        true,
        "get_settings",
        "call update_settings again with the old values (returned)",
    ),
    a(
        "ignore_list",
        "Settings",
        "Add paths to or remove paths from the ignore list.",
        true,
        "get_settings",
        "call ignore_list again the other way",
    ),
    a(
        "menu_bar_agent",
        "Settings",
        "Turn the menu bar agent (and its login item) on or off.",
        true,
        "get_settings",
        "undo with undo_with",
    ),
    a(
        "install_update",
        "Updates",
        "Install the newer CleanCraft (refused while the CleanCraft app is open).",
        true,
        "update_status",
        "History puts the old version back",
    ),
    a(
        "list_apps",
        "Apps",
        "Installed apps with size, version, last opened and maker.",
        false,
        "",
        "",
    ),
    a(
        "check_updates",
        "Apps",
        "Newer versions of the apps and macOS (installs nothing).",
        false,
        "",
        "",
    ),
    a(
        "system_status",
        "Monitor",
        "CPU, memory, disk, Trash, battery and network now.",
        false,
        "",
        "",
    ),
    a(
        "list_tasks",
        "Tune-up",
        "Tune-up Tasks with their exact commands.",
        false,
        "",
        "",
    ),
    a(
        "list_startup_items",
        "Startup Items",
        "Login items, agents and daemons, and heavy apps.",
        false,
        "",
        "",
    ),
    a(
        "full_disk_access",
        "Permissions",
        "Whether CleanCraft has Full Disk Access.",
        false,
        "",
        "",
    ),
    a(
        "history",
        "History",
        "Recent CleanCraft actions with their ids.",
        false,
        "",
        "",
    ),
    a(
        "update_status",
        "Updates",
        "CleanCraft's version and whether a newer release exists.",
        false,
        "",
        "",
    ),
    a(
        "agent_permissions",
        "Permissions",
        "Whether agents may change the Mac and how the user turns it on.",
        false,
        "",
        "",
    ),
    a("list_actions", "Help", "This list.", false, "", ""),
    a(
        "open_full_disk_access_settings",
        "Permissions",
        "Open System Settings at Full Disk Access so the user can turn it on.",
        false,
        "",
        "",
    ),
    a(
        "undo",
        "History",
        "Put back one action or a whole cleanup.",
        false,
        "history",
        "",
    ),
];

pub fn list_actions_json() -> Value {
    json!({
        "actions": ACTIONS,
        "confirm_rule": "For every action that changes the Mac: show the user the preview, ask for a clear yes, then act. Changes are refused while 'Let AI assistants clean after you confirm' is off in CleanCraft Settings.",
    })
}

pub fn agent_permissions_json(env: &Env) -> Value {
    let s = Settings::load(env);
    json!({
        "agents_may_clean": s.agents_may_clean,
        "setting": crate::SWITCH_NAME,
        "how_to_turn_on": "The user opens CleanCraft, goes to Settings, and in the AI assistants group turns on 'Let AI assistants clean after you confirm'. Agents cannot turn it on: no tool changes it.",
        "how_to_turn_off": "The same switch in CleanCraft Settings, AI assistants. Everything already done stays in History with Undo.",
        "when_off": "Agents can scan, select items and preview, but every tool that changes the Mac is refused.",
        "changing_tools": ACTIONS.iter().filter(|a| a.changes).map(|a| a.tool).collect::<Vec<_>>(),
        "administrator_items": ADMIN_PROMPT,
    })
}

// ---------------------------------------------------------------------------------------------
// Apps

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AppsArg {
    /// The apps: names ("Editor"), bundle ids or paths.
    pub apps: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QuitArg {
    /// The app: name, bundle id or path.
    pub app: String,
    /// true: stop what ignored the polite quit (kill -TERM, then -KILL), only processes inside
    /// the app. Default false: quit politely.
    pub force: Option<bool>,
    /// true when the user agreed that system services of the app may be stopped with the
    /// administrator password (macOS shows its dialog on the Mac).
    pub allow_admin: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OneApp {
    /// The app: name, bundle id or path.
    pub app: String,
}

fn find_app(env: &Env, app: &str) -> Result<apps::AppInfo, String> {
    let list = apps::inventory(env, false, &|_| {});
    apps::find(env, &list, app)
        .cloned()
        .ok_or_else(|| format!("no app named '{app}' (see list_apps)"))
}

fn running_json(env: &Env, st: &app_quit::RunningState) -> Value {
    json!({
        "app": st.app_name,
        "path": env.display(&st.app_path),
        "running": !st.is_empty(),
        "blocks_uninstall": st.locked(),
        "headline": st.headline(),
        "parts": st.running().map(|p| json!({"name": p.name, "kind": p.kind, "pid": p.pid, "needs_admin": p.needs_admin(st.me)})).collect::<Vec<_>>(),
        "admin_question": st.admin_question(),
        "notes": st.notes,
    })
}

pub fn app_running_json(env: &Env, app: &str) -> Result<Value, String> {
    let a = find_app(env, app)?;
    let st = app_quit::running_parts(env, &a.path);
    let mut v = running_json(env, &st);
    v["next"] = json!(if st.is_empty() {
        "Nothing of it runs."
    } else {
        "Tell the user what runs and ask before calling quit_app. If admin_question is set, also ask whether macOS may show the administrator dialog (allow_admin)."
    });
    Ok(v)
}

pub fn uninstall_apps_preview_json(
    env: &Env,
    settings: &Settings,
    names: &[String],
) -> Result<Value, String> {
    if names.is_empty() {
        return Err("Name at least one app.".into());
    }
    let list = apps::inventory(env, false, &|_| {});
    let mut paths = Vec::new();
    for n in names {
        let a = apps::find(env, &list, n)
            .ok_or_else(|| format!("no app named '{n}' (see list_apps)"))?;
        paths.push(a.path.clone());
    }
    let (plan, batch) =
        uninstaller::batch_plan(env, settings, &paths).map_err(|e| format!("{e:#}"))?;
    plans::save(env, &plan).map_err(|e| e.to_string())?;
    // Which item belongs to which app, so History can put back one app of the batch.
    uninstaller::save_batch(env, &plan, &batch).map_err(|e| e.to_string())?;
    let mut v = plan_view(env, &plan, 0, 1000);
    v["apps"] = json!(
        batch
            .iter()
            .map(|b| json!({"name": b.name, "path": env.display(&b.path), "running": b.running, "locked": b.lock}))
            .collect::<Vec<_>>()
    );
    v["preview"] = selection_view(env, settings, &plan);
    if batch.iter().any(|b| b.running) {
        v["running_note"] = json!(
            "Some of these apps run, so their files are locked. Ask the user, call quit_app for each, then call uninstall_apps_preview again."
        );
    }
    Ok(v)
}

pub fn quit_app_json(env: &Env, app: &str, force: bool, allow_admin: bool) -> ToolResult {
    gate(env)?;
    let a = find_app(env, app)?;
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
        app_quit::force_quit(env, &a.path, o)?
    } else {
        let mut o = app_quit::QuitOptions {
            allow_admin,
            ..Default::default()
        };
        if fast {
            o.wait = std::time::Duration::ZERO;
            o.poll = std::time::Duration::from_millis(1);
        }
        app_quit::quit_app(env, &a.path, o)?
    };
    let mut v = json!({
        "done": rep.done(),
        "message": rep.message(),
        "stopped": rep.stopped,
        "failed": rep.failed,
        "relaunched": rep.relaunched,
        "admin_needed": rep.admin_needed,
        "admin_cancelled": rep.admin_cancelled,
        "still_running": running_json(env, &rep.after),
        "can_force": rep.can_force(),
    });
    if allow_admin {
        v["admin_note"] = json!(ADMIN_PROMPT);
    }
    ok(v)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PidArg {
    /// The pid of a heavy app from list_startup_items.
    pub pid: u32,
}

pub fn quit_process_json(env: &Env, pid: u32) -> ToolResult {
    gate(env)?;
    let uid = tasks::current_uid(env)?;
    let procs = startup::list_processes(env, uid)?;
    let p = procs
        .iter()
        .find(|p| p.pid == pid)
        .ok_or_else(|| format!("no process {pid} of yours runs now (see list_startup_items)"))?;
    let r = startup::quit_process(env, p, uid).map_err(|e| e.to_string())?;
    ok(json!({"done": format!("Asked {} to quit.", p.name), "action_id": r.id}))
}

// ---------------------------------------------------------------------------------------------
// Trash, Mail, permissions

/// A short fingerprint of what Finder's Trash holds now: empty_trash refuses another one.
fn trash_token(env: &Env) -> String {
    let list = trash::trashes(env);
    let (bytes, entries) = list
        .iter()
        .filter(|t| !t.mail)
        .fold((0u64, 0usize), |(b, e), t| (b + t.size, e + t.entries));
    format!("{bytes}-{entries}")
}

pub fn trash_status_json(env: &Env) -> Value {
    let list = trash::trashes(env);
    json!({
        "trashes": list.iter().map(|t| json!({
            "label": t.label, "path": env.display(&t.path), "size": human_size(t.size), "bytes": t.size,
            "entries": t.entries, "readable": t.readable, "mail": t.mail,
            "mail_account": t.account, "mailbox": t.mailbox, "on_server": t.on_server,
        })).collect::<Vec<_>>(),
        "finder_trash": human_size(trash::finder_trash_size(env)),
        "empty_token": trash_token(env),
        "next": "Emptying the Trash is permanent. Tell the user the size and number of items, get a clear yes, then call empty_trash with empty_token. Mail's deleted messages are erased per account with erase_mail_deleted. To move things into the Trash instead, scan trash_bins.",
    })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TokenArg {
    /// empty_token from trash_status.
    pub token: String,
}

pub fn empty_trash_json(env: &Env, token: &str) -> ToolResult {
    gate(env)?;
    if token != trash_token(env) {
        return Err("Refused: the Trash changed since trash_status. Call trash_status again and show the user the new size.".into());
    }
    let out = trash::empty_trash_in_finder(env);
    let v = json!({"ok": out.ok, "freed": human_size(out.freed), "freed_bytes": out.freed, "message": out.message});
    if out.ok { ok(v) } else { Err(out.message) }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AccountArg {
    /// The Mail account as trash_status shows it (mail_account).
    pub account: String,
}

pub fn erase_mail_deleted_json(env: &Env, account: &str) -> ToolResult {
    gate(env)?;
    let known = trash::trashes(env)
        .into_iter()
        .any(|t| t.mail && t.account.as_deref() == Some(account));
    if !known {
        return Err(format!(
            "Refused: '{account}' is not a Mail account listed by trash_status."
        ));
    }
    let out = trash::erase_in_mail(env, Some(account));
    ok(json!({
        "ok": out.ok, "message": out.message, "opened_mail": out.opened_mail,
        "needs_accessibility": out.needs_accessibility, "needs_automation": out.needs_automation,
        "note": "Mail may ask the user to confirm in its own window, on the Mac.",
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PermissionArgs {
    /// The plan_id of a privacy scan.
    pub plan_id: String,
    /// Ids of its "permissions" items to reset.
    pub item_ids: Vec<usize>,
}

pub fn reset_app_permissions_json(env: &Env, plan_id: &str, ids: &[usize]) -> ToolResult {
    gate(env)?;
    let plan = plans::load(env, plan_id).map_err(|e| e.to_string())?;
    if plan.feature != Some(FeatureId::Privacy) {
        return Err("Refused: this is not a privacy plan (scan privacy first).".into());
    }
    if ids.is_empty() {
        return Err("Name at least one permission item.".into());
    }
    let mut picked = Vec::new();
    for id in ids {
        let it = plan
            .items
            .iter()
            .find(|i| i.id == *id && i.category == "permissions")
            .ok_or_else(|| format!("Refused: item {id} is not an app permission of this plan."))?;
        let (svc, client) = privacy::permission_of(env, &it.path)
            .ok_or_else(|| format!("Refused: item {id} is not an app permission."))?;
        picked.push((svc, client));
    }
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for (svc, client) in picked {
        match privacy::reset_permission(env, &svc, &client) {
            Ok(m) => done.push(m),
            Err(e) => failed.push(e),
        }
    }
    ok(json!({"reset": done, "failed": failed}))
}

// ---------------------------------------------------------------------------------------------
// Quarantine

pub fn quarantine_status_json(env: &Env) -> Value {
    let list = executor::quarantine_list(env);
    let total: u64 = list.iter().map(|(_, s)| s).sum();
    json!({
        "items": list.iter().map(|(p, s)| json!({"path": env.display(p), "size": human_size(*s), "bytes": s})).collect::<Vec<_>>(),
        "total": human_size(total),
        "total_bytes": total,
        "next": "To move items to the Trash, show the user the list and call quarantine_to_trash with the paths they agreed to.",
    })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PathsArg {
    /// Paths as quarantine_status shows them.
    pub paths: Vec<String>,
}

pub fn quarantine_to_trash_json(env: &Env, paths: &[String]) -> ToolResult {
    gate(env)?;
    let list = executor::quarantine_list(env);
    let mut confirmed = Vec::new();
    for p in paths {
        let path = resolve_path(env, p);
        let found = list
            .iter()
            .find(|(q, _)| *q == path || env.display(q) == p.trim())
            .ok_or_else(|| {
                format!("Refused: {p} is not in the quarantine (see quarantine_status).")
            })?;
        confirmed.push(found.0.clone());
    }
    let rep = executor::move_quarantine_items_to_trash(env, &confirmed, &mut |_, _| {})
        .map_err(|e| e.to_string())?;
    ok(report_json(env, &rep))
}

/// A report as agents get it: moved, skipped with reasons, size, undo id.
pub fn report_json(env: &Env, rep: &executor::Report) -> Value {
    json!({
        "freed": human_size(rep.freed), "freed_bytes": rep.freed,
        "message": features::moved_text([rep]),
        "space_note": features::SPACE_NOTE,
        "moved": rep.moved.iter().map(|r| {
            let (path, to) = match &r.kind {
                executor::ActionKind::Moved { original, stored, .. } => (Some(env.display(original)), Some(env.display(stored))),
                _ => (None, None),
            };
            json!({"action_id": r.id, "path": path, "now_at": to, "size": human_size(r.size), "bytes": r.size})
        }).collect::<Vec<_>>(),
        "skipped": rep.skipped.iter().map(|(p, s)| json!({"path": env.display(p), "reason": s.to_string()})).collect::<Vec<_>>(),
        "undo_with": rep.plan_id,
    })
}

// ---------------------------------------------------------------------------------------------
// Settings

pub fn get_settings_json(env: &Env) -> Value {
    let (s, warning) = Settings::load_checked(env);
    json!({
        "agents_may_clean": s.agents_may_clean,
        "method": s.method,
        "ignore": s.ignore.iter().map(|p| env.display(p)).collect::<Vec<_>>(),
        "large_min_bytes": s.large_min_bytes,
        "old_days": s.old_days,
        "downloads_days": s.downloads_days,
        "keep_languages": s.keep_languages,
        "sweep_skip": s.sweep_skip,
        "dev_roots": s.dev_roots.iter().map(|p| env.display(p)).collect::<Vec<_>>(),
        "dev_inactive_days": s.dev_inactive_days,
        "scan_every": s.scan_every,
        "scan_hour": s.scan_hour,
        "scan_weekday": s.scan_weekday,
        "disk_low_percent": s.disk_low_percent,
        "notifications_paused": s.notifications_paused,
        "watch_trash_offer": s.watch_trash_offer,
        "watch_new_items": s.watch_new_items,
        "update_check_auto": s.update_check_auto,
        "update_install_auto": s.update_install_auto,
        "sounds": s.sounds,
        "menu_bar_agent_on": login_agent::is_on(env),
        "settings_warning": warning,
    })
}

/// Settings an agent may change. The AI assistants switch is not here on purpose.
#[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SettingsArgs {
    /// Where removed items go: "trash" or "quarantine".
    pub method: Option<String>,
    /// Large & Old Files: minimum size in bytes (at least 1 MB).
    pub large_min_bytes: Option<u64>,
    /// Large & Old Files: days without opening that make a file old (1 to 3650).
    pub old_days: Option<u64>,
    /// Unused disk images and old downloads: days (1 to 3650).
    pub downloads_days: Option<u64>,
    /// Languages to keep besides English and Base, e.g. ["it", "es"].
    pub keep_languages: Option<Vec<String>>,
    /// Quick Sweep modules to leave out (feature ids).
    pub sweep_skip: Option<Vec<String>>,
    /// Developer Clutter: code folders to look in besides the usual ones.
    pub dev_roots: Option<Vec<String>>,
    /// Developer Clutter: days without changes that make a project inactive (1 to 3650).
    pub dev_inactive_days: Option<u64>,
    /// Scheduled scans (look only): "off", "daily" or "weekly".
    pub scan_every: Option<String>,
    /// Hour of scheduled scans, 0 to 23.
    pub scan_hour: Option<u8>,
    /// Day of weekly scans, 1 (Monday) to 7 (Sunday).
    pub scan_weekday: Option<u8>,
    /// Warn below this percent free on the startup disk, 0 to 50 (0: never).
    pub disk_low_percent: Option<u8>,
    pub notifications_paused: Option<bool>,
    pub watch_trash_offer: Option<bool>,
    pub watch_new_items: Option<bool>,
    pub update_check_auto: Option<bool>,
    pub update_install_auto: Option<bool>,
    pub sounds: Option<bool>,
}

fn in_range<T: PartialOrd + std::fmt::Display + Copy>(
    name: &str,
    v: T,
    lo: T,
    hi: T,
) -> Result<T, String> {
    if v < lo || v > hi {
        Err(format!("{name} must be from {lo} to {hi}"))
    } else {
        Ok(v)
    }
}

/// A code folder an agent asks for, checked like the app does.
fn dev_root(env: &Env, raw: &str) -> Result<PathBuf, String> {
    let p = resolve_path(env, raw);
    let md = std::fs::symlink_metadata(&p).map_err(|_| format!("Nothing is at {raw}."))?;
    if !md.is_dir() {
        return Err(format!("{raw} is not a folder (or it is a link)."));
    }
    if cleancraft_core::policy::starts_with_ci(env.home(), &p) {
        return Err(format!(
            "{raw} holds the whole home folder. Name the folders that hold the projects."
        ));
    }
    Ok(p)
}

pub fn update_settings_json(env: &Env, a: &SettingsArgs) -> ToolResult {
    let mut s = gate(env)?;
    let before = get_settings_json(env);
    if let Some(m) = &a.method {
        s.method = match m.to_ascii_lowercase().as_str() {
            "trash" => Method::Trash,
            "quarantine" => Method::Quarantine,
            _ => return Err("method must be \"trash\" or \"quarantine\"".into()),
        };
    }
    if let Some(v) = a.large_min_bytes {
        s.large_min_bytes = in_range("large_min_bytes", v, 1_000_000, u64::MAX)?;
    }
    if let Some(v) = a.old_days {
        s.old_days = in_range("old_days", v, 1, 3650)?;
    }
    if let Some(v) = a.downloads_days {
        s.downloads_days = in_range("downloads_days", v, 1, 3650)?;
    }
    if let Some(v) = &a.keep_languages {
        s.keep_languages = v.clone();
    }
    if let Some(v) = &a.sweep_skip {
        let mut ids = Vec::new();
        for f in v {
            ids.push(FeatureId::parse(f).ok_or_else(|| format!("unknown feature '{f}'"))?);
        }
        s.sweep_skip = ids;
    }
    if let Some(v) = &a.dev_roots {
        let mut roots = Vec::new();
        for r in v {
            let p = dev_root(env, r)?;
            if !roots.contains(&p) {
                roots.push(p);
            }
        }
        s.dev_roots = roots;
    }
    if let Some(v) = a.dev_inactive_days {
        s.dev_inactive_days = in_range("dev_inactive_days", v, 1, 3650)?;
    }
    if let Some(v) = &a.scan_every {
        s.scan_every = match v.to_ascii_lowercase().as_str() {
            "off" => ScanEvery::Off,
            "daily" => ScanEvery::Daily,
            "weekly" => ScanEvery::Weekly,
            _ => return Err("scan_every must be \"off\", \"daily\" or \"weekly\"".into()),
        };
    }
    if let Some(v) = a.scan_hour {
        s.scan_hour = in_range("scan_hour", v, 0, 23)?;
    }
    if let Some(v) = a.scan_weekday {
        s.scan_weekday = in_range("scan_weekday", v, 1, 7)?;
    }
    if let Some(v) = a.disk_low_percent {
        s.disk_low_percent = in_range("disk_low_percent", v, 0, 50)?;
    }
    let flags: [(Option<bool>, &mut bool); 6] = [
        (a.notifications_paused, &mut s.notifications_paused),
        (a.watch_trash_offer, &mut s.watch_trash_offer),
        (a.watch_new_items, &mut s.watch_new_items),
        (a.update_check_auto, &mut s.update_check_auto),
        (a.update_install_auto, &mut s.update_install_auto),
        (a.sounds, &mut s.sounds),
    ];
    for (v, slot) in flags {
        if let Some(v) = v {
            *slot = v;
        }
    }
    s.save(env)
        .map_err(|e| format!("could not save the settings: {e}"))?;
    ok(json!({
        "saved": true,
        "before": before,
        "now": get_settings_json(env),
        "undo": "Call update_settings again with the values in before.",
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IgnoreArgs {
    /// Paths to add to the ignore list (scans never propose them or anything inside).
    pub add: Option<Vec<String>>,
    /// Paths to take off the ignore list, as get_settings shows them.
    pub remove: Option<Vec<String>>,
}

pub fn ignore_list_json(env: &Env, a: &IgnoreArgs) -> ToolResult {
    let mut s = gate(env)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for raw in a.add.iter().flatten() {
        let p = resolve_path(env, raw);
        if std::fs::symlink_metadata(&p).is_err() {
            return Err(format!("Nothing is at {raw}. Nothing changed."));
        }
        if !s.ignore.contains(&p) {
            s.ignore.push(p.clone());
            added.push(env.display(&p));
        }
    }
    for raw in a.remove.iter().flatten() {
        let p = resolve_path(env, raw);
        let at = s
            .ignore
            .iter()
            .position(|i| *i == p || env.display(i) == raw.trim())
            .ok_or_else(|| format!("{raw} is not on the ignore list. Nothing changed."))?;
        removed.push(env.display(&s.ignore.remove(at)));
    }
    s.save(env)
        .map_err(|e| format!("could not save the settings: {e}"))?;
    ok(json!({
        "added": added,
        "removed": removed,
        "ignore": s.ignore.iter().map(|p| env.display(p)).collect::<Vec<_>>(),
        "note": "Scans made before this change still list what they found; scan again.",
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OnArg {
    /// true turns it on, false turns it off.
    pub on: bool,
}

/// The program the menu bar agent starts: the installed app's agent (a fixed path on the fake
/// Mac of the tests), the same choice as the Settings page.
fn agent_program(env: &Env) -> PathBuf {
    let fixed = env.sys_path("/Applications/CleanCraft.app/Contents/MacOS/CleanCraft");
    if env.is_fake() {
        return fixed;
    }
    match std::env::current_exe() {
        Ok(exe) => login_agent::agent_program(&exe),
        Err(_) => fixed,
    }
}

pub fn menu_bar_agent_json(env: &Env, on: bool) -> ToolResult {
    gate(env)?;
    if login_agent::is_on(env) == on {
        return ok(
            json!({"done": if on { "The menu bar agent is already on." } else { "The menu bar agent is already off." }}),
        );
    }
    let ch = if on {
        login_agent::turn_on(env, &agent_program(env))?
    } else {
        login_agent::turn_off(env)?
    };
    ok(json!({
        "done": if on { "CleanCraft is in the menu bar now and starts there at login." } else { "CleanCraft left the menu bar. Its login item is in the Trash." },
        "note": ch.note,
        "undo_with": ch.record.id,
    }))
}

// ---------------------------------------------------------------------------------------------
// Quick Sweep, updates

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConfirmedPlan {
    pub plan_id: String,
    /// The digest the user confirmed (from sweep_preview, get_plan or select_items).
    pub digest: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SweepRunArgs {
    /// The confirmed plans of the Quick Sweep modules to clean.
    pub runs: Vec<ConfirmedPlan>,
}

pub fn sweep_run_json(env: &Env, runs: &[ConfirmedPlan]) -> ToolResult {
    let settings = gate(env)?;
    if runs.is_empty() {
        return Err("Name at least one confirmed plan.".into());
    }
    let sources = Sources::registered();
    let mut confirmed = Vec::new();
    for r in runs {
        let plan = plans::load(env, &r.plan_id).map_err(|e| e.to_string())?;
        let fid = plan.feature.ok_or("the plan has no feature")?;
        if sources.feature(fid).is_none() {
            return Err(format!(
                "Refused: {} is not a Quick Sweep module.",
                fid.title()
            ));
        }
        if plan.digest() != r.digest {
            return Err(format!(
                "Refused: the digest of {} does not match its plan. Show the user the current list (get_plan) and use its digest. Nothing ran.",
                r.plan_id
            ));
        }
        confirmed.push((plan, r.digest.clone()));
    }
    let out = sweep::run(env, &settings, &sources, &confirmed, &|_| {});
    let reports: Vec<&executor::Report> =
        out.iter().filter_map(|m| m.result.as_ref().ok()).collect();
    ok(json!({
        "message": features::moved_text(reports.iter().copied()),
        "modules": out.iter().map(|m| match &m.result {
            Ok(rep) => { let mut v = report_json(env, rep); v["feature"] = json!(m.id); v }
            Err(e) => json!({"feature": m.id, "error": e}),
        }).collect::<Vec<_>>(),
    }))
}

pub fn install_update_json(env: &Env, current: &str) -> ToolResult {
    gate(env)?;
    let found =
        self_update::check_against(env, current).map_err(|e| self_update::check_failed_text(&e))?;
    let self_update::Found::Available { release } = found else {
        return ok(json!({"installed": false, "message": found.message(current)}));
    };
    let app = self_update::installed_app(env).ok_or_else(|| {
        format!(
            "This copy of CleanCraft does not run from an app bundle. Download the release from {}.",
            self_update::RELEASES_PAGE
        )
    })?;
    if self_update::app_running(env, &app) {
        return Err("Refused: the CleanCraft app is open and may be cleaning. Ask the user to quit it, or to install from Settings, Updates in the app.".into());
    }
    let target = self_update::Target {
        app,
        current: current.to_string(),
        busy: None,
    };
    let done = self_update::install(
        env,
        &release,
        &target,
        &cleancraft_core::progress::Progress::new(),
    )?;
    ok(
        json!({"installed": true, "message": done.message(), "note": done.note, "history": done.record}),
    )
}

// ---------------------------------------------------------------------------------------------
// Tools

#[tool_router(router = control_router, vis = "pub")]
impl CleanCraftMcp {
    #[tool(
        description = "Set the selection of a saved plan to exactly the items you name (item_ids and/or paths), or select_all, or clear. Any item of the plan that is not locked can be chosen, also ones not ticked by default. Saves the plan and returns the full preview of the selection (each item's path, size, reason, risk, whether it needs administrator rights, the totals) and the NEW digest. Show that preview to the user and get a clear yes before calling execute_plan with the new digest; the old digest no longer works."
    )]
    async fn select_items(&self, Parameters(a): Parameters<SelectArgs>) -> ToolResult {
        select_items(&self.env, &a).and_then(ok)
    }

    #[tool(
        description = "Whether the user lets AI agents change the Mac ('Let AI assistants clean after you confirm' in CleanCraft Settings) and how the user turns it on. Agents cannot change this setting. Read only."
    )]
    async fn agent_permissions(&self) -> ToolResult {
        ok(agent_permissions_json(&self.env))
    }

    #[tool(
        description = "Every action an agent can take in CleanCraft: the tool, what it does, whether it changes the Mac (then it needs the user's setting), the look-only step to show first, and how to undo it. Read only."
    )]
    async fn list_actions(&self) -> ToolResult {
        ok(list_actions_json())
    }

    #[tool(
        description = "Preview uninstalling several apps at once (names, bundle ids or paths): each app's bundle and leftovers in one plan, with plan_id and digest. Running apps stay in the plan with their files locked (see app_running and quit_app). Show the preview to the user; adjust with select_items; only after a yes, call execute_plan."
    )]
    async fn uninstall_apps_preview(&self, Parameters(a): Parameters<AppsArg>) -> ToolResult {
        uninstall_apps_preview_json(&self.env, &Settings::load(&self.env), &a.apps).and_then(ok)
    }

    #[tool(
        description = "What of an app runs now (the app, helpers, agents, system services), whether it blocks uninstalling, and whether stopping it needs the administrator password. Read only."
    )]
    async fn app_running(&self, Parameters(a): Parameters<OneApp>) -> ToolResult {
        app_running_json(&self.env, &a.app).and_then(ok)
    }

    #[tool(
        description = "Quit an app and its background helpers (for example before uninstalling it), after the user agreed. force=true stops what ignored the polite quit. allow_admin=true only when the user agreed that macOS may show the administrator dialog on the Mac for the app's system services. Logged in History. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn quit_app(&self, Parameters(a): Parameters<QuitArg>) -> ToolResult {
        quit_app_json(
            &self.env,
            &a.app,
            a.force.unwrap_or(false),
            a.allow_admin.unwrap_or(false),
        )
    }

    #[tool(
        description = "Quit one heavy app listed by list_startup_items (by pid), after the user agreed. Never macOS or CleanCraft. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn quit_process(&self, Parameters(a): Parameters<PidArg>) -> ToolResult {
        quit_process_json(&self.env, a.pid)
    }

    #[tool(
        description = "The Trash bins (Finder's and Mail's deleted messages per account) with sizes, plus empty_token for empty_trash. Read only."
    )]
    async fn trash_status(&self) -> ToolResult {
        ok(trash_status_json(&self.env))
    }

    #[tool(
        description = "Ask Finder to empty the Trash. PERMANENT: emptied items cannot be put back. Only after the user saw the size from trash_status and said yes; pass its empty_token (refused if the Trash changed). Needs the CleanCraft setting that lets agents clean."
    )]
    async fn empty_trash(&self, Parameters(a): Parameters<TokenArg>) -> ToolResult {
        empty_trash_json(&self.env, &a.token)
    }

    #[tool(
        description = "Ask Mail to erase the deleted messages of one account (Mailbox, Erase Deleted Items), after the user agreed. Permanent; Mail may ask again in its own window. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn erase_mail_deleted(&self, Parameters(a): Parameters<AccountArg>) -> ToolResult {
        erase_mail_deleted_json(&self.env, &a.account)
    }

    #[tool(
        description = "Take back access given to apps (camera, microphone, screen, files...) with tccutil reset: items of category permissions from a privacy scan, after the user agreed. Each app asks again next time. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn reset_app_permissions(&self, Parameters(a): Parameters<PermissionArgs>) -> ToolResult {
        reset_app_permissions_json(&self.env, &a.plan_id, &a.item_ids)
    }

    #[tool(description = "What CleanCraft's quarantine holds, with sizes. Read only.")]
    async fn quarantine_status(&self) -> ToolResult {
        ok(quarantine_status_json(&self.env))
    }

    #[tool(
        description = "Move items of the quarantine (paths from quarantine_status) to the Trash, after the user agreed. Undo with undo_with. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn quarantine_to_trash(&self, Parameters(a): Parameters<PathsArg>) -> ToolResult {
        quarantine_to_trash_json(&self.env, &a.paths)
    }

    #[tool(
        description = "CleanCraft's settings: where removed items go, ignore list, sizes and ages, scheduled scans, notifications, update checks, and whether the menu bar agent is on. Read only."
    )]
    async fn get_settings(&self) -> ToolResult {
        ok(get_settings_json(&self.env))
    }

    #[tool(
        description = "Change CleanCraft settings the user asked for (only the fields given): scheduled scans, where removed items go, sizes and ages, Quick Sweep modules, code folders, notifications, update checks. Returns the values before and after. The AI assistants switch cannot be changed here. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn update_settings(&self, Parameters(a): Parameters<SettingsArgs>) -> ToolResult {
        update_settings_json(&self.env, &a)
    }

    #[tool(
        description = "Add paths to or take paths off the ignore list (scans never propose what is on it), as the user asked. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn ignore_list(&self, Parameters(a): Parameters<IgnoreArgs>) -> ToolResult {
        ignore_list_json(&self.env, &a)
    }

    #[tool(
        description = "Turn the CleanCraft menu bar agent (and its login item) on or off, as the user asked. Undo with undo_with. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn menu_bar_agent(&self, Parameters(a): Parameters<OnArg>) -> ToolResult {
        menu_bar_agent_json(&self.env, a.on)
    }

    #[tool(
        description = "Run several confirmed Quick Sweep plans (from sweep_preview, adjusted with select_items) in one go, each with the digest the user confirmed. A module that fails does not stop the others. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn sweep_run(&self, Parameters(a): Parameters<SweepRunArgs>) -> ToolResult {
        sweep_run_json(&self.env, &a.runs)
    }

    #[tool(
        description = "Install the newer CleanCraft from GitHub (same checks as the app: checksum, bundle id, signature) after the user agreed. Refused while the CleanCraft app is open. The old version goes to the Trash with Undo in History. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn install_update(&self) -> ToolResult {
        install_update_json(&self.env, self_update::CURRENT)
    }

    #[tool(
        description = "Open System Settings at Full Disk Access on the Mac so the user can turn it on for CleanCraft. Changes nothing by itself."
    )]
    async fn open_full_disk_access_settings(&self) -> ToolResult {
        cleancraft_core::permissions::open_fda_settings(&self.env)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                ok(json!({"opened": "System Settings, Privacy & Security, Full Disk Access"}))
            })
    }
}
