//! A8, A9, A10: update checks with scripted web answers.

use cleancraft_core::apps;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::updater::{self, Source};

const CAST: &str = "https://example.com/editor/cast.xml";

fn mac_with_apps() -> FakeMac {
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
        build: "90",
        feed: Some("https://down.example.org/cast.xml"),
        ..AppSpec::new("Viewer", "org.example.Viewer", "0.9")
    });
    mac.app("Plain", "com.plain.App", "1.0");
    mac.app_spec(&AppSpec {
        dir: "/System/Applications",
        ..AppSpec::new("Calculator", "com.apple.calculator", "11")
    });
    mac
}

#[test]
fn sparkle_app_store_and_failures() {
    let mac = mac_with_apps();
    mac.web(
        CAST,
        &appcast(&[
            ("200", "2.0", "<p>Old</p>"),
            ("210", "2.1", "<ul><li>Faster</li><li>Fixes</li></ul>"),
        ]),
    );
    mac.web(
        &updater::lookup_url("com.maker.NotesPro"),
        r#"{"resultCount":1,"results":[{"version":"3.2","releaseNotes":"Sync fixes"}]}"#,
    );
    mac.web_fails("https://down.example.org/cast.xml", 28);
    mac.env.fake_sys().unwrap().answer(
        "/usr/sbin/softwareupdate --list",
        "Software Update found the following new or updated software:\n* Label: macOS 15.1-24B83\n\tTitle: macOS Sequoia 15.1, Version: 15.1, Size: 1000KiB, Recommended: YES, Action: restart,\n",
    );
    let list = apps::inventory(&mac.env, false, &|_| {});
    let r = updater::check_all(&mac.env, &list, &|_| {});
    assert_eq!(r.apps.len(), 3, "{:#?}", r.apps);
    assert_eq!(r.not_checked, 1, "Plain has no channel");
    let get = |n: &str| r.apps.iter().find(|u| u.name == n).unwrap();
    let e = get("Editor");
    assert_eq!(e.source, Source::Sparkle);
    assert!(e.available);
    assert_eq!(e.installed, "2.0");
    assert_eq!(e.latest.as_deref(), Some("2.1"));
    assert_eq!(e.notes, "- Faster\n- Fixes");
    assert_eq!(
        e.download_url.as_deref(),
        Some("https://example.com/dl/2.1.zip")
    );
    let n = get("Notes Pro");
    assert_eq!(n.source, Source::AppStore);
    assert!(n.available);
    assert_eq!(n.latest.as_deref(), Some("3.2"));
    let v = get("Viewer");
    assert!(!v.available);
    assert_eq!(
        v.error.as_deref(),
        Some("the update server did not answer in time")
    );
    assert_eq!(r.macos.len(), 1);
    assert_eq!(r.macos[0].title, "macOS Sequoia 15.1");

    // Hand-offs, never a download of our own.
    mac.env.fake_sys().unwrap().answer("/usr/bin/open", "");
    let msg = updater::start_update(&mac.env, e).unwrap();
    assert!(msg.contains("Check for Updates"));
    updater::start_update(&mac.env, n).unwrap();
    updater::open_download(&mac.env, e).unwrap();
    updater::open_software_update(&mac.env).unwrap();
    let fs = mac.env.fake_sys().unwrap();
    assert!(
        fs.calls()
            .contains(&"/usr/bin/open -b com.example.Editor".to_string())
    );
    let urls = fs.with(|s| s.opened_urls.clone());
    assert_eq!(
        urls,
        [
            updater::APP_STORE_UPDATES_URL,
            "https://example.com/dl/2.1.zip",
            updater::SOFTWARE_UPDATE_URL
        ]
    );
    assert!(
        fs.calls()
            .iter()
            .filter(|c| c.starts_with("/usr/bin/curl"))
            .all(|c| !c.contains(".zip")),
        "CleanCraft never downloads the update itself"
    );
}

#[test]
fn up_to_date_and_bad_answers() {
    let mac = mac_with_apps();
    mac.web(CAST, &appcast(&[("200", "2.0", "")]));
    mac.web(
        &updater::lookup_url("com.maker.NotesPro"),
        "<html>busy</html>",
    );
    let list = apps::inventory(&mac.env, false, &|_| {});
    let r = updater::check_all(&mac.env, &list, &|_| {});
    let e = r.apps.iter().find(|u| u.name == "Editor").unwrap();
    assert!(!e.available && e.error.is_none());
    let n = r.apps.iter().find(|u| u.name == "Notes Pro").unwrap();
    assert!(n.error.is_some());
    // softwareupdate not answered: reported, not fatal.
    assert!(r.macos_error.is_some());
    assert_eq!(r.available().count(), 0);
}

#[test]
fn release_notes_link_is_fetched() {
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        build: "1",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "1.0")
    });
    mac.web(CAST, r#"<rss><channel><item><enclosure url="https://e.com/a.zip" sparkle:version="2" sparkle:shortVersionString="1.1"/><sparkle:releaseNotesLink>https://e.com/notes.html</sparkle:releaseNotesLink></item></channel></rss>"#);
    mac.web(
        "https://e.com/notes.html",
        "<html><body><h1>1.1</h1><p>New icon</p></body></html>",
    );
    let list = apps::inventory(&mac.env, false, &|_| {});
    let u = updater::check_app(&mac.env, &list[0]).unwrap();
    assert!(u.available);
    assert_eq!(u.notes, "1.1\n\nNew icon");
}

#[test]
fn same_version_newer_build_shows_the_builds() {
    // As uTorrent Web on a real Mac: 1.6.0 build 6413 installed, the feed has 1.6.0 build 6575.
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        build: "6413",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "1.6.0")
    });
    mac.web(CAST, &appcast(&[("6575", "1.6.0", "")]));
    let list = apps::inventory(&mac.env, false, &|_| {});
    let u = updater::check_app(&mac.env, &list[0]).unwrap();
    assert!(u.available);
    assert_eq!(u.installed, "1.6.0 (6413)");
    assert_eq!(u.latest.as_deref(), Some("1.6.0 (6575)"));

    // A different version keeps the plain numbers.
    mac.web(CAST, &appcast(&[("6600", "1.7.0", "")]));
    let u = updater::check_app(&mac.env, &list[0]).unwrap();
    assert_eq!(
        (u.installed.as_str(), u.latest.as_deref()),
        ("1.6.0", Some("1.7.0"))
    );
}
