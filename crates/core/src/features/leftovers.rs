//! Files not tied to an installed app: leftovers of apps that are gone (A5) and installer
//! files in Downloads (A6). Both are part of the App Remover's own scan.
//!
//! Leftovers are found conservatively: an entry is offered only with positive evidence that it
//! belonged to an app that is gone ([`orphan_evidence`]):
//! - its name is a bundle id with a maker and an app part (`com.maker.App`);
//! - it is not a shared framework, library or macOS id ([`SHARED_IDS`]);
//! - it matches no installed bundle: no app, no bundle embedded in an app (extensions,
//!   XPC services, frameworks, login items, Quick Look plug-ins) and no settings pane, file
//!   system or plug-in in the Library ([`crate::apps::installed_ids`]), exactly or as a
//!   prefix either way (`com.autodesk.AutoCAD2026.AcQuickLookPreviewer` belongs to the installed
//!   `com.autodesk.AutoCAD2026`);
//! - no installed app comes from the same maker (`com.maker.*`, or the same maker part with
//!   another first part, `org.freecad.FreeCAD` keeps `com.freecad.FreeCAD`) or carries the
//!   app part as its name;
//! - Spotlight finds no app with that id anywhere on the Mac. When Spotlight cannot be asked,
//!   nothing is proposed.
//!
//! Entries are never selected for you. An app that left a container or saved windows, or
//! files in two kinds of folders, is a sure match; a single cache, log or settings file is a
//! medium one and says "Possibly from an app that is gone". Startup agents are left to Startup
//! Items.

use std::collections::{BTreeMap, BTreeSet};
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

/// Only an app leaves these: a sandbox container or saved windows.
const APP_ONLY_DIRS: &[&str] = &["~/Library/Containers", "~/Library/Saved Application State"];

/// Shared ids that are never a leftover of one removed app, in one place. Each entry matches
/// the id itself and every id below it (`org.cups` matches `org.cups.PrintingPrefs`); an
/// entry ending in `*` matches any id starting with it (`com.google.firebase*` matches
/// `com.google.firebase.crashlytics` and `com.google.firebaseinstallations`).
pub const SHARED_IDS: &[&str] = &[
    // macOS itself and its parts.
    "com.apple",
    "org.cups",
    "org.webkit",
    "com.openssh",
    // Update frameworks many apps ship.
    "org.sparkle-project",
    "com.microsoft.autoupdate*",
    "com.google.keystone",
    "com.google.googlesoftwareupdate",
    "com.github.squirrel",
    // Language runtimes.
    "net.java",
    "com.oracle.java",
    "org.openjdk",
    "org.python",
    "org.r-project.r.framework",
    "org.gnu",
    // Crash reporting, analytics and app SDKs.
    "com.crashlytics",
    "io.fabric",
    "com.firebase",
    "com.google.firebase*",
    "com.google.gtm",
    "com.google.analytics",
    "com.segment",
    "io.sentry*",
    "com.getsentry",
    "com.bugsnag",
    "com.hockeyapp",
    "net.hockeyapp",
    "com.plausiblelabs",
    "com.amplitude",
    "com.mixpanel",
    "io.branch",
    "com.appcenter",
    "com.microsoft.appcenter",
    "com.datadoghq",
    "com.newrelic",
    "org.chromium.crashpad",
    // Data shared by every Mozilla app.
    "org.mozilla",
    // File system and driver kits installed for many apps.
    "io.macfuse",
    "com.github.osxfuse",
    "org.osxfuse",
];

/// Is `id` (any case) a shared framework, library or macOS id ([`SHARED_IDS`])?
pub fn is_shared_id(id: &str) -> bool {
    let l = id.to_lowercase();
    SHARED_IDS.iter().any(|p| match p.strip_suffix('*') {
        Some(pre) => l.starts_with(pre),
        None => l == *p || l.starts_with(&format!("{p}.")),
    })
}

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

/// Bundle id parts that never name a maker (`com.app.x`, `io.github.x`).
const NOT_MAKER: &[&str] = &[
    "app",
    "apps",
    "co",
    "com",
    "dev",
    "github",
    "gitlab",
    "io",
    "mac",
    "macos",
    "net",
    "org",
    "osx",
    "www",
    "example",
    "local",
    "localhost",
];

/// Is this id still in use: an installed app with the same id, a sub-id or the same maker
/// (macOS ids always are)?
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

/// Is there positive evidence that `id` belonged to an app that is gone? False when it is
/// shared, installed (also inside an app), or not clearly an app's. `installed` is
/// [`crate::apps::installed_ids`].
pub fn orphan_evidence(id: &str, apps: &[AppInfo], installed: &BTreeSet<String>) -> bool {
    let l = id.to_lowercase();
    let parts: Vec<&str> = l.split('.').collect();
    if parts.len() < 3 || is_shared_id(&l) || belongs_to_installed(&l, apps) {
        return false;
    }
    let (maker, app_part) = (parts[1], parts[2]);
    if maker.len() < 2 || NOT_MAKER.contains(&maker) || app_part.len() < 2 {
        return false;
    }
    // An installed bundle (an app or one embedded in it) with this id, a sub-id or a parent id.
    if installed
        .iter()
        .any(|b| l == *b || l.starts_with(&format!("{b}.")) || b.starts_with(&format!("{l}.")))
    {
        return false;
    }
    // The same maker with another first part, or an installed app carrying the app part's name.
    !apps.iter().any(|a| {
        let b = a.bundle_id.to_lowercase();
        let bp: Vec<&str> = b.split('.').collect();
        (bp.len() >= 2 && bp[1] == maker && maker.len() >= 3)
            || a.name.to_lowercase().replace(' ', "") == app_part
            || a.bundle_name.to_lowercase().replace(' ', "") == app_part
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
    let installed = crate::apps::installed_ids(env, apps);
    // id -> entries (path, label, folder)
    type Found<'a> = BTreeMap<String, Vec<(PathBuf, &'a str, &'a str)>>;
    let mut found: Found = BTreeMap::new();
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
            if !orphan_evidence(&id, apps, &installed) || !ctx.allowed(allow, &entry) {
                continue;
            }
            found.entry(id).or_default().push((entry, label, dir));
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
        // Sure: an app container or saved windows (only apps leave them), or files in two
        // kinds of folders. Else medium.
        let kinds: BTreeSet<&str> = entries.iter().map(|(_, _, d)| *d).collect();
        let sure = kinds.len() >= 2 || kinds.iter().any(|d| APP_ONLY_DIRS.contains(d));
        for (path, label, _) in entries {
            let size = tree_size(&path).0;
            if size == 0 {
                continue;
            }
            let why = if sure {
                format!("{label} of {id}, an app that is no longer installed")
            } else {
                format!("{label} of {id}. Possibly from an app that is gone")
            };
            let i = plan.push(path, size, CAT_ORPHANS, why, Risk::Medium, false);
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

    #[test]
    fn shared_ids() {
        for id in [
            "org.cups.PrintingPrefs",
            "org.sparkle-project.Sparkle.Autoupdate",
            "net.java.openjdk.java",
            "org.python.python",
            "com.crashlytics.data",
            "com.firebase.FIRInstallations",
            "com.google.firebase.crashlytics",
            "com.google.firebaseinstallations",
            "com.segment.storage.oai",
            "io.sentry",
            "io.sentrycocoa.x",
            "org.mozilla.firefox.shared",
            "com.microsoft.autoupdate2",
            "com.microsoft.autoupdate.fba",
            "com.apple.finder",
            "io.macfuse.app",
        ] {
            assert!(is_shared_id(id), "{id}");
        }
        for id in [
            "com.gone.Writer",
            "org.cupsy.x",
            "com.segmentify.App",
            "com.crashlyticsish.a",
        ] {
            assert!(!is_shared_id(id), "{id}");
        }
    }

    #[test]
    fn evidence() {
        let none = BTreeSet::new();
        assert!(orphan_evidence("com.gone.Writer", &[], &none));
        // No maker or app part.
        assert!(!orphan_evidence("io.github.x", &[], &none));
        assert!(!orphan_evidence("com.app.Thing", &[], &none));
        // Embedded or installed bundles, either way round.
        let inst: BTreeSet<String> = ["com.autodesk.autocad2026".to_string()].into();
        assert!(!orphan_evidence(
            "com.autodesk.AutoCAD2026.AcQuickLookPreviewer",
            &[],
            &inst
        ));
        let inst: BTreeSet<String> = ["com.x.app.helper".to_string()].into();
        assert!(!orphan_evidence("com.x.App", &[], &inst));
    }
}
