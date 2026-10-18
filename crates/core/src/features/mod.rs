//! The features. Each cleanup feature is one file implementing [`Feature`]: its allow-list and
//! a scan that returns a [`Plan`]. Removing is never done here; the executor does it after the
//! user confirmed the plan. Features that run commands or toggle settings (maintenance,
//! startup items) have their own action functions, which also log to the journal.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::env::Env;
use crate::executor::{self, ExecError, Method, Report, SkipReason};
use crate::plan::{FeatureId, Item, Plan};
use crate::policy;
use crate::settings::Settings;

pub mod downloads;
pub mod duplicates;
pub mod large_old;
pub mod leftovers;
pub mod mail;
pub mod malware;
pub mod privacy;
pub mod similar;
pub mod space_lens;
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
    /// May items the plan marks [`crate::plan::Item::needs_admin`] be moved with the
    /// administrator password (one `mv` per item into the quarantine, after the same checks)?
    /// Off unless a feature turns it on.
    fn allows_admin(&self) -> bool {
        false
    }
    /// Check one selected item again, right before anything moves, with the same rules the
    /// scan used to propose it. A saved plan is a plain file that a person or an agent can
    /// edit, so neither its categories, its locks nor its groups are trusted: only what this
    /// function finds on disk now. `Err` says why the item is refused (it is skipped, the
    /// rest goes on). The allow-list and [`policy::check_move`] run after this anyway.
    fn validate(
        &self,
        _env: &Env,
        _settings: &Settings,
        _plan: &Plan,
        _item: &Item,
    ) -> Result<(), String> {
        Ok(())
    }
    /// Rules about the selection as a whole (Duplicates: one copy of every content stays on
    /// disk), checked after [`Feature::validate`] on the items that passed it. `Err` stops
    /// the whole run: nothing moves.
    fn validate_set(&self, _env: &Env, _settings: &Settings, _plan: &Plan) -> anyhow::Result<()> {
        Ok(())
    }
    /// Work needed right before the confirmed items move, e.g. unloading a launch agent.
    /// Runs only for the confirmed selection; must log what it does to the journal.
    fn prepare(&self, _env: &Env, _plan: &Plan) -> anyhow::Result<()> {
        Ok(())
    }
    /// Work for what really moved, right after the executor (stopping a launch agent whose
    /// property list moved). Items that were skipped are left untouched. Must log what it
    /// does; a failure here is in the journal, the move itself stands.
    fn finish(&self, _env: &Env, _plan: &Plan, _report: &Report) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Carry out a confirmed plan the one way every front end uses: every selected item checked
/// again by its feature ([`Feature::validate`], [`Feature::validate_set`]) and against the
/// ignore list, the feature's allow-list, its method override, its preparation, then the
/// executor. Refused items are reported as skipped and never reach the executor.
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
    let (checked, refused) = validated(f, env, settings, plan);
    f.validate_set(env, settings, &checked)?;
    let allow = f.allow_roots(env);
    let method = f.method_override().unwrap_or(settings.method);
    f.prepare(env, &checked)?;
    let mut report = executor::execute_with(
        env,
        &checked,
        &allow,
        &checked.digest(),
        method,
        f.allows_admin(),
        progress,
    )?;
    // The files moved; what finish could not do is logged in the journal by finish itself.
    let _ = f.finish(env, &checked, &report);
    report.skipped.splice(0..0, refused);
    Ok(report)
}

/// `plan` with every selected item that fails [`Feature::validate`] or is on the ignore list
/// unselected, and those items with the reason.
fn validated(
    f: &dyn Feature,
    env: &Env,
    settings: &Settings,
    plan: &Plan,
) -> (Plan, Vec<(PathBuf, SkipReason)>) {
    let mut checked = plan.clone();
    let mut refused = Vec::new();
    // By position, not by `Item::id`: an edited plan may carry any ids.
    for (n, it) in plan.items.iter().enumerate() {
        if !(it.selected && it.locked.is_none()) {
            continue;
        }
        let why = if settings.is_ignored(&it.path) {
            Err("it is on your ignore list".to_string())
        } else {
            f.validate(env, settings, plan, it)
        };
        if let Err(why) = why {
            checked.items[n].selected = false;
            refused.push((it.path.clone(), SkipReason::Refused(why)));
        }
    }
    (checked, refused)
}

/// Keep only the items `only` (item ids) of the plan's confirmed selection: what an agent may
/// do with MCP `execute_plan` `only_items`. It can only narrow the list the user confirmed
/// (the digest was checked on it), never add an item to it: an id that is not selected now is
/// refused and nothing changes.
pub fn narrow_selection(plan: &mut Plan, only: &[usize]) -> Result<(), String> {
    for id in only {
        let ok = plan
            .items
            .iter()
            .any(|i| i.id == *id && i.selected && i.locked.is_none());
        if !ok {
            return Err(format!(
                "Refused: item {id} is not in the confirmed list. only_items can only leave items out."
            ));
        }
    }
    for it in plan.items.iter_mut() {
        if !only.contains(&it.id) {
            it.selected = false;
        }
    }
    Ok(())
}

/// How many selected items of `plan` will move with the administrator password (its feature
/// allows it and the scan marked them). Confirmations say so when this is not zero.
pub fn admin_count(plan: &Plan) -> usize {
    let allowed = plan.feature.and_then(get).is_some_and(|f| f.allows_admin());
    if allowed {
        plan.selected_admin_count()
    } else {
        0
    }
}

/// What a cleanup did, in words: "1.20 MB moved to the Trash". Moving to the Trash or the
/// quarantine frees no space yet (it comes back when the Trash is emptied), so the word
/// "freed" is never used here. The bytes stay in `Report::freed` for the CLI and MCP.
pub fn moved_text<'a>(reports: impl IntoIterator<Item = &'a Report>) -> String {
    let mut bytes = 0;
    let (mut trash, mut quarantine) = (false, false);
    for r in reports {
        bytes += r.freed;
        for m in &r.moved {
            if let crate::executor::ActionKind::Moved { method, .. } = &m.kind {
                match method {
                    Method::Trash => trash = true,
                    Method::Quarantine | Method::AdminQuarantine => quarantine = true,
                }
            }
        }
    }
    let size = crate::fsutil::human_size(bytes);
    match (trash, quarantine) {
        (false, false) => "Nothing moved".into(),
        (true, false) => format!("{size} moved to the Trash"),
        (false, true) => format!("{size} moved to the CleanCraft quarantine"),
        (true, true) => format!("{size} moved to the Trash and the CleanCraft quarantine"),
    }
}

/// The line under [`moved_text`]: when the space really comes back.
pub const SPACE_NOTE: &str =
    "The space is freed when the Trash is emptied or the quarantine is cleared.";

/// Every plan-based feature, in sidebar order.
pub fn registry() -> Vec<Box<dyn Feature>> {
    vec![
        Box::new(system_junk::SystemJunk),
        Box::new(large_old::LargeOldFiles),
        Box::new(duplicates::Duplicates),
        Box::new(similar::SimilarImages),
        Box::new(space_lens::SpaceLens),
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

/// Run a feature's scan the one way every front end uses: then a hard-linked file listed
/// twice counts once ([`Plan::count_links_once`]).
pub fn scan_with(f: &dyn Feature, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
    let mut plan = f.scan(ctx)?;
    plan.count_links_once();
    Ok(plan)
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
    scan_with(f.as_ref(), &ctx)
}
