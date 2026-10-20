//! Disk Map snapshots: a compact copy of the last two measured trees of each location, so Mac
//! Health can show "what grew" between two measures.
//!
//! File: `<data>/disk-map-snapshots.json`, `{ "version": 1, "snapshots": [...] }`. A snapshot
//! is written when a Disk Map measure finishes (a stopped one is not kept: its sizes are
//! partial). Per location ([`crate::space::MapLocation::key`]: "home", "startup",
//! "volume:<name>", "folder:<path>") the two newest are kept, older ones dropped.
//!
//! What a snapshot holds: the root's total and every file and folder down to [`DEPTH`] levels
//! below the root, the [`MAX_ENTRIES`] biggest only, each with its size (bytes on disk, the
//! map's formula) and file count. Paths are stored as the user knows them (the fake root
//! stripped in tests), and turned back into env paths when read.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::Env;
use crate::space::{MapLocation, Node, Tree};

/// Levels below the root kept in a snapshot.
pub const DEPTH: usize = 2;
/// Entries kept per snapshot, biggest first.
pub const MAX_ENTRIES: usize = 300;
/// Snapshots kept per location.
pub const KEEP: usize = 2;
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapEntry {
    pub path: PathBuf,
    pub size: u64,
    pub files: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// "home", "startup", "volume:<name>", "folder:<path>".
    pub location: String,
    /// "Home folder", "Startup Disk", the volume's name, the folder's path.
    pub label: String,
    pub root: PathBuf,
    /// Seconds since the epoch.
    pub taken: i64,
    pub total: u64,
    /// Folders that could not be read in this measure.
    pub unreadable: u32,
    pub entries: Vec<SnapEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct File {
    version: u32,
    snapshots: Vec<Snapshot>,
}

/// A path that grew between two snapshots (`before` 0: new).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Growth {
    pub path: PathBuf,
    pub before: u64,
    pub after: u64,
}

impl Growth {
    pub fn grew(&self) -> u64 {
        self.after.saturating_sub(self.before)
    }
}

pub fn path(env: &Env) -> PathBuf {
    env.data_dir().join("disk-map-snapshots.json")
}

/// Stored form of a path: as the user knows it ("/Users/...").
fn stored(env: &Env, p: &Path) -> PathBuf {
    PathBuf::from(env.display(p))
}

/// Back to an env path (inside the fake root in tests).
fn restored(env: &Env, p: &Path) -> PathBuf {
    env.sys_path(&p.to_string_lossy())
}

fn load_file(env: &Env) -> File {
    std::fs::read(path(env))
        .ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok())
        .filter(|f| f.version == VERSION)
        .unwrap_or_default()
}

/// The compact snapshot of `tree` for `loc`, taken at `taken`.
pub fn snapshot_of(env: &Env, loc: &MapLocation, tree: &Tree, taken: i64) -> Option<Snapshot> {
    let root = tree.root.as_ref()?;
    let mut entries = Vec::new();
    fn walk(env: &Env, n: &Node, depth: usize, out: &mut Vec<SnapEntry>) {
        for c in &n.children {
            out.push(SnapEntry {
                path: stored(env, &c.path),
                size: c.size,
                files: c.files,
            });
            if depth + 1 < DEPTH && c.is_dir {
                walk(env, c, depth + 1, out);
            }
        }
    }
    walk(env, root, 0, &mut entries);
    entries.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    entries.truncate(MAX_ENTRIES);
    Some(Snapshot {
        location: loc.key(env),
        label: loc.label(env),
        root: stored(env, &root.path),
        taken,
        total: root.size,
        unreadable: tree.unreadable,
        entries,
    })
}

/// Keep a snapshot of a finished measure of `loc` (a stopped one is not kept). The two newest
/// per location stay. Returns whether one was written.
pub fn record(env: &Env, loc: &MapLocation, tree: &Tree) -> std::io::Result<bool> {
    record_at(env, loc, tree, chrono::Utc::now().timestamp())
}

/// [`record`] with the time given (tests).
pub fn record_at(env: &Env, loc: &MapLocation, tree: &Tree, taken: i64) -> std::io::Result<bool> {
    if tree.stopped {
        return Ok(false);
    }
    let Some(snap) = snapshot_of(env, loc, tree, taken) else {
        return Ok(false);
    };
    let mut f = load_file(env);
    f.version = VERSION;
    f.snapshots.push(snap);
    // Newest first, then at most KEEP per location.
    f.snapshots.sort_by_key(|s| std::cmp::Reverse(s.taken));
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    f.snapshots.retain(|s| {
        let n = seen.entry(s.location.clone()).or_default();
        *n += 1;
        *n <= KEEP
    });
    std::fs::create_dir_all(env.data_dir())?;
    let tmp = path(env).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&f).map_err(std::io::Error::other)?)?;
    std::fs::rename(tmp, path(env))?;
    Ok(true)
}

/// Every kept snapshot, newest first (at most two per location), with env paths.
pub fn saved(env: &Env) -> Vec<Snapshot> {
    let mut v = load_file(env).snapshots;
    v.sort_by_key(|s| std::cmp::Reverse(s.taken));
    for s in &mut v {
        s.root = restored(env, &s.root);
        for e in &mut s.entries {
            e.path = restored(env, &e.path);
        }
    }
    v
}

/// The two kept snapshots of a location: (older, newer).
pub fn latest_pair(env: &Env, location: &str) -> Option<(Snapshot, Snapshot)> {
    let mut v: Vec<Snapshot> = saved(env)
        .into_iter()
        .filter(|s| s.location == location)
        .collect();
    if v.len() < 2 {
        return None;
    }
    let newer = v.remove(0);
    let older = v.remove(0);
    Some((older, newer))
}

/// What grew from `older` to `newer`, biggest growth first. Entries cut from the older
/// snapshot by [`MAX_ENTRIES`] count as new only when the older one had room for them.
pub fn growth(older: &Snapshot, newer: &Snapshot) -> Vec<Growth> {
    let before: std::collections::HashMap<&Path, u64> = older
        .entries
        .iter()
        .map(|e| (e.path.as_path(), e.size))
        .collect();
    let full = older.entries.len() >= MAX_ENTRIES;
    let mut out: Vec<Growth> = newer
        .entries
        .iter()
        .filter_map(|e| {
            let b = match before.get(e.path.as_path()) {
                Some(b) => *b,
                None if full => return None,
                None => 0,
            };
            (e.size > b).then(|| Growth {
                path: e.path.clone(),
                before: b,
                after: e.size,
            })
        })
        .collect();
    out.sort_by(|a, b| b.grew().cmp(&a.grew()).then_with(|| a.path.cmp(&b.path)));
    out
}
