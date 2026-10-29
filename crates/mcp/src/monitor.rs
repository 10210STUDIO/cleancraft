//! MCP tools of the Live Monitor and Quick Sweep: `system_status` (a reading of CPU, memory,
//! storage, battery and network, with alerts) and `sweep_preview` (Quick Sweep's scan; each
//! module's plan is saved so the agent can show it and, after the user confirms, carry it out
//! with `execute_plan` and its digest). Both only look.
//!
//! The coordinator combines this router with the main one:
//! `tool_router: Self::tool_router() + Self::monitor_router()`.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use cleancraft_core::fsutil::human_size;
use cleancraft_core::monitor::{self, Sampler};
use cleancraft_core::sweep::{self, Sources, TileData};
use cleancraft_core::{Env, FeatureId, Settings, plans};

use crate::{CleanCraftMcp, ToolResult, ok};

/// Kept between calls so the second `system_status` call can report a network speed.
static SAMPLER: Mutex<Option<Sampler>> = Mutex::new(None);

/// One reading as JSON. `sampler` keeps the previous network counters.
pub fn system_status_value(env: &Env, sampler: &mut Sampler) -> Value {
    let snap = sampler.sample(env, monitor::now_secs());
    json!({
        "summary": monitor::menu_bar_title(&snap),
        "alerts": monitor::alerts(&snap),
        "reading": snap,
        "notes": if snap.network.is_none() {
            vec!["network speed needs two readings: call system_status again"]
        } else {
            vec![]
        },
    })
}

/// Quick Sweep's scan as JSON; every module plan is saved for `get_plan` and `execute_plan`.
pub fn sweep_preview_value(env: &Env, settings: &Settings, sources: &Sources) -> Value {
    let res = sweep::scan(env, settings, sources, &AtomicBool::new(false), &|_| {});
    let skip = &settings.sweep_skip;
    let tiles: Vec<Value> = res
        .tiles
        .iter()
        .map(|t| {
            let mut v = json!({
                "feature": t.id.as_str(),
                "title": t.id.title(),
                "line": sweep::describe(t),
                "included": t.runnable() && !skip.contains(&t.id),
            });
            match &t.data {
                TileData::Plan(p) => {
                    let saved = plans::save(env, p).map_err(|e| e.to_string());
                    v["plan_id"] = json!(p.id);
                    v["digest"] = json!(p.digest());
                    v["selected_bytes"] = json!(p.selected_size());
                    v["selected"] = json!(human_size(p.selected_size()));
                    v["selected_count"] = json!(p.selected_count());
                    v["item_count"] = json!(p.items.len());
                    if let Err(e) = saved {
                        v["error"] = json!(format!("plan not saved: {e}"));
                    }
                }
                TileData::Count { count, noun } => {
                    v["count"] = json!(count);
                    v["noun"] = json!(noun);
                }
                TileData::Failed(e) => v["error"] = json!(e),
            }
            v
        })
        .collect();
    let ready = res.ready_bytes(skip);
    json!({
        "ready_bytes": ready,
        "ready": human_size(ready),
        "tiles": tiles,
        "next": "Show the user each plan (get_plan). After they confirm, call execute_plan with that plan_id and digest.",
    })
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SweepArgs {
    /// Only these modules (feature ids, e.g. ["system_junk", "mail_downloads"]). Default: the
    /// modules the user keeps on in Quick Sweep.
    pub modules: Option<Vec<String>>,
}

/// The settings Quick Sweep scans with: only `modules` when given.
pub fn sweep_settings(
    settings: &Settings,
    sources: &Sources,
    modules: Option<&[String]>,
) -> Result<Settings, String> {
    let mut s = settings.clone();
    if let Some(m) = modules {
        let mut keep = Vec::new();
        for name in m {
            let id = FeatureId::parse(name).ok_or_else(|| format!("unknown module '{name}'"))?;
            if !sources.ids().contains(&id) {
                return Err(format!("{} is not a Quick Sweep module", id.title()));
            }
            keep.push(id);
        }
        s.sweep_skip = sources
            .ids()
            .into_iter()
            .filter(|i| !keep.contains(i))
            .collect();
    }
    Ok(s)
}

#[tool_router(router = monitor_router, vis = "pub")]
impl CleanCraftMcp {
    #[tool(
        description = "Live reading of the Mac: CPU load and top apps, memory use and pressure, free disk space and Trash size, battery, network speed (from the second call on), plus alerts. Read only."
    )]
    async fn system_status(&self) -> ToolResult {
        let mut g = SAMPLER.lock().map_err(|e| e.to_string())?;
        let s = g.get_or_insert_with(Sampler::new);
        ok(system_status_value(&self.env, s))
    }

    #[tool(
        description = "Quick Sweep preview: scans junk, mail downloads, Trash, threats and other available modules (look only), or only the modules named. Returns one entry per module with size or count, and for cleanup modules a saved plan_id and digest. Show each plan (get_plan, adjust with select_items); after the user says yes, run them with sweep_run or execute_plan."
    )]
    async fn sweep_preview(&self, Parameters(a): Parameters<SweepArgs>) -> ToolResult {
        let sources = Sources::registered();
        let settings = sweep_settings(&Settings::load(&self.env), &sources, a.modules.as_deref())?;
        ok(sweep_preview_value(&self.env, &settings, &sources))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::FeatureId;
    use cleancraft_core::testkit::FakeMac;
    use cleancraft_core::testkit_monitor::*;

    #[test]
    fn status_reports_reading_and_alerts() {
        let m = FakeMac::new();
        answer_all(m.env.fake_sys().unwrap());
        let mut s = Sampler::new();
        let v = system_status_value(&m.env, &mut s);
        assert_eq!(v["reading"]["battery"]["percent"], 64);
        assert_eq!(v["reading"]["storage"]["free"], 80_000_000u64 * 1024);
        assert!(v["summary"].as_str().unwrap().starts_with("CPU 17% RAM "));
        assert_eq!(v["notes"].as_array().unwrap().len(), 1);
        m.env.fake_sys().unwrap().answer("netstat", NETSTAT_2);
        // Speeds need time to pass between readings.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let v2 = system_status_value(&m.env, &mut s);
        assert!(v2["reading"]["network"]["down"].as_u64().unwrap() > 0);
        assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
    }

    #[test]
    fn sweep_preview_saves_plans_with_digests() {
        let m = sweep_mac();
        let v = sweep_preview_value(&m.env, &Settings::default(), &sweep_sources());
        let tiles = v["tiles"].as_array().unwrap();
        assert_eq!(tiles.len(), 5);
        let mail = tiles
            .iter()
            .find(|t| t["feature"] == FeatureId::MailAttachments.as_str())
            .unwrap();
        let plan = plans::load(&m.env, mail["plan_id"].as_str().unwrap()).unwrap();
        assert_eq!(mail["digest"], plan.digest());
        assert_eq!(mail["selected_bytes"], plan.selected_size());
        assert_eq!(mail["included"], true);
        let trash = tiles.iter().find(|t| t["feature"] == "trash_bins").unwrap();
        assert_eq!(trash["included"], false);
        assert!(tiles.iter().any(|t| t["error"].is_string()));
        // Looking only: everything is still there.
        assert!(m.env.home_path("Library/Mail Downloads/b.zip").exists());
    }
}
