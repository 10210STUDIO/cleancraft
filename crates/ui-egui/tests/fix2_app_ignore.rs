//! fix2, DOM-10: App Remover's Ignore button, by clicks. The app goes on the ignore list (saved
//! settings), leaves the list, stays on disk, and an uninstall plan made for it anyway selects
//! nothing.

mod common;

use cleancraft_core::Settings;
use cleancraft_core::features::uninstaller;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::FakeMacApps;
use common::*;

#[test]
fn ignored_app_leaves_the_list_and_is_never_offered() {
    let mac = FakeMac::new();
    let keep = mac.app("Keep Me", "com.example.KeepMe", "1.0");
    mac.app("Other", "com.example.Other", "1.0");
    mac.home_file("Library/Caches/com.example.KeepMe/c", 10_000);
    mac.spotlight_finds_nothing();
    let mut h = harness(&mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert!(has(&h, "Keep Me") && has(&h, "Other"));
    click(&mut h, "Keep Me");
    assert!(has(&h, "Uninstall"), "detail of the app is open");
    click(&mut h, "Hide from List");
    // Saved, as Settings shows it.
    let saved = Settings::load(&mac.env);
    assert!(saved.ignore.contains(&keep), "{:?}", saved.ignore);
    // Back on the list, without the ignored app; the app is untouched.
    assert!(!has(&h, "Keep Me"), "ignored app still listed");
    assert!(has(&h, "Other"));
    assert!(keep.exists());
    let plan = uninstaller::uninstall_plan(&mac.env, &saved, &keep, false).unwrap();
    assert_eq!(plan.selected_count(), 0, "{:?}", plan.items);
    assert!(plan.notes.iter().any(|n| n.contains("ignore list")));
}
