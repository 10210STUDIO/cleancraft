//! Carrying out a confirmed plan, and undoing it.
//!
//! Removing never deletes. Each selected item is moved, with one `rename`, into the Trash of
//! its volume or into CleanCraft's quarantine folder, and a journal line records where it went,
//! so History can put it back. Before each move the item passes the safety fence again
//! ([`crate::policy::check`]), must still exist, and must not be open in any process. If the
//! journal cannot be written nothing is moved.
//!
//! Items in folders only an administrator can change (marked
//! [`crate::plan::Item::needs_admin`]) are moved, when the feature allows it, after the same
//! checks, by ONE administrator command (the macOS password dialog) made only of
//! `/bin/mv -n -- 'from' 'to'` lines into the quarantine. What really moved is then checked on
//! disk and journaled as [`Method::AdminQuarantine`]; undo moves it back the same way.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::{Env, Fault};
use crate::fsutil::tree_size;
use crate::plan::{FeatureId, Item, Plan};
use crate::policy::{self, Refusal};
use crate::sys::{CmdOut, RunningApp, admin_cancelled, admin_move_script};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// The Trash of the item's volume (the user's ~/.Trash for the startup disk).
    #[default]
    Trash,
    /// ~/Library/Application Support/CleanCraft/Quarantine/<plan>/...
    Quarantine,
    /// The quarantine, moved there by an administrator `mv` (the item was in a folder only an
    /// administrator can change); undo needs the password again. Never a user setting.
    AdminQuarantine,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionKind {
    /// A file or folder moved; undo moves it back.
    Moved {
        original: PathBuf,
        stored: PathBuf,
        method: Method,
    },
    /// A system command ran (maintenance). There is nothing to put back.
    Command {
        command: String,
        ok: bool,
        output: String,
    },
    /// A setting changed (a startup item disabled). Undo runs `undo_command`.
    Toggled {
        what: String,
        undo_command: Option<String>,
    },
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
    /// The administrator password dialog was cancelled.
    AdminCancelled,
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
            SkipReason::InUseCheckFailed(e) => {
                write!(f, "could not check whether it is in use ({e})")
            }
            SkipReason::AdminCancelled => {
                write!(
                    f,
                    "the administrator password was not given, so it was not moved"
                )
            }
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
            ExecError::DigestMismatch => write!(
                f,
                "the list changed after it was confirmed; review it again"
            ),
            ExecError::Journal(e) => write!(
                f,
                "could not write the action log, nothing was removed: {e}"
            ),
        }
    }
}

impl std::error::Error for ExecError {}

pub fn journal_path(env: &Env) -> PathBuf {
    env.data_dir().join("journal.jsonl")
}

/// Move the selected items of a confirmed plan. `allow` is the feature's allow-list.
/// Items marked `needs_admin` are moved as the user too (and usually fail with "permission
/// denied"); see [`execute_with`].
pub fn execute(
    env: &Env,
    plan: &Plan,
    allow: &[PathBuf],
    confirmed_digest: &str,
    method: Method,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Report, ExecError> {
    execute_with(env, plan, allow, confirmed_digest, method, false, progress)
}

/// [`execute`], and when `admin` is true (the feature allows it) the selected items marked
/// `needs_admin` go to the quarantine with one administrator command after the same checks.
pub fn execute_with(
    env: &Env,
    plan: &Plan,
    allow: &[PathBuf],
    confirmed_digest: &str,
    method: Method,
    admin: bool,
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
    let mut admin_batch: Vec<(&Item, u64)> = Vec::new();
    for (n, item) in selected.iter().enumerate() {
        progress(n, total);
        let p = &item.path;
        let by_admin = admin && item.needs_admin;
        if let Err(why) = check_item(env, allow, p, &open, &running, by_admin) {
            report.skipped.push((p.clone(), why));
            continue;
        }
        let (size, _) = tree_size(p);
        if by_admin {
            admin_batch.push((item, size));
            continue;
        }
        match move_one(env, p, &plan.id, method) {
            Ok((stored, used)) => {
                let rec = moved_record(plan, item, size, stored.clone(), used);
                if let Err(e) = append_journal(env, &rec) {
                    // No log line, no removal: put it back.
                    let _ = fs::rename(&stored, p);
                    report.skipped.push((
                        p.clone(),
                        SkipReason::Other(format!("could not log the action: {e}")),
                    ));
                    continue;
                }
                report.freed += size;
                report.moved.push(rec);
            }
            Err(e) => {
                report.skipped.push((p.clone(), skip_for(&e)));
            }
        }
    }
    if !admin_batch.is_empty() {
        admin_move(env, plan, &admin_batch, &mut report);
    }
    progress(total, total);
    Ok(report)
}

/// The checks every item passes right before it moves: the fence, still there, not open, not
/// part of a running app, no injected fault. An item moved by the administrator skips the
/// "permission denied" fault: that is why it needs the administrator.
fn check_item(
    env: &Env,
    allow: &[PathBuf],
    p: &Path,
    open: &io::Result<HashSet<PathBuf>>,
    running: &[RunningApp],
    by_admin: bool,
) -> Result<(), SkipReason> {
    if let Err(r) = policy::check(env, allow, p) {
        return Err(SkipReason::Refused(r.to_string()));
    }
    if fs::symlink_metadata(p).is_err() {
        return Err(SkipReason::Missing);
    }
    match open {
        Ok(set) => {
            if is_open(set, p) {
                return Err(SkipReason::InUse);
            }
        }
        Err(e) => return Err(SkipReason::InUseCheckFailed(e.to_string())),
    }
    if let Some(app) = running.iter().find(|a| {
        a.path.as_ref().is_some_and(|ap| {
            let ap = if env.is_fake() {
                env.sys_path(&ap.to_string_lossy())
            } else {
                ap.clone()
            };
            p.starts_with(&ap) || ap.starts_with(p)
        })
    }) {
        return Err(SkipReason::AppRunning(app.name.clone()));
    }
    match env.fault_for(p) {
        Some(Fault::PermissionDenied) if !by_admin => Err(SkipReason::PermissionDenied),
        Some(Fault::InUse) => Err(SkipReason::InUse),
        Some(Fault::DiskFull) => Err(SkipReason::DiskFull),
        _ => Ok(()),
    }
}

fn skip_for(e: &io::Error) -> SkipReason {
    match e.kind() {
        io::ErrorKind::PermissionDenied => SkipReason::PermissionDenied,
        io::ErrorKind::StorageFull => SkipReason::DiskFull,
        io::ErrorKind::ResourceBusy => SkipReason::InUse,
        _ => SkipReason::Other(e.to_string()),
    }
}

fn moved_record(
    plan: &Plan,
    item: &Item,
    size: u64,
    stored: PathBuf,
    method: Method,
) -> ActionRecord {
    ActionRecord {
        id: format!("{}#{}", plan.id, item.id),
        plan_id: plan.id.clone(),
        feature: plan.feature,
        time: chrono::Utc::now().timestamp(),
        size,
        kind: ActionKind::Moved {
            original: item.path.clone(),
            stored,
            method,
        },
        undone: None,
    }
}

/// Run one administrator command made of `mv -n` lines and return it with its result.
/// `Err` carries the reason every pair failed (cancelled dialog, command not started).
fn run_admin_moves(env: &Env, pairs: &[(PathBuf, PathBuf)]) -> Result<CmdOut, SkipReason> {
    let mut text = Vec::new();
    for (a, b) in pairs {
        match (a.to_str(), b.to_str()) {
            (Some(a), Some(b)) => text.push((a, b)),
            _ => {
                return Err(SkipReason::Other(
                    "its name is not plain text, so it cannot be passed to the administrator command".into(),
                ));
            }
        }
    }
    let script = admin_move_script(&text);
    match env.sys.run_admin(&script) {
        Ok(out) if admin_cancelled(&out) => Err(SkipReason::AdminCancelled),
        Ok(out) => Ok(out),
        Err(e) => Err(SkipReason::Other(format!(
            "the administrator command could not run: {e}"
        ))),
    }
}

/// Why an administrator `mv` of `src` did not happen, from the command's error output.
fn admin_failure(out: &CmdOut, src: &Path) -> SkipReason {
    let shown = src.to_string_lossy();
    let line = out
        .stderr
        .lines()
        .find(|l| l.contains(shown.as_ref()))
        .or_else(|| out.stderr.lines().find(|l| !l.trim().is_empty()));
    match line {
        Some(l) if l.contains("Operation not permitted") || l.contains("Permission denied") => {
            SkipReason::Other(format!(
                "macOS refused even the administrator ({})",
                l.trim()
            ))
        }
        Some(l) => SkipReason::Other(format!("the administrator move failed: {}", l.trim())),
        None => SkipReason::Other("the administrator move did not happen".into()),
    }
}

/// Move every item of `batch` into the quarantine with one administrator command, then journal
/// what really moved (checked on disk).
fn admin_move(env: &Env, plan: &Plan, batch: &[(&Item, u64)], report: &mut Report) {
    let dir = env.quarantine_dir().join(sanitize(&plan.id));
    let fail_all = |report: &mut Report, why: SkipReason| {
        for (it, _) in batch {
            report.skipped.push((it.path.clone(), why.clone()));
        }
    };
    if !env.contains(&dir) {
        return fail_all(
            report,
            SkipReason::Other("destination outside the allowed root".into()),
        );
    }
    if let Err(e) = fs::create_dir_all(&dir) {
        return fail_all(report, skip_for(&e));
    }
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut pairs = Vec::new();
    for (it, _) in batch {
        let name = it
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".into());
        let is_dir = fs::symlink_metadata(&it.path)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let dest = free_name_except(&dir, &name, is_dir, &taken);
        taken.insert(dest.clone());
        pairs.push((it.path.clone(), dest));
    }
    let out = match run_admin_moves(env, &pairs) {
        Ok(out) => out,
        Err(why) => return fail_all(report, why),
    };
    let mut unlogged: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for ((it, size), (src, dest)) in batch.iter().zip(&pairs) {
        let moved = fs::symlink_metadata(src).is_err() && fs::symlink_metadata(dest).is_ok();
        if !moved {
            report.skipped.push((src.clone(), admin_failure(&out, src)));
            continue;
        }
        let rec = moved_record(plan, it, *size, dest.clone(), Method::AdminQuarantine);
        match append_journal(env, &rec) {
            Ok(()) => {
                report.freed += size;
                report.moved.push(rec);
            }
            Err(e) => unlogged.push((dest.clone(), src.clone(), e.to_string())),
        }
    }
    if unlogged.is_empty() {
        return;
    }
    // No log line, no removal: put them back (the password is asked again).
    let back: Vec<(PathBuf, PathBuf)> = unlogged
        .iter()
        .map(|(d, s, _)| (d.clone(), s.clone()))
        .collect();
    let res = run_admin_moves(env, &back);
    for (dest, src, e) in unlogged {
        let restored = fs::symlink_metadata(&src).is_ok();
        let why = if restored {
            format!("could not log the action: {e}")
        } else {
            format!(
                "could not log the action ({e}); it is still in the quarantine at {}{}",
                dest.display(),
                match &res {
                    Err(r) => format!(" ({r})"),
                    Ok(_) => String::new(),
                }
            )
        };
        report.skipped.push((src, SkipReason::Other(why)));
    }
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
    free_name_except(dir, name, is_dir, &HashSet::new())
}

/// [`free_name`], also avoiding names already given out in `taken`.
fn free_name_except(dir: &Path, name: &str, is_dir: bool, taken: &HashSet<PathBuf>) -> PathBuf {
    let free = |p: &Path| fs::symlink_metadata(p).is_err() && !taken.contains(p);
    let first = dir.join(name);
    if free(&first) {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2.. {
        let cand = dir.join(format!("{stem} {n}{ext}"));
        if free(&cand) {
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
            Method::Quarantine | Method::AdminQuarantine => {
                env.quarantine_dir().join(sanitize(plan_id))
            }
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
    // Moving as the user is never an administrator move.
    let method = match method {
        Method::AdminQuarantine => Method::Quarantine,
        m => m,
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn ensure_journal(env: &Env) -> io::Result<()> {
    if let Some(Fault::DiskFull) = env.fault_for(&journal_path(env)) {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
    }
    fs::create_dir_all(env.data_dir())?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path(env))?;
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
        .map(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoError {
    NotFound,
    AlreadyUndone,
    NotUndoable,
    /// Something now exists at the original place.
    OriginalTaken(PathBuf),
    /// The stored copy is gone (the Trash was emptied).
    StoredMissing(PathBuf),
    /// The administrator password dialog was cancelled; nothing was put back.
    AdminCancelled,
    Io(String),
}

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoError::NotFound => write!(f, "no such action"),
            UndoError::AlreadyUndone => write!(f, "already put back"),
            UndoError::NotUndoable => write!(f, "this action cannot be undone"),
            UndoError::OriginalTaken(p) => write!(f, "something is already at {}", p.display()),
            UndoError::StoredMissing(p) => write!(
                f,
                "the removed copy is gone from {} (was the Trash emptied?)",
                p.display()
            ),
            UndoError::AdminCancelled => write!(
                f,
                "the administrator password was not given, so nothing was put back"
            ),
            UndoError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Is a moved item ready to go back: inside the root, its place free, the stored copy there?
fn check_restore(env: &Env, original: &Path, stored: &Path) -> Result<(), UndoError> {
    if !env.contains(original) || !env.contains(stored) {
        return Err(UndoError::Io("path outside the allowed root".into()));
    }
    if fs::symlink_metadata(original).is_ok() {
        return Err(UndoError::OriginalTaken(original.to_path_buf()));
    }
    if fs::symlink_metadata(stored).is_err() {
        return Err(UndoError::StoredMissing(stored.to_path_buf()));
    }
    Ok(())
}

fn is_admin_move(r: &ActionRecord) -> bool {
    matches!(
        r.kind,
        ActionKind::Moved {
            method: Method::AdminQuarantine,
            ..
        }
    )
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
    if is_admin_move(&recs[i]) {
        let (_, mut errs) = undo_admin(env, &[action_id.to_string()]);
        if let Some((_, e)) = errs.pop() {
            return Err(e);
        }
        return history(env)
            .into_iter()
            .find(|r| r.id == action_id)
            .ok_or(UndoError::NotFound);
    }
    match recs[i].kind.clone() {
        ActionKind::Moved {
            original, stored, ..
        } => {
            check_restore(env, &original, &stored)?;
            if let Some(parent) = original.parent() {
                fs::create_dir_all(parent).map_err(|e| UndoError::Io(e.to_string()))?;
            }
            fs::rename(&stored, &original).map_err(|e| UndoError::Io(e.to_string()))?;
        }
        ActionKind::Toggled {
            undo_command: Some(cmd),
            ..
        } => {
            let out = env
                .sys
                .run("/bin/sh", &["-c", &cmd])
                .map_err(|e| UndoError::Io(e.to_string()))?;
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

/// Put back administrator moves with one administrator command (one password dialog), then
/// mark in the journal what is really back on disk.
fn undo_admin(env: &Env, ids: &[String]) -> (usize, Vec<(String, UndoError)>) {
    let mut recs = history(env);
    let mut errs = Vec::new();
    let mut ready: Vec<(usize, PathBuf, PathBuf)> = Vec::new();
    for id in ids {
        let Some(i) = recs.iter().position(|r| &r.id == id) else {
            errs.push((id.clone(), UndoError::NotFound));
            continue;
        };
        if recs[i].undone.is_some() {
            errs.push((id.clone(), UndoError::AlreadyUndone));
            continue;
        }
        let ActionKind::Moved {
            original, stored, ..
        } = recs[i].kind.clone()
        else {
            errs.push((id.clone(), UndoError::NotUndoable));
            continue;
        };
        if let Err(e) = check_restore(env, &original, &stored) {
            errs.push((id.clone(), e));
            continue;
        }
        // The folder it came from belongs to the system; CleanCraft does not create it.
        if let Some(parent) = original.parent()
            && fs::symlink_metadata(parent).is_err()
        {
            errs.push((
                id.clone(),
                UndoError::Io(format!("the folder {} is gone", parent.display())),
            ));
            continue;
        }
        ready.push((i, stored, original));
    }
    if ready.is_empty() {
        return (0, errs);
    }
    let pairs: Vec<(PathBuf, PathBuf)> = ready
        .iter()
        .map(|(_, s, o)| (s.clone(), o.clone()))
        .collect();
    let out = match run_admin_moves(env, &pairs) {
        Ok(out) => out,
        Err(why) => {
            let e = match why {
                SkipReason::AdminCancelled => UndoError::AdminCancelled,
                other => UndoError::Io(other.to_string()),
            };
            for (i, _, _) in &ready {
                errs.push((recs[*i].id.clone(), e.clone()));
            }
            return (0, errs);
        }
    };
    let mut ok = 0;
    let now = chrono::Utc::now().timestamp();
    for (i, stored, original) in &ready {
        if fs::symlink_metadata(original).is_ok() && fs::symlink_metadata(stored).is_err() {
            recs[*i].undone = Some(now);
            ok += 1;
        } else {
            errs.push((
                recs[*i].id.clone(),
                UndoError::Io(admin_failure(&out, stored).to_string()),
            ));
        }
    }
    if ok > 0
        && let Err(e) = rewrite_journal(env, &recs)
    {
        errs.push((String::new(), UndoError::Io(e.to_string())));
    }
    (ok, errs)
}

/// Undo every still-undoable action of a plan. Returns how many were put back and the errors.
/// Administrator moves go back together, with one password dialog.
pub fn undo_plan(env: &Env, plan_id: &str) -> (usize, Vec<(String, UndoError)>) {
    let recs: Vec<ActionRecord> = history(env)
        .into_iter()
        .filter(|r| r.plan_id == plan_id && r.can_undo())
        .collect();
    let mut ok = 0;
    let mut errs = Vec::new();
    let admin_ids: Vec<String> = recs
        .iter()
        .filter(|r| is_admin_move(r))
        .map(|r| r.id.clone())
        .collect();
    for r in recs.iter().filter(|r| !is_admin_move(r)) {
        match undo(env, &r.id) {
            Ok(_) => ok += 1,
            Err(e) => errs.push((r.id.clone(), e)),
        }
    }
    if !admin_ids.is_empty() {
        let (n, e) = undo_admin(env, &admin_ids);
        ok += n;
        errs.extend(e);
    }
    (ok, errs)
}

/// Explain a refusal for display.
pub fn explain(r: &Refusal) -> String {
    r.to_string()
}
