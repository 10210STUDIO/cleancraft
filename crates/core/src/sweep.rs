//! Quick Sweep: one scan across several modules, one tile per module, and one confirmation
//! that runs every included module's safe preselection.
//!
//! The modules are looked up at run time: plan modules through [`features::get`] (so a module
//! that is not registered simply has no tile), and count-only modules (tune-up suggestions,
//! app updates) through [`count_sources`]. Removing still goes through
//! [`features::run_plan_with`] with each plan's confirmed digest: nothing here moves files.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::env::Env;
use crate::executor::Report;
use crate::features::{self, Feature, ScanCtx};
use crate::plan::{FeatureId, Plan};
use crate::settings::Settings;

/// Plan modules Quick Sweep includes when they are registered, in tile order.
pub const PLAN_MODULES: [FeatureId; 4] = [
    FeatureId::SystemJunk,
    FeatureId::TrashBins,
    FeatureId::MailAttachments,
    FeatureId::Malware,
];

/// A module that only reports a number (tune-up suggestions, app updates). Its tile opens the
/// module's page; Quick Sweep never runs it.
#[derive(Clone, Copy)]
pub struct CountSource {
    pub id: FeatureId,
    /// "suggestions", "updates".
    pub noun: &'static str,
    pub count: fn(&Env, &Settings) -> anyhow::Result<usize>,
}

/// Count-only modules. Owners add one line each when their module exists, for example
/// `CountSource { id: FeatureId::Maintenance, noun: "suggestions", count: crate::tasks::suggestion_count }`.
pub fn count_sources() -> Vec<CountSource> {
    Vec::new()
}

/// What a sweep looks at.
#[derive(Clone)]
pub struct Sources {
    pub plans: Vec<Arc<dyn Feature>>,
    pub counts: Vec<CountSource>,
}

impl Sources {
    /// The registered modules among [`PLAN_MODULES`], plus [`count_sources`].
    pub fn registered() -> Sources {
        Sources {
            plans: PLAN_MODULES
                .iter()
                .filter_map(|id| features::get(*id))
                .map(Arc::from)
                .collect(),
            counts: count_sources(),
        }
    }

    pub fn feature(&self, id: FeatureId) -> Option<&Arc<dyn Feature>> {
        self.plans.iter().find(|f| f.id() == id)
    }

    pub fn ids(&self) -> Vec<FeatureId> {
        self.plans
            .iter()
            .map(|f| f.id())
            .chain(self.counts.iter().map(|c| c.id))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub enum TileData {
    Plan(Plan),
    Count { count: usize, noun: &'static str },
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Tile {
    pub id: FeatureId,
    pub data: TileData,
}

impl Tile {
    /// Run executes this tile's plan. The Trash has its own page and is never emptied from
    /// the combined run.
    pub fn runnable(&self) -> bool {
        matches!(self.data, TileData::Plan(_)) && self.id != FeatureId::TrashBins
    }

    pub fn plan(&self) -> Option<&Plan> {
        match &self.data {
            TileData::Plan(p) => Some(p),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SweepResult {
    pub tiles: Vec<Tile>,
}

impl SweepResult {
    pub fn tile(&self, id: FeatureId) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.id == id)
    }

    /// Plans Run would carry out: runnable, not skipped, with something selected.
    pub fn to_run(&self, skip: &[FeatureId]) -> Vec<&Plan> {
        self.tiles
            .iter()
            .filter(|t| t.runnable() && !skip.contains(&t.id))
            .filter_map(|t| t.plan())
            .filter(|p| p.selected_count() > 0)
            .collect()
    }

    /// Bytes Run would move.
    pub fn ready_bytes(&self, skip: &[FeatureId]) -> u64 {
        self.to_run(skip).iter().map(|p| p.selected_size()).sum()
    }
}

/// Scan every module (look only). A module that fails gets a Failed tile; the others go on.
/// Stops between modules when `cancel` is set.
pub fn scan(
    env: &Env,
    settings: &Settings,
    sources: &Sources,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&str) + Sync),
) -> SweepResult {
    let mut tiles = Vec::new();
    for f in &sources.plans {
        if cancel.load(Ordering::Relaxed) {
            return SweepResult { tiles };
        }
        progress(f.id().title());
        let ctx = ScanCtx {
            env,
            settings,
            cancel,
            progress,
        };
        let data = match f.scan(&ctx) {
            Ok(p) => TileData::Plan(p),
            Err(e) => TileData::Failed(format!("{e:#}")),
        };
        tiles.push(Tile { id: f.id(), data });
    }
    for c in &sources.counts {
        if cancel.load(Ordering::Relaxed) {
            return SweepResult { tiles };
        }
        progress(c.id.title());
        let data = match (c.count)(env, settings) {
            Ok(n) => TileData::Count {
                count: n,
                noun: c.noun,
            },
            Err(e) => TileData::Failed(format!("{e:#}")),
        };
        tiles.push(Tile { id: c.id, data });
    }
    SweepResult { tiles }
}

#[derive(Clone, Debug)]
pub struct ModuleRun {
    pub id: FeatureId,
    pub result: Result<Report, String>,
}

/// Carry out confirmed plans, each with the digest the user confirmed, one module after the
/// other. A module that fails does not stop the others.
pub fn run(
    env: &Env,
    settings: &Settings,
    sources: &Sources,
    confirmed: &[(Plan, String)],
    progress: &(dyn Fn(&str) + Sync),
) -> Vec<ModuleRun> {
    let mut out = Vec::new();
    for (plan, digest) in confirmed {
        let Some(id) = plan.feature else {
            continue;
        };
        progress(id.title());
        let result = match sources.feature(id) {
            Some(f) => {
                features::run_plan_with(f.as_ref(), env, settings, plan, digest, &mut |_, _| {})
                    .map_err(|e| format!("{e:#}"))
            }
            None => Err(format!("{} is not available", id.title())),
        };
        out.push(ModuleRun { id, result });
    }
    out
}

/// One line per tile, for the CLI and agents.
pub fn describe(t: &Tile) -> String {
    use crate::fsutil::human_size;
    match &t.data {
        TileData::Plan(p) => format!(
            "{}: {} selected in {} of {} items{}",
            t.id.title(),
            human_size(p.selected_size()),
            p.selected_count(),
            p.items.len(),
            if t.runnable() {
                ""
            } else {
                " (cleaned from its own page)"
            }
        ),
        TileData::Count { count, noun } => format!("{}: {count} {noun}", t.id.title()),
        TileData::Failed(e) => format!("{}: could not scan ({e})", t.id.title()),
    }
}
