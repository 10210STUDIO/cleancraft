//! Carrying out a confirmed plan, and undoing it.
//!
//! Removing never deletes. Each selected item is moved, with one `rename`, into the Trash of
//! its volume or into CleanCraft's quarantine folder, and a journal line records where it went,
//! so History can put it back. Before each move the item passes the safety fence again
//! ([`crate::policy::check`]), must still exist, and must not be open in any process. If the
//! journal cannot be written nothing is moved.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::{Env, Fault};
use crate::fsutil::tree_size;
use crate::plan::{FeatureId, Plan};
use crate::policy::{self, Refusal};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// The Trash of the item's volume (the user's ~/.Trash for the startup disk).
    #[default]
    Trash,
    /// ~/Library/Application Support/CleanCraft/Quarantine/<plan>/...
    Quarantine,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionKind {
    /// A file or folder moved; undo moves it back.
    Moved { original: PathBuf, stored: PathBuf, method: Method },
    /// A system command ran (maintenance). There is nothing to put back.
    Command { command: String, ok: bool, output: String },
    /// A setting changed (a startup item disabled). Undo runs `undo_command`.
    Toggled { what: String, undo_command: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ActionRecord {
    pub id: String,
    pub plan_id: String,
    pub feature: Option<FeatureId>,
    pub time: i64,
    pub size: u64,
    #[serde(flatten)]
    pub kind: ActionKind,
    #[serde(default)]
    pub undone: Option<i64>,
}

impl ActionRecord {
    pub fn can_undo(&self) -> bool {
        self.undone.is_none()
            && match &self.kind {
                ActionKind::Moved { .. } => true,
                ActionKind::Toggled { undo_command, .. } => undo_command.is_some(),
                ActionKind::Command { .. } => false,
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Refused(String),
    Missing,
    InUse,
    AppRunning(String),
    PermissionDenied,
    DiskFull,
    InUseCheckFailed(String),
    Other(String),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::Refused(r) => write!(f, "not allowed: {r}"),
            SkipReason::Missing => write!(f, "already gone"),
            SkipReason::InUse => write!(f, "open in a running app"),
            SkipReason::AppRunning(a) => write!(f, "{a} is running"),
            SkipReason::PermissionDenied => write!(f, "permission denied"),
            SkipReason::DiskFull => write!(f, "the disk is full"),
            SkipReason::InUseCheckFailed(e) => write!(f, "could not check whether it is in use ({e})"),
            SkipReason::Other(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Report {
    pub plan_id: String,
    pub moved: Vec<ActionRecord>,
    pub skipped: Vec<(PathBuf, SkipReason)>,
    /// Sum of the moved items' sizes, measured just before each move.
    pub freed: u64,
}

#[derive(Debug)]
pub enum ExecError {
    /// The confirmed list is not the plan's current selection.
    DigestMismatch,
    /// The action journal could not be written; nothing was moved.
    Journal(io::Error),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::DigestMismatch => write!(f, "the list changed after it was confirmed; review it again"),
            ExecError::Journal(e) => write!(f, "could not write the action log, nothing was removed: {e}"),
        }
    }
}

impl std::error::Error for ExecError {}

pub fn journal_path(env: &Env) -> PathBuf {
    env.data_dir().join("journal.jsonl")
}

/// Move the selected items of a confirmed plan. `allow` is the feature's allow-list.
pub fn execute(
    env: &Env,
    plan: &Plan,
    allow: &[PathBuf],
    confirmed_digest: &str,
    method: Method,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Report, ExecError> {
    if plan.digest() != confirmed_digest {
        return Err(ExecError::DigestMismatch);
    }
    // The journal must be writable before anything moves.
    ensure_journal(env).map_err(ExecError::Journal)?;

    let mut report = Report {
        plan_id: plan.id.clone(),
        ..Default::default()
    };
    let selected: Vec<_> = plan.selected().cloned().collect();
    if selected.is_empty() {
        return Ok(report);
    }
    let open = env.sys.open_files();
    let running = env.sys.running_apps();
    let total = selected.len();
    for (n, item) in selected.iter().enumerate() {
        progress(n, total);
        let p = &item.path;
        if let Err(r) = policy::check(env, allow, p) {
            report.skipped.push((p.clone(), SkipReason::Refused(r.to_string())));
            continue;
        }
        if fs::symlink_metadata(p).is_err() {
            report.skipped.push((p.clone(), SkipReason::Missing));
            continue;
        }
        match &open {
            Ok(set) => {
                if is_open(set, p) {
                    report.skipped.push((p.clone(), SkipReason::InUse));
                    continue;
                }
            }
            Err(e) => {
                report.skipped.push((p.clone(), SkipReason::InUseCheckFailed(e.to_string())));
                continue;
            }
        }
        if let Some(app) = running.iter().find(|a| {
            a.path.as_ref().is_some_and(|ap| {
                let ap = if env.is_fake() { env.sys_path(&ap.to_string_lossy()) } else { ap.clone() };
                p.starts_with(&ap) || ap.starts_with(p)
            })
        }) {
            report.skipped.push((p.clone(), SkipReason::AppRunning(app.name.clone())));
            continue;
        }
        match env.fault_for(p) {
            Some(Fault::PermissionDenied) => {
                report.skipped.push((p.clone(), SkipReason::PermissionDenied));
                continue;
            }
            Some(Fault::InUse) => {
                report.skipped.push((p.clone(), SkipReason::InUse));
                continue;
            }
            Some(Fault::DiskFull) => {
                report.skipped.push((p.clone(), SkipReason::DiskFull));
                continue;
            }
            None => {}
        }
        let (size, _) = tree_size(p);
        match move_one(env, p, &plan.id, method) {
            Ok((stored, used)) => {
                let rec = ActionRecord {
                    id: format!("{}#{}", plan.id, item.id),
                    plan_id: plan.id.clone(),
                    feature: plan.feature,
                    time: chrono::Utc::now().timestamp(),
                    size,
                    kind: ActionKind::Moved {
                        original: p.clone(),
                        stored: stored.clone(),
                        method: used,
                    },
                    undone: None,
                };
                if let Err(e) = append_journal(env, &rec) {
                    // No log line, no removal: put it back.
                    let _ = fs::rename(&stored, p);
                    report.skipped.push((p.clone(), SkipReason::Other(format!("could not log the action: {e}"))));
                    continue;
                }
                report.freed += size;
                report.moved.push(rec);
            }
            Err(e) => {
                let reason = match e.kind() {
                    io::ErrorKind::PermissionDenied => SkipReason::PermissionDenied,
                    io::ErrorKind::StorageFull => SkipReason::DiskFull,
                    io::ErrorKind::ResourceBusy => SkipReason::InUse,
                    _ => SkipReason::Other(e.to_string()),
                };
                report.skipped.push((p.clone(), reason));
            }
        }
    }
    progress(total, total);
    Ok(report)
}

fn is_open(set: &HashSet<PathBuf>, p: &Path) -> bool {
    set.contains(p) || set.iter().any(|o| o.starts_with(p))
}

/// The Trash folder for `p`: the user's Trash on the startup disk, `/Volumes/X/.Trashes/<uid>`
/// on other disks.
pub fn trash_dir_for(env: &Env, p: &Path) -> PathBuf {
    let volumes = env.sys_path("/Volumes");
    if let Ok(rest) = p.strip_prefix(&volumes)
        && let Some(vol) = rest.components().next()
    {
        return volumes.join(vol).join(".Trashes").join(uid().to_string());
    }
    env.user_trash()
}

#[cfg(unix)]
fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    dirs::home_dir()
        .and_then(|h| fs::metadata(h).ok())
        .map(|m| m.uid())
        .unwrap_or(501)
}

#[cfg(not(unix))]
fn uid() -> u32 {
    501
}

/// A free name in `dir` following Finder's pattern: "name", "name 2", "file 3.ext" (folders
/// keep dots in their name).
pub fn free_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let first = dir.join(name);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2.. {
        let cand = dir.join(format!("{stem} {n}{ext}"));
        if fs::symlink_metadata(&cand).is_err() {
            return cand;
        }
    }
    first
}

fn move_one(env: &Env, p: &Path, plan_id: &str, method: Method) -> io::Result<(PathBuf, Method)> {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".into());
    let try_dest = |m: Method| -> io::Result<PathBuf> {
        let dir = match m {
            Method::Trash => trash_dir_for(env, p),
            Method::Quarantine => env.quarantine_dir().join(sanitize(plan_id)),
        };
        if !env.contains(&dir) {
            return Err(io::Error::other("destination outside the allowed root"));
        }
        fs::create_dir_all(&dir)?;
        let is_dir = fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false);
        let dest = free_name(&dir, &name, is_dir);
        fs::rename(p, &dest)?;
        Ok(dest)
    };
    match try_dest(method) {
        Ok(d) => Ok((d, method)),
        // The Trash can be off limits (privacy settings); the quarantine is CleanCraft's own.
        Err(e) if method == Method::Trash && e.kind() == io::ErrorKind::PermissionDenied => {
            try_dest(Method::Quarantine).map(|d| (d, Method::Quarantine))
        }
        Err(e) => Err(e),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn ensure_journal(env: &Env) -> io::Result<()> {
    if let Some(Fault::DiskFull) = env.fault_for(&journal_path(env)) {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
    }
    fs::create_dir_all(env.data_dir())?;
    OpenOptions::new().create(true).append(true).open(journal_path(env))?;
    Ok(())
}

pub fn append_journal(env: &Env, rec: &ActionRecord) -> io::Result<()> {
    ensure_journal(env)?;
    let mut f = OpenOptions::new().append(true).open(journal_path(env))?;
    let mut line = serde_json::to_string(rec).map_err(io::Error::other)?;
    line.push('\n');
    f.write_all(line.as_bytes())?;
    f.sync_data()
}

/// Every recorded action, oldest first. Damaged lines are skipped.
pub fn history(env: &Env) -> Vec<ActionRecord> {
    fs::read_to_string(journal_path(env))
        .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

fn rewrite_journal(env: &Env, recs: &[ActionRecord]) -> io::Result<()> {
    let path = journal_path(env);
    let tmp = path.with_extension("jsonl.tmp");
    let mut s = String::new();
    for r in recs {
        s.push_str(&serde_json::to_string(r).map_err(io::Error::other)?);
        s.push('\n');
    }
    fs::write(&tmp, s)?;
    fs::rename(tmp, path)
}

#[derive(Debug, PartialEq, Eq)]
pub enum UndoError {
    NotFound,
    AlreadyUndone,
    NotUndoable,
    /// Something now exists at the original place.
    OriginalTaken(PathBuf),
    /// The stored copy is gone (the Trash was emptied).
    StoredMissing(PathBuf),
    Io(String),
}

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoError::NotFound => write!(f, "no such action"),
            UndoError::AlreadyUndone => write!(f, "already put back"),
            UndoError::NotUndoable => write!(f, "this action cannot be undone"),
            UndoError::OriginalTaken(p) => write!(f, "something is already at {}", p.display()),
            UndoError::StoredMissing(p) => write!(f, "the removed copy is gone from {} (was the Trash emptied?)", p.display()),
            UndoError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Put one action back.
pub fn undo(env: &Env, action_id: &str) -> Result<ActionRecord, UndoError> {
    let mut recs = history(env);
    let Some(i) = recs.iter().position(|r| r.id == action_id) else {
        return Err(UndoError::NotFound);
    };
    if recs[i].undone.is_some() {
        return Err(UndoError::AlreadyUndone);
    }
    match recs[i].kind.clone() {
        ActionKind::Moved { original, stored, .. } => {
            if !env.contains(&original) || !env.contains(&stored) {
                return Err(UndoError::Io("path outside the allowed root".into()));
            }
            if fs::symlink_metadata(&original).is_ok() {
                return Err(UndoError::OriginalTaken(original));
            }
            if fs::symlink_metadata(&stored).is_err() {
                return Err(UndoError::StoredMissing(stored));
            }
            if let Some(parent) = original.parent() {
                fs::create_dir_all(parent).map_err(|e| UndoError::Io(e.to_string()))?;
            }
            fs::rename(&stored, &original).map_err(|e| UndoError::Io(e.to_string()))?;
        }
        ActionKind::Toggled { undo_command: Some(cmd), .. } => {
            let out = env.sys.run("/bin/sh", &["-c", &cmd]).map_err(|e| UndoError::Io(e.to_string()))?;
            if !out.ok() {
                return Err(UndoError::Io(out.stderr));
            }
        }
        _ => return Err(UndoError::NotUndoable),
    }
    recs[i].undone = Some(chrono::Utc::now().timestamp());
    let rec = recs[i].clone();
    rewrite_journal(env, &recs).map_err(|e| UndoError::Io(e.to_string()))?;
    Ok(rec)
}

/// Undo every still-undoable action of a plan. Returns how many were put back and the errors.
pub fn undo_plan(env: &Env, plan_id: &str) -> (usize, Vec<(String, UndoError)>) {
    let ids: Vec<String> = history(env)
        .into_iter()
        .filter(|r| r.plan_id == plan_id && r.can_undo())
        .map(|r| r.id)
        .collect();
    let mut ok = 0;
    let mut errs = Vec::new();
    for id in ids {
        match undo(env, &id) {
            Ok(_) => ok += 1,
            Err(e) => errs.push((id, e)),
        }
    }
    (ok, errs)
}

/// Explain a refusal for display.
pub fn explain(r: &Refusal) -> String {
    r.to_string()
}
