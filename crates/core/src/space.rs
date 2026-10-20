//! Disk Map: a size tree of a folder, the treemap layout that draws it, and the rules for what
//! may be selected.
//!
//! Sizes use the shared formula ([`crate::fsutil::entry_size`], bytes on disk). Like
//! [`crate::fsutil::tree_size`], a file with several hard links is counted once: on the first
//! path met in name order, in the whole tree, also after a refresh. Two more rules keep the
//! numbers honest:
//! - the walk does not cross into another volume (a folder whose device differs from the
//!   root's is listed with size 0 and counted in `other_volumes`),
//! - symbolic links are never followed (they count as their own small size).
//!
//! A tree is built once per scanned folder and kept: drilling down needs no new scan. After a
//! removal or an undo only the folders that changed are measured again ([`Tree::refresh`]).
//!
//! Locations ([`MapLocation`]): the home folder, the startup disk (`/`) and each mounted
//! external volume ([`external_volumes`], from `df -Pk` and `diskutil list -plist external`
//! through `env.sys`), or a folder.
//!
//! The startup disk on APFS: `/` is the sealed, read-only system volume; the user's data lives
//! on the Data volume, mounted at `/System/Volumes/Data` and joined into `/` by firmlinks
//! (`/Users`, `/Applications`, `/Library`, `/private/var` and the others listed in
//! `/usr/share/firmlinks`). A walk of `/` therefore crosses into the Data volume's device
//! (allowed: [`BuildOpts::extra_devs`]; no other device) and skips `/System/Volumes` (the
//! Data volume's own mount point, which would count every firmlinked folder a second time,
//! and the container's other volumes); `/Volumes` (other disks, and the link back to `/`),
//! `/dev`, the Spotlight index and the change log are skipped too ([`BuildOpts::skip`]). On an
//! external disk the walk skips `.Spotlight-V100`, `.fseventsd`, `.Trashes` and Time
//! Machine's `Backups.backupdb`. Skipped space shows in "Not measured". Every file is counted once: hard
//! links by (device, inode) on their first path, symbolic links never followed.
//!
//! Folders that cannot be read (most often for want of Full Disk Access) are kept in the tree
//! with [`Node::unreadable`] set and listed in [`Tree::unreadable_paths`]: the map says "could
//! not be read" for them, never a bare 0, and for a whole disk it shows how much of the used
//! space was not measured ([`not_measured`]).
//!
//! What may be selected ([`lock_reason`]): the allow-list is the home folder (minus protected
//! places, Library, hidden items and apps) plus, on an external volume, what is inside its top
//! user folders ([`volume_user_folders`]; never the volume itself or a top item); the
//! volume's own system folders (`.Spotlight-V100`, `.Trashes`, `.fseventsd`,
//! `Backups.backupdb` and every other hidden item) never. Everything else is shown locked with
//! its reason: "part of macOS" or "outside your home folder".

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::env::Env;
use crate::fsutil::entry_size;
use crate::policy;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    /// Bytes on disk, the folder itself included.
    pub size: u64,
    pub is_dir: bool,
    /// Children, biggest first.
    pub children: Vec<Node>,
    /// Files below (1 for a file).
    pub files: u64,
    /// A Photos library is somewhere below this folder: it can be viewed, never removed whole.
    pub holds_library: bool,
    /// This folder could not be read: its size is not known (at least `size`).
    pub unreadable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub root: Option<Node>,
    pub unreadable: u32,
    pub other_volumes: u32,
    /// The scan was stopped; sizes are partial.
    pub stopped: bool,
    /// Every path of each file that has several hard links, by (device, inode). The first
    /// path (name order) carries the size; the others count 0.
    pub links: HashMap<(u64, u64), BTreeSet<PathBuf>>,
    /// The folders that could not be read (their contents are not counted).
    pub unreadable_paths: Vec<PathBuf>,
    /// How the tree was walked (kept for refreshes).
    pub opts: BuildOpts,
}

/// How a walk treats places: paths left out entirely, and other devices it may enter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildOpts {
    /// Left out of the tree (not counted, not shown).
    pub skip: Vec<PathBuf>,
    /// Devices besides the root's that the walk enters (the APFS Data volume behind `/`).
    pub extra_devs: Vec<u64>,
}

/// Link paths by (device, inode).
type Links = HashMap<(u64, u64), BTreeSet<PathBuf>>;

struct Walker<'a> {
    dev: Option<u64>,
    /// Link paths already in the tree outside the part being measured.
    known: &'a Links,
    /// Link paths met in this walk.
    found: Links,
    unreadable: Vec<PathBuf>,
    other_volumes: u32,
    files: u64,
    opts: &'a BuildOpts,
    /// For injected test faults (a folder macOS refuses to list).
    env: Option<&'a Env>,
    cancel: &'a AtomicBool,
    progress: &'a (dyn Fn(u64, &Path) + Sync),
}

#[cfg(unix)]
fn dev_ino(md: &std::fs::Metadata) -> (u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (md.dev(), md.ino(), md.nlink())
}

#[cfg(not(unix))]
fn dev_ino(_md: &std::fs::Metadata) -> (u64, u64, u64) {
    (0, 0, 1)
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

impl Walker<'_> {
    fn node(&mut self, p: &Path) -> Option<Node> {
        let md = std::fs::symlink_metadata(p).ok()?;
        let (dev, ino, nlink) = dev_ino(&md);
        let mut n = Node {
            name: name_of(p),
            path: p.to_path_buf(),
            size: 0,
            is_dir: md.is_dir(),
            children: Vec::new(),
            files: 0,
            holds_library: false,
            unreadable: false,
        };
        if md.is_dir() {
            if self
                .dev
                .is_some_and(|d| d != dev && !self.opts.extra_devs.contains(&dev))
            {
                self.other_volumes += 1;
                return Some(n);
            }
            n.size = entry_size(&md);
            if self.cancel.load(Ordering::Relaxed) {
                return Some(n);
            }
            let denied = self.env.is_some_and(|e| {
                matches!(e.fault_for(p), Some(crate::env::Fault::PermissionDenied))
            });
            let listing = if denied {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                std::fs::read_dir(p)
            };
            match listing {
                Ok(rd) => {
                    let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                    kids.sort();
                    for k in kids {
                        if self.opts.skip.contains(&k) {
                            continue;
                        }
                        if let Some(c) = self.node(&k) {
                            n.size += c.size;
                            n.files += c.files;
                            n.holds_library |= c.holds_library
                                || policy::is_photos_library(std::ffi::OsStr::new(&c.name));
                            n.children.push(c);
                        }
                    }
                    n.children
                        .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
                }
                Err(_) => {
                    n.unreadable = true;
                    self.unreadable.push(p.to_path_buf());
                }
            }
        } else {
            n.files = 1;
            self.files += 1;
            if self.files.is_multiple_of(500) {
                (self.progress)(self.files, p);
            }
            if nlink > 1 && !md.is_dir() {
                let here = self.found.entry((dev, ino)).or_default();
                let first_here = here.is_empty();
                here.insert(p.to_path_buf());
                let earlier_elsewhere = self
                    .known
                    .get(&(dev, ino))
                    .and_then(|s| s.first())
                    .is_some_and(|q| q.as_path() < p);
                if !first_here || earlier_elsewhere {
                    // Another link to a file counted on an earlier path.
                    return Some(n);
                }
            }
            n.size = entry_size(&md);
        }
        Some(n)
    }
}

/// Measure `root`. `progress` gets the number of files seen so far.
pub fn build(root: &Path, cancel: &AtomicBool, progress: &(dyn Fn(u64, &Path) + Sync)) -> Tree {
    build_opts(root, &BuildOpts::default(), cancel, progress)
}

/// [`build`] with places skipped and devices allowed ([`BuildOpts`]).
pub fn build_opts(
    root: &Path,
    opts: &BuildOpts,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, &Path) + Sync),
) -> Tree {
    build_with(root, cancel, progress, &Links::new(), None, opts, None)
}

/// Measure a Disk Map location (see the module notes for the startup disk).
pub fn build_location(
    env: &Env,
    loc: &MapLocation,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, &Path) + Sync),
) -> Tree {
    build_with(
        &loc.root(env),
        cancel,
        progress,
        &Links::new(),
        None,
        &loc.build_opts(env),
        Some(env),
    )
}

/// [`build`] of a part of a tree whose other link paths are `known`; `dev` is the tree's
/// device.
fn build_with(
    root: &Path,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, &Path) + Sync),
    known: &Links,
    dev: Option<u64>,
    opts: &BuildOpts,
    env: Option<&Env>,
) -> Tree {
    let dev = dev.or_else(|| std::fs::symlink_metadata(root).ok().map(|m| dev_ino(&m).0));
    let mut w = Walker {
        dev,
        known,
        found: Links::new(),
        unreadable: Vec::new(),
        other_volumes: 0,
        files: 0,
        opts,
        env,
        cancel,
        progress,
    };
    let node = w.node(root);
    Tree {
        root: node,
        unreadable: w.unreadable.len() as u32,
        other_volumes: w.other_volumes,
        stopped: cancel.load(Ordering::Relaxed),
        links: w.found,
        unreadable_paths: w.unreadable,
        opts: opts.clone(),
    }
}

impl Node {
    pub fn find(&self, p: &Path) -> Option<&Node> {
        if p == self.path {
            return Some(self);
        }
        if !p.starts_with(&self.path) {
            return None;
        }
        self.children.iter().find_map(|c| c.find(p))
    }

    fn find_mut(&mut self, p: &Path) -> Option<&mut Node> {
        if p == self.path {
            return Some(self);
        }
        if !p.starts_with(&self.path) {
            return None;
        }
        self.children.iter_mut().find_map(|c| c.find_mut(p))
    }

    /// Replace the node at `p` (or add it under its parent) and fix every ancestor's totals.
    /// Returns false when `p`'s parent is not in this tree.
    fn put(&mut self, p: &Path, new: Option<Node>) -> bool {
        if !p.starts_with(&self.path) || p == self.path {
            return false;
        }
        let (old_size, old_files) = self.find(p).map(|n| (n.size, n.files)).unwrap_or((0, 0));
        let (new_size, new_files) = new.as_ref().map(|n| (n.size, n.files)).unwrap_or((0, 0));
        let Some(parent) = p.parent() else {
            return false;
        };
        if self.find(parent).is_none() {
            return false;
        }
        // Walk down from self to the parent, adjusting totals.
        let mut cur: &mut Node = self;
        loop {
            cur.size = (cur.size + new_size).saturating_sub(old_size);
            cur.files = (cur.files + new_files).saturating_sub(old_files);
            if cur.path == parent {
                break;
            }
            let Some(i) = cur
                .children
                .iter()
                .position(|c| parent.starts_with(&c.path))
            else {
                return false;
            };
            cur = &mut cur.children[i];
        }
        cur.children.retain(|c| c.path != p);
        if let Some(n) = new {
            cur.children.push(n);
        }
        self.resort_towards(parent);
        true
    }

    /// Set the size of the file node at `p` and fix every ancestor's total.
    fn set_file_size(&mut self, p: &Path, size: u64) {
        if let Some(mut n) = self.find(p).cloned()
            && !n.is_dir
            && n.size != size
        {
            n.size = size;
            self.put(p, Some(n));
        }
    }

    /// Keep "biggest first" on every folder from here down to `p` after sizes changed.
    fn resort_towards(&mut self, p: &Path) {
        if let Some(c) = self.children.iter_mut().find(|c| p.starts_with(&c.path)) {
            c.resort_towards(p);
        }
        self.children
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        self.holds_library = self
            .children
            .iter()
            .any(|c| c.holds_library || policy::is_photos_library(std::ffi::OsStr::new(&c.name)));
    }
}

impl Tree {
    pub fn find(&self, p: &Path) -> Option<&Node> {
        self.root.as_ref()?.find(p)
    }

    pub fn root_path(&self) -> Option<&Path> {
        self.root.as_ref().map(|r| r.path.as_path())
    }

    /// The part of this tree at folder `p`, as a tree of its own (its link paths included).
    pub fn subtree(&self, p: &Path) -> Option<Tree> {
        let node = self.find(p)?.clone();
        let mut links = Links::new();
        for (id, paths) in &self.links {
            let inside: BTreeSet<PathBuf> =
                paths.iter().filter(|q| q.starts_with(p)).cloned().collect();
            if !inside.is_empty() {
                links.insert(*id, inside);
            }
        }
        let unreadable_paths: Vec<PathBuf> = self
            .unreadable_paths
            .iter()
            .filter(|q| q.starts_with(p))
            .cloned()
            .collect();
        Some(Tree {
            root: Some(node),
            links,
            unreadable: unreadable_paths.len() as u32,
            unreadable_paths,
            opts: self.opts.clone(),
            ..Default::default()
        })
    }

    /// Measure `p` again after it changed (moved away, put back, created). Climbs to the
    /// nearest ancestor present in the tree when `p` is new. A hard-linked file still counts
    /// once in the whole tree, on its first path in name order, as in a new scan.
    pub fn refresh(&mut self, p: &Path) {
        let Some(root) = self.root.as_mut() else {
            return;
        };
        if !p.starts_with(&root.path) {
            return;
        }
        if p == root.path {
            let never = AtomicBool::new(false);
            let opts = self.opts.clone();
            *self = build_opts(p, &opts, &never, &|_, _| {});
            return;
        }
        if self.opts.skip.iter().any(|s| p.starts_with(s)) {
            return;
        }
        // The nearest folder from p upward whose parent is in the tree.
        let mut target = p.to_path_buf();
        while let Some(parent) = target.parent() {
            if root.find(parent).is_some() {
                break;
            }
            target = parent.to_path_buf();
        }
        let dev = std::fs::symlink_metadata(&root.path)
            .ok()
            .map(|m| dev_ino(&m).0);
        // Link paths of the measured part are forgotten; the walk finds them again.
        let carriers_before: HashMap<(u64, u64), PathBuf> = self
            .links
            .iter()
            .filter_map(|(id, s)| s.first().map(|c| (*id, c.clone())))
            .collect();
        for s in self.links.values_mut() {
            s.retain(|q| !q.starts_with(&target));
        }
        let never = AtomicBool::new(false);
        let fresh = if std::fs::symlink_metadata(&target).is_ok() {
            build_with(
                &target,
                &never,
                &|_, _| {},
                &self.links,
                dev,
                &self.opts,
                None,
            )
        } else {
            Tree::default()
        };
        self.unreadable_paths.retain(|q| !q.starts_with(&target));
        self.unreadable_paths
            .extend(fresh.unreadable_paths.iter().cloned());
        self.unreadable = self.unreadable_paths.len() as u32;
        let Some(root) = self.root.as_mut() else {
            return;
        };
        root.put(&target, fresh.root);
        for (id, paths) in fresh.links {
            self.links.entry(id).or_default().extend(paths);
        }
        self.links.retain(|_, s| !s.is_empty());
        // A link outside the measured part may have to start or stop carrying the size.
        for (id, before) in carriers_before {
            let Some(now) = self.links.get(&id).and_then(|s| s.first()).cloned() else {
                continue;
            };
            if now == before || now.starts_with(&target) {
                if !before.starts_with(&target) && now != before {
                    root.set_file_size(&before, 0);
                }
                continue;
            }
            // `now` lies outside the measured part and was counting 0.
            if let Ok(md) = std::fs::symlink_metadata(&now) {
                root.set_file_size(&now, entry_size(&md));
            }
            if !before.starts_with(&target) {
                root.set_file_size(&before, 0);
            }
        }
    }

    /// Measure again what a removal or an undo changed: each item's old and new place, and
    /// CleanCraft's own folder (the action log grows).
    pub fn refresh_after(&mut self, env: &Env, records: &[crate::executor::ActionRecord]) {
        for r in records {
            if let crate::executor::ActionKind::Moved {
                original, stored, ..
            } = &r.kind
            {
                self.refresh(original);
                self.refresh(stored);
            }
        }
        self.refresh(env.data_dir());
    }

    /// Mutable access for tests and callers that patch a node.
    pub fn find_mut(&mut self, p: &Path) -> Option<&mut Node> {
        self.root.as_mut()?.find_mut(p)
    }
}

/// Top-level folders of the startup disk that belong to macOS: shown, never selectable.
pub const MACOS_TOP: &[&str] = &[
    "System", "Library", "usr", "bin", "sbin", "private", "etc", "var", "tmp", "dev", "cores",
    "Volumes",
];

/// Folders at the top of an external volume that the volume's system keeps: never selectable,
/// with the reason shown. Every other hidden item at any depth is locked too.
pub const VOLUME_SYSTEM: &[(&str, &str)] = &[
    (".Spotlight-V100", "the disk's Spotlight index"),
    (".Trashes", "the disk's Trash"),
    (".fseventsd", "the disk's record of file changes"),
    ("Backups.backupdb", "Time Machine backups"),
    (".DocumentRevisions-V100", "saved versions of documents"),
    (".TemporaryItems", "temporary files of the disk"),
];

/// The external volume folder (`/Volumes/<name>`) holding `p`, from the path alone: a real
/// folder (not a link, which is how `/Volumes/Macintosh HD` points back to `/`) whose name is
/// not hidden. The executor's allow-list ([`crate::features::space_lens::SpaceLens`]) only has
/// the volumes `df` reports mounted.
pub fn volume_root_of(env: &Env, p: &Path) -> Option<PathBuf> {
    let vols = env.sys_path("/Volumes");
    let rest = p.strip_prefix(&vols).ok()?;
    let name = rest.components().next()?.as_os_str();
    if name.to_string_lossy().starts_with('.') {
        return None;
    }
    let root = vols.join(name);
    match std::fs::symlink_metadata(&root) {
        Ok(md) if md.is_dir() => Some(root),
        _ => None,
    }
}

/// Why `p`, on the external volume at `vol`, cannot be selected.
fn volume_lock_reason(env: &Env, vol: &Path, p: &Path) -> Option<String> {
    if p == vol {
        return Some("the disk itself".into());
    }
    let rel = p.strip_prefix(vol).ok()?;
    let top = rel.components().count() == 1;
    if let Some(first) = rel.components().next() {
        let first = first.as_os_str().to_string_lossy();
        if let Some((_, why)) = VOLUME_SYSTEM.iter().find(|(n, _)| {
            policy::same_name(std::ffi::OsStr::new(n), std::ffi::OsStr::new(&*first))
        }) {
            return Some(format!("system data of this disk: {why}"));
        }
    }
    if rel
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Some("hidden system data of this disk".into());
    }
    if rel.components().any(|c| {
        Path::new(c.as_os_str())
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    }) {
        return Some("an app (use App Remover)".into());
    }
    if let Some(why) = policy::protected_reason(env, p) {
        return Some(format!("protected: {why}"));
    }
    // The allow-list is the user folders at the top of the disk, never the disk itself: a top
    // folder is opened, what is inside it can be removed; a file at the top stays.
    if top {
        return Some(if p.is_dir() {
            "a top folder of this disk (open it to choose what is inside)".into()
        } else {
            "at the top of the disk (only items inside its folders can be removed)".into()
        });
    }
    if let Err(r) = policy::check(env, &volume_user_folders(vol), p) {
        return Some(r.to_string());
    }
    None
}

/// The allow-list on an external volume: its top folders that hold the user's files (real
/// folders, not hidden, not the disk's system folders, not apps). Never the volume itself.
pub fn volume_user_folders(vol: &Path) -> Vec<PathBuf> {
    crate::fsutil::children(vol)
        .into_iter()
        .filter(|c| {
            let name = c
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            !name.starts_with('.')
                && !VOLUME_SYSTEM
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(&name))
                && !c.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
                && std::fs::symlink_metadata(c).is_ok_and(|m| m.is_dir())
        })
        .collect()
}

/// Why `p` cannot be selected in the Disk Map, if it cannot. The allow-list is the home folder
/// and the user folders of external volumes (see the module notes).
pub fn lock_reason(env: &Env, p: &Path) -> Option<String> {
    if let Some(vol) = volume_root_of(env, p) {
        return volume_lock_reason(env, &vol, p);
    }
    let home = env.home();
    if !p.starts_with(home) || p == home {
        let top = p
            .strip_prefix(env.root())
            .ok()
            .and_then(|r| r.components().next())
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        let macos = p == env.root()
            || top
                .as_deref()
                .is_some_and(|t| MACOS_TOP.iter().any(|m| m.eq_ignore_ascii_case(t)));
        return Some(if macos && p != home {
            "part of macOS".into()
        } else {
            "outside your home folder".into()
        });
    }
    if let Some(why) = policy::protected_reason(env, p) {
        return Some(format!("protected: {why}"));
    }
    let lib = env.home_path("Library");
    if policy::starts_with_ci(p, &lib) {
        return Some("app data in Library (use System Junk or App Remover)".into());
    }
    if let Ok(rel) = p.strip_prefix(home)
        && rel
            .components()
            .next()
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Some("hidden settings and the Trash".into());
    }
    let mut cur = Some(p);
    while let Some(c) = cur {
        if c.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) {
            return Some("an app (use App Remover)".into());
        }
        if c == home {
            break;
        }
        cur = c.parent();
    }
    if let Err(r) = policy::check(env, &[home.to_path_buf()], p) {
        return Some(r.to_string());
    }
    None
}

/// Why the map row `node` cannot be selected: [`lock_reason`], or a protected place inside the
/// folder (a Photos library found while measuring it, or a known protected folder below it).
/// Uses the measured tree, no disk access, so it is cheap enough for every drawn row.
pub fn node_lock_reason(env: &Env, node: &Node) -> Option<String> {
    if let Some(why) = lock_reason(env, &node.path) {
        return Some(why);
    }
    if node.holds_library {
        return Some("protected: it holds a Photos library".into());
    }
    None
}

/// [`lock_reason`] plus a look inside a folder for protected places
/// ([`policy::protected_inside`]); reads the disk, for one-off checks such as a click.
pub fn lock_reason_deep(env: &Env, p: &Path) -> Option<String> {
    lock_reason(env, p)
        .or_else(|| policy::protected_inside(env, p).map(|w| format!("protected: {w}")))
}

/// Check a folder typed for the map: inside the home folder, existing, a real folder. Protected
/// folders may be viewed (their items stay locked).
pub fn parse_map_folder(env: &Env, input: &str) -> Result<PathBuf, String> {
    match crate::features::large_old::parse_folder(env, input) {
        Ok(p) => Ok(p),
        Err(e) if e.contains("protected") || e.contains("Library") => {
            let s = input.trim().trim_end_matches('/');
            let p = if let Some(rest) = s.strip_prefix("~/") {
                env.home_path(rest)
            } else if s.starts_with('/') {
                let p = PathBuf::from(s);
                if env.contains(&p) { p } else { env.sys_path(s) }
            } else {
                env.home_path(s)
            };
            match std::fs::symlink_metadata(&p) {
                Ok(md) if md.is_dir() => Ok(p),
                _ => Err(format!("There is no folder at {}.", env.display(&p))),
            }
        }
        Err(e) => Err(e),
    }
}

/// Selected paths without those inside another selected path (moving the folder moves them).
pub fn outermost(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = paths.to_vec();
    v.sort();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in v {
        if !out.iter().any(|o| p.starts_with(o)) {
            out.push(p);
        }
    }
    out
}

/// Total, used and free bytes of the volume holding `path`, from `df -Pk`: (total, used,
/// free). Used is total minus free, like Finder and Live Monitor: on APFS df's own Used column
/// counts only this volume, while other volumes of the container share the space, so it would
/// not add up with free to the disk size.
pub fn parse_df(stdout: &str) -> Option<(u64, u64, u64)> {
    let line = stdout.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = cols.get(1)?.parse().ok()?;
    let free: u64 = cols.get(3)?.parse().ok()?;
    let (total, free) = (total * 1024, free * 1024);
    Some((total, total.saturating_sub(free), free))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskUse {
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
}

/// Disk use of the volume holding `path`: `df -Pk`, else only the free space.
pub fn disk_use(env: &Env, path: &Path) -> DiskUse {
    let p = path.to_string_lossy();
    if let Ok(out) = env.sys.run("/bin/df", &["-Pk", &p])
        && out.ok()
        && let Some((t, u, f)) = parse_df(&out.stdout)
    {
        return DiskUse {
            total: Some(t),
            used: Some(u),
            free: Some(f),
        };
    }
    DiskUse {
        free: env.sys.free_space(path).ok(),
        ..Default::default()
    }
}

/// One line of `df -Pk`: the device, total and free bytes, and where it is mounted (as macOS
/// writes it, may contain spaces).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DfLine {
    pub device: String,
    pub total: u64,
    pub free: u64,
    pub mount: String,
}

/// Every line of `df -Pk` (all mounted file systems). Columns: device, 1024-blocks, used,
/// available, capacity, mount point (the rest of the line: names may hold spaces).
pub fn parse_df_all(stdout: &str) -> Vec<DfLine> {
    let mut out = Vec::new();
    for line in stdout.lines().skip(1) {
        let mut rest = line.trim_start();
        let mut cols: Vec<&str> = Vec::new();
        for _ in 0..5 {
            let Some(end) = rest.find(char::is_whitespace) else {
                break;
            };
            cols.push(&rest[..end]);
            rest = rest[end..].trim_start();
        }
        if cols.len() < 5 || rest.is_empty() {
            continue;
        }
        let (Ok(total), Ok(free)) = (cols[1].parse::<u64>(), cols[3].parse::<u64>()) else {
            continue;
        };
        out.push(DfLine {
            device: cols[0].to_string(),
            total: total * 1024,
            free: free * 1024,
            mount: rest.trim_end().to_string(),
        });
    }
    out
}

/// What `diskutil list -plist external` says: the external disks and their partitions and
/// volumes ("disk4", "disk4s1", "disk5s1"), and the names of mounted ones by mount point.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternalDisks {
    pub disks: BTreeSet<String>,
    pub names: HashMap<String, String>,
}

/// Parse `diskutil list -plist external` (XML property list): `AllDisks` and, from
/// `AllDisksAndPartitions` (with their `Partitions` and `APFSVolumes`), each `MountPoint` with
/// its `VolumeName`.
pub fn parse_diskutil_external(stdout: &str) -> Option<ExternalDisks> {
    let v = plist::Value::from_reader_xml(stdout.as_bytes()).ok()?;
    let d = v.as_dictionary()?;
    let mut out = ExternalDisks::default();
    for disk in d.get("AllDisks")?.as_array()? {
        if let Some(s) = disk.as_string() {
            out.disks.insert(s.to_string());
        }
    }
    fn walk(v: &plist::Value, out: &mut ExternalDisks) {
        let Some(d) = v.as_dictionary() else {
            return;
        };
        if let Some(id) = d.get("DeviceIdentifier").and_then(|x| x.as_string()) {
            out.disks.insert(id.to_string());
        }
        if let (Some(m), Some(n)) = (
            d.get("MountPoint").and_then(|x| x.as_string()),
            d.get("VolumeName").and_then(|x| x.as_string()),
        ) {
            out.names.insert(m.to_string(), n.to_string());
        }
        for key in ["Partitions", "APFSVolumes"] {
            if let Some(a) = d.get(key).and_then(|x| x.as_array()) {
                for x in a {
                    walk(x, out);
                }
            }
        }
    }
    if let Some(a) = d.get("AllDisksAndPartitions").and_then(|x| x.as_array()) {
        for x in a {
            walk(x, &mut out);
        }
    }
    Some(out)
}

/// Volume names under `/Volumes` that are macOS's own, never listed.
pub const SYSTEM_VOLUME_NAMES: &[&str] = &[
    "Recovery",
    "Preboot",
    "VM",
    "Update",
    "xarts",
    "iSCPreboot",
    "Hardware",
];

/// A mounted external volume the Disk Map can measure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// The name Finder shows.
    pub name: String,
    /// Where it is mounted (in the env: inside the fake root in tests).
    pub mount: PathBuf,
    pub total: u64,
    pub free: u64,
}

/// The mounted external volumes, from `df -Pk` (every mount under `/Volumes` on a `/dev/disk`
/// device; hidden and macOS volumes left out) checked against `diskutil list -plist external`
/// when it answers (only disks it calls external: no disk images it does not list, no
/// internal volumes). An empty list when `df` does not answer.
pub fn external_volumes(env: &Env) -> Vec<Volume> {
    let Ok(out) = env.sys.run("/bin/df", &["-Pk"]) else {
        return Vec::new();
    };
    if !out.ok() {
        return Vec::new();
    }
    let ext = env
        .sys
        .run("/usr/sbin/diskutil", &["list", "-plist", "external"])
        .ok()
        .filter(|o| o.ok())
        .and_then(|o| parse_diskutil_external(&o.stdout));
    let mut vols: Vec<Volume> = Vec::new();
    for l in parse_df_all(&out.stdout) {
        let Some(dev) = l.device.strip_prefix("/dev/") else {
            continue;
        };
        if !dev.starts_with("disk") {
            continue;
        }
        let Some(rest) = l.mount.strip_prefix("/Volumes/") else {
            continue;
        };
        if rest.is_empty()
            || rest.contains('/')
            || rest.starts_with('.')
            || rest.starts_with("com.apple.")
            || SYSTEM_VOLUME_NAMES
                .iter()
                .any(|n| n.eq_ignore_ascii_case(rest))
        {
            continue;
        }
        if let Some(e) = &ext
            && !e.disks.contains(dev)
        {
            continue;
        }
        let mount = env.sys_path(&l.mount);
        // A real folder, not a link (`/Volumes/Macintosh HD` links back to `/`).
        if !std::fs::symlink_metadata(&mount).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let name = ext
            .as_ref()
            .and_then(|e| e.names.get(&l.mount).cloned())
            .unwrap_or_else(|| rest.to_string());
        if vols.iter().any(|v| v.mount == mount) {
            continue;
        }
        vols.push(Volume {
            name,
            mount,
            total: l.total,
            free: l.free,
        });
    }
    vols.sort_by_key(|v| v.name.to_lowercase());
    vols
}

/// Where the Disk Map measures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MapLocation {
    #[default]
    Home,
    /// The whole startup disk, from `/`.
    Startup,
    Volume(Volume),
    /// A folder (typed or chosen).
    Folder(PathBuf),
}

impl MapLocation {
    /// The folder walked.
    pub fn root(&self, env: &Env) -> PathBuf {
        match self {
            MapLocation::Home => env.home().to_path_buf(),
            MapLocation::Startup => env.sys_path("/"),
            MapLocation::Volume(v) => v.mount.clone(),
            MapLocation::Folder(p) => p.clone(),
        }
    }

    /// The name shown: "Home folder", "Startup Disk", the volume's name, the folder's path.
    pub fn label(&self, env: &Env) -> String {
        match self {
            MapLocation::Home => "Home folder".into(),
            MapLocation::Startup => "Startup Disk".into(),
            MapLocation::Volume(v) => v.name.clone(),
            MapLocation::Folder(p) => env.display(p),
        }
    }

    /// A stable key for snapshots: "home", "startup", "volume:<name>", "folder:<path>".
    pub fn key(&self, env: &Env) -> String {
        match self {
            MapLocation::Home => "home".into(),
            MapLocation::Startup => "startup".into(),
            MapLocation::Volume(v) => format!("volume:{}", v.name),
            MapLocation::Folder(p) => format!("folder:{}", env.display(p)),
        }
    }

    /// Does this location cover a whole disk (so the used space can be compared)?
    pub fn whole_disk(&self) -> bool {
        matches!(self, MapLocation::Startup | MapLocation::Volume(_))
    }

    /// How to walk it (the startup disk: see the module notes).
    pub fn build_opts(&self, env: &Env) -> BuildOpts {
        match self {
            MapLocation::Startup => {
                let data = env.sys_path("/System/Volumes/Data");
                BuildOpts {
                    skip: vec![
                        // The Data volume (seen through the firmlinks instead) and the other
                        // APFS volumes of the container (Preboot, VM, Update...).
                        env.sys_path("/System/Volumes"),
                        env.sys_path("/Volumes"),
                        env.sys_path("/dev"),
                        env.sys_path("/.Spotlight-V100"),
                        env.sys_path("/.fseventsd"),
                    ],
                    extra_devs: std::fs::symlink_metadata(&data)
                        .ok()
                        .map(|m| dev_ino(&m).0)
                        .into_iter()
                        .collect(),
                }
            }
            // An external disk: its index, change log and Time Machine backups are not
            // walked (huge, hard-linked, never removable): they count as not measured.
            MapLocation::Volume(v) => BuildOpts {
                skip: [
                    ".Spotlight-V100",
                    ".fseventsd",
                    "Backups.backupdb",
                    ".Trashes",
                ]
                .iter()
                .map(|n| v.mount.join(n))
                .collect(),
                extra_devs: Vec::new(),
            },
            _ => BuildOpts::default(),
        }
    }
}

/// The locations the picker offers: home folder, startup disk, then every external volume.
pub fn locations(env: &Env) -> Vec<MapLocation> {
    let mut v = vec![MapLocation::Home, MapLocation::Startup];
    v.extend(external_volumes(env).into_iter().map(MapLocation::Volume));
    v
}

/// The location a mapped folder is (for its name and its snapshot key).
pub fn location_of(env: &Env, root: &Path, vols: &[Volume]) -> MapLocation {
    if root == env.home() {
        MapLocation::Home
    } else if root == env.sys_path("/") {
        MapLocation::Startup
    } else if let Some(v) = vols.iter().find(|v| v.mount == root) {
        MapLocation::Volume(v.clone())
    } else {
        MapLocation::Folder(root.to_path_buf())
    }
}

/// For a whole disk: the part of its used space the walk did not count (folders that could
/// not be read, space macOS keeps for itself such as snapshots and other APFS volumes).
/// `None` when the use is unknown or the walk found as much.
pub fn not_measured(tree: &Tree, disk: &DiskUse) -> Option<u64> {
    let used = disk.used?;
    let measured = tree.root.as_ref()?.size;
    (used > measured).then(|| used - measured)
}

/// A rectangle: x, y, width, height.
pub type Rect = [f32; 4];

/// Squarified treemap (Bruls, Huizing, van Wijk): one rectangle per value, areas proportional
/// to the values, as close to squares as the row-by-row method gets. `values` must be sorted
/// biggest first; zero or negative values get an empty rectangle.
pub fn squarify(values: &[f64], area: Rect) -> Vec<Rect> {
    let n = values.len();
    let mut out = vec![[area[0], area[1], 0.0, 0.0]; n];
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    if total <= 0.0 || area[2] <= 0.0 || area[3] <= 0.0 {
        return out;
    }
    let scale = f64::from(area[2]) * f64::from(area[3]) / total;
    let areas: Vec<f64> = values.iter().map(|v| v.max(0.0) * scale).collect();
    let (mut x, mut y, mut w, mut h) = (
        f64::from(area[0]),
        f64::from(area[1]),
        f64::from(area[2]),
        f64::from(area[3]),
    );
    let worst = |row: &[f64], side: f64| -> f64 {
        let s: f64 = row.iter().sum();
        if s <= 0.0 {
            return f64::INFINITY;
        }
        let mx = row.iter().cloned().fold(0.0, f64::max);
        let mn = row.iter().cloned().fold(f64::INFINITY, f64::min);
        if mn <= 0.0 {
            return f64::INFINITY;
        }
        let s2 = s * s;
        let side2 = side * side;
        (side2 * mx / s2).max(s2 / (side2 * mn))
    };
    let mut i = 0;
    while i < n && areas[i] > 0.0 {
        let side = w.min(h);
        let mut j = i + 1;
        let mut best = worst(&areas[i..j], side);
        while j < n && areas[j] > 0.0 {
            let next = worst(&areas[i..=j], side);
            if next <= best {
                best = next;
                j += 1;
            } else {
                break;
            }
        }
        let row_sum: f64 = areas[i..j].iter().sum();
        if w >= h {
            // A column on the left.
            let cw = if h > 0.0 { row_sum / h } else { 0.0 };
            let mut yy = y;
            for k in i..j {
                let hh = if cw > 0.0 { areas[k] / cw } else { 0.0 };
                out[k] = [x as f32, yy as f32, cw as f32, hh as f32];
                yy += hh;
            }
            x += cw;
            w -= cw;
        } else {
            // A row on top.
            let rh = if w > 0.0 { row_sum / w } else { 0.0 };
            let mut xx = x;
            for k in i..j {
                let ww = if rh > 0.0 { areas[k] / rh } else { 0.0 };
                out[k] = [xx as f32, y as f32, ww as f32, rh as f32];
                xx += ww;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treemap_areas_add_up() {
        let vals = [50.0, 25.0, 12.0, 8.0, 3.0, 2.0, 0.0];
        let area = [10.0, 20.0, 400.0, 300.0];
        let r = squarify(&vals, area);
        let total: f64 = vals.iter().sum();
        for (v, rect) in vals.iter().zip(&r) {
            let a = f64::from(rect[2]) * f64::from(rect[3]);
            let want = v / total * 400.0 * 300.0;
            assert!((a - want).abs() < 1.0, "{v}: {a} vs {want}");
            if *v > 0.0 {
                assert!(rect[0] >= 9.99 && rect[1] >= 19.99);
                assert!(rect[0] + rect[2] <= 410.01 && rect[1] + rect[3] <= 320.01);
            }
        }
        // No two rectangles overlap.
        for a in 0..r.len() {
            for b in a + 1..r.len() {
                let (p, q) = (r[a], r[b]);
                let ox = (p[0] + p[2]).min(q[0] + q[2]) - p[0].max(q[0]);
                let oy = (p[1] + p[3]).min(q[1] + q[3]) - p[1].max(q[1]);
                assert!(ox <= 0.01 || oy <= 0.01, "{a} and {b} overlap");
            }
        }
    }

    #[test]
    fn df_columns() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 1000 600 400 60% /\n";
        assert_eq!(parse_df(out), Some((1000 * 1024, 600 * 1024, 400 * 1024)));
        // APFS: this volume's Used (500) + free (400) is not the disk (1000).
        let apfs = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 1000 500 400 56% /\n";
        assert_eq!(parse_df(apfs), Some((1000 * 1024, 600 * 1024, 400 * 1024)));
    }

    #[test]
    fn outermost_drops_nested() {
        let v = outermost(&["/a/b".into(), "/a".into(), "/c".into(), "/a/b/c".into()]);
        assert_eq!(v, vec![PathBuf::from("/a"), PathBuf::from("/c")]);
    }
}
