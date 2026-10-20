//! The safety fence. Before anything is moved, [`check`] proves the path is
//! - strictly inside one of the feature's allow-list folders (never the folder itself),
//! - reached without following a symbolic link: no folder on the way to it, from the top of
//!   the disk down to its parent, is a link (macOS's own `/var`, `/tmp` and `/etc` links into
//!   `/private` excepted), so an allow-list folder that is itself a link, or lies under one
//!   (`~/.npm`, `~/.cache` linked into Documents or anywhere else), is never cleaned,
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
    // No folder on the way to the item may be a link: not below the allow-list folder, not the
    // allow-list folder itself, not above it (CHK7-1). The item itself may be a link (moving a
    // link never moves what it points to).
    if let Some(l) = link_above(env, p) {
        return Err(match &l.protected {
            Some(why) => Refusal::Protected(format!(
                "{why}, reached through the link {}",
                env.display(&l.link)
            )),
            None => Refusal::SymlinkOnPath(l.link),
        });
    }
    // Through one of macOS's own links (/var, /tmp): the real place must not be protected.
    if let Some(real) = real_place(env, p)
        && let Some(why) = protected_reason(env, &real)
    {
        return Err(Refusal::Protected(why));
    }
    Ok(())
}

/// macOS's own links at the top of the startup disk: they lead into `/private` and are not
/// a way around the fence.
pub const SYSTEM_LINKS: &[&str] = &["/var", "/tmp", "/etc"];

/// A symbolic link found on the way to a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkAbove {
    /// The link as written (`~/.npm`).
    pub link: PathBuf,
    /// Where the path really is, inside the env root; `None` when it leads out of it or cannot
    /// be found.
    pub real: Option<PathBuf>,
    /// The protected place the real path lies in, if any (Documents, Desktop, iCloud Drive, a
    /// Photos library...). A real place outside the env root counts as protected.
    pub protected: Option<String>,
}

fn is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

fn is_system_link(env: &Env, p: &Path) -> bool {
    SYSTEM_LINKS.iter().any(|s| p == env.sys_path(s))
}

/// The first symbolic link (from the top down) among the folders above `p`: every folder
/// between the env root (excluded) and `p` (excluded). macOS's own [`SYSTEM_LINKS`] are not
/// counted. `None`: `p` is reached through real folders only.
pub fn link_above(env: &Env, p: &Path) -> Option<LinkAbove> {
    let mut above: Vec<&Path> = p
        .ancestors()
        .skip(1)
        .take_while(|a| a.starts_with(env.root()) && *a != env.root())
        .collect();
    above.reverse();
    let link = above
        .into_iter()
        .find(|a| is_link(a) && !is_system_link(env, a))?;
    Some(describe_link(env, link.to_path_buf(), p))
}

/// [`link_above`], also counting `p` itself: an allow-list folder that is a link.
pub fn link_at_or_above(env: &Env, p: &Path) -> Option<LinkAbove> {
    link_above(env, p).or_else(|| {
        (is_link(p) && !is_system_link(env, p)).then(|| describe_link(env, p.to_path_buf(), p))
    })
}

fn describe_link(env: &Env, link: PathBuf, p: &Path) -> LinkAbove {
    let real = resolve(env, p);
    let protected = match &real {
        Some(r) => protected_reason(env, r).or_else(|| {
            // Under the link but not protected by name: a folder holding a protected place
            // (the link leads to the home folder or to Documents' parent) is protected too.
            if same_path_ci(r, env.home()) || starts_with_ci(env.home(), r) {
                Some("your home folder".into())
            } else {
                None
            }
        }),
        None => Some("a place outside your Mac's folders CleanCraft looks in".into()),
    };
    LinkAbove {
        link,
        real,
        protected,
    }
}

/// Where `p` really is, with every link resolved, written inside the env root; `None` when it
/// cannot be found or lies outside the env root. A path that does not exist is resolved
/// through its deepest existing folder.
fn resolve(env: &Env, p: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(env.root()).ok()?;
    let mut rest: Vec<&OsStr> = Vec::new();
    let mut cur = p;
    let real = loop {
        if let Ok(r) = std::fs::canonicalize(cur) {
            break r;
        }
        rest.push(cur.file_name()?);
        cur = cur.parent()?;
    };
    let mut out = env.root().join(real.strip_prefix(&root).ok()?);
    for n in rest.into_iter().rev() {
        out.push(n);
    }
    Some(out)
}

/// The real place of `p` when the way to it crosses one of macOS's own [`SYSTEM_LINKS`];
/// `None` otherwise (then the path as written is the real place).
fn real_place(env: &Env, p: &Path) -> Option<PathBuf> {
    let crosses = SYSTEM_LINKS
        .iter()
        .any(|s| p.starts_with(env.sys_path(s)) && is_link(&env.sys_path(s)));
    if !crosses {
        return None;
    }
    // The item itself is not resolved: moving a link moves the link.
    let parent = resolve(env, p.parent()?)?;
    Some(parent.join(p.file_name()?))
}

/// The lock reason for things a scan cannot offer because the way to them goes through a
/// link: plain words and the next step.
pub fn link_lock_reason(env: &Env, l: &LinkAbove) -> String {
    let link = env.display(&l.link);
    let place = match (&l.protected, &l.real) {
        (Some(why), Some(real)) => format!("a link into {why} ({})", env.display(real)),
        (Some(why), None) => format!("a link into {why}"),
        (None, Some(real)) => format!(
            "a link to {}, outside the folders this feature cleans",
            env.display(real)
        ),
        (None, None) => "a link to another place".into(),
    };
    format!(
        "{link} is {place}. CleanCraft never cleans through a link, so nothing in it is moved. Clear it with the app or tool that made it, or make {link} a real folder again, then scan again"
    )
}

/// The allow-list folders that are links or lie under one, grouped by the link: (the link,
/// its allow-list folders that exist, what the link is). Scans list these locked with
/// [`link_lock_reason`] so the person sees why nothing there is offered.
pub fn linked_roots(env: &Env, allow: &[PathBuf]) -> Vec<(LinkAbove, Vec<PathBuf>)> {
    let mut out: Vec<(LinkAbove, Vec<PathBuf>)> = Vec::new();
    for r in allow {
        if std::fs::symlink_metadata(r).is_err() {
            continue;
        }
        let Some(l) = link_at_or_above(env, r) else {
            continue;
        };
        match out.iter_mut().find(|(o, _)| o.link == l.link) {
            Some((_, roots)) => roots.push(r.clone()),
            None => out.push((l, vec![r.clone()])),
        }
    }
    out
}

/// The ignore-list entries that lie strictly inside `p` (a folder that holds an ignored item:
/// it never moves as a whole, CHK7-2).
pub fn ignored_inside<'a>(ignore: &'a [PathBuf], p: &Path) -> Vec<&'a PathBuf> {
    ignore
        .iter()
        .filter(|i| starts_with_ci(i, p) && !same_path_ci(i, p))
        .collect()
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
