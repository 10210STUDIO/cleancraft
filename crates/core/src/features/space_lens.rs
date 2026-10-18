//! Disk Map removal: the items the user ticked in the map, as a plan for the executor.
//! Allow-list: the home folder. Protected places, Library, hidden items and apps are locked
//! ([`crate::space::lock_reason`]); [`Feature::prepare`] refuses a plan that still selects one.

use std::path::PathBuf;

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::plan::{FeatureId, Plan, Risk};
use crate::space::{self, lock_reason, outermost};

pub struct SpaceLens;

pub const CATEGORY: &str = "selected";

/// A plan for the items selected in the map: `(path, size shown)`. Items inside another
/// selected item are left out (moving the folder moves them).
pub fn plan_for(env: &Env, selected: &[(PathBuf, u64)]) -> Plan {
    let mut plan = Plan::new(FeatureId::SpaceLens);
    plan.add_category(
        CATEGORY,
        "Selected in the Disk Map",
        "Files and folders you ticked in the map.",
        Risk::High,
    );
    let keep = outermost(&selected.iter().map(|s| s.0.clone()).collect::<Vec<_>>());
    for p in keep {
        let size = selected.iter().find(|s| s.0 == p).map(|s| s.1).unwrap_or(0);
        let reason = if p.is_dir() {
            "Folder you selected in the Disk Map"
        } else {
            "File you selected in the Disk Map"
        };
        let id = plan.push(p.clone(), size, CATEGORY, reason, Risk::High, true);
        if let Some(why) = lock_reason(env, &p) {
            plan.lock(id, why);
        }
    }
    plan
}

impl Feature for SpaceLens {
    fn id(&self) -> FeatureId {
        FeatureId::SpaceLens
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.home().to_path_buf()]
    }

    /// The top level of the home folder, measured, nothing selected (for the CLI and agents).
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let env = ctx.env;
        (ctx.progress)(&env.display(env.home()));
        let tree = space::build(env.home(), ctx.cancel, &|_, _| {});
        let mut plan = Plan::new(FeatureId::SpaceLens);
        plan.add_category(
            CATEGORY,
            "Home folder",
            "The biggest items in your home folder.",
            Risk::High,
        );
        if let Some(root) = &tree.root {
            for c in &root.children {
                let id = plan.push(
                    c.path.clone(),
                    c.size,
                    CATEGORY,
                    if c.is_dir {
                        format!("Folder with {} files", c.files)
                    } else {
                        "File".to_string()
                    },
                    Risk::High,
                    false,
                );
                if let Some(why) = lock_reason(env, &c.path) {
                    plan.lock(id, why);
                }
            }
        }
        if tree.unreadable > 0 {
            plan.notes.push(format!(
                "{} folders could not be read; their size is not counted.",
                tree.unreadable
            ));
        }
        Ok(plan)
    }

    fn prepare(&self, env: &Env, plan: &Plan) -> anyhow::Result<()> {
        for it in plan.selected() {
            if let Some(why) = lock_reason(env, &it.path) {
                anyhow::bail!(
                    "{} cannot be removed ({why}); nothing was moved.",
                    env.display(&it.path)
                );
            }
        }
        Ok(())
    }
}
