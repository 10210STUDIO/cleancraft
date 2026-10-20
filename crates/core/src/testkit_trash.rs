//! Test helpers for Trash Bins' Mail trash (files6): Mail accounts in UUID folders, named in
//! the system account store, answered through `FakeSys`.

use std::path::PathBuf;

use crate::env::Env;
use crate::features::{mail, trash};
use crate::testkit::FakeMac;
use crate::testkit_cleanup::MAIL_V;

/// A UUID account folder's trash mailbox with `messages` messages of `bytes` each.
pub fn mail_trash(
    m: &FakeMac,
    account: &str,
    mailbox: &str,
    messages: usize,
    bytes: usize,
) -> PathBuf {
    for i in 0..messages {
        m.home_file(
            &format!("{MAIL_V}/{account}/{mailbox}.mbox/0A/Data/Messages/{i}.emlx"),
            bytes,
        );
    }
    m.env
        .home_path(&format!("{MAIL_V}/{account}/{mailbox}.mbox"))
}

/// The system account store lists `rows` ("UUID|Description|user"): the sqlite3 answer for
/// [`trash::ACCOUNT_NAMES_QUERY`].
pub fn answer_account_names(env: &Env, rows: &str) {
    let db = env.home_path(mail::ACCOUNTS_DB);
    if let Some(d) = db.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&db, b"SQLite format 3\0");
    let key = format!(
        "/usr/bin/sqlite3 -separator | {} {}",
        db.to_string_lossy(),
        trash::ACCOUNT_NAMES_QUERY
    );
    if let Some(f) = env.fake_sys() {
        f.answer(&key, rows);
    }
}

/// The `osascript` line [`trash::erase_in_mail`] runs for `account`.
pub fn erase_call(account: Option<&str>) -> String {
    let mut parts = vec![trash::OSASCRIPT.to_string()];
    for l in trash::erase_script(account) {
        parts.push("-e".into());
        parts.push(l);
    }
    parts.join(" ")
}
