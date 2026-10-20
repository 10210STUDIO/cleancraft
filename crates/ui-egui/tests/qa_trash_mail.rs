//! Trash Bins after the user's report (real Mac screenshot): Mail trash groups named by the
//! account with "Erase in Mail..." always visible, a confirmation that says Mail erases for
//! good and CleanCraft cannot undo it, Cancel, the Accessibility error with its next steps,
//! Mail running or not, the list measured again; the startup Trash always a row (empty,
//! unreadable with Full Disk Access offered); no "0 KB selected" header on locked rows.

mod common;

use cleancraft_core::features::trash;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{make_unlistable, set_running};
use cleancraft_core::testkit_trash::{answer_account_names, erase_call, mail_trash};
use cleancraft_core::{FeatureId, Plan};
use cleancraft_ui_egui::pages::extras_cleanup::{
    ACCESS_BUTTON, ERASE_BUTTON, ERASE_CONFIRM, FDA_BUTTON, OPEN_MAIL_BUTTON,
};
use common::*;
use egui_kittest::kittest::Queryable;

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";
const GROUP: &str = "Mail trash, iCloud (izzo@icloud.com)";

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::TrashBins]
        .plan
        .clone()
        .expect("plan")
}

fn mac() -> (FakeMac, std::path::PathBuf, std::path::PathBuf) {
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    let a = mail_trash(&m, UUID, "Deleted Messages", 4, 30_000);
    let b = mail_trash(&m, UUID, "Trash", 1, 20_000);
    answer_account_names(&m.env, &format!("{UUID}|iCloud|izzo@icloud.com\n"));
    (m, a, b)
}

fn open(m: &FakeMac) -> H {
    let mut h = harness(m);
    click(&mut h, "Clean");
    click(&mut h, "Trash Bins");
    click(&mut h, "Scan");
    h
}

fn erase_calls(m: &FakeMac) -> usize {
    m.env
        .fake_sys()
        .unwrap()
        .calls()
        .iter()
        .filter(|c| c.contains("Erase Deleted Items"))
        .count()
}

#[test]
fn rows_are_named_and_the_next_step_is_shown() {
    let (m, a, b) = mac();
    let h = open(&m);
    let total = tree_size(&a).0 + tree_size(&b).0;
    // The account's header, its size, and its action; no "selected" for locked rows.
    assert!(has(&h, &format!("{GROUP}: {}", human_size(total))));
    assert!(!has_text(&h, "0 KB selected"));
    assert!(has(&h, &format!("{ERASE_BUTTON} {GROUP}")));
    // Rows by mailbox, never by the UUID folder.
    assert!(has(&h, "Deleted Messages"));
    assert!(has(&h, "Trash"));
    assert!(!has_text(&h, UUID));
    // The empty Trash is a row, and the bar says what to do.
    assert!(has_text(&h, trash::REASON_EMPTY));
    assert!(has_text(&h, "Use Erase in Mail to remove deleted messages"));
}

#[test]
fn erase_confirm_cancel_then_erase() {
    let (m, a, _) = mac();
    let f = m.env.fake_sys().unwrap();
    f.answer(&erase_call(Some("iCloud")), "");
    let mut h = open(&m);
    click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
    // What happens, exactly.
    assert!(has_text(&h, "Erase 5 deleted messages in Mail"));
    assert!(has_text(&h, "CleanCraft cannot undo this"));
    assert!(has_text(&h, "Mailbox > Erase Deleted Items > iCloud"));
    click(&mut h, "Cancel");
    assert!(!has_text(&h, "Erase 5 deleted messages"));
    assert_eq!(erase_calls(&m), 0, "Cancel asks nothing of Mail");
    click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
    click(&mut h, ERASE_CONFIRM);
    assert_eq!(erase_calls(&m), 1);
    assert!(has_text(&h, "Mail is erasing the deleted messages"));
    // Mail erased them (in the fake: the store emptied, as Mail would): the list is measured
    // again and the group is gone.
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(
        m.env
            .home_path(&format!("Library/Mail/V10/{UUID}/Trash.mbox")),
    )
    .unwrap();
    click(&mut h, "Scan Again");
    assert!(!plan(&h).items.iter().any(|i| i.category == "mail_trash"));
    assert!(!has(&h, &format!("{ERASE_BUTTON} {GROUP}")));
}

#[test]
fn scan_runs_again_after_erase() {
    let (m, _, _) = mac();
    m.env
        .fake_sys()
        .unwrap()
        .answer(&erase_call(Some("iCloud")), "");
    let mut h = open(&m);
    let before = plan(&h).id;
    click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
    click(&mut h, ERASE_CONFIRM);
    assert_ne!(plan(&h).id, before, "measured again right after");
}

#[test]
fn accessibility_missing_shows_next_steps() {
    let (m, _, _) = mac();
    let f = m.env.fake_sys().unwrap();
    f.answer_full(
        &erase_call(Some("iCloud")),
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr:
                "System Events got an error: osascript is not allowed assistive access. (-1719)"
                    .into(),
        },
    );
    f.answer("/usr/bin/open -a Mail", "");
    let mut h = open(&m);
    assert!(!has(&h, ACCESS_BUTTON));
    click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
    click(&mut h, ERASE_CONFIRM);
    assert!(has_text(&h, "needs the Accessibility permission"));
    assert!(has_text(&h, "Mailbox > Erase Deleted Items > iCloud"));
    click(&mut h, ACCESS_BUTTON);
    assert!(
        f.with(|s| s.opened_urls.clone())
            .iter()
            .any(|u| u == trash::ACCESSIBILITY_URL)
    );
    click(&mut h, OPEN_MAIL_BUTTON);
    assert!(f.calls().iter().any(|c| c == "/usr/bin/open -a Mail"));
    assert!(has_text(&h, "Mail is open"));
}

#[test]
fn mail_running_or_not() {
    for running in [false, true] {
        let (m, _, _) = mac();
        if running {
            set_running(&m.env, "Mail", "/System/Applications/Mail.app");
        }
        m.env
            .fake_sys()
            .unwrap()
            .answer(&erase_call(Some("iCloud")), "");
        let mut h = open(&m);
        click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
        click(&mut h, ERASE_CONFIRM);
        // The same command either way: Mail is activated (opened when it was not running).
        assert_eq!(erase_calls(&m), 1);
        assert!(has_text(&h, "Mail is erasing"));
    }
}

#[test]
fn unreadable_trash_offers_full_disk_access() {
    let m = FakeMac::new();
    m.home_file(".Trash/a.zip", 10_000);
    make_unlistable(&m.env.user_trash());
    let mut h = open(&m);
    assert!(has_text(&h, trash::REASON_UNREADABLE));
    click(&mut h, FDA_BUTTON);
    let f = m.env.fake_sys().unwrap();
    assert!(
        f.with(|s| s.opened_urls.clone())
            .iter()
            .any(|u| u.contains("Privacy_AllFiles"))
    );
}
