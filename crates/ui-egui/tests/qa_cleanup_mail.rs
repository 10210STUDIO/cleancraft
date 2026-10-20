//! Mail Downloads (M1 to M3) by clicks: numbers against disk, nothing preselected (the user
//! ticks attachments; modified ones say why to keep them), POP and On My Mac left out, Mail
//! running, cleaning, undo and errors.

mod common;

use std::path::PathBuf;

use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, MAIL_V, mail_mac};
use cleancraft_core::{Fault, FeatureId, Plan};
use common::*;
use egui_kittest::kittest::Queryable;

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::MailAttachments]
        .plan
        .clone()
        .expect("plan")
}

fn open(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "Mail Downloads");
    click(&mut h, "Scan");
    h
}

/// Click an item's checkbox (named after the file) on the plan page.
fn tick(h: &mut H, name: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, name)
        .click();
    steps(h);
}

/// Show one category's rows ("Opened attachments  500 KB"), so each row is on screen.
fn show_category(h: &mut H, cat: &str, title: &str) {
    let size: u64 = plan(h)
        .items
        .iter()
        .filter(|i| i.category == cat)
        .map(|i| i.size)
        .sum();
    click(h, &format!("{title}  {}", human_size(size)));
}

/// The user ticks every attachment that can be moved, then unticks the two changed after Mail
/// saved them (what their reason tells them to keep).
fn tick_unchanged(h: &mut H) {
    click(h, "Select All");
    show_category(h, "mail_opened", "Opened attachments");
    tick(h, "Contract.docx");
    show_category(h, "mail_downloaded", "Downloaded attachments");
    tick(h, "edited.key");
}

fn att(m: &FakeMac, account: &str, mailbox: &str, msg: &str, file: &str) -> PathBuf {
    m.env.home_path(&format!(
        "{MAIL_V}/{account}/{mailbox}.mbox/0F0F/Data/1/Attachments/{msg}/1/{file}"
    ))
}

#[test]
fn scan_shows_disk_numbers_and_clean_keeps_modified() {
    let mac = mail_mac();
    let mut h = open(&mac);
    let p = plan(&h);
    let total: u64 = p.items.iter().map(|i| tree_size(&i.path).0).sum();
    assert!(has(&h, &human_size(total)), "total {}", human_size(total));
    for (cat, title) in [
        ("mail_opened", "Opened attachments"),
        ("mail_downloaded", "Downloaded attachments"),
    ] {
        let s: u64 = p
            .items
            .iter()
            .filter(|i| i.category == cat)
            .map(|i| tree_size(&i.path).0)
            .sum();
        assert!(has(&h, &format!("{title}  {}", human_size(s))), "{title}");
    }
    assert!(has_text(
        &h,
        "attachments in POP or On My Mac mailboxes were left out"
    ));
    assert!(!has(&h, "only-copy.pdf") && !has(&h, "local.pdf"));
    assert!(has_text(
        &h,
        "changed after Mail saved it, so it is not selected"
    ));
    assert!(has_text(
        &h,
        "could not tell whether the mail server keeps a copy"
    ));
    // Nothing is preselected: the user ticks what to move.
    assert_eq!(p.selected_count(), 0, "an attachment is preselected");
    assert!(has(&h, "0 selected, 0 KB"));
    assert!(!has(&h, "Review and Clean"));
    tick_unchanged(&mut h);
    let p = plan(&h);
    assert!(
        p.selected()
            .all(|i| !i.reason.contains("changed after Mail saved it"))
    );
    let sel: Vec<(PathBuf, u64)> = p
        .selected()
        .map(|i| (i.path.clone(), tree_size(&i.path).0))
        .collect();
    let sel_size: u64 = sel.iter().map(|(_, s)| s).sum();
    assert!(has(
        &h,
        &format!("{} selected, {}", sel.len(), human_size(sel_size))
    ));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    for (path, _) in &sel {
        assert!(!path.exists(), "{} not moved", path.display());
    }
    let edited = att(&mac, "IMAP-me@example.com", "INBOX", "12346", "edited.key");
    let pop = att(&mac, "POP-old@pop.example", "INBOX", "9", "only-copy.pdf");
    let mystery = att(
        &mac,
        kit::UUID_UNKNOWN_ACCOUNT,
        "INBOX",
        "66",
        "mystery.zip",
    );
    assert!(edited.exists() && pop.exists() && mystery.exists());
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(sel_size))
    ));
    click(&mut h, "Undo");
    for (path, _) in &sel {
        assert!(path.exists(), "{} not restored", path.display());
    }
}

#[test]
fn mail_running_locks_and_blocks() {
    let mac = mail_mac();
    kit::set_running(&mac.env, "Mail", "/System/Applications/Mail.app");
    let h = open(&mac);
    assert!(has(&h, "Mail is open. Quit Mail and scan again to clean."));
    assert!(has_text(&h, "(cannot be selected: Quit Mail first)"));
    assert!(!has(&h, "Review and Clean"), "nothing can be selected");
    assert!(has(&h, "0 selected, 0 KB"));

    // Mail opened between the scan and the cleanup: the cleanup is refused.
    let mac = mail_mac();
    let mut h = open(&mac);
    tick_unchanged(&mut h);
    let p = plan(&h);
    assert!(p.selected_count() > 0);
    kit::set_running(&mac.env, "Mail", "/System/Applications/Mail.app");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(has(&h, "Mail is open. Quit Mail, then scan again."));
    assert!(p.selected().all(|i| i.path.exists()));
}

#[test]
fn errors_per_attachment() {
    let mac = mail_mac();
    let mut h = open(&mac);
    tick_unchanged(&mut h);
    let p = plan(&h);
    let photo = att(&mac, "IMAP-me@example.com", "INBOX", "12345", "photo.jpg");
    let deck = att(
        &mac,
        kit::UUID_PLIST_ACCOUNT,
        "Sent Messages",
        "55",
        "deck.pdf",
    );
    let budget = att(&mac, kit::UUID_DB_ACCOUNT, "INBOX", "77", "budget.xlsx");
    mac.env.inject_fault(&photo, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&deck);
    mac.env.inject_fault(&budget, Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(photo.exists() && deck.exists() && budget.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has(&h, "the disk is full"));
    // What moved is exactly the rest of the selection; the freed number is its size.
    let rest: u64 = p
        .selected()
        .filter(|i| ![&photo, &deck, &budget].contains(&&i.path))
        .map(|i| i.size)
        .sum();
    assert!(has(&h, &format!("{} moved to the Trash", human_size(rest))));
}

#[test]
fn without_full_disk_access_the_page_says_so() {
    let mac = mail_mac();
    kit::make_unlistable(&mac.env.home_path("Library/Mail"));
    let h = open(&mac);
    assert!(has_text(
        &h,
        "Turn on Full Disk Access to include attachments"
    ));
    assert!(plan(&h).items.iter().all(|i| i.category == "mail_opened"));
}
