//! App Remover: remove an app together with the files it left around the Library, or reset an
//! app (remove its data, keep the app).
//!
//! Leftovers are matched in the folders of [`LEFTOVER_DIRS`]:
//! - high confidence: the entry is named after the app's bundle id (`com.example.App`,
//!   `com.example.App.plist`, `com.example.App.savedState`, `TEAMID.com.example.App`...);
//!   these are selected for you;
//! - medium confidence: the entry is named after the app (`Application Support/Example`);
//!   shown, never selected for you;
//! - the maker's shared folder (`Application Support/Google`): shown with a warning, never
//!   selected, because other apps from the same maker may use it.
//!
//! Apps that are part of macOS are listed locked. A running app is locked with "Quit it
//! first", and [`Uninstaller::prepare`] refuses again right before anything moves.
//!
//! The feature's own scan (used by agents and the CLI `scan uninstaller`) lists what is not
//! tied to one app: leftovers of removed apps, installer files and extensions.

use std::path::{Path, PathBuf};

use anyhow::bail;

use crate::apps::{self, AppInfo, resolve};
use crate::env::Env;
use crate::extensions;
use crate::features::{Feature, ScanCtx, leftovers};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Item, Plan, Risk};
use crate::policy;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};

pub struct Uninstaller;

/// Category of the app bundle item in an uninstall or reset plan.
pub const CAT_APP: &str = "app";
pub const CAT_HIGH: &str = "files_sure";
pub const CAT_MEDIUM: &str = "files_maybe";
pub const CAT_SHARED: &str = "files_shared";

/// Where apps leave files, with the words shown next to each match.
pub const LEFTOVER_DIRS: &[(&str, &str)] = &[
    ("~/Library/Application Support", "Support files"),
    ("~/Library/Caches", "Cache"),
    ("~/Library/Preferences", "Preferences"),
    ("~/Library/Containers", "App container"),
    ("~/Library/Group Containers", "Shared container"),
    ("~/Library/Saved Application State", "Saved window state"),
    ("~/Library/HTTPStorages", "Web storage"),
    ("~/Library/WebKit", "Web data"),
    ("~/Library/Cookies", "Cookies"),
    ("~/Library/Logs", "Logs"),
    ("~/Library/LaunchAgents", "Startup agent"),
    ("~/Library/Application Scripts", "App scripts"),
    ("/Library/Application Support", "Support files (all users)"),
    ("/Library/Caches", "Cache (all users)"),
    ("/Library/Preferences", "Preferences (all users)"),
    ("/Library/LaunchAgents", "Startup agent (all users)"),
    ("/Library/LaunchDaemons", "Background service"),
    ("/Library/PrivilegedHelperTools", "Helper tool"),
];

/// Folders where a vendor's shared folder can be found.
const VENDOR_DIRS: &[&str] = &[
    "~/Library/Application Support",
    "/Library/Application Support",
];

/// How sure a leftover belongs to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Shared,
}

/// Strip what macOS adds around a bundle id in a Library entry name.
pub fn id_part(name: &str) -> &str {
    let mut n = name;
    for suf in [".plist", ".savedState", ".binarycookies"] {
        if let Some(s) = n.strip_suffix(suf) {
            n = s;
            break;
        }
    }
    // Group containers: "group.com.x.y", "ABCDE12345.com.x.y", "ABCDE12345.group.com.x".
    if let Some((first, rest)) = n.split_once('.')
        && first.len() == 10
        && first
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        n = rest;
    }
    n.strip_prefix("group.").unwrap_or(n)
}

/// Does a Library entry belong to `bundle_id`? Exact id, or the id followed by a dot
/// (`com.x.App.helper`), unless a longer installed id is the better match.
pub fn matches_id(name: &str, bundle_id: &str, other_ids: &[&str]) -> bool {
    let n = id_part(name);
    let lower = n.to_lowercase();
    let id = bundle_id.to_lowercase();
    let hit = lower == id || lower.starts_with(&format!("{id}."));
    if !hit {
        return false;
    }
    // "com.x.App.pro" belongs to the app "com.x.App.pro" when it is installed.
    !other_ids.iter().any(|o| {
        let o = o.to_lowercase();
        o.len() > id.len() && (lower == o || lower.starts_with(&format!("{o}.")))
    })
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

/// The three leftover categories of an uninstall plan (sure, maybe, shared).
pub fn add_leftover_categories(plan: &mut Plan) {
    plan.add_category(
        CAT_HIGH,
        "Files named after the app's id",
        "Settings, caches and data that carry the app's identifier. Selected for you.",
        Risk::High,
    );
    plan.add_category(
        CAT_MEDIUM,
        "Files named like the app",
        "Folders with the app's name. Check them before selecting.",
        Risk::High,
    );
    plan.add_category(
        CAT_SHARED,
        "Shared with other apps from the same maker",
        "Removing these can affect other apps from this maker.",
        Risk::High,
    );
}

/// Add `app`'s leftovers to `plan` (categories from [`add_leftover_categories`]): files named
/// after its id are selected, the rest only shown. Ignored paths, paths outside `allow` and
/// files of macOS are left out. The app bundle itself is not added.
pub fn push_leftovers(
    env: &Env,
    settings: &Settings,
    app: &AppInfo,
    all: &[AppInfo],
    allow: &[PathBuf],
    plan: &mut Plan,
) {
    // Another copy of the same app (same bundle id) keeps using the files named after the id.
    let twin = all
        .iter()
        .find(|a| a.path != app.path && a.bundle_id.eq_ignore_ascii_case(&app.bundle_id));
    for (path, label, conf) in find_leftovers(env, app, all) {
        if settings.is_ignored(&path) || policy::check(env, allow, &path).is_err() {
            continue;
        }
        // Apple's App Store apps (iMovie, GarageBand) are removable, but their files named
        // after an Apple id stay: the same check as before anything moves (`validate_set`).
        if apps::apple_part(&path, 3, true).is_some() {
            continue;
        }
        let (size, unreadable) = leftover_size(env, &path);
        let (cat, mut why, sel) = match conf {
            Confidence::High if twin.is_some() => (
                CAT_SHARED,
                format!(
                    "{label}: named after {}, which {} also uses",
                    app.bundle_id,
                    twin.map(|t| env.display(&t.path)).unwrap_or_default()
                ),
                false,
            ),
            Confidence::High => (
                CAT_HIGH,
                format!("{label}: named after {}", app.bundle_id),
                true,
            ),
            Confidence::Medium => (
                CAT_MEDIUM,
                format!("{label}: named like {}", app.name),
                false,
            ),
            Confidence::Shared => (
                CAT_SHARED,
                format!(
                    "{label}: shared folder of {}; other apps from this maker may need it",
                    app.vendor
                ),
                false,
            ),
        };
        if unreadable > 0 {
            why.push_str(SIZE_PARTIAL);
        }
        let id = plan.push(path.clone(), size, cat, why, Risk::High, sel);
        if let Some(parent) = path.parent()
            && !env.can_modify(parent)
        {
            plan.mark_admin(id);
        }
    }
}

/// The plan to uninstall (`reset` false) or reset (`reset` true) the app at `app_path`.
/// `reset` keeps the bundle: it is listed, locked, so the plan says what stays.
pub fn uninstall_plan(
    env: &Env,
    settings: &Settings,
    app_path: &Path,
    reset: bool,
) -> anyhow::Result<Plan> {
    let all = apps::inventory(env, false, &|_| {});
    let Some(app) = all.iter().find(|a| a.path == app_path).cloned() else {
        bail!(
            "{} is not an app in the Applications folders",
            env.display(app_path)
        );
    };
    Ok(plan_for(env, settings, &all, &app, reset))
}

/// [`uninstall_plan`] for `app` of the app list `all` (read once for several apps).
fn plan_for(env: &Env, settings: &Settings, all: &[AppInfo], app: &AppInfo, reset: bool) -> Plan {
    let app = app.clone();
    let mut plan = Plan::new(FeatureId::Uninstaller);
    let allow = Uninstaller.allow_roots(env);
    plan.add_category(CAT_APP, "Application", "The app itself.", Risk::High);
    add_leftover_categories(&mut plan);
    let bundle = plan.push(
        app.path.clone(),
        tree_size(&app.path).0,
        CAT_APP,
        format!("{} {}", app.name, app.version).trim().to_string(),
        Risk::High,
        !reset,
    );
    if reset {
        plan.lock(bundle, "Kept: a reset keeps the app");
    } else if !env.can_modify(&app.path) {
        // Installed by a package: owned by the system, so it moves with the administrator
        // password. (/Applications is shared with the admin group, so only the bundle counts.)
        plan.mark_admin(bundle);
    }
    push_leftovers(env, settings, &app, all, &allow, &mut plan);
    if settings.is_ignored(&app.path) {
        let why = format!(
            "{} is on your ignore list (Settings), so CleanCraft does not remove it",
            app.name
        );
        for i in 0..plan.items.len() {
            plan.lock(i, why.clone());
        }
        plan.notes.push(why);
    }
    if let Some(why) = app_lock(env, &app) {
        for i in 0..plan.items.len() {
            plan.lock(i, why.clone());
        }
        plan.notes.push(why);
    }
    plan
}

/// Added to a leftover's reason when part of it could not be read: its size is a lower bound
/// (often 0 KB for a folder macOS keeps private, or one owned by another user).
pub const SIZE_PARTIAL: &str = ". Some of it could not be read, so it may be bigger than shown";

/// The size shown for an item is only what could be read ([`SIZE_PARTIAL`]).
pub fn size_is_partial(item: &crate::plan::Item) -> bool {
    item.reason.ends_with(SIZE_PARTIAL)
}

/// Size of a leftover with the shared formula ([`tree_size`]: allocated blocks, links not
/// followed, a hard-linked file once), and how many parts could not be read. Folders an
/// injected `PermissionDenied` fault covers count as unreadable (tests run as root here).
pub fn leftover_size(env: &Env, p: &Path) -> (u64, u32) {
    fn walk(env: &Env, p: &Path, seen: &mut std::collections::HashSet<(u64, u64)>) -> (u64, u32) {
        let Ok(md) = std::fs::symlink_metadata(p) else {
            return (0, 1);
        };
        let mut total = 0;
        if crate::fsutil::link_id(&md).is_none_or(|id| seen.insert(id)) {
            total += crate::fsutil::entry_size(&md);
        }
        let mut unreadable = 0;
        if md.is_dir() {
            let denied = matches!(env.fault_for(p), Some(crate::env::Fault::PermissionDenied));
            match std::fs::read_dir(p) {
                Ok(rd) if !denied => {
                    for e in rd.flatten() {
                        let (s, u) = walk(env, &e.path(), seen);
                        total += s;
                        unreadable += u;
                    }
                }
                _ => unreadable += 1,
            }
        }
        (total, unreadable)
    }
    walk(env, p, &mut std::collections::HashSet::new())
}

/// Why nothing of this app may be touched now: part of macOS, or running.
pub fn app_lock(env: &Env, app: &AppInfo) -> Option<String> {
    if app.apple {
        return Some(format!(
            "{} is part of macOS and cannot be removed",
            app.name
        ));
    }
    if is_running(env, &app.path) {
        return Some(format!("Quit {} first", app.name));
    }
    None
}

/// Is an app inside `app_path` running? Same mapping as the executor's check.
pub fn is_running(env: &Env, app_path: &Path) -> bool {
    env.sys.running_apps().iter().any(|r| {
        r.path.as_ref().is_some_and(|ap| {
            let ap = if env.is_fake() {
                env.sys_path(&ap.to_string_lossy())
            } else {
                ap.clone()
            };
            ap.starts_with(app_path)
        })
    })
}

/// Every Library entry that belongs to `app`, with the folder's label and the confidence.
pub fn find_leftovers(
    env: &Env,
    app: &AppInfo,
    all: &[AppInfo],
) -> Vec<(PathBuf, &'static str, Confidence)> {
    let others: Vec<&str> = all
        .iter()
        .filter(|a| a.path != app.path)
        .map(|a| a.bundle_id.as_str())
        .collect();
    let other_names: Vec<String> = all
        .iter()
        .filter(|a| a.path != app.path)
        .flat_map(|a| [a.name.to_lowercase(), a.bundle_name.to_lowercase()])
        .collect();
    let mut names: Vec<String> = vec![app.name.to_lowercase(), app.bundle_name.to_lowercase()];
    names.retain(|n| n.chars().count() >= 3 && !other_names.contains(n));
    names.dedup();
    // The maker's folder name comes from the bundle id ("com.jetbrains.goland": "jetbrains"),
    // never from the shown developer name ("JetBrains s.r.o.").
    let vendor = apps::vendor_segment(&app.bundle_id);
    let mut out = Vec::new();
    for (dir, label) in LEFTOVER_DIRS {
        let root = resolve(env, dir);
        let name_ok = !dir.ends_with("Containers");
        for entry in children(&root) {
            let Some(fname) = entry.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if fname.starts_with('.') {
                continue;
            }
            if matches_id(&fname, &app.bundle_id, &others) {
                out.push((entry, *label, Confidence::High));
            } else if name_ok
                && (names.contains(&fname.to_lowercase())
                    || names.contains(&stem(&fname).to_lowercase()))
            {
                out.push((entry, *label, Confidence::Medium));
            } else if VENDOR_DIRS.contains(dir)
                && !app.apple
                && vendor != "apple"
                && vendor.chars().count() >= 3
                && fname.to_lowercase() == vendor
                && !names.contains(&vendor)
            {
                out.push((entry, *label, Confidence::Shared));
            }
        }
    }
    out
}

// ---------- several apps at once ----------

/// Plan id prefix of an uninstall of several apps ([`batch_plan`]).
pub const BATCH_PREFIX: &str = "uninstaller-batch";

/// Item group prefix in a batch plan: `app:<bundle path>`.
pub const GROUP_PREFIX: &str = "app:";

/// One app of a batch uninstall.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchApp {
    pub path: PathBuf,
    pub name: String,
    /// The [`Item::group`] of its items.
    pub group: String,
    /// Why nothing of it may move now (it runs), as its own plan says; its items are locked.
    pub lock: Option<String>,
    /// The app (or something inside its bundle) runs.
    pub running: bool,
}

/// CleanCraft's own bundle: never offered for removal in bulk (remove it by hand in Finder).
pub fn is_cleancraft(app: &Path) -> bool {
    app.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("CleanCraft.app"))
        || apps::read_bundle(app).is_some_and(|b| {
            b.bundle_id
                .eq_ignore_ascii_case("com.cristianizzo.cleancraft")
        })
}

/// The group key of an app's items in a batch plan.
pub fn group_key(app: &Path) -> String {
    format!("{GROUP_PREFIX}{}", app.display())
}

/// The app an item of a batch plan belongs to, from its group key.
pub fn group_app(it: &Item) -> Option<PathBuf> {
    it.group
        .as_deref()
        .and_then(|g| g.strip_prefix(GROUP_PREFIX))
        .map(PathBuf::from)
}

/// Is this the plan of an uninstall of several apps?
pub fn is_batch(plan: &Plan) -> bool {
    plan.id.starts_with(BATCH_PREFIX)
}

/// One plan to uninstall several apps: each app's own uninstall plan ([`uninstall_plan`]),
/// with the same preselection (the bundle and the files named after its id), joined. Every
/// item carries its app's [`group_key`]. A file two apps would list is listed once, under the
/// first app; it is selected when either plan selects it, and locked when either locks it.
/// A running app stays in the plan with its items locked, so the others can go. Apps that are
/// part of macOS, or on the ignore list, are refused.
pub fn batch_plan(
    env: &Env,
    settings: &Settings,
    app_paths: &[PathBuf],
) -> anyhow::Result<(Plan, Vec<BatchApp>)> {
    let all = apps::inventory(env, false, &|_| {});
    let mut plan = Plan::new(FeatureId::Uninstaller);
    plan.id = plan
        .id
        .replacen(FeatureId::Uninstaller.as_str(), BATCH_PREFIX, 1);
    let mut out = Vec::new();
    let mut seen: Vec<&Path> = Vec::new();
    for path in app_paths {
        if seen.contains(&path.as_path()) {
            continue;
        }
        seen.push(path);
        let Some(app) = all.iter().find(|a| &a.path == path) else {
            bail!(
                "{} is not an app in the Applications folders",
                env.display(path)
            );
        };
        if app.apple || apps::macos_app(&app.path) || apps::in_system(env, &app.path) {
            bail!("{} is part of macOS and cannot be removed", app.name);
        }
        if is_cleancraft(&app.path) {
            bail!("CleanCraft does not remove itself in a batch");
        }
        if settings.is_ignored(&app.path) {
            bail!(
                "{} is on your ignore list (Settings), so CleanCraft does not remove it",
                app.name
            );
        }
        let one = plan_for(env, settings, &all, app, false);
        let group = group_key(&app.path);
        for c in &one.categories {
            plan.add_category(&c.id, &c.title, &c.description, c.risk);
        }
        for it in one.items {
            if let Some(e) = plan.items.iter_mut().find(|e| e.path == it.path) {
                if let Some(l) = it.locked {
                    if e.locked.is_none() {
                        e.locked = Some(l);
                    }
                    e.selected = false;
                } else if e.locked.is_none() {
                    e.selected |= it.selected;
                }
                e.needs_admin |= it.needs_admin;
                continue;
            }
            let id = plan.items.len();
            plan.items.push(Item {
                id,
                group: Some(group.clone()),
                ..it
            });
        }
        plan.notes.extend(one.notes.iter().cloned());
        out.push(BatchApp {
            path: app.path.clone(),
            name: app.name.clone(),
            group,
            lock: one.notes.first().cloned(),
            running: is_running(env, &app.path),
        });
    }
    if out.is_empty() {
        bail!("No app was chosen");
    }
    Ok((plan, out))
}

/// The size an app's uninstall would move with the usual preselection (the bundle and the
/// files named after its id): what the list's selection bar adds up. One app list for all.
pub fn preselected_sizes(
    env: &Env,
    settings: &Settings,
    app_paths: &[PathBuf],
) -> Vec<(PathBuf, u64)> {
    let all = apps::inventory(env, false, &|_| {});
    app_paths
        .iter()
        .filter_map(|p| {
            let app = all.iter().find(|a| &a.path == p)?;
            let plan = plan_for(env, settings, &all, app, false);
            // A running app is locked now, but it is what it will move once it quits.
            let size = plan
                .items
                .iter()
                .filter(|i| i.selected || (i.locked.is_some() && preselected(i)))
                .map(|i| i.size)
                .sum();
            Some((p.clone(), size))
        })
        .collect()
}

/// Would this item be selected for you when nothing locks it?
fn preselected(it: &Item) -> bool {
    it.category == CAT_APP || it.category == CAT_HIGH
}

/// The app of a batch plan that `item` belongs to: its group, else (an edited plan without
/// groups) the app of the plan whose bundle holds it or whose bundle id names it on disk.
fn owner_app(plan: &Plan, item: &Item) -> Option<PathBuf> {
    if let Some(p) = group_app(item)
        && plan
            .items
            .iter()
            .any(|i| i.category == CAT_APP && i.path == p)
    {
        return Some(p);
    }
    let name = item.path.file_name()?.to_string_lossy().into_owned();
    plan.items
        .iter()
        .filter(|i| i.category == CAT_APP)
        .find(|a| {
            item.path.starts_with(&a.path)
                || apps::read_bundle(&a.path).is_some_and(|b| {
                    !b.bundle_id.is_empty() && matches_id(&name, &b.bundle_id, &[])
                })
        })
        .map(|a| a.path.clone())
}

/// Where a batch's app groups are kept, so one app of it can be put back from History.
fn batches_path(env: &Env) -> PathBuf {
    env.data_dir().join("uninstall-batches.json")
}

/// One app of a batch that ran: its name and the plan item ids of its files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchEntry {
    pub path: PathBuf,
    pub name: String,
    pub items: Vec<usize>,
}

/// How many batches are remembered for per-app Undo (the newest).
const BATCHES_KEPT: usize = 100;

type Batches = Vec<(String, Vec<BatchEntry>)>;

fn read_batches(env: &Env) -> Batches {
    std::fs::read(batches_path(env))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Remember which items of a batch plan belong to which app (before it runs).
pub fn save_batch(env: &Env, plan: &Plan, apps: &[BatchApp]) -> std::io::Result<()> {
    let entries: Vec<BatchEntry> = apps
        .iter()
        .map(|a| BatchEntry {
            path: a.path.clone(),
            name: a.name.clone(),
            items: plan
                .items
                .iter()
                .filter(|i| i.group.as_deref() == Some(a.group.as_str()))
                .map(|i| i.id)
                .collect(),
        })
        .collect();
    let mut all = read_batches(env);
    all.retain(|(id, _)| id != &plan.id);
    all.push((plan.id.clone(), entries));
    if all.len() > BATCHES_KEPT {
        all.drain(0..all.len() - BATCHES_KEPT);
    }
    std::fs::create_dir_all(env.data_dir())?;
    let path = batches_path(env);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec(&all).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

/// The apps of a batch that ran (empty for any other plan, or when it was not remembered).
pub fn batch_entries(env: &Env, plan_id: &str) -> Vec<BatchEntry> {
    read_batches(env)
        .into_iter()
        .find(|(id, _)| id == plan_id)
        .map(|(_, e)| e)
        .unwrap_or_default()
}

/// The plan item id of a journal record id (`<plan>#<item>` or `<plan>#<item>~2`).
fn record_item(plan_id: &str, record_id: &str) -> Option<usize> {
    let rest = record_id.strip_prefix(plan_id)?.strip_prefix('#')?;
    rest.split('~').next()?.parse().ok()
}

/// Journal ids of the still undoable moves of one app of a batch.
pub fn batch_app_records(env: &Env, plan_id: &str, app: &Path) -> Vec<String> {
    let Some(entry) = batch_entries(env, plan_id)
        .into_iter()
        .find(|e| e.path == app)
    else {
        return Vec::new();
    };
    crate::executor::history(env)
        .into_iter()
        .filter(|r| r.plan_id == plan_id && r.can_undo())
        .filter(|r| record_item(plan_id, &r.id).is_some_and(|k| entry.items.contains(&k)))
        .map(|r| r.id)
        .collect()
}

/// Put back one app of a batch (its bundle and its files), with one password dialog for its
/// administrator moves.
pub fn undo_batch_app(
    env: &Env,
    plan_id: &str,
    app: &Path,
) -> (usize, Vec<(String, crate::executor::UndoError)>) {
    let ids = batch_app_records(env, plan_id, app);
    crate::executor::undo_actions(env, &ids)
}

/// Carry out a confirmed batch plan: its app groups are remembered first (per-app Undo),
/// then it runs like any plan ([`crate::features::run_plan`]): one administrator command for
/// every item that needs the password, items of a running app refused.
pub fn run_batch(
    env: &Env,
    settings: &Settings,
    plan: &Plan,
    apps: &[BatchApp],
    confirmed_digest: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> anyhow::Result<crate::executor::Report> {
    if !is_batch(plan) {
        bail!("this is not a plan of several apps");
    }
    // Without it only per-app Undo is lost (Undo All still works); the journal decides.
    let _ = save_batch(env, plan, apps);
    crate::features::run_plan(env, settings, plan, confirmed_digest, progress)
}

/// What a batch did for one app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppResult {
    pub path: PathBuf,
    pub name: String,
    /// Journal ids of what moved.
    pub moved: Vec<String>,
    pub moved_size: u64,
    pub skipped: Vec<(PathBuf, crate::executor::SkipReason)>,
    /// Its bundle moved: it is uninstalled.
    pub removed: bool,
}

/// The report of a batch, per app (in the batch's order).
pub fn batch_results(
    plan: &Plan,
    apps: &[BatchApp],
    report: &crate::executor::Report,
) -> Vec<AppResult> {
    let mut out: Vec<AppResult> = apps
        .iter()
        .map(|a| AppResult {
            path: a.path.clone(),
            name: a.name.clone(),
            ..Default::default()
        })
        .collect();
    let slot = |it: &Item| {
        apps.iter()
            .position(|a| it.group.as_deref() == Some(a.group.as_str()))
    };
    for r in &report.moved {
        let Some(it) = record_item(&plan.id, &r.id).and_then(|k| plan.items.get(k)) else {
            continue;
        };
        if let Some(i) = slot(it) {
            out[i].moved.push(r.id.clone());
            out[i].moved_size += r.size;
            if it.category == CAT_APP {
                out[i].removed = true;
            }
        }
    }
    for (p, why) in &report.skipped {
        if let Some(i) = plan.items.iter().find(|it| &it.path == p).and_then(slot) {
            out[i].skipped.push((p.clone(), why.clone()));
        }
    }
    out
}

/// Is this an uninstall, reset or Trash-leftovers plan (not the feature's own scan)?
fn is_app_plan(plan: &Plan) -> bool {
    plan.items
        .iter()
        .any(|i| [CAT_APP, CAT_HIGH, CAT_MEDIUM, CAT_SHARED].contains(&i.category.as_str()))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
}

/// Apps directly in the user's Trash (real folders, not links), for the leftovers plan of an
/// app dragged there.
fn trashed_apps(env: &Env) -> Vec<AppInfo> {
    children(&env.user_trash())
        .into_iter()
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
                && std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
        })
        .filter_map(|p| {
            let b = apps::read_bundle(&p)?;
            if b.bundle_id.split('.').count() < 2 {
                return None;
            }
            Some(AppInfo {
                name: p.file_stem()?.to_string_lossy().into_owned(),
                bundle_name: b.name.clone(),
                bundle_id: b.bundle_id.clone(),
                version: b.short_version.clone(),
                build: b.version.clone(),
                size: 0,
                last_used: None,
                last_active: None,
                store: apps::Store::Other,
                vendor: String::new(),
                apple: false,
                feed_url: None,
                path: p,
            })
        })
        .collect()
}

/// Right before anything moves, an uninstall plan may only move its own apps and what
/// [`find_leftovers`] finds for them now, read again from disk: a saved plan is plain JSON,
/// and an edited one must not take another app, its settings or any other file. In a plan of
/// several apps each app item must be the app its group names; a plan of one app names one
/// app; a plan without an app (an app already in the Trash) takes only the leftovers of the
/// apps in the Trash. Anything else stops the whole run: nothing moves.
fn own_files_only(env: &Env, plan: &Plan) -> anyhow::Result<()> {
    if !is_app_plan(plan) {
        return Ok(());
    }
    let refuse = |p: &Path, why: &str| -> anyhow::Result<()> {
        bail!(
            "{} {why}, so this list was not made by CleanCraft for these apps. Nothing was moved.",
            env.display(p)
        )
    };
    let all = apps::inventory(env, false, &|_| {});
    let app_items: Vec<&Item> = plan
        .items
        .iter()
        .filter(|i| i.category == CAT_APP)
        .collect();
    if !is_batch(plan) && app_items.len() > 1 {
        return refuse(&app_items[1].path, "is a second app in a plan for one app");
    }
    let mut own: Vec<AppInfo> = Vec::new();
    for it in &app_items {
        if is_batch(plan) && group_app(it).is_none_or(|g| !same_path(&g, &it.path)) {
            return refuse(&it.path, "is not the app its group names");
        }
        match all.iter().find(|a| same_path(&a.path, &it.path)) {
            Some(a) => own.push(a.clone()),
            None => return refuse(&it.path, "is not an app in the Applications folders"),
        }
    }
    if own.is_empty() {
        own = trashed_apps(env);
    }
    let allowed: Vec<PathBuf> = own
        .iter()
        .flat_map(|a| find_leftovers(env, a, &all).into_iter().map(|(p, _, _)| p))
        .collect();
    for it in plan.selected() {
        if it.category == CAT_APP {
            continue;
        }
        if !allowed.iter().any(|a| same_path(a, &it.path)) {
            return refuse(&it.path, "is not a file of the apps in this plan");
        }
    }
    Ok(())
}

impl Feature for Uninstaller {
    fn id(&self) -> FeatureId {
        FeatureId::Uninstaller
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = ["/Applications", "~/Applications"]
            .iter()
            .map(|d| resolve(env, d))
            .collect();
        v.extend(LEFTOVER_DIRS.iter().map(|(d, _)| resolve(env, d)));
        v.push(env.home_path("Downloads"));
        v.extend(extensions::dirs(env).into_iter().map(|(p, _)| p));
        v.sort();
        v.dedup();
        v
    }

    /// Apps installed by a package, leftovers and extensions in folders of the system move
    /// with the administrator password.
    fn allows_admin(&self) -> bool {
        true
    }

    /// Leftovers of removed apps, installer files in Downloads and extensions.
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut plan = Plan::new(FeatureId::Uninstaller);
        let allow = self.allow_roots(ctx.env);
        (ctx.progress)("Applications");
        let all = apps::inventory(ctx.env, false, &|_| {});
        leftovers::scan_orphans(ctx, &allow, &all, &mut plan);
        if !ctx.cancelled() {
            leftovers::scan_installers(ctx, &allow, &mut plan);
        }
        if !ctx.cancelled() {
            extensions::scan(ctx, &allow, &mut plan);
        }
        if ctx.cancelled() {
            plan.notes.push("Scan stopped before the end.".into());
        }
        Ok(plan)
    }

    /// Again before anything moves, whatever the plan's categories say: nothing inside a
    /// running app.
    fn validate(
        &self,
        env: &Env,
        _settings: &Settings,
        plan: &Plan,
        item: &crate::plan::Item,
    ) -> Result<(), String> {
        for a in item.path.ancestors() {
            if a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) && is_running(env, a) {
                return Err(format!(
                    "Quit {} first",
                    a.file_stem().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        // Several apps: the files of an app that runs stay, the other apps go on.
        if is_batch(plan)
            && let Some(owner) = owner_app(plan, item)
            && is_running(env, &owner)
        {
            return Err(format!(
                "Quit {} first",
                owner.file_stem().unwrap_or_default().to_string_lossy()
            ));
        }
        Ok(())
    }

    /// Nothing that is, is inside or holds an app of macOS, or is named after an Apple id, may
    /// be selected, whatever the plan's categories say; one is enough to stop the whole run.
    fn validate_set(&self, env: &Env, _settings: &Settings, plan: &Plan) -> anyhow::Result<()> {
        for it in plan.selected() {
            if let Some(why) = apps::apple_part(&it.path, 3, true) {
                bail!("{why}: {}. Nothing was moved.", env.display(&it.path));
            }
        }
        own_files_only(env, plan)
    }

    /// Right before anything moves: no app of the plan may be running, and no app that is
    /// part of macOS may be selected.
    fn prepare(&self, env: &Env, plan: &Plan) -> anyhow::Result<()> {
        for it in plan.items.iter().filter(|i| i.category == CAT_APP) {
            let name = it
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            // One app: nothing moves while it runs. Several: its files were refused one by one
            // ([`Feature::validate`]) and the other apps go on.
            if is_running(env, &it.path) && (it.selected || !is_batch(plan)) {
                bail!("Quit {name} first. Nothing was moved.");
            }
            if it.selected && (apps::macos_app(&it.path) || apps::in_system(env, &it.path)) {
                bail!("{name} is part of macOS and cannot be removed.");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_parts() {
        assert_eq!(id_part("com.x.App.plist"), "com.x.App");
        assert_eq!(id_part("com.x.App.savedState"), "com.x.App");
        assert_eq!(id_part("ABCDE12345.com.x.App"), "com.x.App");
        assert_eq!(id_part("group.com.x.App"), "com.x.App");
        assert_eq!(id_part("ABCDE12345.group.com.x.App"), "com.x.App");
    }

    #[test]
    fn id_matching() {
        assert!(matches_id("com.x.App", "com.x.App", &[]));
        assert!(matches_id("com.x.app.plist", "com.x.App", &[]));
        assert!(matches_id("com.x.App.helper.plist", "com.x.App", &[]));
        assert!(!matches_id("com.x.Apple", "com.x.App", &[]));
        assert!(!matches_id(
            "com.x.App.pro",
            "com.x.App",
            &["com.x.App.pro"]
        ));
        assert!(!matches_id(
            "com.x.App.pro.plist",
            "com.x.App",
            &["com.x.App.pro"]
        ));
    }
}
