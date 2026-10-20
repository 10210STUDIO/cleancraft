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
use crate::fsutil::tree_size_once;
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
        /// What was at `stored` right after the move, so Undo never takes back a different
        /// thing that later got the same name there (the Trash emptied, then refilled).
        /// Missing in lines written before it existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stored_id: Option<StoredId>,
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

/// Identity of a moved item at its stored place: same volume and inode (a rename keeps both),
/// same kind, and for a file the same length and modification time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StoredId {
    pub dev: u64,
    pub ino: u64,
    pub dir: bool,
    /// Files only: length in bytes.
    #[serde(default)]
    pub len: Option<u64>,
    /// Files only: modification time, nanoseconds since the epoch.
    #[serde(default)]
    pub mtime_ns: Option<i64>,
}

impl StoredId {
    /// The identity of what is at `p` now (links are not followed).
    pub fn of(p: &Path) -> Option<StoredId> {
        let md = fs::symlink_metadata(p).ok()?;
        #[cfg(unix)]
        let (dev, ino) = {
            use std::os::unix::fs::MetadataExt;
            (md.dev(), md.ino())
        };
        #[cfg(not(unix))]
        let (dev, ino) = (0, 0);
        let file = !md.is_dir();
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64);
        Some(StoredId {
            dev,
            ino,
            dir: md.is_dir(),
            len: file.then_some(md.len()),
            mtime_ns: if file { mtime_ns } else { None },
        })
    }
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
    /// Open in this app (named by the process that holds it).
    InUseBy(String),
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
            SkipReason::InUseBy(a) => write!(f, "open in {a}"),
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
    let mut used = used_ids(env);
    let open = env.sys.open_files_by();
    let running = env.sys.running_apps();
    let total = selected.len();
    let mut admin_batch: Vec<(&Item, u64)> = Vec::new();
    let mut counted = HashSet::new();
    for (n, item) in selected.iter().enumerate() {
        progress(n, total);
        let p = &item.path;
        let by_admin = admin && item.needs_admin;
        if let Err(why) = check_item(env, allow, p, &open, &running, by_admin) {
            report.skipped.push((p.clone(), why));
            continue;
        }
        // A hard-linked file counts once in the whole report, like in the plan and the map.
        let (size, _) = tree_size_once(p, &mut counted);
        if by_admin {
            admin_batch.push((item, size));
            continue;
        }
        match move_one(env, p, &plan.id, method) {
            Ok((stored, used_method)) => {
                let rec = moved_record(plan, item, size, stored.clone(), used_method, &mut used);
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
        admin_move(env, plan, &admin_batch, &mut report, &mut used);
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
    open: &io::Result<Vec<(PathBuf, String)>>,
    running: &[RunningApp],
    by_admin: bool,
) -> Result<(), SkipReason> {
    if let Err(r) = policy::check_move(env, allow, p) {
        return Err(SkipReason::Refused(r.to_string()));
    }
    if fs::symlink_metadata(p).is_err() {
        return Err(SkipReason::Missing);
    }
    match open {
        Ok(list) => {
            if let Some(why) = in_use(list, p) {
                return Err(why);
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

/// An action id not used yet: `<plan>#<item>`, or `<plan>#<item>~2`, `~3`... when the same plan
/// ran before (run again after an Undo), so every record stays reachable by its id.
fn fresh_id(base: String, used: &mut HashSet<String>) -> String {
    let mut id = base.clone();
    let mut n = 2;
    while used.contains(&id) {
        id = format!("{base}~{n}");
        n += 1;
    }
    used.insert(id.clone());
    id
}

/// Ids already in the journal, damaged lines included as far as they show one.
fn used_ids(env: &Env) -> HashSet<String> {
    journal_lines(env)
        .into_iter()
        .filter_map(|(_, r)| r.map(|r| r.id))
        .collect()
}

fn moved_record(
    plan: &Plan,
    item: &Item,
    size: u64,
    stored: PathBuf,
    method: Method,
    used: &mut HashSet<String>,
) -> ActionRecord {
    ActionRecord {
        id: fresh_id(format!("{}#{}", plan.id, item.id), used),
        plan_id: plan.id.clone(),
        feature: plan.feature,
        time: chrono::Utc::now().timestamp(),
        size,
        kind: ActionKind::Moved {
            original: item.path.clone(),
            stored_id: StoredId::of(&stored),
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
fn admin_move(
    env: &Env,
    plan: &Plan,
    batch: &[(&Item, u64)],
    report: &mut Report,
    used: &mut HashSet<String>,
) {
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
        let rec = moved_record(plan, it, *size, dest.clone(), Method::AdminQuarantine, used);
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

/// Is `p` (or something inside it) open? Names compare like a default Mac volume: an edited
/// plan may spell the path in another letter case than lsof reports it. The reason names the
/// app holding it when lsof said which.
fn in_use(list: &[(PathBuf, String)], p: &Path) -> Option<SkipReason> {
    let mut found = false;
    for (o, who) in list {
        if o == p || policy::starts_with_ci(o, p) {
            if !who.is_empty() {
                return Some(SkipReason::InUseBy(who.clone()));
            }
            found = true;
        }
    }
    found.then_some(SkipReason::InUse)
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
        let cand = dir.join(numbered(stem, ext, n));
        if free(&cand) {
            return cand;
        }
    }
    first
}

/// The longest file name macOS volumes take, in bytes.
pub const NAME_MAX: usize = 255;

/// "stem n.ext", shortened at the end of the stem (whole characters) to fit [`NAME_MAX`] the
/// way Finder does with long names; a very long extension is shortened with the stem.
fn numbered(stem: &str, ext: &str, n: u32) -> String {
    let (stem, ext) = if ext.len() + 12 > NAME_MAX {
        (format!("{stem}{ext}"), String::new())
    } else {
        (stem.to_string(), ext.to_string())
    };
    let tail = format!(" {n}{ext}");
    let room = NAME_MAX.saturating_sub(tail.len());
    let mut cut = stem.len().min(room);
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{tail}", &stem[..cut])
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

/// Add one line to the journal. When the last line was cut short (a crash while it was being
/// written, no newline at the end), the new record starts on a line of its own so it stays
/// readable; the damaged line is left as it is.
pub fn append_journal(env: &Env, rec: &ActionRecord) -> io::Result<()> {
    ensure_journal(env)?;
    let path = journal_path(env);
    let mut line = serde_json::to_string(rec).map_err(io::Error::other)?;
    line.push('\n');
    if !ends_with_newline(&path)? {
        line.insert(0, '\n');
    }
    let mut f = OpenOptions::new().append(true).open(&path)?;
    f.write_all(line.as_bytes())?;
    f.sync_data()
}

/// True for an empty file or one whose last byte is a newline.
fn ends_with_newline(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    f.seek(SeekFrom::Start(len - 1))?;
    let mut b = [0u8; 1];
    f.read_exact(&mut b)?;
    Ok(b[0] == b'\n')
}

/// Every journal line, oldest first, with its record when it can be read.
fn journal_lines(env: &Env) -> Vec<(Vec<u8>, Option<ActionRecord>)> {
    fs::read(journal_path(env))
        .map(|b| {
            b.split(|c| *c == b'\n')
                .filter(|l| !l.is_empty())
                .map(|l| (l.to_vec(), serde_json::from_slice(l).ok()))
                .collect()
        })
        .unwrap_or_default()
}

/// Every recorded action, oldest first. Damaged lines are skipped.
pub fn history(env: &Env) -> Vec<ActionRecord> {
    journal_lines(env)
        .into_iter()
        .filter_map(|(_, r)| r)
        .collect()
}

/// Write the journal again with the records `recs` (from [`history`], same order) in place of
/// the lines they came from. Lines CleanCraft cannot read (damaged, or written by a newer
/// version) are kept exactly as they were.
fn rewrite_journal(env: &Env, recs: &[ActionRecord]) -> io::Result<()> {
    let path = journal_path(env);
    let raw = fs::read(&path)?;
    let tmp = path.with_extension("jsonl.tmp");
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut next = recs.iter();
    for l in raw.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let readable = serde_json::from_slice::<ActionRecord>(l).is_ok();
        match readable.then(|| next.next()).flatten() {
            Some(r) => out.extend(serde_json::to_vec(r).map_err(io::Error::other)?),
            None => out.extend_from_slice(l),
        }
        out.push(b'\n');
    }
    fs::write(&tmp, out)?;
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

/// Is a moved item ready to go back: inside the root, its place free, the stored copy there and
/// still the very item that was moved (`id`, when the record has it)? Something else with the
/// stored name counts as the removed copy being gone.
fn check_restore(
    env: &Env,
    original: &Path,
    stored: &Path,
    id: Option<&StoredId>,
) -> Result<(), UndoError> {
    if !env.contains(original) || !env.contains(stored) {
        return Err(UndoError::Io("path outside the allowed root".into()));
    }
    if fs::symlink_metadata(original).is_ok() {
        return Err(UndoError::OriginalTaken(original.to_path_buf()));
    }
    if fs::symlink_metadata(stored).is_err() {
        return Err(UndoError::StoredMissing(stored.to_path_buf()));
    }
    if let Some(id) = id
        && StoredId::of(stored).as_ref() != Some(id)
    {
        return Err(UndoError::StoredMissing(stored.to_path_buf()));
    }
    Ok(())
}

/// The record of `action_id`: the newest one not put back yet, else the newest (ids are unique
/// since each run gets fresh ones; older journals may repeat them).
fn find_record(recs: &[ActionRecord], action_id: &str) -> Option<usize> {
    let mut found = None;
    for (i, r) in recs.iter().enumerate() {
        if r.id == action_id && (found.is_none() || r.undone.is_none()) {
            found = Some(i);
        }
    }
    found
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
    let Some(i) = find_record(&recs, action_id) else {
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
        let recs = history(env);
        return find_record(&recs, action_id)
            .map(|i| recs[i].clone())
            .ok_or(UndoError::NotFound);
    }
    match recs[i].kind.clone() {
        ActionKind::Moved {
            original,
            stored,
            stored_id,
            ..
        } => {
            check_restore(env, &original, &stored, stored_id.as_ref())?;
            if let Some(parent) = original.parent() {
                fs::create_dir_all(parent).map_err(|e| UndoError::Io(e.to_string()))?;
            }
            fs::rename(&stored, &original).map_err(|e| UndoError::Io(e.to_string()))?;
            relink(&mut recs, i, &stored, &original);
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
        let Some(i) = find_record(&recs, id) else {
            errs.push((id.clone(), UndoError::NotFound));
            continue;
        };
        if recs[i].undone.is_some() {
            errs.push((id.clone(), UndoError::AlreadyUndone));
            continue;
        }
        let ActionKind::Moved {
            original,
            stored,
            stored_id,
            ..
        } = recs[i].kind.clone()
        else {
            errs.push((id.clone(), UndoError::NotUndoable));
            continue;
        };
        if let Err(e) = check_restore(env, &original, &stored, stored_id.as_ref()) {
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
            relink(&mut recs, *i, stored, original);
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
    let ids: Vec<String> = history(env)
        .into_iter()
        .filter(|r| r.plan_id == plan_id && r.can_undo())
        .map(|r| r.id)
        .collect();
    undo_actions(env, &ids)
}

/// Undo these actions (ids from the journal), like [`undo_plan`] does for a whole plan: the
/// administrator moves among them go back together, with one password dialog. Ids that are
/// unknown, already put back or not undoable are reported, nothing else changes for them.
pub fn undo_actions(env: &Env, ids: &[String]) -> (usize, Vec<(String, UndoError)>) {
    let all = history(env);
    let mut recs: Vec<ActionRecord> = Vec::new();
    let mut errs = Vec::new();
    for id in ids {
        match find_record(&all, id).map(|i| &all[i]) {
            Some(r) if r.can_undo() => recs.push(r.clone()),
            Some(r) if r.undone.is_some() => errs.push((id.clone(), UndoError::AlreadyUndone)),
            Some(_) => errs.push((id.clone(), UndoError::NotUndoable)),
            None => errs.push((id.clone(), UndoError::NotFound)),
        }
    }
    let mut ok = 0;
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

/// Plan id prefix of [`move_quarantine_to_trash`] runs. Their records move an item from the
/// quarantine to the Trash; Undo puts it back in the quarantine.
pub const QUARANTINE_TO_TRASH: &str = "quarantine-to-trash";

/// Is this record a move from the quarantine to the Trash?
pub fn is_relocation(r: &ActionRecord) -> bool {
    r.plan_id.starts_with(QUARANTINE_TO_TRASH)
}

/// After record `i` moved its item from `from` back to `to` (an Undo), keep the other records
/// that point at the same item true:
/// - a quarantine-to-Trash move put back in the quarantine: the cleanup that put the item in
///   the quarantine finds it there again (its stored place follows);
/// - a cleanup put back from the Trash after the quarantine went to the Trash: that
///   quarantine-to-Trash move is over too (the item left the Trash).
fn relink(recs: &mut [ActionRecord], i: usize, from: &Path, to: &Path) {
    let reloc = is_relocation(&recs[i]);
    let now = chrono::Utc::now().timestamp();
    for (j, r) in recs.iter_mut().enumerate() {
        if j == i || r.undone.is_some() {
            continue;
        }
        let other_reloc = is_relocation(r);
        if let ActionKind::Moved {
            stored, stored_id, ..
        } = &mut r.kind
            && stored.as_path() == from
        {
            if reloc && !other_reloc {
                *stored = to.to_path_buf();
                *stored_id = StoredId::of(to);
            } else if !reloc && other_reloc {
                r.undone = Some(now);
            }
        }
    }
}

/// What the quarantine holds: each item a cleanup put there (the entries of its per-run
/// folders), oldest folder first.
pub fn quarantine_items(env: &Env) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut runs: Vec<PathBuf> = fs::read_dir(env.quarantine_dir())
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    runs.sort();
    for run in runs {
        if fs::symlink_metadata(&run).is_ok_and(|m| m.is_dir()) {
            let mut inner: Vec<PathBuf> = fs::read_dir(&run)
                .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
                .unwrap_or_default();
            inner.sort();
            out.extend(inner);
        } else {
            out.push(run);
        }
    }
    out
}

/// Bytes and items in the quarantine.
pub fn quarantine_size(env: &Env) -> (u64, usize) {
    let items = quarantine_items(env);
    let mut counted = HashSet::new();
    let bytes = items
        .iter()
        .map(|p| tree_size_once(p, &mut counted).0)
        .sum();
    (bytes, items.len())
}

/// Move everything in the quarantine to the Trash of the startup disk. Nothing is deleted:
/// each item is renamed into the Trash and journaled under one run
/// (`quarantine-to-trash-<time>`, so History's Undo All puts them all back in the quarantine).
/// The cleanup that put an item in the quarantine now points at its place in the Trash, so
/// its own Undo still brings it back where it came from. Items that cannot move (open,
/// refused, a fault) are reported as skipped and stay in the quarantine. If the journal cannot
/// be written nothing moves.
pub fn move_quarantine_to_trash(
    env: &Env,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Report, ExecError> {
    ensure_journal(env).map_err(ExecError::Journal)?;
    let plan_id = format!(
        "{QUARANTINE_TO_TRASH}-{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S%3f")
    );
    let mut report = Report {
        plan_id: plan_id.clone(),
        ..Default::default()
    };
    let items = quarantine_items(env);
    if items.is_empty() {
        return Ok(report);
    }
    let trash = env.user_trash();
    let qdir = env.quarantine_dir();
    let mut used = used_ids(env);
    let open = env.sys.open_files_by();
    let total = items.len();
    let mut counted = HashSet::new();
    // (quarantine place, Trash place) of what moved and was journaled.
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (n, q) in items.iter().enumerate() {
        progress(n, total);
        let skip = if !q.starts_with(&qdir) || !env.contains(q) || !env.contains(&trash) {
            Some(SkipReason::Refused(
                "outside the CleanCraft quarantine".into(),
            ))
        } else {
            match &open {
                Ok(list) => in_use(list, q),
                Err(e) => Some(SkipReason::InUseCheckFailed(e.to_string())),
            }
        }
        .or(match env.fault_for(q) {
            Some(Fault::PermissionDenied) => Some(SkipReason::PermissionDenied),
            Some(Fault::InUse) => Some(SkipReason::InUse),
            Some(Fault::DiskFull) => Some(SkipReason::DiskFull),
            _ => None,
        });
        if let Some(why) = skip {
            report.skipped.push((q.clone(), why));
            continue;
        }
        let (size, _) = tree_size_once(q, &mut counted);
        let name = q
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".into());
        let is_dir = fs::symlink_metadata(q).is_ok_and(|m| m.is_dir());
        let dest = match fs::create_dir_all(&trash) {
            Ok(()) => free_name(&trash, &name, is_dir),
            Err(e) => {
                report.skipped.push((q.clone(), skip_for(&e)));
                continue;
            }
        };
        if let Err(e) = fs::rename(q, &dest) {
            report.skipped.push((q.clone(), skip_for(&e)));
            continue;
        }
        let rec = ActionRecord {
            id: fresh_id(format!("{plan_id}#{n}"), &mut used),
            plan_id: plan_id.clone(),
            feature: None,
            time: chrono::Utc::now().timestamp(),
            size,
            kind: ActionKind::Moved {
                original: q.clone(),
                stored_id: StoredId::of(&dest),
                stored: dest.clone(),
                method: Method::Trash,
            },
            undone: None,
        };
        if let Err(e) = append_journal(env, &rec) {
            // No log line, no move: put it back.
            let _ = fs::rename(&dest, q);
            report.skipped.push((
                q.clone(),
                SkipReason::Other(format!("could not log the action: {e}")),
            ));
            continue;
        }
        report.freed += size;
        report.moved.push(rec);
        moved.push((q.clone(), dest));
    }
    progress(total, total);
    if moved.is_empty() {
        return Ok(report);
    }
    // The cleanups that put these items in the quarantine now find them in the Trash.
    let mut recs = history(env);
    for r in recs.iter_mut() {
        if r.undone.is_some() || is_relocation(r) {
            continue;
        }
        if let ActionKind::Moved {
            stored, stored_id, ..
        } = &mut r.kind
            && let Some((_, dest)) = moved.iter().find(|(q, _)| q == stored)
        {
            *stored = dest.clone();
            *stored_id = StoredId::of(dest);
        }
    }
    if let Err(e) = rewrite_journal(env, &recs) {
        // The log cannot say where they are: put every item back in the quarantine and
        // mark the moves as undone, so History stays true.
        let now = chrono::Utc::now().timestamp();
        let mut recs = history(env);
        for (q, dest) in &moved {
            let _ = fs::rename(dest, q);
            report.skipped.push((
                q.clone(),
                SkipReason::Other(format!("could not log the action: {e}")),
            ));
        }
        for r in recs.iter_mut().filter(|r| r.plan_id == plan_id) {
            r.undone = Some(now);
        }
        let _ = rewrite_journal(env, &recs);
        report.moved.clear();
        report.freed = 0;
    }
    Ok(report)
}

/// Explain a refusal for display.
pub fn explain(r: &Refusal) -> String {
    r.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_names_fit_the_limit() {
        assert_eq!(numbered("report", ".pdf", 2), "report 2.pdf");
        let long = "é".repeat(127); // 254 bytes
        let n = numbered(&long, ".txt", 12);
        assert!(n.len() <= NAME_MAX, "{}", n.len());
        assert!(n.ends_with(" 12.txt"));
        let ext = format!(".{}", "x".repeat(250));
        assert!(numbered("a", &ext, 3).len() <= NAME_MAX);
    }

    #[test]
    fn fresh_ids_never_repeat() {
        let mut used = HashSet::from(["p#1".to_string(), "p#1~2".to_string()]);
        assert_eq!(fresh_id("p#1".into(), &mut used), "p#1~3");
        assert_eq!(fresh_id("p#2".into(), &mut used), "p#2");
    }
}
