//! Files not tied to an installed app: leftovers of apps that are gone (A5) and installer
//! files in Downloads (A6). Both are part of the App Remover's own scan.
//!
//! Leftovers are found conservatively. An entry counts only when its name is a bundle id
//! (`com.maker.App`), the id is not Apple's, no installed app has that id or one from the same
//! maker (`com.maker.*`), and Spotlight finds no app with that id anywhere on the Mac. When
//! Spotlight cannot be asked, nothing is proposed. Startup agents are left to Startup Items.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::apps::{AppInfo, resolve};
use crate::features::ScanCtx;
use crate::features::uninstaller::id_part;
use crate::fsutil::{children, tree_size};
use crate::plan::{Plan, Risk};

pub const CAT_ORPHANS: &str = "leftovers";
pub const CAT_INSTALLERS: &str = "installers";

/// Folders searched for leftovers of removed apps.
pub const ORPHAN_DIRS: &[(&str, &str)] = &[
    ("~/Library/Application Support", "Support files"),
    ("~/Library/Caches", "Cache"),
    ("~/Library/Preferences", "Preferences"),
    ("~/Library/Containers", "App container"),
    ("~/Library/Saved Application State", "Saved window state"),
    ("~/Library/HTTPStorages", "Web storage"),
    ("~/Library/WebKit", "Web data"),
    ("~/Library/Cookies", "Cookies"),
    ("~/Library/Logs", "Logs"),
];

/// The bundle id a Library entry is named after, if its name is one.
pub fn bundle_id_in(name: &str) -> Option<String> {
    let n = id_part(name);
    let parts: Vec<&str> = n.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let tld = parts[0];
    if !(2..=6).contains(&tld.len()) || !tld.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let ok = parts.iter().all(|p| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    ok.then(|| n.to_string())
}

fn vendor_key(id: &str) -> String {
    id.split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
        .to_lowercase()
}

/// Is this id still in use by an installed app (same id, a sub-id, or the same maker)?
pub fn belongs_to_installed(id: &str, apps: &[AppInfo]) -> bool {
    let l = id.to_lowercase();
    if l.starts_with("com.apple.") {
        return true;
    }
    let vk = vendor_key(&l);
    apps.iter().any(|a| {
        let b = a.bundle_id.to_lowercase();
        l == b
            || l.starts_with(&format!("{b}."))
            || b.starts_with(&format!("{l}."))
            || vendor_key(&b) == vk
    })
}

pub fn scan_orphans(ctx: &ScanCtx<'_>, allow: &[PathBuf], apps: &[AppInfo], plan: &mut Plan) {
    let env = ctx.env;
    plan.add_category(
        CAT_ORPHANS,
        "Leftovers of removed apps",
        "Settings and data of apps that are no longer on this Mac.",
        Risk::Medium,
    );
    // id -> entries
    let mut found: BTreeMap<String, Vec<(PathBuf, &str)>> = BTreeMap::new();
    for (dir, label) in ORPHAN_DIRS {
        if ctx.cancelled() {
            return;
        }
        let root = resolve(env, dir);
        (ctx.progress)(&env.display(&root));
        for entry in children(&root) {
            let Some(name) = entry.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some(id) = bundle_id_in(&name) else {
                continue;
            };
            if belongs_to_installed(&id, apps) || !ctx.allowed(allow, &entry) {
                continue;
            }
            found.entry(id).or_default().push((entry, label));
        }
    }
    let mut spotlight_failed = false;
    for (id, entries) in found {
        // An app with this id elsewhere on the Mac (another disk, a folder) keeps its files.
        let query = format!("kMDItemCFBundleIdentifier == '{id}'");
        match env.sys.run("/usr/bin/mdfind", &[&query]) {
            Ok(o) if o.ok() => {
                if !o.stdout.trim().is_empty() {
                    continue;
                }
            }
            _ => {
                spotlight_failed = true;
                continue;
            }
        }
        for (path, label) in entries {
            let size = tree_size(&path).0;
            if size == 0 {
                continue;
            }
            let i = plan.push(
                path,
                size,
                CAT_ORPHANS,
                format!("{label} of {id}, which is no longer installed"),
                Risk::Medium,
                false,
            );
            plan.items[i].group = Some(id.clone());
        }
    }
    if spotlight_failed {
        plan.notes.push(
            "Spotlight could not confirm that some apps are gone, so their files are not listed."
                .into(),
        );
    }
}

/// `.pkg`, `.mpkg` and `.dmg` files at the top of Downloads.
pub fn scan_installers(ctx: &ScanCtx<'_>, allow: &[PathBuf], plan: &mut Plan) {
    let env = ctx.env;
    plan.add_category(
        CAT_INSTALLERS,
        "Installer files",
        "Installer packages and disk images in Downloads. The apps they installed stay.",
        Risk::Medium,
    );
    let dl = env.home_path("Downloads");
    (ctx.progress)(&env.display(&dl));
    for entry in children(&dl) {
        let ext = entry
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let what = match ext.as_str() {
            "pkg" | "mpkg" => "Installer package",
            "dmg" => "Disk image",
            _ => continue,
        };
        if !ctx.allowed(allow, &entry) {
            continue;
        }
        let size = tree_size(&entry).0;
        plan.push(entry, size, CAT_INSTALLERS, what, Risk::Medium, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_in_names() {
        assert_eq!(
            bundle_id_in("com.gone.App.plist").as_deref(),
            Some("com.gone.App")
        );
        assert_eq!(
            bundle_id_in("com.gone.App").as_deref(),
            Some("com.gone.App")
        );
        assert_eq!(bundle_id_in("Google"), None);
        assert_eq!(bundle_id_in("loginwindow.plist"), None);
        assert_eq!(bundle_id_in("Some Folder.with.dots"), None);
    }
}
