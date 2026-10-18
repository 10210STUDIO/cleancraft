//! Trash Bins: how much is waiting in every Trash.
//!
//! - The startup disk's Trash (`~/.Trash`), the Trash of every other disk
//!   (`/Volumes/<disk>/.Trashes/<uid>`), and Mail's deleted-messages mailboxes (T1).
//! - CleanCraft never deletes for good, and these items are already in a Trash, so every item is
//!   listed locked and nothing is in the allow-list. The one action is asking Finder to empty
//!   the Trash (T2), after the user confirmed, logged as a command. Mail's trash is emptied in
//!   Mail (Mailbox > Erase Deleted Items), which the item says.

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::features::mail::version_dirs;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Plan, Risk};

pub struct TrashBins;

pub const LOCK_IN_TRASH: &str =
    "Already in the Trash: use Empty Trash in Finder to remove it for good";
pub const LOCK_MAIL_TRASH: &str =
    "Deleted mail: erase it in Mail with Mailbox > Erase Deleted Items";

pub const EMPTY_SCRIPT: &str = "tell application \"Finder\" to empty trash";
pub const OSASCRIPT: &str = "/usr/bin/osascript";

/// Mail mailboxes that hold deleted messages.
pub const MAIL_TRASH_NAMES: &[&str] = &["Deleted Messages.mbox", "Trash.mbox"];

/// The numeric user id owning the home folder (names the per-user Trash on other disks).
pub fn uid(env: &Env) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(env.home())
            .map(|m| m.uid())
            .unwrap_or(501)
    }
    #[cfg(not(unix))]
    {
        let _ = env;
        501
    }
}

/// One Trash: where it is and what is in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashInfo {
    pub path: PathBuf,
    pub label: String,
    /// Bytes of its contents (the sum of each entry's tree size).
    pub size: u64,
    pub entries: usize,
    /// Mail's trash is emptied in Mail, not Finder.
    pub mail: bool,
    pub readable: bool,
}

/// Size of a Trash's contents: each entry with the shared formula.
pub fn contents_size(dir: &Path) -> (u64, usize) {
    let kids = children(dir);
    (kids.iter().map(|c| tree_size(c).0).sum(), kids.len())
}

fn info(env: &Env, path: PathBuf, label: String, mail: bool) -> TrashInfo {
    let readable = crate::features::system_junk::can_read(env, &path);
    let (size, entries) = if mail {
        let (s, _) = tree_size(&path);
        (s, count_messages(&path))
    } else {
        contents_size(&path)
    };
    TrashInfo {
        path,
        label,
        size,
        entries,
        mail,
        readable,
    }
}

fn count_messages(mbox: &Path) -> usize {
    let mut files = Vec::new();
    crate::fsutil::walk_files(mbox, &|_| false, &mut files);
    files
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "emlx"))
        .count()
}

/// Every Trash that exists: the user's, other disks', Mail's.
pub fn trashes(env: &Env) -> Vec<TrashInfo> {
    let mut v = Vec::new();
    let user = env.user_trash();
    if user.exists() {
        v.push(info(env, user, "Trash of the startup disk".into(), false));
    }
    let id = uid(env).to_string();
    for vol in children(&env.sys_path("/Volumes")) {
        let t = vol.join(".Trashes").join(&id);
        if std::fs::symlink_metadata(&t).is_ok_and(|m| m.is_dir()) {
            let name = vol
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            v.push(info(env, t, format!("Trash of {name}"), false));
        }
    }
    for ver in version_dirs(env) {
        for acct in children(&ver) {
            for n in MAIL_TRASH_NAMES {
                let mb = acct.join(n);
                if std::fs::symlink_metadata(&mb).is_ok_and(|m| m.is_dir()) {
                    let a = acct
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let a = if a == "Mailboxes" {
                        "On My Mac".to_string()
                    } else {
                        a
                    };
                    v.push(info(env, mb, format!("Mail trash ({a})"), true));
                }
            }
        }
    }
    v
}

/// Total bytes Finder's Empty Trash would free (user Trash plus other disks).
pub fn finder_trash_size(env: &Env) -> u64 {
    trashes(env)
        .iter()
        .filter(|t| !t.mail)
        .map(|t| t.size)
        .sum()
}

impl Feature for TrashBins {
    fn id(&self) -> FeatureId {
        FeatureId::TrashBins
    }

    /// Nothing: items are already in a Trash and CleanCraft never deletes for good.
    fn allow_roots(&self, _env: &Env) -> Vec<PathBuf> {
        Vec::new()
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let env = ctx.env;
        let mut plan = Plan::new(FeatureId::TrashBins);
        (ctx.progress)("the Trash");
        for t in trashes(env) {
            if !t.readable {
                plan.notes.push(format!(
                    "{} could not be read. Turn on Full Disk Access to see what is in it.",
                    env.display(&t.path)
                ));
                continue;
            }
            if t.entries == 0 || t.size == 0 || ctx.ignored(&t.path) {
                continue;
            }
            let (cat, title, desc, lock) = if t.mail {
                (
                    "mail_trash",
                    "Mail trash",
                    "Messages you deleted in Mail.",
                    LOCK_MAIL_TRASH,
                )
            } else if t.path == env.user_trash() {
                (
                    "startup_trash",
                    "Startup disk Trash",
                    "Items you moved to the Trash on this Mac's disk.",
                    LOCK_IN_TRASH,
                )
            } else {
                (
                    "external_trash",
                    "Other disks",
                    "Items moved to the Trash on external disks.",
                    LOCK_IN_TRASH,
                )
            };
            plan.add_category(cat, title, desc, Risk::Medium);
            let what = if t.mail { "messages" } else { "items" };
            let id = plan.push(
                t.path.clone(),
                t.size,
                cat,
                format!("{}: {} {what}", t.label, t.entries),
                Risk::Medium,
                false,
            );
            plan.items[id].group = Some(t.label.clone());
            plan.lock(id, lock);
        }
        Ok(plan)
    }
}

/// The action journal can be written (checked before a command runs, like the executor does
/// before moving anything).
pub fn journal_ready(env: &Env) -> std::io::Result<()> {
    let path = executor::journal_path(env);
    if let Some(crate::env::Fault::DiskFull) = env.fault_for(&path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "disk full",
        ));
    }
    std::fs::create_dir_all(env.data_dir())?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
}

/// Result of asking Finder to empty the Trash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyOutcome {
    pub ok: bool,
    /// Bytes in Finder's Trashes before, minus after.
    pub freed: u64,
    pub message: String,
}

/// Ask Finder to empty the Trash (the user confirmed in CleanCraft first). Logged as a
/// command in the journal. Never deletes anything itself.
pub fn empty_trash_in_finder(env: &Env) -> EmptyOutcome {
    if let Err(e) = journal_ready(env) {
        return EmptyOutcome {
            ok: false,
            freed: 0,
            message: format!(
                "The action log could not be written ({e}), so Finder was not asked to empty the Trash."
            ),
        };
    }
    let before = finder_trash_size(env);
    let command = format!("osascript -e '{EMPTY_SCRIPT}'");
    let res = env.sys.run(OSASCRIPT, &["-e", EMPTY_SCRIPT]);
    let (ok, output) = match &res {
        Ok(o) => (
            o.ok(),
            format!("{}{}", o.stdout, o.stderr).trim().to_string(),
        ),
        Err(e) => (false, e.to_string()),
    };
    let after = finder_trash_size(env);
    let freed = before.saturating_sub(after);
    let now = chrono::Utc::now();
    let rec = ActionRecord {
        id: format!("trash-empty-{}", now.format("%Y%m%d%H%M%S%3f")),
        plan_id: format!("trash_bins-empty-{}", now.format("%Y%m%d%H%M%S%3f")),
        feature: Some(FeatureId::TrashBins),
        time: now.timestamp(),
        size: freed,
        kind: ActionKind::Command {
            command,
            ok,
            output: output.clone(),
        },
        undone: None,
    };
    let logged = executor::append_journal(env, &rec);
    let message = if ok {
        format!(
            "Finder emptied the Trash. {} freed.",
            crate::fsutil::human_size(freed)
        )
    } else if output.contains("-1743") || output.contains("Not authorized") {
        "macOS did not let CleanCraft ask Finder. Allow it in System Settings > Privacy & Security > Automation, then try again.".to_string()
    } else if output.contains("-128") {
        "Emptying the Trash was cancelled.".to_string()
    } else {
        format!("Finder could not empty the Trash: {output}")
    };
    let message = match logged {
        Ok(()) => message,
        Err(e) => format!("{message} (The action log could not be written: {e})"),
    };
    EmptyOutcome { ok, freed, message }
}
