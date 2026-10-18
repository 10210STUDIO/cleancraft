//! Duplicates: files with exactly the same content, in the location the user picked (same
//! areas and exclusions as Large & Old Files).
//!
//! Matching, cheapest first: files longer than [`MIN_LEN`] bytes are grouped by length, then by
//! the BLAKE3 hash of their first [`PARTIAL`] bytes, then by the BLAKE3 hash of the whole file.
//! Hard links (same device and inode) are one file and never count as copies.
//!
//! Every set gets a `group` key. One copy per set stays unselected (the keeper); the others are
//! selected. The suggested keeper ("Smart") is the copy with the shortest path outside
//! Downloads, then the oldest modified. [`check_keeps`] refuses a plan where every copy of a
//! set is selected, so a set can never lose all its copies; the same rule guards Similar
//! Images.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::plan::{FeatureId, Item, Plan, Risk};

use super::large_old::{
    KINDS, LargeOldFiles, Location, ScanOptions, kind_of, parse_folder, walk_area,
};

pub struct Duplicates;

/// Only files longer than this (100 KB) are compared.
pub const MIN_LEN: u64 = 100_000;
/// Bytes hashed in the first pass (64 KiB).
pub const PARTIAL: usize = 64 * 1024;

/// How to choose the copy kept in each set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepRule {
    /// The scan's suggestion: Duplicates keep the shortest path outside Downloads, then the
    /// oldest; Similar Images keep the most pixels, then the biggest file.
    Smart,
    Newest,
    Oldest,
    Largest,
}

fn hash_partial(p: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = File::open(p)?;
    let mut buf = vec![0u8; PARTIAL];
    let mut n = 0;
    while n < PARTIAL {
        let r = f.read(&mut buf[n..])?;
        if r == 0 {
            break;
        }
        n += r;
    }
    Ok(*blake3::hash(&buf[..n]).as_bytes())
}

fn hash_full(p: &Path) -> std::io::Result<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    h.update_reader(File::open(p)?)?;
    Ok(*h.finalize().as_bytes())
}

/// A file taking part in duplicate matching.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub len: u64,
    /// Bytes on disk.
    pub size: u64,
    pub modified: Option<i64>,
    /// (device, inode) on Unix.
    pub inode: Option<(u64, u64)>,
}

#[cfg(unix)]
fn inode_of(md: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((md.dev(), md.ino()))
}

#[cfg(not(unix))]
fn inode_of(_md: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

/// Sets of identical files, each sorted by path; sets sorted by wasted space, biggest first.
/// The second value counts files that could not be read.
pub fn find_groups(
    files: Vec<Candidate>,
    cancelled: &dyn Fn() -> bool,
) -> (Vec<Vec<Candidate>>, usize) {
    // Hard links: keep the first path of each inode.
    let mut seen = std::collections::HashSet::new();
    let mut by_len: BTreeMap<u64, Vec<Candidate>> = BTreeMap::new();
    for c in files {
        if c.len <= MIN_LEN {
            continue;
        }
        if let Some(i) = c.inode
            && !seen.insert(i)
        {
            continue;
        }
        by_len.entry(c.len).or_default().push(c);
    }
    let mut unreadable = 0usize;
    let mut groups = Vec::new();
    for (_, same_len) in by_len {
        if same_len.len() < 2 {
            continue;
        }
        if cancelled() {
            break;
        }
        let mut by_partial: HashMap<[u8; 32], Vec<Candidate>> = HashMap::new();
        for c in same_len {
            match hash_partial(&c.path) {
                Ok(h) => by_partial.entry(h).or_default().push(c),
                Err(_) => unreadable += 1,
            }
        }
        for (_, same_start) in by_partial {
            if same_start.len() < 2 {
                continue;
            }
            let mut by_full: HashMap<[u8; 32], Vec<Candidate>> = HashMap::new();
            for c in same_start {
                match hash_full(&c.path) {
                    Ok(h) => by_full.entry(h).or_default().push(c),
                    Err(_) => unreadable += 1,
                }
            }
            for (_, mut same) in by_full {
                if same.len() >= 2 {
                    same.sort_by(|a, b| a.path.cmp(&b.path));
                    groups.push(same);
                }
            }
        }
    }
    groups.sort_by(|a, b| {
        let wa = a[0].size * (a.len() as u64 - 1);
        let wb = b[0].size * (b.len() as u64 - 1);
        wb.cmp(&wa).then_with(|| a[0].path.cmp(&b[0].path))
    });
    (groups, unreadable)
}

/// Smart order for duplicates: outside Downloads first, then the shortest path, then the
/// oldest, then by name. The first is kept.
fn smart_key(env: &Env, it: &Item) -> (bool, usize, i64, PathBuf) {
    let in_downloads = it.path.starts_with(env.home_path("Downloads"));
    (
        in_downloads,
        it.path.as_os_str().len(),
        it.modified.unwrap_or(i64::MAX),
        it.path.clone(),
    )
}

/// Item ids per group, in plan order.
pub fn groups(plan: &Plan) -> BTreeMap<String, Vec<usize>> {
    let mut m: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for it in &plan.items {
        if let Some(g) = &it.group {
            m.entry(g.clone()).or_default().push(it.id);
        }
    }
    m
}

/// The copy `rule` keeps among the unlocked items `ids`.
pub fn keeper(env: &Env, plan: &Plan, ids: &[usize], rule: KeepRule) -> Option<usize> {
    let items: Vec<&Item> = ids
        .iter()
        .map(|i| &plan.items[*i])
        .filter(|i| i.locked.is_none())
        .collect();
    let pick = match rule {
        KeepRule::Smart if plan.feature == Some(FeatureId::SimilarPhotos) => {
            items.iter().max_by(|a, b| {
                let pa = super::similar::pixel_count(env, &a.path);
                let pb = super::similar::pixel_count(env, &b.path);
                pa.cmp(&pb)
                    .then(a.size.cmp(&b.size))
                    .then_with(|| b.path.cmp(&a.path))
            })
        }
        KeepRule::Smart => items.iter().min_by_key(|i| smart_key(env, i)),
        KeepRule::Newest => items.iter().max_by(|a, b| {
            a.modified
                .unwrap_or(0)
                .cmp(&b.modified.unwrap_or(0))
                .then_with(|| b.path.cmp(&a.path))
        }),
        KeepRule::Oldest => items.iter().min_by(|a, b| {
            a.modified
                .unwrap_or(i64::MAX)
                .cmp(&b.modified.unwrap_or(i64::MAX))
                .then_with(|| a.path.cmp(&b.path))
        }),
        KeepRule::Largest => items
            .iter()
            .max_by(|a, b| a.size.cmp(&b.size).then_with(|| b.path.cmp(&a.path))),
    };
    pick.map(|i| i.id)
}

/// In every set: keep one copy by `rule`, select the others.
pub fn apply_keep_rule(env: &Env, plan: &mut Plan, rule: KeepRule) {
    for (_, ids) in groups(plan) {
        let Some(k) = keeper(env, plan, &ids, rule) else {
            continue;
        };
        for id in ids {
            plan.set_selected(id, id != k);
        }
    }
}

/// True when selecting item `id` would leave its set with no copy kept.
pub fn would_select_every_copy(plan: &Plan, id: usize) -> bool {
    let Some(g) = plan.items.get(id).and_then(|i| i.group.clone()) else {
        return false;
    };
    plan.items
        .iter()
        .filter(|i| i.group.as_ref() == Some(&g) && i.id != id)
        .all(|i| i.selected && i.locked.is_none())
}

/// Make sure every set keeps at least one copy: where every copy is selected, the Smart
/// keeper is deselected. Returns how many sets were changed.
pub fn keep_one_per_group(env: &Env, plan: &mut Plan) -> usize {
    let mut fixed = 0;
    for (_, ids) in groups(plan) {
        let all = ids
            .iter()
            .all(|i| plan.items[*i].selected && plan.items[*i].locked.is_none());
        if all && let Some(k) = keeper(env, plan, &ids, KeepRule::Smart) {
            plan.set_selected(k, false);
            fixed += 1;
        }
    }
    fixed
}

/// The rule the executor's caller enforces before any move: in every set at least one copy
/// that still exists stays unselected.
pub fn check_keeps(plan: &Plan) -> anyhow::Result<()> {
    for (_, ids) in groups(plan) {
        let kept = ids.iter().any(|i| {
            let it = &plan.items[*i];
            !(it.selected && it.locked.is_none()) && std::fs::symlink_metadata(&it.path).is_ok()
        });
        if !kept {
            let name = plan.items[ids[0]]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            anyhow::bail!(
                "Every copy of {name} is selected. Keep at least one copy in each set; nothing was moved."
            );
        }
    }
    Ok(())
}

impl Feature for Duplicates {
    fn id(&self) -> FeatureId {
        FeatureId::Duplicates
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        LargeOldFiles.allow_roots(env)
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let loc = ScanOptions::load(ctx.env).duplicates;
        scan_location(ctx, &loc)
    }

    fn prepare(&self, _env: &Env, plan: &Plan) -> anyhow::Result<()> {
        check_keeps(plan)
    }
}

pub fn scan_location(ctx: &ScanCtx<'_>, loc: &Location) -> anyhow::Result<Plan> {
    let env = ctx.env;
    let allow = Duplicates.allow_roots(env);
    let mut plan = Plan::new(FeatureId::Duplicates);
    for (id, title, _) in KINDS {
        plan.add_category(
            id,
            title,
            "Sets of files with the same content. One copy of each set always stays.",
            Risk::High,
        );
    }
    if let Location::Folder(p) = loc
        && let Err(e) = parse_folder(env, &p.to_string_lossy())
    {
        plan.notes.push(e);
        return Ok(plan);
    }
    let root = loc.root(env);
    if !root.is_dir() {
        plan.notes
            .push(format!("{} does not exist.", env.display(&root)));
        return Ok(plan);
    }
    let w = walk_area(ctx, &root, &allow, MIN_LEN + 1, false);
    let files: Vec<Candidate> = w
        .found
        .into_iter()
        .map(|f| Candidate {
            len: f.md.len(),
            size: f.size,
            modified: f
                .md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
            inode: inode_of(&f.md),
            path: f.path,
        })
        .collect();
    (ctx.progress)("file contents");
    let (sets, unreadable) = find_groups(files, &|| ctx.cancelled());
    for (n, set) in sets.iter().enumerate() {
        let key = format!("dup-{:04}", n + 1);
        let others = set.len() - 1;
        // The whole set goes in the category of its first file, so a set is never split.
        let cat = kind_of(&set[0].path);
        for c in set {
            let reason = if others == 1 {
                "Same content as 1 other file".to_string()
            } else {
                format!("Same content as {others} other files")
            };
            let id = plan.push(c.path.clone(), c.size, cat, reason, Risk::High, false);
            plan.items[id].group = Some(key.clone());
            plan.items[id].modified = c.modified;
        }
    }
    apply_keep_rule(env, &mut plan, KeepRule::Smart);
    if unreadable > 0 {
        plan.notes.push(format!(
            "{unreadable} files could not be read and were left out."
        ));
    }
    if !w.unreadable.is_empty() {
        plan.notes.push(format!(
            "{} folders could not be read and were skipped.",
            w.unreadable.len()
        ));
    }
    if w.stopped || ctx.cancelled() {
        plan.notes.push("Scan stopped before the end.".into());
    }
    Ok(plan)
}
