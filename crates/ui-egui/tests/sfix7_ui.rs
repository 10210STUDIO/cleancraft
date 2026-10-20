//! sfix7, UI side: the confirmations of the delegated erase actions list exactly what goes
//! (CHK7-3: Empty Trash in Finder, Erase in Mail), Empty Trash never runs when the Trash
//! changed after the list was shown, and Move Quarantine to Trash lists and moves exactly the
//! confirmed items (CHK7-5). Driven by clicks on a FakeMac.

mod common;

use cleancraft_core::FeatureId;
use cleancraft_core::Settings;
use cleancraft_core::executor::{self, Method};
use cleancraft_core::features::{self, trash};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{junk_mac, trash_mac};
use cleancraft_core::testkit_trash::{answer_account_names, erase_call, mail_trash};
use cleancraft_ui_egui::pages::extras_cleanup::{
    CHANGED_NOTE, EMPTY_BUTTON, EMPTY_CONFIRM, ERASE_BUTTON, ERASE_NOTE,
};
use common::*;

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";
const GROUP: &str = "Mail trash, iCloud (izzo@icloud.com)";

fn empty_calls(m: &FakeMac) -> usize {
    m.env
        .fake_sys()
        .unwrap()
        .calls()
        .iter()
        .filter(|c| c.contains(trash::EMPTY_SCRIPT))
        .count()
}

fn open_trash(m: &FakeMac) -> H {
    let mut h = harness(m);
    click(&mut h, "Clean");
    click(&mut h, "Trash Bins");
    click(&mut h, "Scan");
    h
}

/// CHK7-3: the Empty Trash confirmation lists each item with its size and which Trash it is
/// in, says Mail and Finder erase these for good, and runs only on the list shown: an item
/// that came into the Trash after the list was made shows a new list instead of being erased
/// unseen.
#[test]
fn empty_trash_lists_items_and_refuses_a_changed_trash() {
    let m = trash_mac();
    m.env.fake_sys().unwrap().answer(trash::OSASCRIPT, "");
    let mut h = open_trash(&m);
    click(&mut h, EMPTY_BUTTON);
    assert!(has_text(&h, ERASE_NOTE));
    assert!(has_text(&h, "Trash of USB"));
    let user = m.env.user_trash();
    let folder = tree_size(&user.join("Folder")).0;
    assert!(has_text(&h, &human_size(folder)));
    // Something new lands in the Trash while the dialog is open.
    m.home_file(".Trash/late arrival.pdf", 40_000);
    click(&mut h, EMPTY_CONFIRM);
    assert_eq!(
        empty_calls(&m),
        0,
        "Finder was asked with a list the person never saw"
    );
    assert!(has_text(&h, CHANGED_NOTE));
    assert!(
        has_text(&h, "late arrival.pdf"),
        "the new list shows the new item"
    );
    click(&mut h, EMPTY_CONFIRM);
    assert_eq!(empty_calls(&m), 1);
}

/// CHK7-3: the Erase in Mail confirmation lists each mailbox Mail erases, with its message
/// count and size, and the same plain warning.
#[test]
fn erase_in_mail_lists_mailboxes_with_counts() {
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    let a = mail_trash(&m, UUID, "Deleted Messages", 4, 30_000);
    let b = mail_trash(&m, UUID, "Trash", 1, 20_000);
    answer_account_names(&m.env, &format!("{UUID}|iCloud|izzo@icloud.com\n"));
    m.env
        .fake_sys()
        .unwrap()
        .answer(&erase_call(Some("iCloud")), "");
    let mut h = open_trash(&m);
    click(&mut h, &format!("{ERASE_BUTTON} {GROUP}"));
    assert!(has_text(&h, ERASE_NOTE));
    assert!(has(&h, "Deleted Messages of iCloud"));
    assert!(has(&h, "Trash of iCloud"));
    assert!(has(&h, "4 messages"));
    assert!(has(&h, "1 message"));
    for p in [&a, &b] {
        let s = human_size(tree_size(p).0);
        assert!(has_text(&h, &s), "{s} not shown");
    }
}

/// CHK7-5: Move Quarantine to Trash lists every item with its size; exactly the listed items
/// move, and one that came into the quarantine after the dialog opened stays there.
#[test]
fn quarantine_confirmation_lists_and_moves_only_those_items() {
    let m = junk_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
    let settings = Settings {
        method: Method::Quarantine,
        ..Default::default()
    };
    settings.save(&m.env).unwrap();
    let plan = features::scan(&m.env, &settings, FeatureId::SystemJunk).unwrap();
    let rep = features::run_plan(&m.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(!rep.moved.is_empty());
    let listed = executor::quarantine_list(&m.env);
    let mut h = harness(&m);
    h.state_mut().navigate("settings");
    steps(&mut h);
    click(&mut h, "Move Quarantine to Trash...");
    let total: u64 = listed.iter().map(|(_, s)| s).sum();
    assert!(has_text(
        &h,
        &format!(
            "Move the quarantine to the Trash ({}, {})?",
            human_size(total),
            cleancraft_core::fsutil::items(listed.len())
        )
    ));
    // The rows in view at the top of the scrollable list (the rest scroll into view).
    for (p, size) in listed.iter().take(5) {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(has(&h, &name), "{name} is not listed");
        assert!(has_text(&h, &human_size(*size)));
    }
    let late = m.env.quarantine_dir().join("zz-late/late.bin");
    m.write(&late, &vec![1u8; 10_000]);
    click(&mut h, "Move to Trash");
    assert!(late.exists(), "an item the person did not see moved");
    for (p, _) in &listed {
        assert!(!p.exists(), "{} stayed", p.display());
    }
    assert_eq!(executor::quarantine_list(&m.env).len(), 1);
}
