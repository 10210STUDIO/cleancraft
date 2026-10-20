//! fix7: App Store apps (Numbers, Pages) after an uninstall, by clicks on a fake Mac.
//!
//! The user saw Numbers and Pages still listed after removing them. Root cause found here:
//! such a bundle is owned by root, so it moves only with the administrator password, and on
//! macOS 13 and later even the administrator is refused without App Management. The leftovers
//! moved, the app stayed, and the single-app Done screen still said "Uninstalled." These tests
//! pin both outcomes: when the move happens the list drops the app at once and matches a fresh
//! inventory; when macOS refuses, the Done screen says the app is still installed, why, and
//! what to do, and the list keeps it (it is still there). Apps in the Trash or the quarantine
//! are never listed.

mod common;

use std::path::PathBuf;

use cleancraft_core::Fault;
use cleancraft_core::apps;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use common::*;

fn store_app(m: &FakeMac, name: &str, id: &str) -> PathBuf {
    let app = m.app_spec(&AppSpec {
        name,
        bundle_id: id,
        version: "14.0",
        build: "1",
        dir: "/Applications",
        mas: true,
        feed: None,
        size: 200_000,
        binary_plist: false,
    });
    // Installed by the App Store: owned by root.
    m.env.inject_fault(&app, Fault::PermissionDenied);
    app
}

fn mac() -> (FakeMac, PathBuf, PathBuf) {
    let m = FakeMac::new();
    let numbers = store_app(&m, "Numbers", "com.apple.iWork.Numbers");
    let pages = store_app(&m, "Pages", "com.apple.iWork.Pages");
    m.app("Editor", "com.example.Editor", "2.0");
    m.home_file("Library/Containers/com.apple.iWork.Numbers/Data/x", 40_000);
    m.spotlight_finds_nothing();
    let f = m.env.fake_sys().unwrap();
    f.answer("/usr/bin/open", "");
    (m, numbers, pages)
}

fn listed(h: &H) -> Vec<String> {
    h.state()
        .apps
        .list
        .as_ref()
        .map(|l| l.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default()
}

fn fresh(m: &FakeMac) -> Vec<String> {
    apps::inventory(&m.env, false, &|_| {})
        .into_iter()
        .map(|a| a.name)
        .collect()
}

#[test]
fn moved_with_the_password_it_leaves_the_list_at_once() {
    let (m, numbers, _) = mac();
    let mut h = harness(&m);
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert!(listed(&h).contains(&"Numbers".to_string()));
    click(&mut h, "Numbers");
    assert!(has_text(&h, "(needs the administrator password)"));
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(!numbers.exists(), "moved by the administrator");
    assert!(has_text(&h, "Uninstalled."));
    click(&mut h, "Done");
    steps(&mut h);
    let now = listed(&h);
    assert!(!now.contains(&"Numbers".to_string()), "{now:?}");
    assert_eq!(now, fresh(&m), "the list matches a fresh look");
}

#[test]
fn refused_by_macos_the_done_screen_says_it_stayed_and_what_to_do() {
    let (m, numbers, _) = mac();
    m.env.fake_sys().unwrap().fail_admin_move(&numbers);
    let mut h = harness(&m);
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    click(&mut h, "Numbers");
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(numbers.exists());
    assert!(!has_text(&h, "Uninstalled."), "never says Uninstalled");
    assert!(has_text(&h, "Numbers was not removed"));
    assert!(has_text(&h, "App Management"));
    assert!(has(&h, "Show Numbers in Finder"));
    click(&mut h, "Show Numbers in Finder");
    let spawned = m.env.fake_sys().unwrap().spawned();
    assert!(
        spawned
            .iter()
            .any(|c| c.starts_with("/usr/bin/open -R") && c.ends_with("Numbers.app")),
        "{spawned:?}"
    );
    click(&mut h, "Open App Management Settings");
    click(&mut h, "Done");
    steps(&mut h);
    // Still installed: still listed, like a fresh look.
    assert!(listed(&h).contains(&"Numbers".to_string()));
    assert_eq!(listed(&h), fresh(&m));
}

#[test]
fn several_store_apps_at_once_refused_one_moved_one() {
    let (m, numbers, pages) = mac();
    m.env.fake_sys().unwrap().fail_admin_move(&pages);
    let mut h = harness(&m);
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    click(&mut h, "Select Numbers");
    click(&mut h, "Select Pages");
    let review = buttons_starting(&h, "Review and Uninstall");
    click(&mut h, &review);
    let go = buttons_starting(&h, "Uninstall 2");
    click(&mut h, &go);
    click(&mut h, "Move to Trash");
    assert!(!numbers.exists() && pages.exists());
    assert!(has_text(&h, "1 app uninstalled."));
    assert!(has_text(&h, "Still installed. macOS protects this app."));
    click(&mut h, "Done");
    steps(&mut h);
    let now = listed(&h);
    assert!(!now.contains(&"Numbers".to_string()) && now.contains(&"Pages".to_string()));
    assert_eq!(now, fresh(&m));
}

#[test]
fn apps_in_the_trash_or_the_quarantine_are_never_listed() {
    let (m, _, _) = mac();
    m.app_spec(&AppSpec {
        name: "Old",
        bundle_id: "com.example.old",
        version: "1",
        build: "1",
        dir: "~/.Trash",
        mas: false,
        feed: None,
        size: 1000,
        binary_plist: false,
    });
    let q = m.env.quarantine_dir().join("plan-1");
    std::fs::create_dir_all(&q).unwrap();
    std::fs::create_dir_all(q.join("Gone.app/Contents")).unwrap();
    let names = fresh(&m);
    assert!(!names.contains(&"Old".to_string()) && !names.contains(&"Gone".to_string()));
    let mut h = harness(&m);
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    let l = listed(&h);
    assert!(
        !l.contains(&"Old".to_string()) && !l.contains(&"Gone".to_string()),
        "{l:?}"
    );
}

/// The label of the first control whose label starts with `prefix`.
fn buttons_starting(h: &H, prefix: &str) -> String {
    use egui_kittest::kittest::NodeT;
    h.root()
        .children_recursive()
        .filter_map(|n| n.accesskit_node().label())
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no control starting with {prefix}"))
}
