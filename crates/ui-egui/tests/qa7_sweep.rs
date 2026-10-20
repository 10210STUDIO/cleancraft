//! QA round 7: the error cases of Quick Sweep's Run (Q2) and of its Duplicate Downloads tile
//! (Q4), by clicks on the real Quick Sweep page on a FakeMac: permission denied and disk full
//! per item (the item stays, its reason is shown, every other selected item moves, nothing
//! unselected moves, the freed size equals a recalculation from disk), the action log that
//! cannot be written (nothing moves at all), and Undo after a partial run.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use cleancraft_core::executor;
use cleancraft_core::features::duplicates::DownloadDuplicates;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sweep::Sources;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{Fault, FeatureId, Plan};
use common::*;

fn plan_of(h: &H, id: FeatureId) -> Plan {
    h.state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tile(id)
        .unwrap()
        .plan()
        .unwrap()
        .clone()
}

fn scanned(m: &FakeMac) -> H {
    let mut h = harness(m);
    h.state_mut().sweep.sources = Some(sweep_sources());
    steps(&mut h);
    click(&mut h, "Scan");
    h
}

fn on_disk(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|p| tree_size(p).0).sum()
}

/// Q2: one System Junk item refused (permission denied) and one Mail item that hits a full
/// disk. Both stay with their reason on the result screen, every other selected item moves,
/// the freed size is the recalculated size of what moved, and Undo per module puts back only
/// what moved.
#[test]
fn qa7_q2_permission_denied_and_disk_full_are_shown_and_the_rest_moves() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    let trash = plan_of(&h, FeatureId::TrashBins);
    let denied = junk.selected().next().expect("a junk item").path.clone();
    let full = mail.selected().next().expect("a mail item").path.clone();
    m.env.inject_fault(&denied, Fault::PermissionDenied);
    m.env.inject_fault(&full, Fault::DiskFull);
    let go: Vec<PathBuf> = junk
        .selected()
        .chain(mail.selected())
        .map(|i| i.path.clone())
        .filter(|p| *p != denied && *p != full)
        .collect();
    assert!(!go.is_empty());
    let expect = on_disk(&go);
    let mail_go: Vec<PathBuf> = mail
        .selected()
        .map(|i| i.path.clone())
        .filter(|p| *p != full)
        .collect();
    let junk_go: Vec<PathBuf> = junk
        .selected()
        .map(|i| i.path.clone())
        .filter(|p| *p != denied)
        .collect();
    let mail_size = on_disk(&mail_go);
    let junk_size = on_disk(&junk_go);

    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");

    // The two faulted items stay, everything else selected moved, nothing else touched.
    assert!(denied.exists(), "permission denied item moved");
    assert!(full.exists(), "disk full item moved");
    for p in &go {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    for i in junk
        .items
        .iter()
        .chain(&mail.items)
        .chain(&trash.items)
        .filter(|i| !i.selected)
    {
        assert!(i.path.exists(), "unselected {} moved", i.path.display());
    }
    // Each reason is shown next to its file, numbers match the disk.
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "the disk is full"));
    assert!(has(&h, &m.env.display(&denied)));
    assert!(has(&h, &m.env.display(&full)));
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect))
    ));
    assert!(has(
        &h,
        &format!(
            "{} moved, 1 skipped, {}",
            junk_go.len(),
            human_size(junk_size)
        )
    ));
    assert!(has(
        &h,
        &format!(
            "{} moved, 1 skipped, {}",
            mail_go.len(),
            human_size(mail_size)
        )
    ));
    // The journal holds only what moved.
    assert_eq!(executor::history(&m.env).len(), go.len());

    // Undo Mail: only its moved files come back; the junk stays in the Trash.
    m.env.clear_faults();
    click_nth(&mut h, "Undo", 1);
    for p in &mail_go {
        assert!(p.exists(), "{} not put back", p.display());
    }
    for p in &junk_go {
        assert!(!p.exists(), "junk came back");
    }
    assert!(has(&h, "Put back"));
}

/// Q2: the action log cannot be written (disk full): no module moves anything and the error
/// is said on each module's card.
#[test]
fn qa7_q2_disk_full_on_the_action_log_moves_nothing_and_says_so() {
    let m = sweep_mac();
    let mut h = scanned(&m);
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    for i in junk.items.iter().chain(&mail.items) {
        assert!(i.path.exists(), "{} moved", i.path.display());
    }
    assert!(has_text(&h, "could not write the action log"));
    assert!(executor::history(&m.env).is_empty());
    assert!(!has_text(&h, "moved, "), "a module reported a move");
}

// Q4: Duplicate Downloads.

fn blob(seed: u8, n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

struct Dl {
    m: FakeMac,
    kept: Vec<PathBuf>,
    extra: Vec<PathBuf>,
}

/// Two recent sets in Downloads: photo.jpg (kept) with two copies, doc.pdf (kept) with one.
fn dl_mac() -> Dl {
    let m = FakeMac::new();
    let a = blob(3, 400_000);
    let b = blob(11, 250_000);
    let put = |rel: &str, d: &[u8]| {
        let p = m.env.home_path(rel);
        m.write(&p, d);
        p
    };
    let kept = vec![put("Downloads/photo.jpg", &a), put("Downloads/doc.pdf", &b)];
    let extra = vec![
        put("Downloads/photo (1).jpg", &a),
        put("Downloads/photo (2).jpg", &a),
        put("Downloads/doc copy.pdf", &b),
    ];
    Dl { m, kept, extra }
}

fn sweep_dl(m: &FakeMac) -> H {
    let mut h = harness(m);
    h.state_mut().sweep.sources = Some(Sources {
        plans: vec![Arc::new(DownloadDuplicates)],
        counts: vec![],
    });
    steps(&mut h);
    click(&mut h, "Scan");
    h
}

fn q4_run_with(fault: Fault, why: &str) {
    let d = dl_mac();
    let mut h = sweep_dl(&d.m);
    let p = plan_of(&h, FeatureId::Duplicates);
    let mut sel: Vec<PathBuf> = p.selected().map(|i| i.path.clone()).collect();
    sel.sort();
    let mut want = d.extra.clone();
    want.sort();
    assert_eq!(sel, want, "the extra copies are preselected");
    let bad = d.extra[0].clone();
    d.m.env.inject_fault(&bad, fault);
    let go = [d.extra[1].clone(), d.extra[2].clone()];
    let expect = on_disk(&go);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(bad.exists(), "the faulted copy moved");
    assert!(go.iter().all(|c| !c.exists()), "the other copies stayed");
    assert!(d.kept.iter().all(|c| c.exists()), "a kept copy moved");
    assert!(has(&h, why));
    assert!(has(&h, &d.m.env.display(&bad)));
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect))
    ));
    assert!(has(
        &h,
        &format!("2 moved, 1 skipped, {}", human_size(expect))
    ));
    assert_eq!(executor::history(&d.m.env).len(), 2);
    // Undo puts back the two that moved.
    click(&mut h, "Undo");
    assert!(d.extra.iter().all(|c| c.exists()));
    assert!(d.kept.iter().all(|c| c.exists()));
}

#[test]
fn qa7_q4_disk_full_keeps_that_copy_and_says_so() {
    q4_run_with(Fault::DiskFull, "the disk is full");
}

#[test]
fn qa7_q4_permission_denied_keeps_that_copy_and_says_so() {
    q4_run_with(Fault::PermissionDenied, "permission denied");
}

/// Q4: the action log cannot be written: no copy moves.
#[test]
fn qa7_q4_disk_full_on_the_action_log_moves_nothing() {
    let d = dl_mac();
    let mut h = sweep_dl(&d.m);
    d.m.env
        .inject_fault(&executor::journal_path(&d.m.env), Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(d.extra.iter().chain(&d.kept).all(|c| c.exists()));
    assert!(has_text(&h, "could not write the action log"));
}
