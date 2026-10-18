//! Mail Downloads: copies of attachments Mail keeps on disk.
//!
//! - Opened attachments (M1): `~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads`.
//! - Downloaded attachments (M2): `~/Library/Mail/V*/<account>/**/Attachments/**`, only for
//!   accounts whose server keeps the original (IMAP, Exchange). POP and "On My Mac" mailboxes
//!   are left out: their attachment may be the only copy. When the account type cannot be
//!   told, the files are listed but locked.
//! - Modified attachments (M3): a file changed after Mail saved it is never preselected.
//!
//! Mail must be quit: while it runs every item is locked, and the cleanup refuses to start.
//!
//! Account type, in order: the folder name (`IMAP-`, `EWS-`, `POP-`, `Mailboxes` for On My
//! Mac), Mail's `MailData/Accounts.plist` (older Mail), then the system account store
//! `~/Library/Accounts/Accounts4.sqlite` read with `sqlite3` (newer Mail, UUID folders).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::env::Env;
use crate::features::system_junk::can_read;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, tree_size, walk_files};
use crate::plan::{FeatureId, Plan, Risk};

pub struct MailDownloads;

pub const MAIL_DOWNLOADS: &str = "Library/Containers/com.apple.mail/Data/Library/Mail Downloads";
pub const MAIL_DIR: &str = "Library/Mail";
pub const ACCOUNTS_DB: &str = "Library/Accounts/Accounts4.sqlite";
pub const ACCOUNTS_QUERY: &str = "SELECT ZACCOUNT.ZIDENTIFIER, ZACCOUNTTYPE.ZIDENTIFIER FROM ZACCOUNT JOIN ZACCOUNTTYPE ON ZACCOUNT.ZACCOUNTTYPE = ZACCOUNTTYPE.Z_PK;";

pub const LOCK_MAIL_OPEN: &str = "Quit Mail first";
pub const LOCK_UNKNOWN_ACCOUNT: &str =
    "CleanCraft could not tell whether the mail server keeps a copy, so it is kept";

/// A file whose modification time is this much later than when Mail saved it was changed by
/// the user.
pub const MODIFIED_SLACK: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountKind {
    /// The server keeps the original (IMAP, Exchange).
    Server,
    /// POP or On My Mac: the local copy may be the only one.
    Local,
    Unknown,
}

/// Mail's per-account attachment folders.
#[derive(Clone, Debug)]
pub struct AttachmentDir {
    pub account: String,
    pub kind: AccountKind,
    pub mailbox: String,
    pub dir: PathBuf,
}

/// Is Mail running?
pub fn mail_running(env: &Env) -> bool {
    env.sys.running_apps().iter().any(|a| {
        a.name == "Mail"
            || a.bundle_id.as_deref() == Some("com.apple.mail")
            || a.path.as_ref().is_some_and(|p| p.ends_with("Mail.app"))
    })
}

/// `~/Library/Mail/V2`, `V10`... real folders.
pub fn version_dirs(env: &Env) -> Vec<PathBuf> {
    children(&env.home_path(MAIL_DIR))
        .into_iter()
        .filter(|d| {
            let n = d
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            n.starts_with('V')
                && n[1..].chars().all(|c| c.is_ascii_digit())
                && n.len() > 1
                && std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir())
        })
        .collect()
}

/// Account kinds known from Mail's own Accounts.plist and the system account store.
pub fn known_accounts(env: &Env) -> BTreeMap<String, (AccountKind, Option<String>)> {
    let mut m = BTreeMap::new();
    for v in version_dirs(env) {
        if let Ok(pl) = plist::Value::from_file(v.join("MailData/Accounts.plist")) {
            for (id, kind, name) in parse_accounts_plist(&pl) {
                m.insert(id, (kind, name));
            }
        }
    }
    let db = env.home_path(ACCOUNTS_DB);
    if db.exists() {
        let dbs = db.to_string_lossy().into_owned();
        if let Ok(out) = env.sys.run(
            "/usr/bin/sqlite3",
            &["-separator", "|", &dbs, ACCOUNTS_QUERY],
        ) && out.ok()
        {
            for (id, kind) in parse_accounts_db(&out.stdout) {
                m.entry(id).or_insert((kind, None));
            }
        }
    }
    m
}

/// Kind from an account type name: "IMAPAccount", "com.apple.account.POP", "EWSAccount"...
pub fn kind_of_type(t: &str) -> AccountKind {
    let t = t.to_lowercase();
    if t.contains("pop") || t.contains("local") {
        AccountKind::Local
    } else if [
        "imap", "ews", "exchange", "google", "gmail", "yahoo", "aol", "icloud", "hotmail",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        AccountKind::Server
    } else {
        AccountKind::Unknown
    }
}

/// `MailAccounts` array of Mail's Accounts.plist: (uniqueId, kind, AccountName).
pub fn parse_accounts_plist(v: &plist::Value) -> Vec<(String, AccountKind, Option<String>)> {
    let Some(list) = v
        .as_dictionary()
        .and_then(|d| d.get("MailAccounts"))
        .and_then(|a| a.as_array())
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|a| {
            let d = a.as_dictionary()?;
            let s = |k: &str| d.get(k).and_then(|x| x.as_string()).map(str::to_string);
            let id = s("uniqueId")?;
            let kind = kind_of_type(&s("AccountType").unwrap_or_default());
            Some((id, kind, s("AccountName")))
        })
        .collect()
}

/// `sqlite3 -separator '|'` rows of [`ACCOUNTS_QUERY`]: "UUID|com.apple.account.IMAP".
pub fn parse_accounts_db(out: &str) -> Vec<(String, AccountKind)> {
    out.lines()
        .filter_map(|l| {
            let (id, t) = l.trim().split_once('|')?;
            Some((id.trim().to_string(), kind_of_type(t)))
        })
        .filter(|(id, _)| !id.is_empty())
        .collect()
}

/// Kind and display name of an account folder.
pub fn classify(
    folder: &str,
    known: &BTreeMap<String, (AccountKind, Option<String>)>,
) -> (AccountKind, String) {
    if folder == "Mailboxes" {
        return (AccountKind::Local, "On My Mac".into());
    }
    for (prefix, kind) in [
        ("IMAP-", AccountKind::Server),
        ("EWS-", AccountKind::Server),
        ("Exchange-", AccountKind::Server),
        ("POP-", AccountKind::Local),
    ] {
        if let Some(rest) = folder.strip_prefix(prefix) {
            return (kind, rest.to_string());
        }
    }
    match known.get(folder) {
        Some((k, name)) => (k.clone(), name.clone().unwrap_or_else(|| folder.into())),
        None => (AccountKind::Unknown, folder.to_string()),
    }
}

/// Every `Attachments` folder in Mail's storage, with its account and mailbox. Message folders
/// are not entered.
pub fn attachment_dirs(env: &Env) -> Vec<AttachmentDir> {
    let known = known_accounts(env);
    let mut out = Vec::new();
    for v in version_dirs(env) {
        for acct in children(&v) {
            let folder = acct
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if folder == "MailData" || folder.starts_with('.') {
                continue;
            }
            if !std::fs::symlink_metadata(&acct).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let (kind, account) = classify(&folder, &known);
            find_attachments(&acct, &acct, &account, &kind, &mut out);
        }
    }
    out
}

fn find_attachments(
    acct_root: &Path,
    dir: &Path,
    account: &str,
    kind: &AccountKind,
    out: &mut Vec<AttachmentDir>,
) {
    for c in children(dir) {
        let Ok(md) = std::fs::symlink_metadata(&c) else {
            continue;
        };
        if !md.is_dir() {
            continue;
        }
        let n = c
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if n == "Messages" {
            continue;
        }
        if n == "Attachments" {
            out.push(AttachmentDir {
                account: account.to_string(),
                kind: kind.clone(),
                mailbox: mailbox_of(acct_root, &c),
                dir: c,
            });
            continue;
        }
        find_attachments(acct_root, &c, account, kind, out);
    }
}

/// "INBOX.mbox/Work.mbox/..." -> "INBOX/Work".
fn mailbox_of(acct_root: &Path, p: &Path) -> String {
    let rel = p.strip_prefix(acct_root).unwrap_or(p);
    let v: Vec<String> = rel
        .components()
        .filter_map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .strip_suffix(".mbox")
                .map(str::to_string)
        })
        .collect();
    if v.is_empty() {
        "Mailbox".into()
    } else {
        v.join("/")
    }
}

/// True when the file changed after Mail saved it: modified well after it was created, or
/// after its folder last changed. (Saving in place keeps the creation date on macOS.)
pub fn changed_after_saved(p: &Path) -> bool {
    let Ok(md) = std::fs::metadata(p) else {
        return false;
    };
    let Ok(m) = md.modified() else {
        return false;
    };
    let later = |t: SystemTime| m.duration_since(t).is_ok_and(|d| d > MODIFIED_SLACK);
    if md.created().is_ok_and(later) {
        return true;
    }
    p.parent()
        .and_then(|d| std::fs::metadata(d).ok())
        .and_then(|d| d.modified().ok())
        .is_some_and(later)
}

impl Feature for MailDownloads {
    fn id(&self) -> FeatureId {
        FeatureId::MailAttachments
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        let mut v = vec![env.home_path(MAIL_DOWNLOADS)];
        v.extend(
            attachment_dirs(env)
                .into_iter()
                .filter(|a| a.kind == AccountKind::Server)
                .map(|a| a.dir),
        );
        v
    }

    fn prepare(&self, env: &Env, plan: &Plan) -> anyhow::Result<()> {
        if plan.selected_count() > 0 && mail_running(env) {
            anyhow::bail!("Mail is open. Quit Mail, then scan again.");
        }
        Ok(())
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let env = ctx.env;
        let allow = self.allow_roots(env);
        let mut plan = Plan::new(FeatureId::MailAttachments);
        let running = mail_running(env);

        (ctx.progress)("Mail Downloads");
        let opened = env.home_path(MAIL_DOWNLOADS);
        if opened.exists() && !can_read(env, &opened) {
            plan.notes.push(format!(
                "{} could not be read. Full Disk Access may be needed.",
                env.display(&opened)
            ));
        }
        let mut files = Vec::new();
        if can_read(env, &opened) {
            walk_files(&opened, &|_| false, &mut files);
        }
        for (f, _) in files {
            add_file(
                ctx,
                &allow,
                &mut plan,
                &f,
                "mail_opened",
                "Opened attachments",
                "Copies Mail saved when you opened an attachment. The message still has it.",
                Risk::Low,
                "Opened from Mail",
            );
        }

        if ctx.cancelled() {
            plan.notes.push("Scan stopped before the end.".into());
            return Ok(plan);
        }
        (ctx.progress)("Mail");
        let mail = env.home_path(MAIL_DIR);
        if mail.exists() && !can_read(env, &mail) {
            plan.notes.push(format!(
                "{} could not be read. Turn on Full Disk Access to include attachments stored with your messages.",
                env.display(&mail)
            ));
        }
        let mut left_out = 0usize;
        let dirs = if can_read(env, &mail) {
            attachment_dirs(env)
        } else {
            Vec::new()
        };
        for a in dirs {
            let mut files = Vec::new();
            walk_files(&a.dir, &|_| false, &mut files);
            let group = format!("{}: {}", a.account, a.mailbox);
            match a.kind {
                AccountKind::Local => left_out += files.len(),
                AccountKind::Server => {
                    for (f, _) in files {
                        add_file(
                            ctx,
                            &allow,
                            &mut plan,
                            &f,
                            "mail_downloaded",
                            "Downloaded attachments",
                            "Attachments stored with messages of accounts whose server keeps the original. Mail downloads them again when you open the message.",
                            Risk::Medium,
                            &group,
                        );
                    }
                }
                AccountKind::Unknown => {
                    for (f, _) in files {
                        if ctx.ignored(&f) {
                            continue;
                        }
                        let (size, _) = tree_size(&f);
                        if size == 0 {
                            continue;
                        }
                        plan.add_category(
                            "mail_downloaded",
                            "Downloaded attachments",
                            "Attachments stored with messages of accounts whose server keeps the original. Mail downloads them again when you open the message.",
                            Risk::Medium,
                        );
                        let id = plan.push(
                            f.clone(),
                            size,
                            "mail_downloaded",
                            format!("Attachment in {group}"),
                            Risk::Medium,
                            false,
                        );
                        plan.items[id].group = Some(group.clone());
                        plan.lock(id, LOCK_UNKNOWN_ACCOUNT);
                    }
                }
            }
        }
        if left_out > 0 {
            plan.notes.push(format!(
                "{left_out} attachments in POP or On My Mac mailboxes were left out: they may be the only copy."
            ));
        }
        if running {
            for i in 0..plan.items.len() {
                plan.lock(i, LOCK_MAIL_OPEN);
            }
            if !plan.items.is_empty() {
                plan.notes
                    .push("Mail is open. Quit Mail and scan again to clean.".into());
            }
        }
        Ok(plan)
    }
}

#[allow(clippy::too_many_arguments)]
fn add_file(
    ctx: &ScanCtx<'_>,
    allow: &[PathBuf],
    plan: &mut Plan,
    f: &Path,
    cat: &str,
    title: &str,
    desc: &str,
    risk: Risk,
    group: &str,
) {
    if !ctx.allowed(allow, f) {
        return;
    }
    let (size, _) = tree_size(f);
    if size == 0 {
        return;
    }
    plan.add_category(cat, title, desc, risk);
    let changed = changed_after_saved(f);
    let name = f
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let reason = if changed {
        format!("{name}: changed after Mail saved it, so it is not selected")
    } else {
        format!("{name}: attachment copy")
    };
    let id = plan.push(f.to_path_buf(), size, cat, reason, risk, !changed);
    plan.items[id].group = Some(group.to_string());
    plan.items[id].modified = std::fs::metadata(f)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind_of_type("IMAPAccount"), AccountKind::Server);
        assert_eq!(
            kind_of_type("com.apple.account.Exchange"),
            AccountKind::Server
        );
        assert_eq!(kind_of_type("POPAccount"), AccountKind::Local);
        assert_eq!(kind_of_type("LocalAccount"), AccountKind::Local);
        assert_eq!(
            kind_of_type("com.apple.account.CardDAV"),
            AccountKind::Unknown
        );
        let rows = parse_accounts_db("A1|com.apple.account.IMAP\nB2|com.apple.account.POP\n\n");
        assert_eq!(
            rows,
            vec![
                ("A1".into(), AccountKind::Server),
                ("B2".into(), AccountKind::Local)
            ]
        );
        let known = BTreeMap::new();
        assert_eq!(classify("Mailboxes", &known).0, AccountKind::Local);
        assert_eq!(
            classify("IMAP-me@example.com", &known),
            (AccountKind::Server, "me@example.com".into())
        );
        assert_eq!(classify("POP-me@x", &known).0, AccountKind::Local);
        assert_eq!(classify("1234-ABCD", &known).0, AccountKind::Unknown);
    }
}
