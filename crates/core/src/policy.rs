//! The safety fence. Before anything is moved, [`check`] proves the path is
//! - strictly inside one of the feature's allow-list folders (never the folder itself),
//! - reached without following a symbolic link,
//! - not in a protected location (SIP-protected system folders, iCloud Drive and other cloud
//!   folders, Photos libraries, the contents of Documents and Desktop, keychains, CleanCraft's
//!   own data),
//! - inside the env root (the temporary folder in tests).
//!
//! Scanners use the same function to skip what they must not even propose.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use unicode_normalization::UnicodeNormalization;

use crate::env::{Env, has_parent_component};

/// One file name the way a default Mac volume (APFS, case-insensitive) compares it: letter case
/// and Unicode composition do not matter ("Résumé" typed with a combining accent, "résumé",
/// "RÉSUMÉ" are one name).
pub fn fold_name(s: &str) -> String {
    s.nfd().flat_map(char::to_lowercase).nfd().collect()
}

/// Are two file names the same name on a default Mac volume?
pub fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if a == b {
        return true;
    }
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) if a.is_ascii() && b.is_ascii() => a.eq_ignore_ascii_case(b),
        (Some(a), Some(b)) => fold_name(a) == fold_name(b),
        _ => false,
    }
}

fn same_component(a: Component<'_>, b: Component<'_>) -> bool {
    match (a, b) {
        (Component::Normal(x), Component::Normal(y)) => same_name(x, y),
        (x, y) => x == y,
    }
}

/// `p.starts_with(base)`, comparing names like a default Mac volume (see [`fold_name`]).
/// Protected places and the ignore list use it, so `~/documents` is `~/Documents`.
pub fn starts_with_ci(p: &Path, base: &Path) -> bool {
    let mut pc = p.components();
    for b in base.components() {
        match pc.next() {
            Some(c) if same_component(c, b) => {}
            _ => return false,
        }
    }
    true
}

/// `a == b`, comparing names like a default Mac volume.
pub fn same_path_ci(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && starts_with_ci(a, b)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    OutsideRoot,
    NotAbsolute,
    NotInAllowList,
    IsAllowRoot,
    Protected(String),
    SymlinkOnPath(PathBuf),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::OutsideRoot => write!(f, "outside the scanned area"),
            Refusal::NotAbsolute => write!(f, "not an absolute path"),
            Refusal::NotInAllowList => write!(f, "not in a location this feature may clean"),
            Refusal::IsAllowRoot => write!(
                f,
                "a top folder this feature cleans inside, never removed itself"
            ),
            Refusal::Protected(why) => write!(f, "protected: {why}"),
            Refusal::SymlinkOnPath(p) => {
                write!(f, "reached through a symbolic link ({})", p.display())
            }
        }
    }
}

/// Protected system locations, as absolute macOS paths. `/usr/local` is not protected.
pub const PROTECTED_SYSTEM: &[(&str, &str)] = &[
    ("/System", "macOS system files (SIP)"),
    ("/bin", "macOS system files (SIP)"),
    ("/sbin", "macOS system files (SIP)"),
    ("/usr/bin", "macOS system files (SIP)"),
    ("/usr/sbin", "macOS system files (SIP)"),
    ("/usr/lib", "macOS system files (SIP)"),
    ("/usr/libexec", "macOS system files (SIP)"),
    ("/usr/share", "macOS system files (SIP)"),
    ("/usr/standalone", "macOS system files (SIP)"),
    ("/private/var/db", "macOS system database (SIP)"),
    ("/private/var/vm", "macOS virtual memory"),
    ("/private/etc", "macOS configuration"),
    ("/etc", "macOS configuration"),
    ("/dev", "devices"),
    ("/Library/Apple", "macOS system files (SIP)"),
    ("/Library/Keychains", "keychains"),
];

/// Protected folders inside the home folder.
pub const PROTECTED_HOME: &[(&str, &str)] = &[
    ("Documents", "the contents of Documents"),
    ("Desktop", "the contents of Desktop"),
    ("Library/Mobile Documents", "iCloud Drive"),
    ("Library/CloudStorage", "cloud storage folders"),
    ("Library/Application Support/CloudDocs", "iCloud Drive"),
    ("Library/Keychains", "keychains"),
    (
        "Library/Application Support/CleanCraft",
        "CleanCraft's own data",
    ),
    ("Library/Group Containers/group.com.apple.notes", "Notes"),
    ("Library/Messages", "Messages"),
];

/// Why `p` is protected, if it is.
pub fn protected_reason(env: &Env, p: &Path) -> Option<String> {
    for (abs, why) in PROTECTED_SYSTEM {
        if starts_with_ci(p, &env.sys_path(abs)) {
            return Some((*why).to_string());
        }
    }
    for (rel, why) in PROTECTED_HOME {
        if starts_with_ci(p, &env.home().join(rel)) {
            return Some((*why).to_string());
        }
    }
    if starts_with_ci(p, env.data_dir()) {
        return Some("CleanCraft's own data".into());
    }
    if p.components().any(|c| is_photos_library(c.as_os_str())) {
        return Some("a Photos library".into());
    }
    if same_path_ci(p, env.home()) || same_path_ci(p, env.root()) {
        return Some("a top level folder".into());
    }
    None
}

/// Photos, iPhoto and Aperture libraries, by name.
pub fn is_photos_library(name: &OsStr) -> bool {
    let s = name.to_string_lossy().to_lowercase();
    s.ends_with(".photoslibrary") || s.ends_with(".photolibrary") || s.ends_with(".aplibrary")
}

/// How deep [`protected_inside`] looks below a folder. Deeper than any real layout of a Photos
/// library, kept finite so a link loop or a pathological tree cannot stall a removal.
pub const INSIDE_DEPTH: usize = 24;

/// Why the folder `p` must not move as a whole, if it holds a protected place: a known
/// protected folder (Documents, iCloud Drive, keychains, CleanCraft's data, the home folder...)
/// lies inside it, or a Photos library is found below it. Symbolic links are not followed
/// (moving a link never moves what it points to). Files and links return `None`.
pub fn protected_inside(env: &Env, p: &Path) -> Option<String> {
    match std::fs::symlink_metadata(p) {
        Ok(md) if md.is_dir() => {}
        _ => return None,
    }
    let shown = |why: &str| format!("it holds {why}");
    let below = |base: &Path| starts_with_ci(base, p) && !same_path_ci(base, p);
    for (abs, why) in PROTECTED_SYSTEM {
        if below(&env.sys_path(abs)) {
            return Some(shown(why));
        }
    }
    for (rel, why) in PROTECTED_HOME {
        if below(&env.home().join(rel)) {
            return Some(shown(why));
        }
    }
    if below(env.data_dir()) {
        return Some(shown("CleanCraft's own data"));
    }
    if below(env.home()) {
        return Some(shown("your home folder"));
    }
    let mut stack = vec![(p.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name();
            if is_photos_library(&name) {
                return Some(shown("a Photos library"));
            }
            if depth + 1 < INSIDE_DEPTH && e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    None
}

/// [`check`] for an item about to move: also refuses a folder holding a protected place
/// ([`protected_inside`]). The executor runs it right before each move.
pub fn check_move(env: &Env, allow: &[PathBuf], p: &Path) -> Result<(), Refusal> {
    check(env, allow, p)?;
    if let Some(why) = protected_inside(env, p) {
        return Err(Refusal::Protected(why));
    }
    Ok(())
}

/// The fence every removal passes. `allow` are the feature's allow-list folders.
pub fn check(env: &Env, allow: &[PathBuf], p: &Path) -> Result<(), Refusal> {
    if !p.is_absolute() || has_parent_component(p) {
        return Err(Refusal::NotAbsolute);
    }
    if !env.contains(p) {
        return Err(Refusal::OutsideRoot);
    }
    if let Some(why) = protected_reason(env, p) {
        return Err(Refusal::Protected(why));
    }
    let Some(root) = allow.iter().find(|r| p.starts_with(r)) else {
        return Err(Refusal::NotInAllowList);
    };
    if p == root {
        return Err(Refusal::IsAllowRoot);
    }
    // Every folder from the allow root down to the item's parent must be a real folder.
    let mut cur = p.parent();
    while let Some(dir) = cur {
        if !dir.starts_with(root) {
            break;
        }
        if let Ok(md) = std::fs::symlink_metadata(dir)
            && md.file_type().is_symlink()
        {
            return Err(Refusal::SymlinkOnPath(dir.to_path_buf()));
        }
        if dir == root {
            break;
        }
        cur = dir.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences() {
        let d = tempfile::tempdir().unwrap();
        let env = Env::fake(d.path());
        let caches = env.home_path("Library/Caches");
        std::fs::create_dir_all(caches.join("com.x")).unwrap();
        let allow = vec![caches.clone()];
        assert_eq!(check(&env, &allow, &caches.join("com.x")), Ok(()));
        assert_eq!(check(&env, &allow, &caches), Err(Refusal::IsAllowRoot));
        assert_eq!(
            check(&env, &allow, &env.home_path("Library/Logs/a")),
            Err(Refusal::NotInAllowList)
        );
        assert!(matches!(
            check(&env, &allow, &env.home_path("Documents/a")),
            Err(Refusal::Protected(_))
        ));
        assert!(matches!(
            check(&env, &allow, &env.sys_path("/System/Library/x")),
            Err(Refusal::Protected(_))
        ));
        assert_eq!(
            check(&env, &allow, Path::new("/etc/hosts")),
            Err(Refusal::OutsideRoot)
        );
        assert_eq!(
            check(&env, &allow, &caches.join("../Caches/com.x")),
            Err(Refusal::NotAbsolute)
        );
        let lib = env.home_path("Pictures/Photos Library.photoslibrary/x");
        assert!(matches!(
            check(&env, &[env.home_path("Pictures")], &lib),
            Err(Refusal::Protected(_))
        ));
    }

    #[test]
    fn names_compare_like_apfs() {
        // "Résumé" precomposed, decomposed and in capitals are one name on a default Mac.
        let nfc = "R\u{e9}sum\u{e9}";
        let nfd = "Re\u{301}sume\u{301}";
        assert!(same_name(OsStr::new(nfc), OsStr::new(nfd)));
        assert!(same_name(OsStr::new("R\u{c9}SUM\u{c9}"), OsStr::new(nfd)));
        assert!(!same_name(OsStr::new("Resume"), OsStr::new(nfc)));
        let d = tempfile::tempdir().unwrap();
        let env = Env::fake(d.path());
        let docs = env.home().join(format!("Documents/{nfd}.pdf"));
        assert!(starts_with_ci(&docs, &env.home_path("DOCUMENTS")));
        assert!(protected_reason(&env, &env.home_path("dEsKtOp/x")).is_some());
        assert!(protected_reason(&env, &env.home_path("Downloads/x")).is_none());
    }

    #[test]
    fn folder_holding_a_protected_place() {
        let d = tempfile::tempdir().unwrap();
        let env = Env::fake(d.path());
        let lib = env.home_path("Pictures/Trips/Old.photoslibrary/db");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(env.home_path("Library/Keychains")).unwrap();
        std::fs::create_dir_all(env.home_path("Projects/a")).unwrap();
        assert!(protected_inside(&env, &env.home_path("Pictures")).is_some());
        assert!(protected_inside(&env, &env.home_path("Library")).is_some());
        assert!(protected_inside(&env, &env.home_path("Projects")).is_none());
        let home = vec![env.home().to_path_buf()];
        assert!(check(&env, &home, &env.home_path("Pictures")).is_ok());
        assert!(matches!(
            check_move(&env, &home, &env.home_path("Pictures")),
            Err(Refusal::Protected(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_on_path_refused() {
        let d = tempfile::tempdir().unwrap();
        let env = Env::fake(d.path());
        let caches = env.home_path("Library/Caches");
        let docs = env.home_path("Projects");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::create_dir_all(docs.join("secret")).unwrap();
        std::os::unix::fs::symlink(&docs, caches.join("link")).unwrap();
        let r = check(
            &env,
            std::slice::from_ref(&caches),
            &caches.join("link/secret"),
        );
        assert!(matches!(r, Err(Refusal::SymlinkOnPath(_))), "{r:?}");
        // The link itself may be moved (the link, not what it points to).
        assert_eq!(
            check(&env, std::slice::from_ref(&caches), &caches.join("link")),
            Ok(())
        );
    }
}
