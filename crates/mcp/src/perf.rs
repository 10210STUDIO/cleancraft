//! MCP tools for Tune-up Tasks and Startup Items.
//!
//! Listing is free. Running a task or turning a startup item off or on changes the Mac, so it
//! needs the same setting as cleaning ("Let AI agents carry out confirmed cleanups"). Tasks
//! that need the administrator password still show the macOS password dialog to the user,
//! who can cancel it. Every change is logged; startup changes can be undone with `undo`.

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::json;

use cleancraft_core::startup::{self, Domain};
use cleancraft_core::tasks::{self, TASKS};
use cleancraft_core::{Env, Settings};

use crate::{CleanCraftMcp, ToolResult, ok};

pub const REFUSED: &str = "Refused: in CleanCraft Settings, 'Let AI agents (MCP) carry out confirmed cleanups' is off. Ask the user to do this in the app, or to turn that on.";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskArg {
    /// Task id from list_tasks, e.g. "flush_dns".
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ToggleArg {
    /// Item id from list_startup_items, e.g. "user_agents:com.example.helper" or
    /// "login:Dropbox".
    pub id: String,
    /// true turns it on, false turns it off.
    pub on: bool,
}

pub fn list_tasks(env: &Env) -> ToolResult {
    let last = tasks::last_runs(env);
    ok(TASKS
        .iter()
        .map(|t| {
            let ready = tasks::prepare(env, t.id);
            json!({
                "id": t.id,
                "title": t.title,
                "description": t.description,
                "commands": match &ready { Ok(p) => p.commands(), Err(_) => vec![t.command.to_string()] },
                "needs_admin_password": t.admin,
                "risk": t.risk,
                "available": ready.is_ok(),
                "why_not": ready.err(),
                "last_run": last.get(t.id).map(|(ts, ok)| json!({"time": ts, "ok": ok})),
            })
        })
        .collect::<Vec<_>>())
}

pub fn run_task(env: &Env, id: &str) -> ToolResult {
    if !Settings::load(env).agents_may_clean {
        return Err(REFUSED.into());
    }
    let r = tasks::run_by_id(env, id)?;
    ok(r)
}

pub fn list_startup_items(env: &Env) -> ToolResult {
    let uid = tasks::current_uid(env).ok();
    let login = startup::list_login_items(env);
    let (launch, notes) = startup::list_launch_items(env, uid);
    let heavy = uid
        .ok_or_else(|| "could not read the user id".to_string())
        .and_then(|u| startup::list_processes(env, u))
        .map(|v| startup::heavy_apps(&v));
    ok(json!({
        "login_items": match &login {
            Ok(v) => json!(v.iter().map(|l| json!({"id": format!("login:{}", l.name), "name": l.name, "path": l.path, "enabled": true})).collect::<Vec<_>>()),
            Err(e) => json!({"error": e}),
        },
        "removed_login_items": startup::removed_login_items(env, login.as_deref().unwrap_or(&[])).iter()
            .map(|(_, l)| json!({"id": format!("login:{}", l.name), "name": l.name, "path": l.path, "enabled": false})).collect::<Vec<_>>(),
        "launch_items": launch.iter().map(|i| json!({
            "id": i.id(), "label": i.label, "kind": i.domain, "plist": i.plist_shown,
            "program": i.program, "program_exists": i.program_exists, "signed_by": i.signer.label(),
            "enabled": i.enabled, "part_of_macos": i.apple,
            "needs_admin_password": i.domain.admin(),
        })).collect::<Vec<_>>(),
        "notes": notes,
        "heavy_apps": match heavy {
            Ok(v) => json!(v),
            Err(e) => json!({"error": e}),
        },
    }))
}

pub fn toggle_startup_item(env: &Env, id: &str, on: bool) -> ToolResult {
    if !Settings::load(env).agents_may_clean {
        return Err(REFUSED.into());
    }
    if let Some(name) = id.strip_prefix("login:") {
        let current = startup::list_login_items(env)?;
        if on {
            let removed = startup::removed_login_items(env, &current);
            let (rec, _) = removed
                .iter()
                .find(|(_, l)| l.name == name)
                .ok_or_else(|| format!("'{name}' is not a login item CleanCraft turned off"))?;
            let r = cleancraft_core::executor::undo(env, rec).map_err(|e| e.to_string())?;
            return ok(json!({"done": format!("{name} opens at login again"), "action_id": r.id}));
        }
        let item = current
            .iter()
            .find(|l| l.name == name)
            .ok_or_else(|| format!("no login item named '{name}'"))?;
        let r = startup::remove_login_item(env, item).map_err(|e| e.to_string())?;
        return ok(json!({"done": format!("{name} no longer opens at login"), "undo_with": r.id}));
    }
    let uid = tasks::current_uid(env)?;
    let (items, _) = startup::list_launch_items(env, Some(uid));
    let item = items
        .iter()
        .find(|i| i.id() == id)
        .ok_or_else(|| format!("no startup item '{id}' (see list_startup_items)"))?;
    let r = startup::set_launch_item_enabled(env, item, uid, on).map_err(|e| e.to_string())?;
    ok(json!({
        "done": format!("{} {}", if on { "Turned on" } else { "Turned off" }, item.label),
        "needed_admin_password": item.domain == Domain::Daemon,
        "undo_with": r.id,
    }))
}

#[tool_router(router = perf_router, vis = "pub")]
impl CleanCraftMcp {
    #[tool(
        description = "List Tune-up Tasks (maintenance commands that ship with macOS) with the exact commands, whether each can run now and why not, and the last run."
    )]
    async fn list_tasks(&self) -> ToolResult {
        list_tasks(&self.env)
    }

    #[tool(
        description = "Run one Tune-up Task by id after the user agreed to the exact command shown by list_tasks. Tasks that need the administrator password show the macOS password dialog to the user. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn run_task(&self, Parameters(a): Parameters<TaskArg>) -> ToolResult {
        run_task(&self.env, &a.id)
    }

    #[tool(
        description = "List login items, launch agents and daemons (program, signer, on or off, part of macOS) and the apps using a lot of CPU or memory."
    )]
    async fn list_startup_items(&self) -> ToolResult {
        list_startup_items(&self.env)
    }

    #[tool(
        description = "Turn a startup item off or on by id from list_startup_items. Undoable with undo. Items that are part of macOS are refused. Needs the CleanCraft setting that lets agents clean."
    )]
    async fn toggle_startup_item(&self, Parameters(a): Parameters<ToggleArg>) -> ToolResult {
        toggle_startup_item(&self.env, &a.id, a.on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::testkit_perf as tp;

    fn allow_agents(env: &Env) {
        let s = Settings {
            agents_may_clean: true,
            ..Settings::default()
        };
        s.save(env).unwrap();
    }

    #[test]
    fn tools_are_registered() {
        let m = tp::startup_mac();
        let s = CleanCraftMcp::new(m.env.clone());
        let names: Vec<String> = s
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in [
            "list_tasks",
            "run_task",
            "list_startup_items",
            "toggle_startup_item",
            "scan",
        ] {
            assert!(names.contains(&n.to_string()), "{n} missing: {names:?}");
        }
    }

    #[test]
    fn tasks_listed_and_run_only_when_allowed() {
        let m = tp::startup_mac();
        let v: serde_json::Value = serde_json::from_str(&list_tasks(&m.env).unwrap()).unwrap();
        let flush = v
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "flush_dns")
            .unwrap();
        assert_eq!(flush["available"], true);
        assert_eq!(
            flush["commands"][0],
            "dscacheutil -flushcache; killall -HUP mDNSResponder"
        );
        let periodic = v
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "periodic_scripts")
            .unwrap();
        assert_eq!(periodic["available"], false);
        let e = run_task(&m.env, "flush_dns").unwrap_err();
        assert!(e.starts_with("Refused"));
        assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
        allow_agents(&m.env);
        let r: serde_json::Value =
            serde_json::from_str(&run_task(&m.env, "flush_dns").unwrap()).unwrap();
        assert_eq!(r["outcome"], "done");
        assert_eq!(m.env.fake_sys().unwrap().admin_calls().len(), 1);
        m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
        let r: serde_json::Value =
            serde_json::from_str(&run_task(&m.env, "free_ram").unwrap()).unwrap();
        assert_eq!(r["outcome"], "cancelled");
        assert_eq!(r["message"], "Cancelled, nothing changed");
    }

    #[test]
    fn startup_items_listed_and_toggled_only_when_allowed() {
        let m = tp::startup_mac();
        let v: serde_json::Value =
            serde_json::from_str(&list_startup_items(&m.env).unwrap()).unwrap();
        assert_eq!(v["launch_items"].as_array().unwrap().len(), 7);
        assert_eq!(v["login_items"].as_array().unwrap().len(), 2);
        assert_eq!(v["heavy_apps"].as_array().unwrap().len(), 3);
        assert!(toggle_startup_item(&m.env, "user_agents:com.example.helper", false).is_err());
        allow_agents(&m.env);
        let r = toggle_startup_item(&m.env, "user_agents:com.example.helper", false).unwrap();
        assert!(r.contains("Turned off com.example.helper"));
        assert!(
            m.env
                .fake_sys()
                .unwrap()
                .calls()
                .contains(&"/bin/launchctl disable gui/501/com.example.helper".to_string())
        );
        let e = toggle_startup_item(&m.env, "global_agents:com.apple.sampled", false).unwrap_err();
        assert!(e.contains("part of macOS"));
        let e = toggle_startup_item(&m.env, "user_agents:nope", false).unwrap_err();
        assert!(e.contains("no startup item"));
    }
}
