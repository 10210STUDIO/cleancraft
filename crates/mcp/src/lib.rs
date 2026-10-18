//! MCP server: CleanCraft for AI agents, over stdio.
//!
//! Agents can scan and read plans freely. Carrying out a plan needs two things: the setting
//! "Let AI agents carry out confirmed cleanups" (off by default, set in the app), and the plan
//! digest returned by `scan`, which proves the agent acts on exactly the list it showed.
//! Removal is a move to the Trash or quarantine, logged, undoable.

pub mod perf;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use cleancraft_core::executor;
use cleancraft_core::features;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::permissions;
use cleancraft_core::plans;
use cleancraft_core::{Env, FeatureId, Settings};

pub mod apps;
pub mod monitor;

pub type ToolResult = Result<String, String>;

pub fn ok(v: impl Serialize) -> ToolResult {
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

const INSTRUCTIONS: &str = "CleanCraft cleans a Mac safely. Workflow: list_features, then scan(feature) which returns a plan_id, the items with sizes and reasons, and a digest. Show the list to the user. Only after the user confirms, call execute_plan(plan_id, digest). Nothing is deleted: items move to the Trash or CleanCraft's quarantine and undo puts them back. execute_plan is refused unless the user turned on 'Let AI agents carry out confirmed cleanups' in CleanCraft Settings.";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FeatureArg {
    /// Feature id from list_features, e.g. "system_junk".
    pub feature: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanArg {
    pub plan_id: String,
    /// First item to return (paging), default 0.
    pub offset: Option<usize>,
    /// How many items, default 200.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteArgs {
    pub plan_id: String,
    /// The digest scan returned for this plan.
    pub digest: String,
    /// Optional: only these item ids (from the plan). Default: the plan's own selection.
    pub only_items: Option<Vec<usize>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UndoArgs {
    /// An action id from history, or a plan id to undo a whole cleanup.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LimitArg {
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct CleanCraftMcp {
    pub env: Env,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl CleanCraftMcp {
    pub fn new(env: Env) -> Self {
        Self {
            env,
            tool_router: Self::tool_router()
                + Self::monitor_router()
                + Self::apps_router()
                + Self::perf_router(),
        }
    }

    fn settings(&self) -> Settings {
        Settings::load(&self.env)
    }
}

fn plan_view(
    env: &Env,
    plan: &cleancraft_core::Plan,
    offset: usize,
    limit: usize,
) -> serde_json::Value {
    json!({
        "plan_id": plan.id,
        "feature": plan.feature,
        "digest": plan.digest(),
        "total": human_size(plan.total_size()),
        "total_bytes": plan.total_size(),
        "selected": human_size(plan.selected_size()),
        "selected_bytes": plan.selected_size(),
        "selected_count": plan.selected_count(),
        "item_count": plan.items.len(),
        "categories": plan.categories.iter().map(|c| json!({
            "id": c.id, "title": c.title, "risk": c.risk,
            "size": human_size(plan.category_size(&c.id)), "count": plan.category_count(&c.id)})).collect::<Vec<_>>(),
        "notes": plan.notes,
        "items": plan.items.iter().skip(offset).take(limit).map(|i| json!({
            "id": i.id, "path": env.display(&i.path), "size": human_size(i.size), "bytes": i.size,
            "category": i.category, "reason": i.reason, "risk": i.risk,
            "selected": i.selected, "locked": i.locked})).collect::<Vec<_>>(),
    })
}

#[tool_router]
impl CleanCraftMcp {
    #[tool(description = "List CleanCraft features with ids. Plan-based ones can be scanned.")]
    async fn list_features(&self) -> ToolResult {
        let scannable: Vec<FeatureId> = features::registry().iter().map(|f| f.id()).collect();
        ok(FeatureId::ALL
            .iter()
            .map(|f| json!({"id": f.as_str(), "title": f.title(), "scan": scannable.contains(f)}))
            .collect::<Vec<_>>())
    }

    #[tool(
        description = "Scan one feature (look only, nothing changes). Returns plan_id, digest, totals, categories and the first 200 items."
    )]
    async fn scan(&self, Parameters(a): Parameters<FeatureArg>) -> ToolResult {
        let f = FeatureId::parse(&a.feature)
            .ok_or_else(|| format!("unknown feature '{}'", a.feature))?;
        let plan = features::scan(&self.env, &self.settings(), f).map_err(|e| format!("{e:#}"))?;
        plans::save(&self.env, &plan).map_err(|e| e.to_string())?;
        ok(plan_view(&self.env, &plan, 0, 200))
    }

    #[tool(description = "Read a saved plan, with paging over its items.")]
    async fn get_plan(&self, Parameters(a): Parameters<PlanArg>) -> ToolResult {
        let plan = plans::load(&self.env, &a.plan_id).map_err(|e| e.to_string())?;
        ok(plan_view(
            &self.env,
            &plan,
            a.offset.unwrap_or(0),
            a.limit.unwrap_or(200),
        ))
    }

    #[tool(
        description = "Carry out a plan the user confirmed: moves the selected items to the Trash or quarantine (never deletes). Needs the digest from scan and the CleanCraft setting that lets agents clean."
    )]
    async fn execute_plan(&self, Parameters(a): Parameters<ExecuteArgs>) -> ToolResult {
        let settings = self.settings();
        if !settings.agents_may_clean {
            return Err("Refused: in CleanCraft Settings, 'Let AI agents (MCP) carry out confirmed cleanups' is off. Ask the user to clean from the app, or to turn that on.".into());
        }
        let mut plan = plans::load(&self.env, &a.plan_id).map_err(|e| e.to_string())?;
        if plan.digest() != a.digest {
            return Err("Refused: the digest does not match this plan. Scan again and show the user the new list.".into());
        }
        if let Some(only) = &a.only_items {
            for i in 0..plan.items.len() {
                plan.set_selected(i, only.contains(&i));
            }
        }
        let digest = plan.digest();
        let rep = features::run_plan(&self.env, &settings, &plan, &digest, &mut |_, _| {})
            .map_err(|e| format!("{e:#}"))?;
        ok(json!({
            "freed": human_size(rep.freed), "freed_bytes": rep.freed,
            "message": features::moved_text([&rep]),
            "moved": rep.moved.iter().map(|r| json!({"action_id": r.id, "size": r.size, "kind": r.kind})).collect::<Vec<_>>(),
            "skipped": rep.skipped.iter().map(|(p, s)| json!({"path": self.env.display(p), "reason": s.to_string()})).collect::<Vec<_>>(),
            "undo_with": rep.plan_id,
        }))
    }

    #[tool(description = "Undo one action (action id) or a whole cleanup (plan id).")]
    async fn undo(&self, Parameters(a): Parameters<UndoArgs>) -> ToolResult {
        if a.id.contains('#') {
            executor::undo(&self.env, &a.id)
                .map_err(|e| e.to_string())
                .and_then(|r| ok(json!({"undone": r.id})))
        } else {
            let (n, errs) = executor::undo_plan(&self.env, &a.id);
            ok(
                json!({"undone": n, "errors": errs.iter().map(|(id, e)| json!({"id": id, "error": e.to_string()})).collect::<Vec<_>>()}),
            )
        }
    }

    #[tool(description = "Recent CleanCraft actions, newest first.")]
    async fn history(&self, Parameters(a): Parameters<LimitArg>) -> ToolResult {
        let mut h = executor::history(&self.env);
        h.reverse();
        h.truncate(a.limit.unwrap_or(50));
        ok(h)
    }

    #[tool(
        description = "Whether CleanCraft has Full Disk Access (needed for Mail, Safari and Trash data)."
    )]
    async fn full_disk_access(&self) -> ToolResult {
        ok(json!({"access": format!("{:?}", permissions::full_disk_access(&self.env))}))
    }
}

#[tool_handler]
impl ServerHandler for CleanCraftMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("cleancraft", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
