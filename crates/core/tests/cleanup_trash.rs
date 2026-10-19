//! Trash Bins (T1, T2): sizes of every Trash, items locked (nothing CleanCraft can move),
//! Empty Trash handed to Finder and logged.

use cleancraft_core::executor::{self, ActionKind, SkipReason};
use cleancraft_core::features::{self, Feature, trash};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit_cleanup::{MAIL_V, trash_mac};
use cleancraft_core::{Fault, FeatureId, Settings};

const EMPTY_CALL: &str = "/usr/bin/osascript -e tell application \"Finder\" to empty trash";

#[test]
fn every_trash_is_measured_and_locked() {
    let m = trash_mac();
    let p = features::scan(&m.env, &Settings::default(), FeatureId::TrashBins).unwrap();
    let uid = trash::uid(&m.env);
    let user = m.env.user_trash();
    let usb = m.env.sys_path(&format!("/Volumes/USB/.Trashes/{uid}"));
    let mail = m.env.home_path(&format!(
        "{MAIL_V}/IMAP-me@example.com/Deleted Messages.mbox"
    ));
    assert_eq!(p.items.len(), 3, "{:#?}", p.items);
    let by = |path: &std::path::Path| p.items.iter().find(|i| i.path == path).unwrap();
    // Sizes: each entry of a Trash with the shared formula; Mail's trash as a tree.
    let expect_user = tree_size(&user.join("old.zip")).0 + tree_size(&user.join("Folder")).0;
    assert_eq!(by(&user).size, expect_user);
    assert_eq!(by(&user).category, "startup_trash");
    assert!(by(&user).reason.ends_with("2 items"));
    assert_eq!(by(&usb).size, tree_size(&usb.join("movie.mov")).0);
    assert_eq!(by(&usb).category, "external_trash");
    // One entry reads "1 item" (REVIEW-B 10), and the item is named after its Trash, not the
    // numeric user-id folder it lives in (REVIEW-A 34).
    assert_eq!(by(&usb).reason, "Trash of USB: 1 item");
    assert_eq!(by(&usb).group.as_deref(), Some("Trash of USB"));
    assert_eq!(by(&mail).size, tree_size(&mail).0);
    assert!(by(&mail).reason.ends_with("2 messages"));
    assert_eq!(by(&user).locked.as_deref(), Some(trash::LOCK_IN_TRASH));
    assert_eq!(by(&usb).locked.as_deref(), Some(trash::LOCK_IN_TRASH));
    assert_eq!(by(&mail).locked.as_deref(), Some(trash::LOCK_MAIL_TRASH));
    assert_eq!(p.selected_count(), 0);
    assert!(trash::TrashBins.allow_roots(&m.env).is_empty());
    assert_eq!(
        trash::finder_trash_size(&m.env),
        by(&user).size + by(&usb).size
    );
}

#[test]
fn an_edited_plan_still_cannot_move_trash() {
    let m = trash_mac();
    let s = Settings::default();
    let mut p = features::scan(&m.env, &s, FeatureId::TrashBins).unwrap();
    for it in p.items.iter_mut() {
        it.locked = None;
        it.selected = true;
    }
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty());
    assert!(
        rep.skipped
            .iter()
            .all(|(_, r)| matches!(r, SkipReason::Refused(_)))
    );
    assert!(m.env.user_trash().join("old.zip").exists());
}

#[test]
fn empty_asks_finder_and_logs_it() {
    let m = trash_mac();
    m.env.fake_sys().unwrap().answer(EMPTY_CALL, "");
    let out = trash::empty_trash_in_finder(&m.env);
    assert!(out.ok, "{out:?}");
    assert!(
        m.env
            .fake_sys()
            .unwrap()
            .calls()
            .contains(&EMPTY_CALL.to_string())
    );
    let h = executor::history(&m.env);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].feature, Some(FeatureId::TrashBins));
    assert!(!h[0].can_undo());
    match &h[0].kind {
        ActionKind::Command { command, ok, .. } => {
            assert_eq!(
                command,
                "osascript -e 'tell application \"Finder\" to empty trash'"
            );
            assert!(ok);
        }
        k => panic!("{k:?}"),
    }
    // CleanCraft itself removed nothing (only Finder would).
    assert!(m.env.user_trash().join("old.zip").exists());
}

#[test]
fn empty_errors_are_explained() {
    let m = trash_mac();
    m.env.fake_sys().unwrap().answer_full(
        EMPTY_CALL,
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "execution error: Not authorized to send Apple events to Finder. (-1743)"
                .into(),
        },
    );
    let out = trash::empty_trash_in_finder(&m.env);
    assert!(!out.ok);
    assert!(out.message.contains("Automation"), "{}", out.message);
    match &executor::history(&m.env)[0].kind {
        ActionKind::Command { ok, output, .. } => {
            assert!(!ok);
            assert!(output.contains("-1743"));
        }
        k => panic!("{k:?}"),
    }
    // No answer at all (osascript missing): reported, logged as failed.
    let m = trash_mac();
    let out = trash::empty_trash_in_finder(&m.env);
    assert!(!out.ok);
    assert!(out.message.starts_with("Finder could not empty the Trash"));
    // The log cannot be written: Finder is not even asked.
    let m = trash_mac();
    m.env.fake_sys().unwrap().answer(EMPTY_CALL, "");
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    let out = trash::empty_trash_in_finder(&m.env);
    assert!(!out.ok);
    assert!(out.message.contains("action log could not be written"));
    assert!(m.env.fake_sys().unwrap().calls().is_empty());
}

#[test]
fn unreadable_trash_is_noted() {
    let m = trash_mac();
    cleancraft_core::testkit_cleanup::make_unlistable(&m.env.user_trash());
    let p = features::scan(&m.env, &Settings::default(), FeatureId::TrashBins).unwrap();
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("/Users/tester/.Trash could not be read"))
    );
    assert!(!p.items.iter().any(|i| i.path == m.env.user_trash()));
    assert_eq!(p.items.len(), 2);
}
