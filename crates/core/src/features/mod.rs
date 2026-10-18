//! The features. Each cleanup feature is one file implementing [`Feature`]: its allow-list and
//! a scan that returns a [`Plan`]. Removing is never done here; the executor does it after the
//! user confirmed the plan. Features that run commands or toggle settings (maintenance,
//! startup items) have their own action functions, which also log to the journal.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::env::Env;
use crate::executor::{self, ExecError, Method, Report};
use crate::plan::{FeatureId, Plan};
use crate::policy;
use crate::settings::Settings;

pub mod leftovers;
pub mod mail;
pub mod malware;
pub mod privacy;
pub mod system_junk;
pub mod trash;
pub mod uninstaller;

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
    /// Where this feature's items go, when it must not follow the user's setting
    /// (threats always go to the quarantine).
    fn method_override(&self) -> Option<Method> {
        None
    }
    /// Work needed right before the confirmed items move, e.g. unloading a launch agent.
    /// Runs only for the confirmed selection; must log what it does to the journal.
    fn prepare(&self, _env: &Env, _plan: &Plan) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Carry out a confirmed plan the one way every front end uses: the feature's allow-list,
/// its method override, its preparation, then the executor.
pub fn run_plan(
    env: &Env,
    settings: &Settings,
    plan: &Plan,
    confirmed_digest: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> anyhow::Result<Report> {
    let fid = plan
        .feature
        .ok_or_else(|| anyhow::anyhow!("the plan has no feature"))?;
    let f = get(fid).ok_or_else(|| anyhow::anyhow!("{} has no cleanup", fid.title()))?;
    run_plan_with(f.as_ref(), env, settings, plan, confirmed_digest, progress)
}

/// [`run_plan`] with the feature already looked up (Quick Sweep runs several in a row).
/// Same checks: the plan must belong to `f` and match the confirmed digest.
pub fn run_plan_with(
    f: &dyn Feature,
    env: &Env,
    settings: &Settings,
    plan: &Plan,
    confirmed_digest: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> anyhow::Result<Report> {
    if plan.feature != Some(f.id()) {
        anyhow::bail!("this plan does not belong to {}", f.id().title());
    }
    if plan.digest() != confirmed_digest {
        return Err(ExecError::DigestMismatch.into());
    }
    let allow = f.allow_roots(env);
    let method = f.method_override().unwrap_or(settings.method);
    f.prepare(env, plan)?;
    Ok(executor::execute(
        env,
        plan,
        &allow,
        confirmed_digest,
        method,
        progress,
    )?)
}

/// Every plan-based feature, in sidebar order.
pub fn registry() -> Vec<Box<dyn Feature>> {
    vec![
        Box::new(system_junk::SystemJunk),
        Box::new(uninstaller::Uninstaller),
        Box::new(malware::Malware),
        Box::new(crate::startup::StartupFeature),
        Box::new(mail::MailDownloads),
        Box::new(trash::TrashBins),
        Box::new(privacy::Privacy),
    ]
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
