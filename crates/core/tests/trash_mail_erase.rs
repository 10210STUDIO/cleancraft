//! Trash Bins, user report from a real Mac: Mail trash rows named by the account (not the
//! UUID folder), the startup Trash always a row (empty, unreadable), and "Erase in Mail" asking
//! Mail's own command through System Events, with the Accessibility and Automation errors
//! explained, logged, never touching Mail's files.

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::{self, trash};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{make_unlistable, set_running, trash_mac};
use cleancraft_core::testkit_trash::{answer_account_names, erase_call, mail_trash};
use cleancraft_core::{FeatureId, Settings};

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";

fn scan(m: &FakeMac) -> cleancraft_core::Plan {
    features::scan(&m.env, &Settings::default(), FeatureId::TrashBins).unwrap()
}

#[test]
fn mail_trash_rows_are_named_by_account() {
    let m = FakeMac::new();
    let deleted = mail_trash(&m, UUID, "Deleted Messages", 3, 20_000);
    let other = mail_trash(&m, UUID, "Trash", 1, 10_000);
    answer_account_names(
        &m.env,
        &format!("{UUID}|iCloud|izzo@icloud.com\nFFFF|com.apple.account.IMAP|\n"),
    );
    let p = scan(&m);
    let by = |path: &std::path::Path| p.items.iter().find(|i| i.path == path).unwrap();
    let a = by(&deleted);
    let b = by(&other);
    // One group per account, named after it; the two mailboxes under it.
    assert_eq!(
        a.group.as_deref(),
        Some("Mail trash, iCloud (izzo@icloud.com)")
    );
    assert_eq!(b.group, a.group);
    assert!(
        a.reason.starts_with("Deleted Messages: 3 messages"),
        "{}",
        a.reason
    );
    assert!(b.reason.starts_with("Trash: 1 message"), "{}", b.reason);
    assert!(
        !p.items
            .iter()
            .any(|i| i.group.as_deref().is_some_and(|g| g.contains(UUID)))
    );
    assert_eq!(a.locked.as_deref(), Some(trash::LOCK_MAIL_TRASH));
    // The menu name and totals the Erase in Mail dialog uses.
    let (acct, n, bytes) = trash::mail_group(&m.env, &p, a.group.as_deref().unwrap()).unwrap();
    assert_eq!(acct.as_deref(), Some("iCloud"));
    assert_eq!(n, 4);
    assert_eq!(bytes, a.size + b.size);
}

#[test]
fn unnamed_account_gets_a_short_name_and_on_my_mac_is_local() {
    let m = FakeMac::new();
    let x = mail_trash(&m, UUID, "Deleted Messages", 1, 20_000);
    let local = mail_trash(&m, "Mailboxes", "Trash", 2, 20_000);
    let p = scan(&m);
    let by = |path: &std::path::Path| p.items.iter().find(|i| i.path == path).unwrap();
    assert_eq!(
        by(&x).group.as_deref(),
        Some("Mail trash, account BDEB6609")
    );
    assert_eq!(by(&local).group.as_deref(), Some("Mail trash, On My Mac"));
    assert!(by(&local).reason.contains("stored only on this Mac"));
    // Not named: no Erase (Mail's "In All Accounts" would erase more than is shown, SAF6-7).
    let (acct, _, _) = trash::mail_group(&m.env, &p, "Mail trash, account BDEB6609").unwrap();
    assert_eq!(acct, None);
    let (acct, _, _) = trash::mail_group(&m.env, &p, "Mail trash, On My Mac").unwrap();
    assert_eq!(acct.as_deref(), Some("On My Mac"));
}

#[test]
fn startup_trash_is_always_a_row() {
    // Empty.
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    let p = scan(&m);
    let it = p
        .items
        .iter()
        .find(|i| i.path == m.env.user_trash())
        .unwrap();
    assert_eq!(it.reason, trash::REASON_EMPTY);
    assert_eq!(it.size, 0);
    assert_eq!(it.locked.as_deref(), Some(trash::LOCK_TRASH_EMPTY));
    // Missing folder: empty too.
    let m = FakeMac::new();
    let p = scan(&m);
    assert!(p.items.iter().any(|i| i.reason == trash::REASON_EMPTY));
    // Unreadable: a row saying so, with Full Disk Access as the lock reason.
    let m = trash_mac();
    make_unlistable(&m.env.user_trash());
    let p = scan(&m);
    let it = p
        .items
        .iter()
        .find(|i| i.path == m.env.user_trash())
        .unwrap();
    assert_eq!(it.reason, trash::REASON_UNREADABLE);
    assert_eq!(it.locked.as_deref(), Some(trash::LOCK_NEEDS_ACCESS));
    assert!(p.notes.iter().any(|n| n.contains("could not be read")));
}

#[test]
fn erase_uses_mails_own_command_and_is_logged() {
    let m = FakeMac::new();
    let mb = mail_trash(&m, UUID, "Deleted Messages", 2, 20_000);
    let f = m.env.fake_sys().unwrap();
    f.answer(&erase_call(Some("iCloud")), "");
    let out = trash::erase_in_mail(&m.env, Some("iCloud"));
    assert!(out.ok, "{out:?}");
    assert!(out.opened_mail, "Mail was not running");
    assert!(
        out.message
            .contains("Mailbox > Erase Deleted Items > iCloud")
    );
    let calls = f.calls();
    assert_eq!(calls, vec![erase_call(Some("iCloud"))]);
    assert!(calls[0].contains("tell application \"Mail\" to activate"));
    assert!(calls[0].contains("click menu item \"iCloud\""));
    // CleanCraft never touches Mail's store.
    assert!(mb.join("0A/Data/Messages/0.emlx").exists());
    let h = executor::history(&m.env);
    assert_eq!(h.len(), 1);
    assert!(!h[0].can_undo());
    assert!(
        matches!(&h[0].kind, ActionKind::Command { ok: true, command, .. } if command.contains("Erase Deleted Items"))
    );
    // Mail running: not opened.
    set_running(&m.env, "Mail", "/System/Applications/Mail.app");
    let out = trash::erase_in_mail(&m.env, Some("iCloud"));
    assert!(out.ok && !out.opened_mail);
}

#[test]
fn erase_errors_are_explained() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    let fail = |stderr: &str| CmdOut {
        status: 1,
        stdout: String::new(),
        stderr: stderr.into(),
    };
    let call = erase_call(Some("Work"));
    f.answer_full(
        &call,
        fail("System Events got an error: osascript is not allowed assistive access. (-1719)"),
    );
    let out = trash::erase_in_mail(&m.env, Some("Work"));
    assert!(!out.ok && out.needs_accessibility);
    assert!(out.message.contains("Accessibility"), "{}", out.message);
    assert!(out.message.contains("Mailbox > Erase Deleted Items > Work"));
    f.answer_full(
        &call,
        fail("Not authorized to send Apple events to System Events. (-1743)"),
    );
    let out = trash::erase_in_mail(&m.env, Some("Work"));
    assert!(out.needs_automation && out.message.contains("Automation"));
    f.answer_full(&call, fail("User canceled. (-128)"));
    let out = trash::erase_in_mail(&m.env, Some("Work"));
    assert!(!out.ok && out.message.contains("cancelled"));
    // No menu item with that name: nothing else is clicked, and it says so.
    f.answer_full(
        &call,
        fail("System Events got an error: Can't get menu item \"Work\". (-1728)"),
    );
    let out = trash::erase_in_mail(&m.env, Some("Work"));
    assert!(
        !out.ok && out.message.contains("nothing was erased"),
        "{}",
        out.message
    );
    // Every attempt is in the log.
    assert_eq!(executor::history(&m.env).len(), 4);
}

#[test]
fn unnamed_account_only_opens_mail() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer(&erase_call(None), "");
    let script = trash::erase_script(None).join("\n");
    assert!(!script.contains("In All Accounts") && !script.contains("click"));
    let out = trash::erase_in_mail(&m.env, None);
    assert!(!out.ok, "nothing was erased");
    assert!(
        out.message.contains("Choose Mailbox > Erase Deleted Items"),
        "{}",
        out.message
    );
    for named in [Some("iCloud"), Some("On My Mac")] {
        assert!(
            !trash::erase_script(named)
                .join("\n")
                .contains("In All Accounts")
        );
    }
}

#[test]
fn erase_needs_the_log() {
    let m = FakeMac::new();
    m.env.inject_fault(
        &executor::journal_path(&m.env),
        cleancraft_core::Fault::DiskFull,
    );
    let out = trash::erase_in_mail(&m.env, Some("iCloud"));
    assert!(!out.ok);
    assert!(out.message.contains("action log could not be written"));
    assert!(m.env.fake_sys().unwrap().calls().is_empty());
}

#[test]
fn account_names_parse() {
    let v = trash::parse_account_names(
        "A|iCloud|me@icloud.com\nB||bob@x.com\nC|com.apple.account.IMAP|\n\n",
    );
    assert_eq!(
        v[0],
        (
            "A".into(),
            Some("iCloud".into()),
            Some("me@icloud.com".into())
        )
    );
    assert_eq!(v[1], ("B".into(), None, Some("bob@x.com".into())));
    assert_eq!(v[2], ("C".into(), None, None));
    assert_eq!(trash::mail_label("Work", Some("Work")), "Mail trash, Work");
}
