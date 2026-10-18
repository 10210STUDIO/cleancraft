//! The apps on this Mac: every `.app` in `/Applications`, `~/Applications` (and one level of
//! subfolders such as `/Applications/Utilities` or `/Applications/Setapp`) and the macOS apps in
//! `/System/Applications`.
//!
//! For each app: name, bundle id, version, size (the shared [`tree_size`] formula), when it was
//! last opened (Spotlight, `mdls -name kMDItemLastUsedDate -raw`), where it came from (App Store
//! receipt, Setapp folder, other), the maker (from the bundle id) and whether it is part of macOS
//! (never removable).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::Env;
use crate::fsutil::{children, tree_size};

/// Apps not opened for this many days are in the "Unused" group.
pub const UNUSED_DAYS: i64 = 180;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    AppStore,
    Setapp,
    Other,
}

impl Store {
    pub fn label(self) -> &'static str {
        match self {
            Store::AppStore => "App Store",
            Store::Setapp => "Setapp",
            Store::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AppInfo {
    pub path: PathBuf,
    /// What Finder shows: the bundle's file name without `.app`.
    pub name: String,
    /// `CFBundleName`, used with `name` to find leftovers named after the app.
    pub bundle_name: String,
    pub bundle_id: String,
    /// `CFBundleShortVersionString` (what people call the version).
    pub version: String,
    /// `CFBundleVersion` (the build number Sparkle compares).
    pub build: String,
    /// Bytes on disk; 0 in a quick listing.
    pub size: u64,
    /// Last opened, seconds since the epoch, when Spotlight knows.
    pub last_used: Option<i64>,
    pub store: Store,
    /// The maker, from the bundle id: "com.google.Chrome" gives "Google".
    pub vendor: String,
    /// Part of macOS or signed by Apple as macOS software: never removable.
    pub apple: bool,
    /// Sparkle appcast address (`SUFeedURL`), when the app has one.
    pub feed_url: Option<String>,
}

impl AppInfo {
    /// Not opened for [`UNUSED_DAYS`]. Unknown dates are never "unused".
    pub fn unused(&self, now: i64) -> bool {
        self.last_used
            .is_some_and(|t| now - t > UNUSED_DAYS * 86_400)
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The keys CleanCraft reads from `Contents/Info.plist`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleInfo {
    pub bundle_id: String,
    pub name: String,
    pub short_version: String,
    pub version: String,
    pub feed_url: Option<String>,
}

/// Read an app's Info.plist (binary or XML).
pub fn read_bundle(app: &Path) -> Option<BundleInfo> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let d = v.as_dictionary()?;
    let s = |k: &str| {
        d.get(k)
            .and_then(|v| v.as_string())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let bundle_id = s("CFBundleIdentifier");
    if bundle_id.is_empty() {
        return None;
    }
    let feed = s("SUFeedURL");
    Some(BundleInfo {
        bundle_id,
        name: {
            let n = s("CFBundleName");
            if n.is_empty() {
                s("CFBundleDisplayName")
            } else {
                n
            }
        },
        short_version: s("CFBundleShortVersionString"),
        version: s("CFBundleVersion"),
        feed_url: (!feed.is_empty()).then_some(feed),
    })
}

/// "com.google.Chrome" -> "Google", "org.mozilla.firefox" -> "Mozilla".
pub fn vendor_of(bundle_id: &str) -> String {
    let parts: Vec<&str> = bundle_id.split('.').collect();
    let raw = if parts.len() >= 3 {
        parts[1]
    } else if parts.len() == 2 {
        parts[0]
    } else {
        ""
    };
    if raw.is_empty() {
        return "Unknown".into();
    }
    let mut c = raw.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Unknown".into(),
    }
}

/// `mdls -raw` prints "2024-05-01 10:22:33 +0000", or "(null)" when Spotlight has no date.
pub fn parse_mdls_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s == "(null)" {
        return None;
    }
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z")
        .ok()
        .map(|d| d.timestamp())
}

/// True when `codesign -dv` output names Apple's own macOS signing authority.
pub fn signed_by_apple(codesign_out: &str) -> bool {
    codesign_out
        .lines()
        .any(|l| l.trim() == "Authority=Software Signing")
}

/// The folders apps live in, as (absolute macOS path, apps there are part of macOS).
pub const APP_DIRS: &[(&str, bool)] = &[
    ("/Applications", false),
    ("~/Applications", false),
    ("/System/Applications", true),
];

pub fn resolve(env: &Env, p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rel) => env.home_path(rel),
        None => env.sys_path(p),
    }
}

fn is_app(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "app")
        && std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

/// Every app bundle path: the app folders and one level of subfolders. Symbolic links are
/// not followed. Each entry says whether it lives in a macOS system folder.
pub fn app_paths(env: &Env) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    for (dir, system) in APP_DIRS {
        let root = resolve(env, dir);
        for c in children(&root) {
            if is_app(&c) {
                out.push((c, *system));
            } else if std::fs::symlink_metadata(&c).is_ok_and(|m| m.is_dir()) {
                for cc in children(&c) {
                    if is_app(&cc) {
                        out.push((cc, *system));
                    }
                }
            }
        }
    }
    out
}

/// The app list. `deep` adds size, last-opened date and the signature check (slower: one
/// `mdls` and one `codesign` per app); without it the list is quick, for matching leftovers.
pub fn inventory(env: &Env, deep: bool, progress: &dyn Fn(&str)) -> Vec<AppInfo> {
    let setapp = env.sys_path("/Applications/Setapp");
    let mut out = Vec::new();
    for (path, system) in app_paths(env) {
        let Some(b) = read_bundle(&path) else {
            continue;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        progress(&name);
        let store = if path.join("Contents/_MASReceipt/receipt").is_file() {
            Store::AppStore
        } else if path.starts_with(&setapp) {
            Store::Setapp
        } else {
            Store::Other
        };
        let mut apple = system || b.bundle_id.starts_with("com.apple.");
        let shown = env.display(&path);
        let (mut size, mut last_used) = (0, None);
        if deep {
            size = tree_size(&path).0;
            last_used = env
                .sys
                .run(
                    "/usr/bin/mdls",
                    &["-name", "kMDItemLastUsedDate", "-raw", &shown],
                )
                .ok()
                .filter(|o| o.ok())
                .and_then(|o| parse_mdls_date(&o.stdout));
            if !apple
                && let Ok(o) = env
                    .sys
                    .run("/usr/bin/codesign", &["-dv", "--verbose=2", &shown])
            {
                // codesign writes its report to stderr.
                apple = signed_by_apple(&o.stderr) || signed_by_apple(&o.stdout);
            }
        }
        out.push(AppInfo {
            vendor: if apple {
                "Apple".into()
            } else {
                vendor_of(&b.bundle_id)
            },
            path,
            name,
            bundle_name: b.name,
            bundle_id: b.bundle_id,
            version: b.short_version,
            build: b.version,
            size,
            last_used,
            store,
            apple,
            feed_url: b.feed_url,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Find an app by path (shown or real), file name, name or bundle id, ignoring case.
pub fn find<'a>(env: &Env, apps: &'a [AppInfo], query: &str) -> Option<&'a AppInfo> {
    let q = query.trim().trim_end_matches('/');
    let ql = q.to_lowercase();
    apps.iter()
        .find(|a| env.display(&a.path) == q || a.path == Path::new(q))
        .or_else(|| apps.iter().find(|a| a.bundle_id.to_lowercase() == ql))
        .or_else(|| {
            apps.iter()
                .find(|a| a.name.to_lowercase() == ql || a.file_name().to_lowercase() == ql)
        })
}

/// "3 days ago", "Never" style text for the last-opened column.
pub fn last_used_text(last_used: Option<i64>, now: i64) -> String {
    match last_used {
        None => "Unknown".into(),
        Some(t) => {
            // Calendar days on the user's clock: an app opened late yesterday is "Yesterday".
            let days = crate::fsutil::calendar_days_ago(t, now);
            match days {
                0 => "Today".into(),
                1 => "Yesterday".into(),
                d if d < 60 => format!("{d} days ago"),
                d if d < 730 => format!("{} months ago", d / 30),
                d => format!("{} years ago", d / 365),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors() {
        assert_eq!(vendor_of("com.google.Chrome"), "Google");
        assert_eq!(vendor_of("org.mozilla.firefox"), "Mozilla");
        assert_eq!(vendor_of("foo"), "Unknown");
        assert_eq!(vendor_of("io.x"), "Io");
    }

    #[test]
    fn mdls_dates() {
        assert_eq!(
            parse_mdls_date("2024-05-01 10:22:33 +0000\n"),
            Some(1_714_558_953)
        );
        assert_eq!(parse_mdls_date("(null)"), None);
        assert_eq!(parse_mdls_date("garbage"), None);
    }

    #[test]
    fn apple_signature() {
        let apple = "Executable=/Applications/Safari.app/Contents/MacOS/Safari\nAuthority=Software Signing\nAuthority=Apple Code Signing Certification Authority\nAuthority=Apple Root CA\n";
        let mas = "Authority=Apple Mac OS Application Signing\nAuthority=Apple Worldwide Developer Relations Certification Authority\n";
        let devid = "Authority=Developer ID Application: Example Ltd (ABCDE12345)\n";
        assert!(signed_by_apple(apple));
        assert!(!signed_by_apple(mas));
        assert!(!signed_by_apple(devid));
    }

    #[test]
    fn last_used_words() {
        let now = 1_000_000_000;
        assert_eq!(last_used_text(None, now), "Unknown");
        assert_eq!(last_used_text(Some(now - 100), now), "Today");
        assert_eq!(last_used_text(Some(now - 3 * 86_400), now), "3 days ago");
        assert_eq!(
            last_used_text(Some(now - 200 * 86_400), now),
            "6 months ago"
        );
    }
}
