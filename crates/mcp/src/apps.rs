//! MCP tools for apps: list them, preview an uninstall or reset (a saved plan with its digest,
//! carried out with the existing `execute_plan`, with the same gate), and check for updates.
//!
//! Wiring: the coordinator adds `+ Self::apps_router()` to the router in `CleanCraftMcp::new`.

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use cleancraft_core::apps;
use cleancraft_core::features::uninstaller;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::plans;
use cleancraft_core::updater;
use cleancraft_core::{Env, Settings};

use crate::{CleanCraftMcp, ToolResult, ok, plan_view};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AppArg {
    /// The app: name ("Editor"), bundle id ("com.example.Editor") or path.
    pub app: String,
    /// true: reset the app (remove its data, keep the app). Default false: uninstall.
    pub reset: Option<bool>,
}

pub fn list_apps_json(env: &Env) -> Value {
    let now = chrono_now();
    let list = apps::inventory(env, true, &|_| {});
    json!(
        list.iter()
            .map(|a| json!({
                "name": a.name,
                "path": env.display(&a.path),
                "bundle_id": a.bundle_id,
                "version": a.version,
                "size": human_size(a.size),
                "bytes": a.size,
                "last_opened": apps::last_used_text(a.last_used, now),
                "unused": a.unused(now),
                "store": a.store,
                "vendor": a.vendor,
                "part_of_macos": a.apple,
            }))
            .collect::<Vec<_>>()
    )
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn uninstall_preview_json(
    env: &Env,
    settings: &Settings,
    app: &str,
    reset: bool,
) -> Result<Value, String> {
    let list = apps::inventory(env, false, &|_| {});
    let a = apps::find(env, &list, app)
        .ok_or_else(|| format!("no app named '{app}' (see list_apps)"))?;
    let plan =
        uninstaller::uninstall_plan(env, settings, &a.path, reset).map_err(|e| format!("{e:#}"))?;
    plans::save(env, &plan).map_err(|e| e.to_string())?;
    let mut v = plan_view(env, &plan, 0, 500);
    v["app"] = json!(a.name);
    v["action"] = json!(if reset { "reset" } else { "uninstall" });
    Ok(v)
}

pub fn check_updates_json(env: &Env) -> Value {
    let list = apps::inventory(env, false, &|_| {});
    let r = updater::check_all(env, &list, &|_| {});
    json!({
        "updates": r.available().collect::<Vec<_>>(),
        "up_to_date": r.apps.iter().filter(|u| !u.available && u.error.is_none()).map(|u| json!({"name": u.name, "version": u.installed})).collect::<Vec<_>>(),
        "could_not_check": r.apps.iter().filter(|u| u.error.is_some()).map(|u| json!({"name": u.name, "error": u.error})).collect::<Vec<_>>(),
        "macos": r.macos,
        "macos_error": r.macos_error,
        "how_to_update": "CleanCraft does not install updates. The user installs them from the app itself (its Check for Updates menu), the App Store Updates page, or System Settings > Software Update.",
    })
}

#[tool_router(router = apps_router, vis = "pub(crate)")]
impl CleanCraftMcp {
    #[tool(
        description = "List installed apps with size, version, last opened, store, maker and whether they are part of macOS (never removable)."
    )]
    async fn list_apps(&self) -> ToolResult {
        ok(list_apps_json(&self.env))
    }

    #[tool(
        description = "Preview uninstalling (or resetting, with reset=true) one app: the app and the files it left, each with size, reason and whether it is selected. Returns plan_id and digest. Show the list to the user; only after they confirm, call execute_plan(plan_id, digest)."
    )]
    async fn uninstall_preview(&self, Parameters(a): Parameters<AppArg>) -> ToolResult {
        uninstall_preview_json(
            &self.env,
            &self.settings(),
            &a.app,
            a.reset.unwrap_or(false),
        )
        .and_then(ok)
    }

    #[tool(
        description = "Check apps (developer update feeds and the App Store) and macOS for newer versions. Looks only; installs nothing."
    )]
    async fn check_updates(&self) -> ToolResult {
        ok(check_updates_json(&self.env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::testkit::FakeMac;
    use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};

    #[test]
    fn preview_then_execute_with_the_existing_gate() {
        let mac = FakeMac::new();
        let editor = mac.app("Editor", "com.example.Editor", "1.0");
        let cache = mac.env.home_path("Library/Caches/com.example.Editor");
        mac.home_file("Library/Caches/com.example.Editor/c", 5_000);
        let v = list_apps_json(&mac.env);
        assert_eq!(v[0]["bundle_id"], "com.example.Editor");

        let p = uninstall_preview_json(&mac.env, &Settings::default(), "editor", false).unwrap();
        assert_eq!(p["selected_count"], 2);
        let plan_id = p["plan_id"].as_str().unwrap().to_string();
        let digest = p["digest"].as_str().unwrap().to_string();
        let mcp = CleanCraftMcp::new(mac.env.clone());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let run = |d: &str| {
            rt.block_on(mcp.execute_plan(Parameters(crate::ExecuteArgs {
                plan_id: plan_id.clone(),
                digest: d.to_string(),
                only_items: None,
            })))
        };
        // Agents may not clean until the user allows it.
        assert!(run(&digest).unwrap_err().contains("Refused"));
        assert!(editor.exists());
        let s = Settings {
            agents_may_clean: true,
            ..Default::default()
        };
        s.save(&mac.env).unwrap();
        assert!(run("bad").unwrap_err().contains("digest"));
        let out = run(&digest).unwrap();
        assert!(out.contains("undo_with"));
        assert!(!editor.exists() && !cache.exists());
        assert!(uninstall_preview_json(&mac.env, &s, "nothing", false).is_err());
    }

    #[test]
    fn updates_json() {
        let mac = FakeMac::new();
        mac.app_spec(&AppSpec {
            build: "1",
            feed: Some("https://e.com/c.xml"),
            ..AppSpec::new("Editor", "com.example.Editor", "1.0")
        });
        mac.web(
            "https://e.com/c.xml",
            &appcast(&[("2", "1.1", "<p>New</p>")]),
        );
        let v = check_updates_json(&mac.env);
        assert_eq!(v["updates"][0]["latest"], "1.1");
        assert_eq!(v["updates"][0]["notes"], "New");
        assert!(v["macos_error"].is_string());
    }
}
