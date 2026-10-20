//! App Updates end to end, by clicks: check, versions and release notes shown, each Update
//! button hands off to the right place (commands recorded by FakeSys), a network failure is
//! shown for that app only, macOS updates open Software Update.

mod common;

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::updater;
use common::*;

const CAST: &str = "https://example.com/editor/cast.xml";
const DOWN: &str = "https://down.example.org/cast.xml";

fn mac() -> FakeMac {
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        build: "200",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Notes Pro", "com.maker.NotesPro", "3.1")
    });
    mac.app_spec(&AppSpec {
        build: "9",
        feed: Some(DOWN),
        ..AppSpec::new("Viewer", "org.example.Viewer", "0.9")
    });
    mac.web(
        CAST,
        &appcast(&[("210", "2.1", "<ul><li>Faster search</li></ul>")]),
    );
    mac.web(
        &updater::lookup_url("com.maker.NotesPro"),
        r#"{"resultCount":1,"results":[{"version":"3.2","releaseNotes":"Sync fixes"}]}"#,
    );
    mac.web_fails(DOWN, 6);
    let fs = mac.env.fake_sys().unwrap();
    fs.answer(
        "/usr/sbin/softwareupdate --list",
        "* Label: macOS 15.1-24B83\n\tTitle: macOS Sequoia 15.1, Version: 15.1, Size: 2000000KiB, Recommended: YES, Action: restart,\n",
    );
    fs.answer("/usr/bin/open", "");
    mac
}

#[test]
fn check_show_and_hand_off() {
    let mac = mac();
    let mut h = harness(&mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Updates");
    click(&mut h, "Check for Updates");
    assert!(has(&h, "2 updates available"));
    assert!(has(&h, "2.0 -> 2.1"));
    assert!(has(&h, "3.1 -> 3.2"));
    // Network failure is per app, the others still checked.
    assert!(has(
        &h,
        "Viewer: the update server could not be found (no internet?)"
    ));
    // Release notes.
    assert!(!has_text(&h, "Faster search"));
    click(&mut h, "What's new in Editor");
    assert!(has_text(&h, "- Faster search"));
    // Hand-offs.
    click(&mut h, "Update Editor");
    assert!(
        mac.env
            .fake_sys()
            .unwrap()
            .calls()
            .contains(&"/usr/bin/open -b com.example.Editor".to_string())
    );
    assert!(has_text(&h, "Editor is open. Choose Check for Updates"));
    click(&mut h, "Download Editor");
    click(&mut h, "Update Notes Pro");
    assert!(has_text(&h, "The App Store opened on its Updates page."));
    // macOS.
    assert!(has_text(&h, "macOS Sequoia 15.1  15.1"));
    assert!(has_text(&h, "needs a restart"));
    click(&mut h, "Open Software Update");
    let urls = mac.env.fake_sys().unwrap().with(|s| s.opened_urls.clone());
    assert_eq!(
        urls,
        [
            "https://example.com/dl/2.1.zip",
            updater::APP_STORE_UPDATES_URL,
            updater::SOFTWARE_UPDATE_URL
        ]
    );
}

#[test]
fn everything_offline() {
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.web_fails(CAST, 28);
    let mut h = harness(&mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Updates");
    click(&mut h, "Check for Updates");
    assert!(has(&h, "All checked apps are up to date"));
    assert!(has(&h, "Editor: the update server did not answer in time"));
    assert!(has_text(&h, "Software Update could not check"));
    // Check Again after the network is back.
    mac.web(CAST, &appcast(&[("3", "2.5", "")]));
    click(&mut h, "Check Again");
    assert!(has(&h, "1 update available"));
    assert!(has(&h, "2.0 -> 2.5"));
}
