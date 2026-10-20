//! Trash Bins: how much is waiting in every Trash.
//!
//! - The startup disk's Trash (`~/.Trash`), the Trash of every other disk
//!   (`/Volumes/<disk>/.Trashes/<uid>`), and Mail's deleted-messages mailboxes (T1).
//! - CleanCraft never deletes for good, and these items are already in a Trash, so every item is
//!   listed locked and nothing is in the allow-list. The one action is asking Finder to empty
//!   the Trash (T2), after the user confirmed, logged as a command. Mail's trash is emptied in
//!   Mail (Mailbox > Erase Deleted Items), which the item says.
//! - The startup disk's Trash is always a row: "Your Trash is empty" when it is, and a row
//!   saying it could not be read (with Full Disk Access as the next step) when macOS hides it.
//! - Mail's trash: one group per Mail account, named after the account ("Mail trash, iCloud
//!   (name@icloud.com)", from Mail's Accounts.plist or the system account store
//!   Accounts4.sqlite, the same sources Mail Downloads uses), one row per trash mailbox
//!   ("Deleted Messages", "Trash": an IMAP account can have both on its server; "On My Mac"
//!   is stored only on this Mac). [`erase_in_mail`] asks Mail to erase them with its own
//!   command, Mailbox > Erase Deleted Items > <account>, by UI scripting through System
//!   Events (Mail's AppleScript dictionary has no erase command; deleting a message that is
//!   already in the trash is not reliably permanent). That needs the Accessibility permission;
//!   without it the error says so and the page offers the settings and opening Mail. The
//!   `.mbox` folders are never moved by CleanCraft: that would damage Mail's store.

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
pub const LOCK_MAIL_TRASH: &str = "Only Mail can erase deleted mail: use Erase in Mail";
pub const LOCK_TRASH_EMPTY: &str = "Nothing to empty";
pub const LOCK_NEEDS_ACCESS: &str = "CleanCraft needs Full Disk Access to see inside the Trash";
pub const REASON_EMPTY: &str = "Your Trash is empty";
pub const REASON_UNREADABLE: &str = "Could not be read without Full Disk Access";
/// The startup disk's Trash row title (its group).
pub const STARTUP_LABEL: &str = "Trash of the startup disk";

/// Accounts query with names: "UUID|Description|user name".
pub const ACCOUNT_NAMES_QUERY: &str =
    "SELECT ZIDENTIFIER, ZACCOUNTDESCRIPTION, ZUSERNAME FROM ZACCOUNT;";
pub const ACCESSIBILITY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

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
    /// Mail only: the account as Mail names it in Mailbox > Erase Deleted Items (None: not
    /// known, or On My Mac).
    pub account: Option<String>,
    /// Mail only: the mailbox's name ("Deleted Messages", "Trash").
    pub mailbox: String,
    /// Mail only: the copy is on the mail server too (IMAP, Exchange).
    pub on_server: Option<bool>,
}

/// Size of a Trash's contents: each entry with the shared formula.
pub fn contents_size(dir: &Path) -> (u64, usize) {
    let (size, entries, _) = crate::fsutil::contents_size(dir);
    (size, entries)
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
        account: None,
        mailbox: String::new(),
        on_server: None,
    }
}

/// `sqlite3 -separator '|'` rows of [`ACCOUNT_NAMES_QUERY`]: id -> (description, user name).
pub fn parse_account_names(out: &str) -> Vec<(String, Option<String>, Option<String>)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.trim().splitn(3, '|');
            let id = it.next()?.trim().to_string();
            let some = |s: Option<&str>| {
                s.map(str::trim)
                    .filter(|s| !s.is_empty() && !s.starts_with("com.apple.account"))
                    .map(str::to_string)
            };
            let desc = some(it.next());
            let user = some(it.next());
            (!id.is_empty()).then_some((id, desc, user))
        })
        .collect()
}

/// Mail account names by account folder: (name Mail shows, user name). From Mail's own
/// Accounts.plist and the system account store.
pub fn account_names(env: &Env) -> std::collections::BTreeMap<String, (String, Option<String>)> {
    let mut m = std::collections::BTreeMap::new();
    for (id, (_, name)) in crate::features::mail::known_accounts(env) {
        if let Some(n) = name {
            m.insert(id, (n, None));
        }
    }
    let db = env.home_path(crate::features::mail::ACCOUNTS_DB);
    if db.exists() {
        let dbs = db.to_string_lossy().into_owned();
        if let Ok(out) = env.sys.run(
            "/usr/bin/sqlite3",
            &["-separator", "|", &dbs, ACCOUNT_NAMES_QUERY],
        ) && out.ok()
        {
            for (id, desc, user) in parse_account_names(&out.stdout) {
                let Some(name) = desc.clone().or_else(|| user.clone()) else {
                    continue;
                };
                let e = m.entry(id).or_insert((name, None));
                if e.1.is_none() && user.as_deref() != Some(e.0.as_str()) {
                    e.1 = user;
                }
            }
        }
    }
    m
}

/// "Mail trash, iCloud (name@icloud.com)", "Mail trash, On My Mac".
pub fn mail_label(name: &str, user: Option<&str>) -> String {
    match user {
        Some(u) if u != name => format!("Mail trash, {name} ({u})"),
        _ => format!("Mail trash, {name}"),
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
    // Always listed: empty, unreadable or full, the page says which.
    let missing = std::fs::symlink_metadata(&user).is_err();
    let mut t = info(env, user, STARTUP_LABEL.into(), false);
    // No Trash folder yet: nothing in it (not "could not be read").
    t.readable |= missing;
    v.push(t);
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
    let dirs = version_dirs(env);
    let names = if dirs.is_empty() {
        Default::default()
    } else {
        account_names(env)
    };
    let known = if dirs.is_empty() {
        Default::default()
    } else {
        crate::features::mail::known_accounts(env)
    };
    for ver in dirs {
        for acct in children(&ver) {
            for n in MAIL_TRASH_NAMES {
                let mb = acct.join(n);
                if std::fs::symlink_metadata(&mb).is_ok_and(|m| m.is_dir()) {
                    let folder = acct
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let (kind, classified) = crate::features::mail::classify(&folder, &known);
                    let (account, user) = if folder == "Mailboxes" {
                        (Some("On My Mac".to_string()), None)
                    } else if let Some((name, user)) = names.get(&folder) {
                        (Some(name.clone()), user.clone())
                    } else if classified != folder {
                        (Some(classified.clone()), None)
                    } else {
                        (None, None)
                    };
                    let shown = match (&account, folder.as_str()) {
                        (_, "Mailboxes") => "On My Mac".to_string(),
                        (Some(a), _) => a.clone(),
                        // Not named anywhere: a short form of the folder's id.
                        (None, f) => format!("account {}", f.chars().take(8).collect::<String>()),
                    };
                    let mut t = info(env, mb, mail_label(&shown, user.as_deref()), true);
                    t.account = account;
                    t.mailbox = n.trim_end_matches(".mbox").to_string();
                    t.on_server = match kind {
                        crate::features::mail::AccountKind::Server => Some(true),
                        crate::features::mail::AccountKind::Local => Some(false),
                        crate::features::mail::AccountKind::Unknown => None,
                    };
                    v.push(t);
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
            let startup = t.path == env.user_trash();
            if !t.readable {
                plan.notes.push(format!(
                    "{} could not be read. Turn on Full Disk Access to see what is in it.",
                    env.display(&t.path)
                ));
                if startup {
                    // Still a row, with the next step (REVIEW: a page of locked rows always
                    // says what to do).
                    plan.add_category(
                        "startup_trash",
                        "Startup disk Trash",
                        "Items you moved to the Trash on this Mac's disk.",
                        Risk::Medium,
                    );
                    let id = plan.push(
                        t.path.clone(),
                        0,
                        "startup_trash",
                        REASON_UNREADABLE,
                        Risk::Medium,
                        false,
                    );
                    plan.items[id].group = Some(t.label.clone());
                    plan.lock(id, LOCK_NEEDS_ACCESS);
                }
                continue;
            }
            if startup && (t.entries == 0 || t.size == 0) && !ctx.ignored(&t.path) {
                plan.add_category(
                    "startup_trash",
                    "Startup disk Trash",
                    "Items you moved to the Trash on this Mac's disk.",
                    Risk::Medium,
                );
                let id = plan.push(
                    t.path.clone(),
                    0,
                    "startup_trash",
                    REASON_EMPTY,
                    Risk::Medium,
                    false,
                );
                plan.items[id].group = Some(t.label.clone());
                plan.lock(id, LOCK_TRASH_EMPTY);
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
            let what = if t.mail {
                crate::fsutil::count(t.entries, "message", "messages")
            } else {
                crate::fsutil::items(t.entries)
            };
            let reason = if t.mail {
                let place = match t.on_server {
                    Some(true) => ", also on the mail server",
                    Some(false) => ", stored only on this Mac",
                    None => "",
                };
                format!("{}: {what}{place}", t.mailbox)
            } else {
                format!("{}: {what}", t.label)
            };
            let id = plan.push(t.path.clone(), t.size, cat, reason, Risk::Medium, false);
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

/// What erasing a Mail account's deleted messages did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EraseOutcome {
    /// Mail's command was chosen (Mail then asks the user to confirm in its own window).
    pub ok: bool,
    /// macOS refused UI scripting: CleanCraft needs the Accessibility permission.
    pub needs_accessibility: bool,
    /// macOS refused to let CleanCraft control System Events or Mail (Automation).
    pub needs_automation: bool,
    /// Mail was not running and was opened.
    pub opened_mail: bool,
    pub message: String,
}

/// AppleScript text in double quotes.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The lines of the script that chooses Mail's Mailbox > Erase Deleted Items > `account`
/// ("In All Accounts" when `account` is None), each passed with its own `-e`.
pub fn erase_script(account: Option<&str>) -> Vec<String> {
    let item = quoted(account.unwrap_or("In All Accounts"));
    vec![
        "tell application \"Mail\" to activate".into(),
        "delay 1".into(),
        "tell application \"System Events\"".into(),
        "tell process \"Mail\"".into(),
        "set m to menu 1 of menu bar item \"Mailbox\" of menu bar 1".into(),
        "try".into(),
        format!("click menu item {item} of menu 1 of menu item \"Erase Deleted Items\" of m"),
        "on error number -1728".into(),
        "click menu item \"Erase Deleted Items\" of m".into(),
        "end try".into(),
        "end tell".into(),
        "end tell".into(),
    ]
}

/// Ask Mail to erase the deleted messages of `account` (as Mail names it in the menu) with its
/// own command, Mailbox > Erase Deleted Items, after the user confirmed in CleanCraft. Mail
/// may ask once more in its own window. Logged as a command (it cannot be undone). CleanCraft
/// itself never touches Mail's files.
pub fn erase_in_mail(env: &Env, account: Option<&str>) -> EraseOutcome {
    let mut out = EraseOutcome {
        ok: false,
        needs_accessibility: false,
        needs_automation: false,
        opened_mail: false,
        message: String::new(),
    };
    if let Err(e) = journal_ready(env) {
        out.message = format!(
            "The action log could not be written ({e}), so Mail was not asked to erase anything."
        );
        return out;
    }
    out.opened_mail = !crate::features::mail::mail_running(env);
    let lines = erase_script(account);
    let mut args: Vec<&str> = Vec::new();
    for l in &lines {
        args.push("-e");
        args.push(l);
    }
    let command = format!(
        "osascript {}",
        lines
            .iter()
            .map(|l| format!("-e '{l}'"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let res = env.sys.run(OSASCRIPT, &args);
    let (ok, output) = match &res {
        Ok(o) => (
            o.ok(),
            format!("{}{}", o.stdout, o.stderr).trim().to_string(),
        ),
        Err(e) => (false, e.to_string()),
    };
    out.ok = ok;
    let now = chrono::Utc::now();
    let rec = ActionRecord {
        id: format!("mail-erase-{}", now.format("%Y%m%d%H%M%S%3f")),
        plan_id: format!("trash_bins-mail-erase-{}", now.format("%Y%m%d%H%M%S%3f")),
        feature: Some(FeatureId::TrashBins),
        time: now.timestamp(),
        size: 0,
        kind: ActionKind::Command {
            command,
            ok,
            output: output.clone(),
        },
        undone: None,
    };
    let logged = executor::append_journal(env, &rec);
    let menu = format!(
        "Mailbox > Erase Deleted Items > {}",
        account.unwrap_or("In All Accounts")
    );
    out.message = if ok {
        format!(
            "Mail is erasing the deleted messages ({menu}). If Mail asks, confirm in its window; the list updates in a few seconds."
        )
    } else if output.contains("-1719")
        || output.contains("-25211")
        || output.to_lowercase().contains("assistive access")
    {
        out.needs_accessibility = true;
        format!(
            "CleanCraft needs the Accessibility permission to use Mail's menu: System Settings > Privacy & Security > Accessibility. Or, in Mail, choose {menu} yourself."
        )
    } else if output.contains("-1743") || output.contains("Not authorized") {
        out.needs_automation = true;
        format!(
            "macOS did not let CleanCraft control Mail. Allow it in System Settings > Privacy & Security > Automation, or in Mail choose {menu} yourself."
        )
    } else if output.contains("-128") {
        "Erasing was cancelled. Nothing changed.".to_string()
    } else {
        format!("Mail could not erase the deleted messages ({output}). In Mail, choose {menu}.")
    };
    if let Err(e) = logged {
        out.message = format!("{} (The action log could not be written: {e})", out.message);
    }
    out
}

/// Open Mail (the fallback when its menu cannot be scripted).
pub fn open_mail(env: &Env) -> Result<(), String> {
    match env.sys.run("/usr/bin/open", &["-a", "Mail"]) {
        Ok(o) if o.ok() => Ok(()),
        Ok(o) => Err(o.stderr.trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The Mail trash rows of a group (one account) in a Trash Bins plan: (account name for the
/// menu, messages, bytes).
pub fn mail_group(env: &Env, plan: &Plan, group: &str) -> Option<(Option<String>, usize, u64)> {
    let rows: Vec<&crate::plan::Item> = plan
        .items
        .iter()
        .filter(|i| i.category == "mail_trash" && i.group.as_deref() == Some(group))
        .collect();
    if rows.is_empty() {
        return None;
    }
    let infos = trashes(env);
    let mut account = None;
    let mut messages = 0;
    let mut bytes = 0;
    for r in rows {
        bytes += r.size;
        if let Some(t) = infos.iter().find(|t| t.path == r.path) {
            messages += t.entries;
            if account.is_none() {
                account = t.account.clone();
            }
        }
    }
    Some((account, messages, bytes))
}
