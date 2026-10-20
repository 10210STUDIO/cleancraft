//! FIX-1 by clicks: items in folders only an administrator can change. Threat Check and App
//! Remover list them selectable with "needs the administrator password", the confirmation
//! says macOS will ask for the password, one administrator command moves them into the
//! quarantine (checked on disk and in the journal), Undo puts them back, and a cancelled
//! password dialog moves nothing and says so.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sys::parse_admin_moves;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::testkit_perf as tp;
use cleancraft_core::{ActionKind, Fault};
use common::*;

const PROG: &str = "/Library/Application Support/Shady/daemon";

/// Threat Check fake Mac with an unsigned daemon in the root-owned /Library/LaunchDaemons.
fn threat_mac() -> (FakeMac, PathBuf) {
    let m = tp::startup_mac();
    let daemon = tp::add_launch_item(
        &m,
        "/Library/LaunchDaemons",
        "com.shady.daemon",
        PROG,
        Some(tp::codesign_unsigned(PROG)),
    );
    (m, daemon)
}

fn review_daemon_only(m: &FakeMac, daemon: &Path) -> (H, u64) {
    let mut h = harness(m);
    // The tool sits in the "Protect" section of the sidebar.
    click(&mut h, "Protect");
    click(&mut h, "Threat Check");
    click(&mut h, "Scan");
    assert!(has_text(
        &h,
        "Launch daemon com.shady.daemon: its program is not signed (needs the administrator password)"
    ));
    click(&mut h, "Select None");
    click(&mut h, "com.shady.daemon.plist");
    let size = tree_size(daemon).0;
    assert!(has(&h, &format!("1 selected, {}", human_size(size))));
    click(&mut h, "Review and Clean");
    assert!(has(
        &h,
        &format!(
            "Move 1 item ({}) to the CleanCraft quarantine?",
            human_size(size)
        )
    ));
    assert!(has(
        &h,
        "1 item is in a folder only an administrator can change. macOS will ask for the administrator password, and it goes to the CleanCraft quarantine."
    ));
    (h, size)
}

#[test]
fn threat_in_a_root_folder_moves_with_the_password_and_comes_back() {
    let (m, daemon) = threat_mac();
    let (mut h, size) = review_daemon_only(&m, &daemon);
    click(&mut h, "Move to Quarantine");
    assert!(!daemon.exists());
    let fs = m.env.fake_sys().unwrap();
    let calls = fs.admin_calls();
    assert_eq!(calls.len(), 1, "one password dialog");
    let pairs = parse_admin_moves(&calls[0]).unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(Path::new(&pairs[0].0), daemon);
    let stored = PathBuf::from(&pairs[0].1);
    assert!(stored.starts_with(m.env.quarantine_dir()) && stored.exists());
    let rec = executor::history(&m.env)
        .into_iter()
        .find(|r| matches!(&r.kind, ActionKind::Moved { original, .. } if original == &daemon))
        .expect("journaled");
    assert!(matches!(
        rec.kind,
        ActionKind::Moved {
            method: Method::AdminQuarantine,
            ..
        }
    ));
    assert_eq!(rec.size, size);
    assert!(has(
        &h,
        &format!("{} moved to the CleanCraft quarantine", human_size(size))
    ));
    assert!(has(&h, "1 item moved."));
    click(&mut h, "Undo");
    assert!(daemon.exists(), "put back");
    assert!(!stored.exists());
    assert_eq!(fs.admin_calls().len(), 2);
    assert!(has_text(&h, "1 item put back."));
}

#[test]
fn cancelled_password_dialog_moves_nothing_and_is_reported() {
    let (m, daemon) = threat_mac();
    let (mut h, _) = review_daemon_only(&m, &daemon);
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    click(&mut h, "Move to Quarantine");
    assert!(daemon.exists());
    assert!(has(&h, "0 items moved. 1 skipped."));
    assert!(has(
        &h,
        "the administrator password was not given, so it was not moved"
    ));
    assert!(has(&h, "Nothing moved"));
    assert!(
        !executor::history(&m.env)
            .iter()
            .any(|r| matches!(r.kind, ActionKind::Moved { .. }))
    );
}

/// An app installed by a package: the bundle belongs to the system.
fn pkg_mac() -> (FakeMac, PathBuf) {
    let m = FakeMac::new();
    let app = m.app("Pkg Tool", "com.example.pkgtool", "1.0");
    m.home_file("Library/Caches/com.example.pkgtool/c", 5_000);
    m.spotlight_finds_nothing();
    m.env.inject_fault(&app, Fault::PermissionDenied);
    (m, app)
}

#[test]
fn package_installed_app_uninstalls_with_the_password_or_stays_when_cancelled() {
    let (m, app) = pkg_mac();
    let cache = m.env.home_path("Library/Caches/com.example.pkgtool");
    let mut h = harness(&m);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    click(&mut h, "Pkg Tool");
    assert!(has_text(&h, "(needs the administrator password)"));
    // First attempt: the password dialog is cancelled.
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    click(&mut h, "Uninstall");
    assert!(has(
        &h,
        "1 item is in a folder only an administrator can change. macOS will ask for the administrator password, and it goes to the CleanCraft quarantine."
    ));
    click(&mut h, "Move to Trash");
    assert!(app.exists());
    assert!(!cache.exists(), "the user's own leftover still moved");
    assert!(has(
        &h,
        "the administrator password was not given, so it was not moved"
    ));
    assert!(has_text(&h, "1 item moved. 1 skipped."));
    click(&mut h, "Undo");
    assert!(cache.exists());
    // Second attempt: the password is given.
    m.env
        .fake_sys()
        .unwrap()
        .with(|s| s.admin_cancelled = false);
    let size = tree_size(&app).0 + tree_size(&cache).0;
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(!app.exists() && !cache.exists());
    assert!(has(
        &h,
        &format!(
            "{} moved to the Trash and the CleanCraft quarantine",
            human_size(size)
        )
    ));
    let stored = executor::history(&m.env)
        .into_iter()
        .rev()
        .find_map(|r| match r.kind {
            ActionKind::Moved {
                original,
                stored,
                method: Method::AdminQuarantine,
                ..
            } if original == app => Some(stored),
            _ => None,
        })
        .expect("bundle journaled as an administrator move");
    assert!(stored.starts_with(m.env.quarantine_dir()) && stored.exists());
}
