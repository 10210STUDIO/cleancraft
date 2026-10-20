//! Mail Downloads (M1 to M3): account types, modified attachments, Mail running, cleaning
//! and errors on a fake Mail tree.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, SkipReason};
use cleancraft_core::features::{self, Feature, mail};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, MAIL_V, mail_mac};
use cleancraft_core::{Fault, FeatureId, Item, Plan, Settings, policy};

fn scan(m: &FakeMac) -> Plan {
    features::scan(&m.env, &Settings::default(), FeatureId::MailAttachments).unwrap()
}

fn find<'a>(p: &'a Plan, name: &str) -> Option<&'a Item> {
    p.items
        .iter()
        .find(|i| i.path.file_name().is_some_and(|n| n == name))
}

fn att(m: &FakeMac, account: &str, mailbox: &str, msg: &str, file: &str) -> PathBuf {
    m.env.home_path(&format!(
        "{MAIL_V}/{account}/{mailbox}.mbox/0F0F/Data/1/Attachments/{msg}/1/{file}"
    ))
}

/// The user ticks every attachment that can be moved, leaving the ones changed after Mail
/// saved them (what the reason text tells them to keep).
fn tick_unchanged(p: &mut Plan) {
    for i in 0..p.items.len() {
        let it = &p.items[i];
        let on = it.locked.is_none() && !it.reason.contains("changed after Mail saved it");
        p.set_selected(i, on);
    }
}

#[test]
fn server_accounts_only_and_modified_kept() {
    let m = mail_mac();
    let p = scan(&m);
    // Nothing is preselected: the user ticks the attachments to move.
    assert_eq!(p.selected_count(), 0, "an attachment is preselected");
    // M1: opened copies; the one edited after opening also says why it should stay.
    let inv = find(&p, "Invoice.pdf").expect("opened attachment");
    assert_eq!(inv.category, "mail_opened");
    assert!(!inv.selected && inv.locked.is_none());
    assert!(!inv.reason.contains("changed after Mail saved it"));
    let con = find(&p, "Contract.docx").expect("edited opened attachment");
    assert!(!con.selected && con.locked.is_none());
    assert!(con.reason.contains("changed after Mail saved it"));
    // M2: IMAP by folder name.
    let photo = find(&p, "photo.jpg").expect("IMAP attachment");
    assert_eq!(photo.category, "mail_downloaded");
    assert!(!photo.selected && photo.locked.is_none());
    assert_eq!(photo.group.as_deref(), Some("me@example.com: INBOX"));
    // M3: edited after download.
    let edited = find(&p, "edited.key").expect("edited IMAP attachment");
    assert!(!edited.selected && edited.locked.is_none());
    assert!(edited.reason.contains("changed after Mail saved it"));
    // Account known from Mail's Accounts.plist (IMAP, named Work).
    let deck = find(&p, "deck.pdf").expect("Accounts.plist account");
    assert!(!deck.selected && deck.locked.is_none());
    assert_eq!(deck.group.as_deref(), Some("Work: Sent Messages"));
    // Account known from the system account store (Exchange).
    let budget = find(&p, "budget.xlsx").expect("Accounts4 account");
    assert!(!budget.selected && budget.locked.is_none());
    // Unknown type: listed, locked.
    let mystery = find(&p, "mystery.zip").expect("unknown account listed");
    assert_eq!(mystery.locked.as_deref(), Some(mail::LOCK_UNKNOWN_ACCOUNT));
    // POP and On My Mac: left out, with a note.
    assert!(find(&p, "only-copy.pdf").is_none());
    assert!(find(&p, "local.pdf").is_none());
    assert!(
        p.notes
            .iter()
            .any(|n| n.starts_with("2 attachments in POP or On My Mac"))
    );
    // Message files never.
    assert!(
        !p.items
            .iter()
            .any(|i| i.path.to_string_lossy().ends_with(".emlx"))
    );
    // Sizes recalculated from disk.
    for it in &p.items {
        assert_eq!(it.size, tree_size(&it.path).0);
    }
    // The fence agrees: POP, On My Mac and unknown accounts are outside the allow-list.
    let allow = mail::MailDownloads.allow_roots(&m.env);
    assert!(policy::check(&m.env, &allow, &photo.path).is_ok());
    assert!(policy::check(&m.env, &allow, &inv.path).is_ok());
    for p in [
        att(&m, "POP-old@pop.example", "INBOX", "9", "only-copy.pdf"),
        att(&m, "Mailboxes", "Archive", "7", "local.pdf"),
        mystery.path.clone(),
    ] {
        assert!(
            policy::check(&m.env, &allow, &p).is_err(),
            "{}",
            p.display()
        );
    }
    // sqlite3 was asked about the system account store, read only by a query.
    let calls = m.env.fake_sys().unwrap().calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("/usr/bin/sqlite3 -separator |"))
    );
}

#[test]
fn mail_running_locks_everything_and_blocks_cleaning() {
    let m = mail_mac();
    kit::set_running(&m.env, "Mail", "/System/Applications/Mail.app");
    let p = scan(&m);
    assert!(!p.items.is_empty());
    assert!(p.items.iter().all(|i| i.locked.as_deref().is_some()));
    assert!(
        p.items
            .iter()
            .filter(|i| i.path.file_name().is_some_and(|n| n != "mystery.zip"))
            .all(|i| i.locked.as_deref() == Some(mail::LOCK_MAIL_OPEN))
    );
    assert_eq!(p.selected_count(), 0);
    assert!(p.notes.iter().any(|n| n.contains("Quit Mail")));

    // Scanned while Mail was quit, the user ticks attachments, then Mail opened before the
    // cleanup: refused.
    let m = mail_mac();
    let mut p = scan(&m);
    tick_unchanged(&mut p);
    assert!(p.selected_count() > 0);
    kit::set_running(&m.env, "Mail", "/System/Applications/Mail.app");
    let s = Settings::default();
    let err = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap_err();
    assert!(err.to_string().contains("Quit Mail"));
    assert!(p.selected().all(|i| i.path.exists()));
}

#[test]
fn clean_moves_selected_and_errors_are_reported() {
    let m = mail_mac();
    let s = Settings::default();
    let mut p = scan(&m);
    tick_unchanged(&mut p);
    let photo = att(&m, "IMAP-me@example.com", "INBOX", "12345", "photo.jpg");
    let deck = att(
        &m,
        kit::UUID_PLIST_ACCOUNT,
        "Sent Messages",
        "55",
        "deck.pdf",
    );
    let budget = att(&m, kit::UUID_DB_ACCOUNT, "INBOX", "77", "budget.xlsx");
    m.env.inject_fault(&photo, Fault::PermissionDenied);
    m.env.fake_sys().unwrap().set_open(&deck);
    m.env.inject_fault(&budget, Fault::DiskFull);
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    let why = |path: &Path| {
        rep.skipped
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, r)| r.clone())
    };
    assert_eq!(why(&photo), Some(SkipReason::PermissionDenied));
    assert_eq!(why(&deck), Some(SkipReason::InUse));
    assert_eq!(why(&budget), Some(SkipReason::DiskFull));
    let inv = find(&p, "Invoice.pdf").unwrap().path.clone();
    assert!(!inv.exists(), "selected opened attachment moved");
    assert!(m.env.user_trash().join("Invoice.pdf").exists());
    let edited = att(&m, "IMAP-me@example.com", "INBOX", "12346", "edited.key");
    assert!(edited.exists(), "modified attachment was not ticked");
    assert_eq!(rep.moved.len(), 1);
    assert_eq!(rep.freed, find(&p, "Invoice.pdf").unwrap().size);
    let (n, errs) = executor::undo_plan(&m.env, &p.id);
    assert!(errs.is_empty() && n == 1);
    assert!(inv.exists());
}

#[test]
fn without_full_disk_access_mail_storage_is_noted() {
    let m = mail_mac();
    kit::make_unlistable(&m.env.home_path(mail::MAIL_DIR));
    let p = scan(&m);
    assert!(p.notes.iter().any(|n| n.contains("Full Disk Access")));
    assert!(!p.items.iter().any(|i| i.category == "mail_downloaded"));
    assert!(
        find(&p, "Invoice.pdf").is_some(),
        "opened attachments still listed"
    );
}

/// Attachments are never preselected; the run moves exactly the ones the user ticked,
/// a modified one included when ticked by hand.
#[test]
fn attachments_move_only_when_ticked() {
    let m = mail_mac();
    let s = Settings::default();
    let mut p = scan(&m);
    assert!(!p.items.is_empty());
    assert_eq!(p.selected_count(), 0);
    // Cleaning the defaults moves nothing.
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty());
    assert!(p.items.iter().all(|i| i.path.exists()));
    // The user ticks one opened copy and the edited IMAP attachment.
    let inv = find(&p, "Invoice.pdf").unwrap().path.clone();
    let edited = att(&m, "IMAP-me@example.com", "INBOX", "12346", "edited.key");
    for i in 0..p.items.len() {
        let on = p.items[i].path == inv || p.items[i].path == edited;
        p.set_selected(i, on);
    }
    assert_eq!(p.selected_count(), 2);
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 2);
    assert!(!inv.exists() && !edited.exists());
    for i in p.items.iter().filter(|i| !i.selected) {
        assert!(i.path.exists(), "{} moved without a tick", i.path.display());
    }
}
