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
use crate::plan::{FeatureId, Plan, Risk};
use crate::policy;
use crate::settings::Settings;

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
    let mut plan = Plan::new(FeatureId::Uninstaller);
    let allow = Uninstaller.allow_roots(env);
    plan.add_category(CAT_APP, "Application", "The app itself.", Risk::High);
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
    for (path, label, conf) in find_leftovers(env, &app, &all) {
        if settings.is_ignored(&path) || policy::check(env, &allow, &path).is_err() {
            continue;
        }
        let size = tree_size(&path).0;
        let (cat, why, sel) = match conf {
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
        let id = plan.push(path.clone(), size, cat, why, Risk::High, sel);
        if let Some(parent) = path.parent()
            && !env.can_modify(parent)
        {
            plan.mark_admin(id);
        }
    }
    if let Some(why) = app_lock(env, &app) {
        for i in 0..plan.items.len() {
            plan.lock(i, why.clone());
        }
        plan.notes.push(why);
    }
    Ok(plan)
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
    let vendor = app.vendor.to_lowercase();
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
                && vendor != "unknown"
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

    /// Right before anything moves: no app of the plan may be running, and no app that is
    /// part of macOS may be selected.
    fn prepare(&self, env: &Env, plan: &Plan) -> anyhow::Result<()> {
        for it in plan.items.iter().filter(|i| i.category == CAT_APP) {
            let name = it
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if is_running(env, &it.path) {
                bail!("Quit {name} first. Nothing was moved.");
            }
            if it.selected
                && apps::read_bundle(&it.path)
                    .is_some_and(|b| b.bundle_id.starts_with("com.apple."))
            {
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
