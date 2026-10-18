//! Disk Map: a size tree of a folder, the treemap layout that draws it, and the rules for what
//! may be selected.
//!
//! Sizes use the shared formula ([`crate::fsutil::entry_size`], bytes on disk) with three
//! differences from [`crate::fsutil::tree_size`], all to keep the numbers honest:
//! - a file with several hard links is counted once (the first path met, in name order),
//! - the walk does not cross into another volume (a folder whose device differs from the
//!   root's is listed with size 0 and counted in `other_volumes`),
//! - symbolic links are never followed (they count as their own small size).
//!
//! A tree is built once per scanned folder and kept: drilling down needs no new scan. After a
//! removal or an undo only the folders that changed are measured again ([`Tree::refresh`]).

use std::collections::HashSet;
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub root: Option<Node>,
    pub unreadable: u32,
    pub other_volumes: u32,
    /// The scan was stopped; sizes are partial.
    pub stopped: bool,
}

struct Walker<'a> {
    dev: Option<u64>,
    seen: HashSet<(u64, u64)>,
    unreadable: u32,
    other_volumes: u32,
    files: u64,
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
        };
        if md.is_dir() {
            if self.dev.is_some_and(|d| d != dev) {
                self.other_volumes += 1;
                return Some(n);
            }
            n.size = entry_size(&md);
            if self.cancel.load(Ordering::Relaxed) {
                return Some(n);
            }
            match std::fs::read_dir(p) {
                Ok(rd) => {
                    let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                    kids.sort();
                    for k in kids {
                        if let Some(c) = self.node(&k) {
                            n.size += c.size;
                            n.files += c.files;
                            n.children.push(c);
                        }
                    }
                    n.children
                        .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
                }
                Err(_) => self.unreadable += 1,
            }
        } else {
            n.files = 1;
            self.files += 1;
            if self.files % 500 == 0 {
                (self.progress)(self.files, p);
            }
            if nlink > 1 && !md.is_dir() && !self.seen.insert((dev, ino)) {
                // Another link to a file already counted.
                return Some(n);
            }
            n.size = entry_size(&md);
        }
        Some(n)
    }
}

/// Measure `root`. `progress` gets the number of files seen so far.
pub fn build(root: &Path, cancel: &AtomicBool, progress: &(dyn Fn(u64, &Path) + Sync)) -> Tree {
    let dev = std::fs::symlink_metadata(root).ok().map(|m| dev_ino(&m).0);
    let mut w = Walker {
        dev,
        seen: HashSet::new(),
        unreadable: 0,
        other_volumes: 0,
        files: 0,
        cancel,
        progress,
    };
    let node = w.node(root);
    Tree {
        root: node,
        unreadable: w.unreadable,
        other_volumes: w.other_volumes,
        stopped: cancel.load(Ordering::Relaxed),
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
            let Some(i) = cur.children.iter().position(|c| parent.starts_with(&c.path)) else {
                return false;
            };
            cur = &mut cur.children[i];
        }
        cur.children.retain(|c| c.path != p);
        if let Some(n) = new {
            cur.children.push(n);
        }
        cur.children
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        true
    }
}

impl Tree {
    pub fn find(&self, p: &Path) -> Option<&Node> {
        self.root.as_ref()?.find(p)
    }

    pub fn root_path(&self) -> Option<&Path> {
        self.root.as_ref().map(|r| r.path.as_path())
    }

    /// Measure `p` again after it changed (moved away, put back, created). Climbs to the
    /// nearest ancestor present in the tree when `p` is new. Hard links inside the measured
    /// part are counted once within that part.
    pub fn refresh(&mut self, p: &Path) {
        let Some(root) = self.root.as_mut() else {
            return;
        };
        if !p.starts_with(&root.path) {
            return;
        }
        if p == root.path {
            let never = AtomicBool::new(false);
            *self = build(p, &never, &|_, _| {});
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
        let never = AtomicBool::new(false);
        let fresh = if std::fs::symlink_metadata(&target).is_ok() {
            build(&target, &never, &|_, _| {}).root
        } else {
            None
        };
        root.put(&target, fresh);
    }

    /// Mutable access for tests and callers that patch a node.
    pub fn find_mut(&mut self, p: &Path) -> Option<&mut Node> {
        self.root.as_mut()?.find_mut(p)
    }
}

/// Why `p` cannot be selected in the Disk Map, if it cannot. The allow-list is the home folder.
pub fn lock_reason(env: &Env, p: &Path) -> Option<String> {
    let home = env.home();
    if !p.starts_with(home) || p == home {
        return Some("outside your home folder".into());
    }
    if let Some(why) = policy::protected_reason(env, p) {
        return Some(format!("protected: {why}"));
    }
    let lib = env.home_path("Library");
    if p.starts_with(&lib) {
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

/// Used and total bytes of the volume holding `path`, from `df -Pk`: (total, used, free).
pub fn parse_df(stdout: &str) -> Option<(u64, u64, u64)> {
    let line = stdout.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = cols.get(1)?.parse().ok()?;
    let used: u64 = cols.get(2)?.parse().ok()?;
    let free: u64 = cols.get(3)?.parse().ok()?;
    Some((total * 1024, used * 1024, free * 1024))
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
    }

    #[test]
    fn outermost_drops_nested() {
        let v = outermost(&["/a/b".into(), "/a".into(), "/c".into(), "/a/b/c".into()]);
        assert_eq!(v, vec![PathBuf::from("/a"), PathBuf::from("/c")]);
    }
}
