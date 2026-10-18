//! The features. Each cleanup feature is one file implementing [`Feature`]: its allow-list and
//! a scan that returns a [`Plan`]. Removing is never done here; the executor does it after the
//! user confirmed the plan. Features that run commands or toggle settings (maintenance,
//! startup items) have their own action functions, which also log to the journal.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::env::Env;
use crate::plan::{FeatureId, Plan};
use crate::policy;
use crate::settings::Settings;

pub mod system_junk;

/// What a scan gets.
pub struct ScanCtx<'a> {
    pub env: &'a Env,
    pub settings: &'a Settings,
    pub cancel: &'a AtomicBool,
    /// Called with the place being looked at, for the progress line.
    pub progress: &'a (dyn Fn(&str) + Sync),
}

impl<'a> ScanCtx<'a> {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Settings' ignore list.
    pub fn ignored(&self, p: &Path) -> bool {
        self.settings.is_ignored(p)
    }

    /// May this path be proposed for this allow-list? (Same fence the executor uses.)
    pub fn allowed(&self, allow: &[PathBuf], p: &Path) -> bool {
        !self.ignored(p) && policy::check(self.env, allow, p).is_ok()
    }
}

pub trait Feature: Send + Sync {
    fn id(&self) -> FeatureId;
    /// Folders this feature may remove things from (strictly inside them).
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf>;
    /// Look, never change anything.
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan>;
}

/// Every plan-based feature, in sidebar order.
pub fn registry() -> Vec<Box<dyn Feature>> {
    vec![Box::new(system_junk::SystemJunk)]
}

pub fn get(id: FeatureId) -> Option<Box<dyn Feature>> {
    registry().into_iter().find(|f| f.id() == id)
}

/// The allow-list of a feature; empty (nothing may be removed) when the feature has no plans.
pub fn allow_roots(env: &Env, id: FeatureId) -> Vec<PathBuf> {
    get(id).map(|f| f.allow_roots(env)).unwrap_or_default()
}

/// Scan with a fresh context.
pub fn scan(env: &Env, settings: &Settings, id: FeatureId) -> anyhow::Result<Plan> {
    let f = get(id).ok_or_else(|| anyhow::anyhow!("{} has no scan", id.title()))?;
    let cancel = AtomicBool::new(false);
    let ctx = ScanCtx {
        env,
        settings,
        cancel: &cancel,
        progress: &|_| {},
    };
    f.scan(&ctx)
}
