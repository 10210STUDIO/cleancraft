//! Trash Bins (T1, T2) by clicks: sizes against disk, items locked (no Review and Clean), Empty
//! Trash in Finder behind a confirmation, the command recorded and logged, then a rescan; and
//! the error paths (Automation denied, log unwritable).

mod common;

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::trash;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::trash_mac;
use cleancraft_core::{Fault, FeatureId, Plan};
use common::*;

const EMPTY_CALL: &str = "/usr/bin/osascript -e tell application \"Finder\" to empty trash";

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::TrashBins]
        .plan
        .clone()
        .expect("plan")
}

fn open(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    // The tool sits in the "Clean" section of the sidebar.
    click(&mut h, "Clean");
    click(&mut h, "Trash Bins");
    click(&mut h, "Scan");
    h
}

fn finder_calls(mac: &FakeMac) -> usize {
    mac.env
        .fake_sys()
        .unwrap()
        .calls()
        .iter()
        .filter(|c| *c == EMPTY_CALL)
        .count()
}

#[test]
fn sizes_locked_items_and_empty_in_finder() {
    let mac = trash_mac();
    mac.env.fake_sys().unwrap().answer(EMPTY_CALL, "");
    let mut h = open(&mac);
    let user = mac.env.user_trash();
    let uid = trash::uid(&mac.env);
    let usb = mac.env.sys_path(&format!("/Volumes/USB/.Trashes/{uid}"));
    let user_size = tree_size(&user.join("old.zip")).0 + tree_size(&user.join("Folder")).0;
    let usb_size = tree_size(&usb.join("movie.mov")).0;
    let mail_size = tree_size(
        &mac.env
            .home_path("Library/Mail/V10/IMAP-me@example.com/Deleted Messages.mbox"),
    )
    .0;
    assert!(has(&h, &human_size(user_size + usb_size + mail_size)));
    assert!(has(
        &h,
        &format!("Startup disk Trash  {}", human_size(user_size))
    ));
    assert!(has(&h, &format!("Other disks  {}", human_size(usb_size))));
    assert!(has(&h, &format!("Mail trash  {}", human_size(mail_size))));
    assert!(has_text(
        &h,
        "use Empty Trash in Finder to remove it for good"
    ));
    assert!(has_text(
        &h,
        "Only Mail can erase deleted mail"
    ));
    assert!(
        !has(&h, "Review and Clean"),
        "Trash items cannot be moved again"
    );
    // Cancel: Finder is not asked.
    click(&mut h, "Empty Trash in Finder");
    assert!(has(
        &h,
        &format!("Empty the Trash ({})?", human_size(user_size + usb_size))
    ));
    click(&mut h, "Cancel");
    assert_eq!(finder_calls(&mac), 0);
    assert!(!has_text(&h, "Empty the Trash ("));
    // Confirm: one osascript call, a journal line, a notice, a fresh scan.
    let first_id = plan(&h).id;
    click(&mut h, "Empty Trash in Finder");
    click(&mut h, "Empty Trash");
    assert_eq!(finder_calls(&mac), 1);
    assert!(has_text(&h, "Finder emptied the Trash."));
    let hist = executor::history(&mac.env);
    assert_eq!(hist.len(), 1);
    assert!(matches!(
        &hist[0].kind,
        ActionKind::Command { ok: true, .. }
    ));
    assert_ne!(plan(&h).id, first_id, "scanned again");
    // CleanCraft itself removed nothing.
    assert!(user.join("old.zip").exists());
    // History shows the command. (The sidebar's History row lies below an 820 px window, a
    // shell layout issue reported to the coordinator, so navigate the way the row does.)
    h.state_mut().navigate("history");
    steps(&mut h);
    assert!(has(
        &h,
        "osascript -e 'tell application \"Finder\" to empty trash'"
    ));
}

#[test]
fn automation_denied_and_log_unwritable() {
    let mac = trash_mac();
    mac.env.fake_sys().unwrap().answer_full(
        EMPTY_CALL,
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "execution error: Not authorized to send Apple events to Finder. (-1743)"
                .into(),
        },
    );
    let mut h = open(&mac);
    click(&mut h, "Empty Trash in Finder");
    click(&mut h, "Empty Trash");
    assert!(has_text(&h, "Privacy & Security > Automation"));
    assert!(matches!(
        &executor::history(&mac.env)[0].kind,
        ActionKind::Command { ok: false, .. }
    ));

    let mac = trash_mac();
    mac.env.fake_sys().unwrap().answer(EMPTY_CALL, "");
    let mut h = open(&mac);
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    click(&mut h, "Empty Trash in Finder");
    click(&mut h, "Empty Trash");
    assert!(has_text(&h, "so Finder was not asked to empty the Trash"));
    assert_eq!(finder_calls(&mac), 0);
}

#[test]
fn empty_trash_button_only_when_finder_has_something() {
    let mac = FakeMac::new();
    mac.home_file(
        "Library/Mail/V10/IMAP-me@example.com/Deleted Messages.mbox/0A/Data/Messages/1.emlx",
        20_000,
    );
    let h = open(&mac);
    assert!(has_text(&h, "Mail trash"));
    assert!(!has(&h, "Empty Trash in Finder"));
    let mac = FakeMac::new();
    let h = open(&mac);
    assert!(has(&h, "Nothing to clean here"));
    assert!(!has(&h, "Empty Trash in Finder"));
}
