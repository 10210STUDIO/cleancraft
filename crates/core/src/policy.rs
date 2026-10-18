//! The safety fence. Before anything is moved, [`check`] proves the path is
//! - strictly inside one of the feature's allow-list folders (never the folder itself),
//! - reached without following a symbolic link,
//! - not in a protected location (SIP-protected system folders, iCloud Drive and other cloud
//!   folders, Photos libraries, the contents of Documents and Desktop, keychains, CleanCraft's
//!   own data),
//! - inside the env root (the temporary folder in tests).
//!
//! Scanners use the same function to skip what they must not even propose.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::{Env, has_parent_component};

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
            Refusal::IsAllowRoot => write!(f, "a top folder this feature cleans inside, never removed itself"),
            Refusal::Protected(why) => write!(f, "protected: {why}"),
            Refusal::SymlinkOnPath(p) => write!(f, "reached through a symbolic link ({})", p.display()),
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
    ("Library/Application Support/CleanCraft", "CleanCraft's own data"),
    ("Library/Group Containers/group.com.apple.notes", "Notes"),
    ("Library/Messages", "Messages"),
];

/// Why `p` is protected, if it is.
pub fn protected_reason(env: &Env, p: &Path) -> Option<String> {
    for (abs, why) in PROTECTED_SYSTEM {
        if p.starts_with(env.sys_path(abs)) {
            return Some((*why).to_string());
        }
    }
    for (rel, why) in PROTECTED_HOME {
        if p.starts_with(env.home().join(rel)) {
            return Some((*why).to_string());
        }
    }
    for c in p.components() {
        let s = c.as_os_str().to_string_lossy().to_lowercase();
        if s.ends_with(".photoslibrary") || s.ends_with(".photolibrary") || s.ends_with(".aplibrary") {
            return Some("a Photos library".into());
        }
    }
    if p == env.home() || p == env.root() {
        return Some("a top level folder".into());
    }
    None
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
        assert_eq!(check(&env, &allow, &env.home_path("Library/Logs/a")), Err(Refusal::NotInAllowList));
        assert!(matches!(check(&env, &allow, &env.home_path("Documents/a")), Err(Refusal::Protected(_))));
        assert!(matches!(check(&env, &allow, &env.sys_path("/System/Library/x")), Err(Refusal::Protected(_))));
        assert_eq!(check(&env, &allow, Path::new("/etc/hosts")), Err(Refusal::OutsideRoot));
        assert_eq!(check(&env, &allow, &caches.join("../Caches/com.x")), Err(Refusal::NotAbsolute));
        let lib = env.home_path("Pictures/Photos Library.photoslibrary/x");
        assert!(matches!(check(&env, &[env.home_path("Pictures")], &lib), Err(Refusal::Protected(_))));
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
        let r = check(&env, &[caches.clone()], &caches.join("link/secret"));
        assert!(matches!(r, Err(Refusal::SymlinkOnPath(_))), "{r:?}");
        // The link itself may be moved (the link, not what it points to).
        assert_eq!(check(&env, &[caches.clone()], &caches.join("link")), Ok(()));
    }
}
