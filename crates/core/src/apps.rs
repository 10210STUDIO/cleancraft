//! The apps on this Mac: every `.app` in `/Applications`, `~/Applications` (and the folders
//! inside them up to [`APP_DEPTH`] levels, such as `/Applications/Utilities`,
//! `/Applications/Setapp` or a suite folder like `/Applications/Autodesk/AutoCAD 2026`) and the
//! macOS apps in `/System/Applications`.
//!
//! Suites: in a folder of its own (not an app folder itself), the app named like the folder is
//! the suite's app and the other apps there are its helpers ([`AppInfo::part_of`]). An
//! uninstaller the maker ships next to an app ("Remove AutoCAD 2026.app",
//! "Uninstall Foo.app", "Foo Uninstaller.app") is offered for it
//! ([`AppInfo::vendor_uninstaller`]).
//!
//! For each app: name, bundle id, version, size (the shared [`tree_size`] formula), when it was
//! last opened (Spotlight, `mdls -name kMDItemLastUsedDate -raw`), where it came from (App Store
//! receipt, Setapp folder, other), the developer ([`developer_name`]: the code signature, else the
//! copyright line of Info.plist) and whether it is part of macOS (never removable: only apps in
//! `/System`, or Apple's own apps installed with macOS such as Safari; Apple's App Store apps
//! like iMovie or Pages are removable).

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
    /// Last opened, seconds since the epoch, when Spotlight knows (`kMDItemLastUsedDate`, else
    /// its day-precision copy `kMDItemLastUsedDate_Ranking`).
    pub last_used: Option<i64>,
    /// When Spotlight has no date: the newest change to the app's own settings or saved
    /// windows ([`app_data_paths`]). A hint that the app ran, never used for "Unused".
    #[serde(default)]
    pub last_active: Option<i64>,
    pub store: Store,
    /// The developer: the name in the Developer ID signature ("Google LLC"), else the copyright
    /// line of Info.plist, else a known name from the bundle id ([`developer_name`]).
    pub vendor: String,
    /// Part of macOS, never removable: in `/System` (`/System/Applications`,
    /// `/System/Library/CoreServices`), an Apple app installed with macOS (`com.apple.` id and
    /// no App Store receipt, such as Safari) or signed as macOS software ("Software Signing").
    /// Apple's App Store apps (iMovie, GarageBand, Pages, Numbers, Keynote) are not.
    pub apple: bool,
    /// Sparkle appcast address (`SUFeedURL`), when the app has one.
    pub feed_url: Option<String>,
    /// The suite's own app when this app is one of its helpers (an app next to it in the
    /// suite folder, its maker's uninstaller included). See [`link_suites`].
    #[serde(default)]
    pub part_of: Option<PathBuf>,
    /// The uninstaller the maker ships next to this app ("Remove AutoCAD 2026.app"). It also
    /// removes licensing services and background helpers the maker installed, so it is the
    /// recommended way to remove the app.
    #[serde(default)]
    pub vendor_uninstaller: Option<PathBuf>,
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
    /// `NSHumanReadableCopyright`, as written.
    pub copyright: String,
    /// `CFBundleGetInfoString` (older apps put their copyright there), as written.
    pub get_info: String,
}

/// Read an app's Info.plist (binary or XML).
/// Why `p` must never be removed as part of macOS, if it must not: it is, is inside, or holds
/// (up to `depth` folders down) an app whose bundle id starts with `com.apple.`, or it is a
/// Library entry named after an Apple id (`com.apple.Safari.plist`, `group.com.apple.notes`)
/// when `by_name` (off for Threat Check: unwanted software often borrows Apple names).
/// Read from disk now, so an edited plan cannot hide it.
pub fn apple_part(p: &Path, depth: usize, by_name: bool) -> Option<String> {
    let apple_id = |id: &str| id.to_ascii_lowercase().starts_with("com.apple.");
    let apple_app =
        |a: &Path| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) && macos_app(a);
    for a in p.ancestors() {
        if apple_app(a) {
            let name = a.file_stem().map(|s| s.to_string_lossy().into_owned());
            return Some(format!(
                "{} is part of macOS and cannot be removed",
                name.unwrap_or_default()
            ));
        }
    }
    if by_name
        && let Some(n) = p.file_name().and_then(|n| n.to_str())
        && apple_id(crate::features::uninstaller::id_part(n))
    {
        return Some(format!("{n} belongs to macOS"));
    }
    let mut stack = vec![(p.to_path_buf(), 0usize)];
    while let Some((dir, d)) = stack.pop() {
        if d >= depth || !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        for c in crate::fsutil::children(&dir) {
            if apple_app(&c) {
                return Some(format!("it holds {}, part of macOS", c.display()));
            }
            stack.push((c, d + 1));
        }
    }
    None
}

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
        copyright: s("NSHumanReadableCopyright"),
        get_info: s("CFBundleGetInfoString"),
    })
}

/// The bundle has an App Store receipt (`Contents/_MASReceipt/receipt`).
pub fn has_store_receipt(app: &Path) -> bool {
    app.join("Contents/_MASReceipt/receipt").is_file()
}

/// An app of macOS, read from disk: a `com.apple.` bundle id and no App Store receipt (Safari,
/// the apps in `/System/Applications` and `/System/Library/CoreServices`). Apple's App Store
/// apps (iMovie, GarageBand, Pages, Numbers, Keynote, Xcode) carry a receipt and are not.
pub fn macos_app(app: &Path) -> bool {
    read_bundle(app).is_some_and(|b| b.bundle_id.to_ascii_lowercase().starts_with("com.apple."))
        && !has_store_receipt(app)
}

/// Shown when no source names the developer.
pub const UNKNOWN_DEVELOPER: &str = "Unknown developer";

/// The maker's part of a bundle id, lower case: "com.jetbrains.goland" gives "jetbrains"
/// ("" when there is none). Used to find a maker's shared folders, never shown.
pub fn vendor_segment(bundle_id: &str) -> String {
    let parts: Vec<&str> = bundle_id.split('.').collect();
    let raw = match parts.len() {
        0 | 1 => "",
        2 => parts[0],
        _ => parts[1],
    };
    raw.to_lowercase()
}

/// Makers whose bundle id part is not their name as written.
const KNOWN_MAKERS: &[(&str, &str)] = &[
    ("adobe", "Adobe"),
    ("apple", "Apple"),
    ("belightsoft", "BeLight Software"),
    ("blenderfoundation", "Blender Foundation"),
    ("docker", "Docker"),
    ("figma", "Figma"),
    ("freecad", "FreeCAD"),
    ("github", "GitHub"),
    ("google", "Google"),
    ("jetbrains", "JetBrains"),
    ("microsoft", "Microsoft"),
    ("mozilla", "Mozilla"),
    ("openai", "OpenAI"),
    ("tinyspeck", "Slack"),
    ("valvesoftware", "Valve"),
    ("videolan", "VideoLAN"),
    ("anthropic", "Anthropic"),
    ("spotify", "Spotify"),
    ("screamingfrog", "Screaming Frog"),
];

/// Bundle id parts that are never a maker's name.
const NOT_MAKERS: &[&str] = &[
    "app", "apps", "co", "com", "corp", "dev", "gmbh", "inc", "io", "llc", "ltd", "mac", "macos",
    "net", "org", "osx", "www",
];

/// Last resort: the maker from the bundle id, only when it reads as a name: a known maker
/// ("com.jetbrains.goland" gives "JetBrains"), a part its developer wrote in mixed case
/// ("com.SweetScape.010Editor" gives "SweetScape") or a plain word of 4 letters or more
/// ("com.example.Editor" gives "Example"). Fragments such as "co" or "io" give
/// [`UNKNOWN_DEVELOPER`].
pub fn vendor_of(bundle_id: &str) -> String {
    let parts: Vec<&str> = bundle_id.split('.').collect();
    let raw = match parts.len() {
        0 | 1 => "",
        2 => parts[0],
        _ => parts[1],
    };
    let low = raw.to_lowercase();
    if let Some((_, name)) = KNOWN_MAKERS.iter().find(|(k, _)| *k == low) {
        return (*name).into();
    }
    if NOT_MAKERS.contains(&low.as_str()) || !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return UNKNOWN_DEVELOPER.into();
    }
    let letters = raw.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if raw.chars().skip(1).any(|c| c.is_ascii_uppercase()) && letters >= 3 {
        return raw.into();
    }
    if letters >= 4 && raw.chars().all(|c| c.is_ascii_lowercase()) {
        let mut c = raw.chars();
        if let Some(f) = c.next() {
            return f.to_uppercase().chain(c).collect();
        }
    }
    UNKNOWN_DEVELOPER.into()
}

/// Who signed an app, from `codesign -dv --verbose=2` (its first `Authority=` line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signer {
    /// "Software Signing": part of macOS.
    Macos,
    /// "Apple Mac OS Application Signing": an App Store app (Apple signs every one, so the
    /// signature does not name the developer).
    AppStore,
    /// "Developer ID Application: Name (TEAMID)" and the other developer certificates.
    Developer(String),
    /// Ad hoc, unsigned or not readable.
    Unknown,
}

/// Read the signer from `codesign -dv` output (it writes its report to stderr).
pub fn parse_signer(codesign_out: &str) -> Signer {
    let Some(first) = codesign_out
        .lines()
        .find_map(|l| l.trim().strip_prefix("Authority="))
    else {
        return Signer::Unknown;
    };
    match first.trim() {
        "Software Signing" => return Signer::Macos,
        "Apple Mac OS Application Signing" => return Signer::AppStore,
        _ => {}
    }
    for pre in [
        "Developer ID Application:",
        "Apple Distribution:",
        "3rd Party Mac Developer Application:",
        "Apple Development:",
        "Mac Developer:",
    ] {
        if let Some(rest) = first.strip_prefix(pre) {
            let mut name = rest.trim();
            // "Name (ABCDE12345)": drop the team id.
            if let Some(open) = name.rfind(" (")
                && name.ends_with(')')
            {
                name = name[..open].trim();
            }
            if !name.is_empty() && !name.contains('@') {
                return Signer::Developer(name.to_string());
            }
        }
    }
    Signer::Unknown
}

/// The holder in a copyright line: "Copyright © 2002-2024 JetBrains s.r.o. All rights
/// reserved." gives "JetBrains s.r.o."; `None` when nothing that reads as a name is left.
pub fn copyright_holder(line: &str) -> Option<String> {
    let mut s = line
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .to_string();
    if let Some(i) = s.to_lowercase().find("all rights reserved") {
        s.truncate(i);
    }
    let marks = ["copyright", "(c)", "©", "℗"];
    let is_year = |w: &str| {
        let w = w.trim_matches(|c: char| ",.;:".contains(c));
        !w.is_empty()
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars()
                .all(|c| c.is_ascii_digit() || "-\u{2013}\u{2014}/".contains(c))
    };
    let words: Vec<&str> = s
        .split_whitespace()
        .flat_map(|w| {
            // "©2024" and "(c)2024": split the mark from the year.
            let lw = w.to_lowercase();
            match marks
                .iter()
                .find(|m| lw.starts_with(*m) && lw.len() > m.len())
            {
                Some(m) => vec![&w[..m.len()], &w[m.len()..]],
                None => vec![w],
            }
        })
        .filter(|w| {
            let lw = w.to_lowercase();
            let lw = lw.trim_matches(|c: char| ",.;:".contains(c));
            !marks.contains(&lw) && !is_year(w) && !["-", "\u{2013}", "\u{2014}"].contains(&lw)
        })
        .collect();
    let mut words = words.as_slice();
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("by")) {
        words = &words[1..];
    }
    let mut name = words.join(" ");
    // Keep the first sentence: "Screaming Frog Ltd. Screaming Frog is a trademark" stops at
    // "Ltd.".
    if let Some(i) = name.find(". ") {
        name.truncate(i + 1);
    }
    let mut name = name
        .trim_matches(|c: char| c.is_whitespace() || ",;:-\u{2013}\u{2014}".contains(c))
        .to_string();
    if name.ends_with('.') {
        let last = name.rsplit(' ').next().unwrap_or("").trim_end_matches('.');
        let abbrev = last.contains('.')
            || ["inc", "ltd", "corp", "co", "bv", "ag", "sa", "srl", "spa"]
                .contains(&last.to_lowercase().as_str());
        if !abbrev {
            name.pop();
        }
    }
    let low = name.to_lowercase();
    let ok = (2..=60).contains(&name.chars().count())
        && name.chars().any(char::is_alphabetic)
        && !["rights", "http", "licen", "version", "trademark"]
            .iter()
            .any(|w| low.contains(w));
    ok.then_some(name)
}

/// The developer from Info.plist alone: the copyright holder, else the copyright part of
/// `CFBundleGetInfoString`, else [`vendor_of`] the bundle id.
pub fn plist_developer(b: &BundleInfo) -> String {
    if let Some(n) = copyright_holder(&b.copyright) {
        return n;
    }
    let low = b.get_info.to_lowercase();
    if let Some(i) = ["©", "copyright", "(c)"]
        .iter()
        .filter_map(|m| low.find(m))
        .min()
        && let Some(n) = b.get_info.get(i..).and_then(copyright_holder)
    {
        return n;
    }
    vendor_of(&b.bundle_id)
}

/// The developer name shown for an app, best source first: part of macOS gives "Apple"; a
/// developer certificate gives its name; an App Store signature (Apple signs them all) or
/// none falls back to Info.plist ([`plist_developer`]).
pub fn developer_name(b: &BundleInfo, signer: &Signer, part_of_macos: bool) -> String {
    if part_of_macos {
        return "Apple".into();
    }
    match signer {
        Signer::Macos => "Apple".into(),
        Signer::Developer(n) => n.clone(),
        Signer::AppStore | Signer::Unknown => plist_developer(b),
    }
}

/// Time limit for one `codesign -dv`.
pub const CODESIGN_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
/// `codesign` runs at most this many at once.
pub const CODESIGN_THREADS: usize = 8;

/// `codesign -dv --verbose=2` for each app, several at once, each with [`CODESIGN_LIMIT`].
/// Results are in the order of `apps`; `Signer::Unknown` when it failed or timed out.
pub fn signers(env: &Env, apps: &[PathBuf]) -> Vec<Signer> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: std::sync::Mutex<Vec<Signer>> =
        std::sync::Mutex::new(vec![Signer::Unknown; apps.len()]);
    let one = |p: &Path| {
        let shown = env.display(p);
        match env.sys.run_timeout(
            "/usr/bin/codesign",
            &["-dv", "--verbose=2", &shown],
            CODESIGN_LIMIT,
        ) {
            // codesign writes its report to stderr.
            Ok(o) => match parse_signer(&o.stderr) {
                Signer::Unknown => parse_signer(&o.stdout),
                s => s,
            },
            Err(_) => Signer::Unknown,
        }
    };
    std::thread::scope(|sc| {
        for _ in 0..CODESIGN_THREADS.min(apps.len()) {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(p) = apps.get(i) else {
                        break;
                    };
                    let s = one(p);
                    out.lock().unwrap_or_else(|e| e.into_inner())[i] = s;
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|e| e.into_inner())
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
    parse_signer(codesign_out) == Signer::Macos
}

/// Under `/System` (`/System/Applications`, `/System/Library/CoreServices`): part of macOS.
pub fn in_system(env: &Env, p: &Path) -> bool {
    p.starts_with(env.sys_path("/System"))
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

/// How deep apps are looked for below an app folder: `/Applications/X.app` is level 1,
/// `/Applications/Utilities/X.app` level 2, `/Applications/Autodesk/AutoCAD 2026/X.app`
/// level 3.
pub const APP_DEPTH: usize = 3;

/// Folders never searched for apps: bundles of other kinds (their insides are not apps of
/// their own) and hidden folders.
fn not_searched(p: &Path) -> bool {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.starts_with('.')
        || [
            ".app",
            ".framework",
            ".bundle",
            ".plugin",
            ".appex",
            ".xpc",
            ".kext",
            ".prefpane",
            ".pkg",
            ".mpkg",
        ]
        .iter()
        .any(|e| name.ends_with(e))
}

/// Every app bundle path: the app folders and their subfolders up to [`APP_DEPTH`] levels.
/// An app bundle is never searched inside, and symbolic links are never followed (a link
/// inside the app folders points at an app already found by its real path, and a link out of
/// them leads to apps CleanCraft must not remove). Each entry says whether it lives in a
/// macOS system folder.
pub fn app_paths(env: &Env) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    for (dir, system) in APP_DIRS {
        let root = resolve(env, dir);
        let mut stack = vec![(root, 0usize)];
        while let Some((d, level)) = stack.pop() {
            let mut kids = children(&d);
            kids.sort();
            for c in kids.into_iter().rev() {
                if is_app(&c) {
                    out.push((c, *system));
                } else if level + 1 < APP_DEPTH
                    && !not_searched(&c)
                    && std::fs::symlink_metadata(&c).is_ok_and(|m| m.is_dir())
                {
                    stack.push((c, level + 1));
                }
            }
        }
    }
    out
}

/// A maker's uninstaller app, by its name: "Remove AutoCAD 2026", "Uninstall Foo",
/// "Foo Uninstaller", "Uninstaller". Returns the name of the app it removes ("" when it does
/// not say).
pub fn uninstaller_target(name: &str) -> Option<String> {
    let low = name.trim().to_lowercase();
    for pre in ["remove ", "uninstall "] {
        if let Some(rest) = low.strip_prefix(pre) {
            return Some(rest.trim().to_string());
        }
    }
    if let Some(rest) = low.strip_suffix("uninstaller") {
        return Some(rest.trim().to_string());
    }
    None
}

/// Folders whose apps are unrelated to each other even when one is named like the folder.
const NOT_SUITES: &[&str] = &["utilities", "setapp"];

/// Tie suite apps together ([`AppInfo::part_of`]) and give apps their maker's uninstaller
/// ([`AppInfo::vendor_uninstaller`]), from the apps' places on disk only:
/// - in a folder of its own (not an app folder, not `Utilities` or `Setapp`), the app named
///   like the folder (else the one an uninstaller there names) is the suite's app, and every
///   other app in that folder is its helper;
/// - an uninstaller names its app ("Remove AutoCAD 2026" removes "AutoCAD 2026"); in a suite
///   folder an uninstaller that names no app there is offered for the folder's own apps.
///
/// Apps that are part of macOS are never tied to an uninstaller.
pub fn link_suites(env: &Env, apps: &mut [AppInfo]) {
    let roots: Vec<PathBuf> = APP_DIRS.iter().map(|(d, _)| resolve(env, d)).collect();
    let mut dirs: Vec<PathBuf> = apps
        .iter()
        .filter_map(|a| a.path.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let idx: Vec<usize> = (0..apps.len())
            .filter(|&i| apps[i].path.parent() == Some(dir.as_path()) && !apps[i].apple)
            .collect();
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let suite_dir = !roots.contains(&dir)
            && !NOT_SUITES.contains(&folder.as_str())
            && !in_system(env, &dir);
        let names: Vec<(String, String)> = apps
            .iter()
            .map(|a| (a.name.to_lowercase(), a.bundle_name.to_lowercase()))
            .collect();
        let named = |i: usize, n: &str| !n.is_empty() && (names[i].0 == n || names[i].1 == n);
        let uninstallers: Vec<(usize, String)> = idx
            .iter()
            .filter_map(|&i| uninstaller_target(&apps[i].name).map(|t| (i, t)))
            .collect();
        let is_un = |i: usize| uninstallers.iter().any(|(u, _)| *u == i);
        let mut main = None;
        if suite_dir {
            main = idx
                .iter()
                .copied()
                .find(|&i| !is_un(i) && named(i, &folder))
                .or_else(|| {
                    idx.iter().copied().find(|&i| {
                        !is_un(i) && uninstallers.iter().any(|(_, t)| named(i, t.as_str()))
                    })
                });
        }
        if let Some(m) = main {
            let mp = apps[m].path.clone();
            for &i in &idx {
                if i != m {
                    apps[i].part_of = Some(mp.clone());
                }
            }
        }
        for (u, target) in &uninstallers {
            let up = apps[*u].path.clone();
            let mut hits: Vec<usize> = idx
                .iter()
                .copied()
                .filter(|&i| !is_un(i) && named(i, target))
                .collect();
            if hits.is_empty() && suite_dir {
                hits = idx
                    .iter()
                    .copied()
                    .filter(|&i| !is_un(i) && apps[i].part_of.is_none())
                    .collect();
            }
            for i in hits {
                if apps[i].vendor_uninstaller.is_none() {
                    apps[i].vendor_uninstaller = Some(up.clone());
                }
            }
        }
    }
}

/// The button that opens an app's own uninstaller: "Open Autodesk Uninstaller", or
/// "Open Remove AutoCAD 2026" when the maker's name is not known.
pub fn vendor_uninstaller_label(app: &AppInfo) -> Option<String> {
    let u = app.vendor_uninstaller.as_ref()?;
    let maker = vendor_of(&app.bundle_id);
    Some(if maker == UNKNOWN_DEVELOPER {
        format!(
            "Open {}",
            u.file_stem().unwrap_or_default().to_string_lossy()
        )
    } else {
        format!("Open {maker} Uninstaller")
    })
}

/// Kinds of bundles an app carries inside: helper apps, app extensions, XPC services,
/// frameworks, plug-ins, Quick Look and Spotlight plug-ins, login items, system extensions.
const BUNDLE_EXTS: &[&str] = &[
    "app",
    "appex",
    "xpc",
    "framework",
    "bundle",
    "plugin",
    "qlgenerator",
    "mdimporter",
    "saver",
    "prefpane",
    "systemextension",
    "kext",
    "fs",
];

/// Folders inside a bundle that hold other bundles.
const BUNDLE_HOLDERS: &[&str] = &[
    "Contents",
    "PlugIns",
    "Plugins",
    "Library",
    "Frameworks",
    "XPCServices",
    "LoginItems",
    "Helpers",
    "Applications",
    "Extensions",
    "SystemExtensions",
    "QuickLook",
    "Spotlight",
    "Versions",
    "MacOS",
    "SharedSupport",
];

/// The bundle id of any bundle (an app, an extension, a framework: frameworks keep their
/// Info.plist in `Resources`).
pub fn any_bundle_id(b: &Path) -> Option<String> {
    for rel in ["Contents/Info.plist", "Resources/Info.plist", "Info.plist"] {
        if let Ok(v) = plist::Value::from_file(b.join(rel))
            && let Some(id) = v
                .as_dictionary()
                .and_then(|d| d.get("CFBundleIdentifier"))
                .and_then(|v| v.as_string())
        {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Every bundle id inside `app` (helper apps, `*.appex` extensions, XPC services,
/// frameworks, Quick Look generators, login items and the like), read from disk. Only the
/// folders that hold bundles are walked, never through a symbolic link, at most 8 levels.
pub fn embedded_ids(app: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(app.join("Contents"), 0usize, false)];
    while let Some((dir, depth, in_library)) = stack.pop() {
        if depth > 8 {
            continue;
        }
        for c in children(&dir) {
            if !std::fs::symlink_metadata(&c).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let name = c
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = c
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if BUNDLE_EXTS.contains(&ext.as_str()) {
                if let Some(id) = any_bundle_id(&c) {
                    out.push(id);
                }
                stack.push((c, depth + 1, false));
            } else if in_library
                || BUNDLE_HOLDERS.contains(&name.as_str())
                || dir.file_name().is_some_and(|n| n == "Versions")
            {
                stack.push((c, depth + 1, name == "Library"));
            }
        }
    }
    out
}

/// Folders outside the apps where installed bundles live (settings panes, file systems,
/// shared frameworks, Quick Look and Spotlight plug-ins, internet plug-ins, screen savers).
/// Their ids count as installed, like apps.
pub const BUNDLE_DIRS: &[&str] = &[
    "/Library/PreferencePanes",
    "~/Library/PreferencePanes",
    "/Library/Filesystems",
    "/Library/Frameworks",
    "~/Library/Frameworks",
    "/Library/QuickLook",
    "~/Library/QuickLook",
    "/Library/Spotlight",
    "~/Library/Spotlight",
    "/Library/Internet Plug-Ins",
    "~/Library/Internet Plug-Ins",
    "/Library/Screen Savers",
    "~/Library/Screen Savers",
    "/Library/Extensions",
    "/Library/Audio/Plug-Ins/HAL",
];

/// Every bundle id on this Mac that counts as installed, lower case: each app's own id, every
/// bundle embedded in it ([`embedded_ids`]), and the bundles in [`BUNDLE_DIRS`].
pub fn installed_ids(env: &Env, apps: &[AppInfo]) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for a in apps {
        ids.insert(a.bundle_id.to_lowercase());
        for id in embedded_ids(&a.path) {
            ids.insert(id.to_lowercase());
        }
    }
    for d in BUNDLE_DIRS {
        for c in children(&resolve(env, d)) {
            if std::fs::symlink_metadata(&c).is_ok_and(|m| m.is_dir())
                && let Some(id) = any_bundle_id(&c)
            {
                ids.insert(id.to_lowercase());
            }
        }
    }
    ids
}

/// The app list. `deep` adds size, last-opened date and the code signature (slower: `mdls`
/// per app, and `codesign` for every app outside `/System`, run [`CODESIGN_THREADS`] at once);
/// without it the list is quick and runs no command, for matching leftovers.
pub fn inventory(env: &Env, deep: bool, progress: &dyn Fn(&str)) -> Vec<AppInfo> {
    let setapp = env.sys_path("/Applications/Setapp");
    let mut out = Vec::new();
    let mut bundles = Vec::new();
    // Asked once, only when an app needs the app-data fallback.
    let mut full_disk: Option<bool> = None;
    for (path, system) in app_paths(env) {
        let Some(b) = read_bundle(&path) else {
            continue;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        progress(&name);
        let store = if has_store_receipt(&path) {
            Store::AppStore
        } else if path.starts_with(&setapp) {
            Store::Setapp
        } else {
            Store::Other
        };
        let apple = system || in_system(env, &path) || macos_app(&path);
        let shown = env.display(&path);
        let (mut size, mut last_used, mut last_active) = (0, None, None);
        if deep {
            size = tree_size(&path).0;
            last_used = spotlight_date(env, "kMDItemLastUsedDate", &shown)
                .or_else(|| spotlight_date(env, "kMDItemLastUsedDate_Ranking", &shown));
            if last_used.is_none() {
                let containers = *full_disk.get_or_insert_with(|| {
                    crate::permissions::full_disk_access(env) == crate::permissions::Access::Granted
                });
                last_active = last_active_from_data(env, &b.bundle_id, containers);
            }
        }
        out.push(AppInfo {
            vendor: String::new(),
            path,
            name,
            bundle_name: b.name.clone(),
            bundle_id: b.bundle_id.clone(),
            version: b.short_version.clone(),
            build: b.version.clone(),
            size,
            last_used,
            last_active,
            store,
            apple,
            feed_url: b.feed_url.clone(),
            part_of: None,
            vendor_uninstaller: None,
        });
        bundles.push(b);
    }
    // The signature: who made the app, and whether macOS signed it as its own. Apps already
    // known to be part of macOS are not asked; every other app is, several at once.
    let ask: Vec<usize> = if deep {
        (0..out.len()).filter(|&i| !out[i].apple).collect()
    } else {
        Vec::new()
    };
    let paths: Vec<PathBuf> = ask.iter().map(|&i| out[i].path.clone()).collect();
    let mut signed = vec![Signer::Unknown; out.len()];
    for (i, s) in ask.into_iter().zip(signers(env, &paths)) {
        signed[i] = s;
    }
    for ((a, b), s) in out.iter_mut().zip(&bundles).zip(&signed) {
        if *s == Signer::Macos {
            a.apple = true;
        }
        a.vendor = developer_name(b, s, a.apple);
    }
    link_suites(env, &mut out);
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// `mdls -name <attr> -raw <app>`: a Spotlight date, `None` for "(null)" or an error.
fn spotlight_date(env: &Env, attr: &str, shown: &str) -> Option<i64> {
    env.sys
        .run("/usr/bin/mdls", &["-name", attr, "-raw", shown])
        .ok()
        .filter(|o| o.ok())
        .and_then(|o| parse_mdls_date(&o.stdout))
}

/// Files an app writes when it runs: its preferences and its saved windows, in the home
/// folder and (when `containers`) in its sandbox container. Reading inside another app's
/// container needs Full Disk Access on recent macOS (else macOS asks the user), so the
/// container paths are only listed with it.
pub fn app_data_paths(env: &Env, bundle_id: &str, containers: bool) -> Vec<PathBuf> {
    if bundle_id.is_empty() || bundle_id.contains('/') || bundle_id.contains("..") {
        return Vec::new();
    }
    let mut v = vec![
        env.home_path(&format!("Library/Preferences/{bundle_id}.plist")),
        env.home_path(&format!(
            "Library/Saved Application State/{bundle_id}.savedState"
        )),
    ];
    if containers {
        let data = format!("Library/Containers/{bundle_id}/Data/Library");
        v.push(env.home_path(&format!("{data}/Preferences/{bundle_id}.plist")));
        v.push(env.home_path(&format!(
            "{data}/Saved Application State/{bundle_id}.savedState"
        )));
    }
    v
}

/// The newest modification time among [`app_data_paths`], seconds since the epoch.
pub fn last_active_from_data(env: &Env, bundle_id: &str, containers: bool) -> Option<i64> {
    app_data_paths(env, bundle_id, containers)
        .iter()
        .filter_map(|p| std::fs::symlink_metadata(p).ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .max()
}

/// The last-opened line for an app: "Opened: 3 days ago" from Spotlight; when Spotlight has
/// no date, "Last active: 3 days ago (app data)" from the app's own files; else
/// "Opened: No record" (Spotlight never saw it open: often an app that was never launched).
pub fn opened_text(a: &AppInfo, now: i64) -> String {
    match (a.last_used, a.last_active) {
        (Some(t), _) => format!("Opened: {}", last_used_text(Some(t), now)),
        (None, Some(t)) => format!("Last active: {} (app data)", last_used_text(Some(t), now)),
        (None, None) => "Opened: No record".into(),
    }
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
        assert_eq!(vendor_of("com.example.Editor"), "Example");
        assert_eq!(vendor_of("com.SweetScape.010Editor"), "SweetScape");
        // REVIEW-A 45: names seen on the real Mac.
        assert_eq!(vendor_of("org.freecad.FreeCAD"), "FreeCAD");
        assert_eq!(vendor_of("com.jetbrains.goland"), "JetBrains");
        assert_eq!(vendor_of("com.openai.chat"), "OpenAI");
        assert_eq!(
            vendor_of("org.blenderfoundation.blender"),
            "Blender Foundation"
        );
        assert_eq!(vendor_of("com.belightsoft.LiveHome3D"), "BeLight Software");
        // Fragments are never a name.
        assert_eq!(vendor_of("co.uk.screamingfrog"), UNKNOWN_DEVELOPER);
        assert_eq!(vendor_of("io.x"), UNKNOWN_DEVELOPER);
        assert_eq!(vendor_of("com.abc.x"), UNKNOWN_DEVELOPER);
        assert_eq!(vendor_of("foo"), UNKNOWN_DEVELOPER);
        assert_eq!(vendor_of(""), UNKNOWN_DEVELOPER);
        assert_eq!(vendor_segment("com.JetBrains.goland"), "jetbrains");
        assert_eq!(vendor_segment("foo.bar"), "foo");
        assert_eq!(vendor_segment("foo"), "");
    }

    #[test]
    fn signers_from_codesign() {
        let dev = "Executable=/Applications/GoLand.app/Contents/MacOS/goland\nIdentifier=com.jetbrains.goland\nAuthority=Developer ID Application: JetBrains s.r.o. (2ZEFAR8TH3)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\nTeamIdentifier=2ZEFAR8TH3\n";
        assert_eq!(
            parse_signer(dev),
            Signer::Developer("JetBrains s.r.o.".into())
        );
        assert_eq!(
            parse_signer("Authority=Developer ID Application: Screaming Frog Ltd (ABCDE12345)\n"),
            Signer::Developer("Screaming Frog Ltd".into())
        );
        assert_eq!(
            parse_signer("Authority=Developer ID Application: Example (Pty) Ltd (ABCDE12345)\n"),
            Signer::Developer("Example (Pty) Ltd".into())
        );
        assert_eq!(
            parse_signer(
                "Authority=Apple Mac OS Application Signing\nAuthority=Apple Worldwide Developer Relations Certification Authority\n"
            ),
            Signer::AppStore
        );
        assert_eq!(
            parse_signer("Authority=Software Signing\nAuthority=Apple Root CA\n"),
            Signer::Macos
        );
        assert_eq!(
            parse_signer("Executable=x\nSignature=adhoc\nTeamIdentifier=not set\n"),
            Signer::Unknown
        );
        assert_eq!(
            parse_signer("Authority=Apple Development: me@example.com (ABCDE12345)\n"),
            Signer::Unknown
        );
    }

    #[test]
    fn copyright_holders() {
        let h = |s: &str| copyright_holder(s);
        assert_eq!(
            h("Copyright © 2002-2024 JetBrains s.r.o. All rights reserved.").as_deref(),
            Some("JetBrains s.r.o.")
        );
        assert_eq!(
            h("Copyright © 2001–2024 Apple Inc. All rights reserved.").as_deref(),
            Some("Apple Inc.")
        );
        assert_eq!(h("© 2024 OpenAI.").as_deref(), Some("OpenAI"));
        assert_eq!(h("©2024 OpenAI").as_deref(), Some("OpenAI"));
        assert_eq!(
            h("Copyright (c) 2001-2023, The FreeCAD project association").as_deref(),
            Some("The FreeCAD project association")
        );
        assert_eq!(
            h("Copyright 2010-2024 Screaming Frog Ltd. Screaming Frog is a trademark.").as_deref(),
            Some("Screaming Frog Ltd.")
        );
        assert_eq!(
            h("Blender Foundation 2024").as_deref(),
            Some("Blender Foundation")
        );
        assert_eq!(
            h("Copyright by BeLight Software, Ltd.").as_deref(),
            Some("BeLight Software, Ltd.")
        );
        assert_eq!(h("All rights reserved."), None);
        assert_eq!(h("© 2024"), None);
        assert_eq!(h(""), None);
        assert_eq!(h("Licensed under the Apache License, Version 2.0"), None);
    }

    #[test]
    fn developer_name_order() {
        let b = BundleInfo {
            bundle_id: "co.uk.screamingfrog.seo.spider".into(),
            copyright: "© 2024 Screaming Frog Ltd".into(),
            get_info: "SEO Spider 20.0, Copyright Other Name".into(),
            ..Default::default()
        };
        let dev = Signer::Developer("Screaming Frog Ltd".into());
        assert_eq!(developer_name(&b, &dev, false), "Screaming Frog Ltd");
        assert_eq!(developer_name(&b, &dev, true), "Apple");
        assert_eq!(developer_name(&b, &Signer::Macos, false), "Apple");
        // App Store: Apple signs every app, so the copyright names the developer.
        assert_eq!(
            developer_name(&b, &Signer::AppStore, false),
            "Screaming Frog Ltd"
        );
        let no_copy = BundleInfo {
            copyright: String::new(),
            ..b.clone()
        };
        assert_eq!(
            developer_name(&no_copy, &Signer::Unknown, false),
            "Other Name"
        );
        let only_id = BundleInfo {
            get_info: "SEO Spider 20.0".into(),
            ..no_copy
        };
        // "co" is a fragment, never a name.
        assert_eq!(
            developer_name(&only_id, &Signer::Unknown, false),
            UNKNOWN_DEVELOPER
        );
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
