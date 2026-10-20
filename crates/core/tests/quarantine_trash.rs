//! REVIEW-U U5: the quarantine can be moved to the Trash from the app. Nothing is deleted:
//! every item is renamed into the Trash and journaled, Undo All puts the run back in the
//! quarantine, and the cleanup that put an item in the quarantine can still be undone (from
//! the Trash, back to where it came from). Fake Mac only.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{
    self, QUARANTINE_TO_TRASH, execute, history, move_quarantine_to_trash, quarantine_items,
    quarantine_size, undo, undo_plan,
};
use cleancraft_core::features::{self, allow_roots};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{ActionKind, Fault, FeatureId, Method, Settings, SkipReason};

/// The sample Mac with System Junk moved to the quarantine. Returns the cleanup's report.
fn quarantined(m: &FakeMac) -> executor::Report {
    let plan = features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let r = execute(
        &m.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Quarantine,
        &mut |_, _| {},
    )
    .unwrap();
    assert!(r.moved.len() >= 2, "the sample Mac has junk to move");
    r
}

fn stored(r: &cleancraft_core::ActionRecord) -> PathBuf {
    match &r.kind {
        ActionKind::Moved { stored, .. } => stored.clone(),
        k => panic!("not a move: {k:?}"),
    }
}

fn original(r: &cleancraft_core::ActionRecord) -> PathBuf {
    match &r.kind {
        ActionKind::Moved { original, .. } => original.clone(),
        k => panic!("not a move: {k:?}"),
    }
}

fn record(m: &FakeMac, id: &str) -> cleancraft_core::ActionRecord {
    history(&m.env)
        .into_iter()
        .rev()
        .find(|r| r.id == id)
        .expect("record")
}

fn in_trash(m: &FakeMac, p: &Path) -> bool {
    p.starts_with(m.env.user_trash())
}

#[test]
fn quarantine_goes_to_the_trash_and_nothing_is_deleted() {
    let m = FakeMac::sample();
    let clean = quarantined(&m);
    let (bytes, items) = quarantine_size(&m.env);
    assert_eq!(items, clean.moved.len());
    assert_eq!(bytes, clean.freed);
    let rep = move_quarantine_to_trash(&m.env, &mut |_, _| {}).unwrap();
    assert!(rep.plan_id.starts_with(QUARANTINE_TO_TRASH));
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert_eq!(rep.moved.len(), items);
    assert_eq!(rep.freed, bytes);
    // The quarantine is empty, every byte is in the Trash.
    assert!(quarantine_items(&m.env).is_empty());
    assert_eq!(quarantine_size(&m.env), (0, 0));
    let mut in_trash_bytes = 0;
    for r in &rep.moved {
        let s = stored(r);
        assert!(in_trash(&m, &s), "{} not in the Trash", s.display());
        in_trash_bytes += tree_size(&s).0;
        assert!(original(r).starts_with(m.env.quarantine_dir()));
        assert!(r.feature.is_none());
    }
    assert_eq!(
        in_trash_bytes, bytes,
        "every byte still exists, in the Trash"
    );
    // The cleanup's records now point at the Trash, so its Undo still works.
    for r in &clean.moved {
        let now = record(&m, &r.id);
        assert!(in_trash(&m, &stored(&now)), "{:?}", now.kind);
        assert!(now.can_undo());
    }
}

#[test]
fn undo_all_puts_the_quarantine_back_and_the_cleanup_undo_still_works() {
    let m = FakeMac::sample();
    let clean = quarantined(&m);
    let before = quarantine_size(&m.env);
    let rep = move_quarantine_to_trash(&m.env, &mut |_, _| {}).unwrap();
    let (n, errs) = undo_plan(&m.env, &rep.plan_id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, rep.moved.len());
    assert_eq!(quarantine_size(&m.env), before, "back in the quarantine");
    // The cleanup's records follow the items back into the quarantine.
    for r in &clean.moved {
        let now = record(&m, &r.id);
        assert!(stored(&now).starts_with(m.env.quarantine_dir()));
    }
    // And the cleanup itself can still be undone: every file back where it was.
    let (n, errs) = undo_plan(&m.env, &clean.plan_id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, clean.moved.len());
    for r in &clean.moved {
        assert!(original(r).exists(), "{} not back", original(r).display());
    }
    assert!(quarantine_items(&m.env).is_empty());
}

#[test]
fn undoing_the_cleanup_from_the_trash_closes_the_move() {
    let m = FakeMac::sample();
    let clean = quarantined(&m);
    let rep = move_quarantine_to_trash(&m.env, &mut |_, _| {}).unwrap();
    // One item of the cleanup goes straight from the Trash back to its place.
    let first = &clean.moved[0];
    undo(&m.env, &first.id).unwrap();
    assert!(original(first).exists());
    // Its quarantine-to-Trash record is over (the item left the Trash): no Undo left on it.
    let reloc = rep
        .moved
        .iter()
        .find(|r| record(&m, &r.id).undone.is_some())
        .expect("the matching move is closed");
    assert!(!record(&m, &reloc.id).can_undo());
    // Undo All of the move puts the rest back in the quarantine, without errors.
    let (n, errs) = undo_plan(&m.env, &rep.plan_id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, rep.moved.len() - 1);
    assert_eq!(quarantine_items(&m.env).len(), clean.moved.len() - 1);
}

#[test]
fn an_item_that_cannot_move_stays_in_the_quarantine() {
    let m = FakeMac::sample();
    quarantined(&m);
    let items = quarantine_items(&m.env);
    m.env.inject_fault(&items[0], Fault::PermissionDenied);
    let rep = move_quarantine_to_trash(&m.env, &mut |_, _| {}).unwrap();
    assert_eq!(rep.skipped.len(), 1);
    assert_eq!(
        rep.skipped[0],
        (items[0].clone(), SkipReason::PermissionDenied)
    );
    assert_eq!(rep.moved.len(), items.len() - 1);
    assert_eq!(quarantine_items(&m.env), vec![items[0].clone()]);
}

#[test]
fn an_empty_quarantine_moves_nothing_and_logs_nothing() {
    let m = FakeMac::sample();
    let lines = history(&m.env).len();
    let rep = move_quarantine_to_trash(&m.env, &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty() && rep.skipped.is_empty());
    assert_eq!(history(&m.env).len(), lines);
}

#[test]
fn a_full_disk_journal_moves_nothing() {
    let m = FakeMac::sample();
    quarantined(&m);
    let before = quarantine_items(&m.env);
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    assert!(move_quarantine_to_trash(&m.env, &mut |_, _| {}).is_err());
    assert_eq!(quarantine_items(&m.env), before);
}
