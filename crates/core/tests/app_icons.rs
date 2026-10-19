//! Real app icons and file previews (app_icons.rs) and the "Opened" fallback for apps
//! Spotlight has no date for, all on a fake Mac with scripted sips / qlmanage / osascript.

use std::time::{Duration, SystemTime};

use cleancraft_core::app_icons::{self, IconSource, Method};
use cleancraft_core::apps;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_icons::{
    FakeMacIcons, IconKind, png_bytes, quicklook_dir, quicklook_line, sips_line, workspace_line,
};

fn calls_with(mac: &FakeMac, prog: &str) -> usize {
    mac.env
        .fake_sys()
        .unwrap()
        .calls()
        .iter()
        .filter(|c| c.starts_with(prog))
        .count()
}

fn bump_mtime(p: &std::path::Path, secs_ahead: u64) {
    let t = SystemTime::now() + Duration::from_secs(secs_ahead);
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

#[test]
fn icon_source_reads_info_plist() {
    let mac = FakeMac::new();
    let a = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    assert_eq!(
        app_icons::icon_source(&a),
        IconSource::Icns(a.join("Contents/Resources/AppIcon.icns"))
    );
    let b = mac.icon_app(
        "Viewer",
        "com.example.Viewer",
        IconKind::Icns("viewer.icns"),
    );
    assert_eq!(
        app_icons::icon_source(&b),
        IconSource::Icns(b.join("Contents/Resources/viewer.icns"))
    );
    let c = mac.icon_app("Notes Pro", "com.example.Notes", IconKind::AssetCatalog);
    assert_eq!(app_icons::icon_source(&c), IconSource::AssetCatalog);
    let d = mac.icon_app("Bare", "com.example.Bare", IconKind::None);
    assert_eq!(app_icons::icon_source(&d), IconSource::Unknown);
    // A bundle with no keys but the usual file name still has its icon found.
    mac.write(&d.join("Contents/Resources/AppIcon.icns"), b"icns");
    assert_eq!(
        app_icons::icon_source(&d),
        IconSource::Icns(d.join("Contents/Resources/AppIcon.icns"))
    );
    // A named file that is missing falls back to the asset catalog when there is one.
    let e = mac.icon_app("Mixed", "com.example.Mixed", IconKind::AssetCatalog);
    let info = e.join("Contents/Info.plist");
    let mut dict = plist::Value::from_file(&info)
        .unwrap()
        .into_dictionary()
        .unwrap();
    dict.insert("CFBundleIconFile".into(), "Gone".into());
    plist::Value::Dictionary(dict).to_file_xml(&info).unwrap();
    assert_eq!(app_icons::icon_source(&e), IconSource::AssetCatalog);
}

#[test]
fn icns_goes_through_sips_then_comes_from_the_cache() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let out = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    assert!(out.starts_with(mac.env.home_path("Library/Caches/CleanCraft/icons")));
    mac.sips_writes(&icns, &out, 128);

    let (png, how) = app_icons::app_icon_png(&mac.env, &app, 128).unwrap();
    assert_eq!((png.as_path(), how), (out.as_path(), Method::Sips));
    let calls = mac.env.fake_sys().unwrap().calls();
    assert!(
        calls.contains(&sips_line(&mac, &icns, &out, 128)),
        "{calls:?}"
    );
    // The command names macOS paths, never the fake root.
    assert!(
        calls
            .iter()
            .all(|c| !c.contains(&*mac.dir.path().to_string_lossy()))
    );
    let img = app_icons::load_png(&png).unwrap();
    assert_eq!((img.width, img.height), (128, 128));
    assert_eq!(img.pixels.len(), 128 * 128 * 4);
    assert_eq!(&img.pixels[..4], &[40, 120, 220, 255]);

    // Cache hit: no second sips.
    let (_, how) = app_icons::app_icon_png(&mac.env, &app, 128).unwrap();
    assert_eq!(how, Method::Cache);
    assert_eq!(calls_with(&mac, "/usr/bin/sips"), 1);

    // Requests round up to 64 or 128: 100 px shares the 128 file.
    let (_, how) = app_icons::app_icon_png(&mac.env, &app, 100).unwrap();
    assert_eq!(how, Method::Cache);
    // 64 is its own file.
    let out64 = app_icons::app_icon_path(&mac.env, &app, 64).unwrap();
    assert_ne!(out64, out);
    mac.sips_writes(&icns, &out64, 64);
    let (p64, how) = app_icons::app_icon_png(&mac.env, &app, 40).unwrap();
    assert_eq!((p64, how), (out64, Method::Sips));
}

#[test]
fn an_updated_app_gets_a_new_icon() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let old = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.sips_writes(&icns, &old, 128);
    app_icons::app_icon_png(&mac.env, &app, 128).unwrap();

    // The update rewrites Info.plist: a new key, a cache miss, sips again.
    bump_mtime(&app.join("Contents/Info.plist"), 120);
    let new = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    assert_ne!(new, old, "a changed Info.plist must change the cache key");
    mac.sips_writes(&icns, &new, 128);
    let (p, how) = app_icons::app_icon_png(&mac.env, &app, 128).unwrap();
    assert_eq!((p, how), (new, Method::Sips));
    assert_eq!(calls_with(&mac, "/usr/bin/sips"), 2);

    // A bundle replaced as a whole (new folder time) also misses.
    let before = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    bump_mtime_dir(&app);
    assert_ne!(
        app_icons::app_icon_path(&mac.env, &app, 128).unwrap(),
        before
    );
}

fn bump_mtime_dir(dir: &std::path::Path) {
    // Adding an entry changes a folder's modification time.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.join("touched"), b"x").unwrap();
}

#[test]
fn asset_catalog_apps_use_quick_look_and_the_file_is_renamed() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Notes Pro", "com.example.Notes", IconKind::AssetCatalog);
    mac.quicklook_writes(&app, 64);
    let (png, how) = app_icons::app_icon_png(&mac.env, &app, 64).unwrap();
    assert_eq!(how, Method::QuickLook);
    assert_eq!(png, app_icons::app_icon_path(&mac.env, &app, 64).unwrap());
    assert!(app_icons::is_png(&png));
    // Renamed out of the Quick Look folder, not copied.
    assert!(!quicklook_dir(&mac).join("Notes Pro.app.png").exists());
    assert!(
        mac.env
            .fake_sys()
            .unwrap()
            .calls()
            .contains(&quicklook_line(&mac, &app, 64))
    );
    assert_eq!(calls_with(&mac, "/usr/bin/sips"), 0, "no .icns, no sips");
}

#[test]
fn sips_failure_falls_back_to_quick_look_then_appkit() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let out = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.sips_fails(&icns, &out, 128);
    mac.quicklook_silent(&app, 128);
    mac.workspace_writes(&app, &out, 128);
    let (png, how) = app_icons::app_icon_png(&mac.env, &app, 128).unwrap();
    assert_eq!((png, how), (out.clone(), Method::Workspace));
    let calls = mac.env.fake_sys().unwrap().calls();
    let order: Vec<usize> = [
        sips_line(&mac, &icns, &out, 128),
        quicklook_line(&mac, &app, 128),
        workspace_line(&mac, &app, &out, 128),
    ]
    .iter()
    .map(|l| calls.iter().position(|c| c == l).expect(l))
    .collect();
    assert!(order[0] < order[1] && order[1] < order[2], "{order:?}");
}

#[test]
fn a_stale_quick_look_file_never_passes_for_a_new_one() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Notes Pro", "com.example.Notes", IconKind::AssetCatalog);
    // Left over from an earlier run; this time qlmanage writes nothing.
    mac.write(
        &quicklook_dir(&mac).join("Notes Pro.app.png"),
        &png_bytes(64, 64, [1, 2, 3, 255]),
    );
    mac.quicklook_silent(&app, 64);
    let fs = mac.env.fake_sys().unwrap();
    fs.answer_full(
        "/usr/bin/osascript",
        CmdOut {
            status: 0,
            stdout: "failed\n".into(),
            stderr: String::new(),
        },
    );
    let err = app_icons::app_icon_png(&mac.env, &app, 64).unwrap_err();
    assert!(err.contains("qlmanage wrote no PNG"), "{err}");
    assert!(err.contains("AppKit could not draw"), "{err}");
}

#[test]
fn when_every_method_fails_it_is_not_retried_for_that_version() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let out = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.sips_fails(&icns, &out, 128);
    mac.quicklook_silent(&app, 128);
    mac.env.fake_sys().unwrap().answer_full(
        "/usr/bin/osascript",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "execution error: -1728\n".into(),
        },
    );
    let err = app_icons::app_icon_png(&mac.env, &app, 128).unwrap_err();
    assert!(err.contains("Unable to render"), "{err}");
    assert!(app_icons::failure_marker(&out).exists());
    let n = mac.env.fake_sys().unwrap().calls().len();
    assert!(app_icons::app_icon_png(&mac.env, &app, 128).is_err());
    assert_eq!(
        mac.env.fake_sys().unwrap().calls().len(),
        n,
        "a known failure runs no tool"
    );
    // A new version of the app is tried again.
    bump_mtime(&app.join("Contents/Info.plist"), 300);
    let new = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.sips_writes(&icns, &new, 128);
    assert_eq!(
        app_icons::app_icon_png(&mac.env, &app, 128).unwrap().1,
        Method::Sips
    );
}

#[test]
fn missing_tools_or_time_outs_leave_no_failure_marker() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let out = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.env.fake_sys().unwrap().time_out("sips");
    // qlmanage and osascript have no answer: "not found".
    assert!(app_icons::app_icon_png(&mac.env, &app, 128).is_err());
    assert!(!app_icons::failure_marker(&out).exists());
    // Later the tools work.
    mac.env.fake_sys().unwrap().with(|s| s.timeouts.clear());
    mac.sips_writes(&icns, &out, 128);
    assert_eq!(
        app_icons::app_icon_png(&mac.env, &app, 128).unwrap().1,
        Method::Sips
    );
    // Each tool has its own time limit.
    assert!(mac.env.fake_sys().unwrap().with(|s| {
        s.limits
            .iter()
            .any(|(l, t)| l.starts_with("/usr/bin/sips") && *t == app_icons::TOOL_TIMEOUT)
    }));
}

#[test]
fn a_corrupt_cache_file_is_made_again() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let icns = app.join("Contents/Resources/AppIcon.icns");
    let out = app_icons::app_icon_path(&mac.env, &app, 128).unwrap();
    mac.write(&out, b"half written");
    mac.sips_writes(&icns, &out, 128);
    assert_eq!(
        app_icons::app_icon_png(&mac.env, &app, 128).unwrap().1,
        Method::Sips
    );
    assert!(app_icons::load_png(&out).is_some());
}

#[test]
fn a_removed_app_has_no_icon() {
    let mac = FakeMac::new();
    let gone = mac.env.sys_path("/Applications/Gone.app");
    assert!(app_icons::app_icon_path(&mac.env, &gone, 64).is_none());
    assert!(app_icons::app_icon_png(&mac.env, &gone, 64).is_err());
    assert!(mac.env.fake_sys().unwrap().calls().is_empty());
}

#[test]
fn image_previews_are_decoded_and_cached_small() {
    let mac = FakeMac::new();
    let photo = mac.home().join("Pictures/beach.png");
    mac.write(&photo, &png_bytes(400, 200, [200, 30, 30, 255]));
    let (png, how) = app_icons::preview_png(&mac.env, &photo, 64).unwrap();
    assert_eq!(how, Method::Decode);
    let img = app_icons::load_png(&png).unwrap();
    assert_eq!((img.width, img.height), (64, 32), "aspect kept, fits 64");
    assert_eq!(
        app_icons::preview_png(&mac.env, &photo, 64).unwrap().1,
        Method::Cache
    );
    assert!(
        mac.env.fake_sys().unwrap().calls().is_empty(),
        "no tool for PNG"
    );
    // The original is untouched.
    assert!(app_icons::load_png(&photo).is_some());

    // A broken image fails without a tool and is not retried.
    let bad = mac.home().join("Pictures/broken.jpg");
    mac.write(&bad, b"not a jpeg");
    let err = app_icons::preview_png(&mac.env, &bad, 64).unwrap_err();
    assert!(err.contains("image"), "{err}");
    assert!(app_icons::preview_png(&mac.env, &bad, 64).is_err());

    // Kinds without a preview, folders and huge files.
    let log = mac.home_file("Library/Logs/app.log", 10);
    assert!(app_icons::preview_png(&mac.env, &log, 64).is_err());
    let dir = mac.home().join("Pictures/album.png");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(app_icons::preview_png(&mac.env, &dir, 64).is_err());
}

#[test]
fn heic_previews_use_sips_then_quick_look() {
    let mac = FakeMac::new();
    let photo = mac.home_file("Pictures/IMG_0001.HEIC", 100);
    let out = app_icons::preview_path(&mac.env, &photo, 128).unwrap();
    mac.sips_writes(&photo, &out, 128);
    assert_eq!(
        app_icons::preview_png(&mac.env, &photo, 128).unwrap().1,
        Method::Sips
    );
    let movie = mac.home_file("Movies/clip.mov", 100);
    mac.quicklook_writes(&movie, 64);
    assert_eq!(
        app_icons::preview_png(&mac.env, &movie, 64).unwrap().1,
        Method::QuickLook
    );
}

// ---------------------------------------------------------------------------------------
// "Opened: Unknown" for apps Spotlight has no date for (iMovie, GarageBand on a real Mac).

fn null_date(mac: &FakeMac, app: &std::path::Path, attr: &str) {
    // What `mdls -raw` prints for an attribute Spotlight does not have.
    mac.env.fake_sys().unwrap().answer(
        &format!("/usr/bin/mdls -name {attr} -raw {}", mac.env.display(app)),
        "(null)",
    );
}

#[test]
fn spotlight_ranking_date_is_used_when_the_exact_date_is_missing() {
    let mac = FakeMac::new();
    let app = mac.app("iMovie", "com.apple.iMovieApp", "10.4.4");
    null_date(&mac, &app, "kMDItemLastUsedDate");
    mac.env.fake_sys().unwrap().answer(
        &format!(
            "/usr/bin/mdls -name kMDItemLastUsedDate_Ranking -raw {}",
            mac.env.display(&app)
        ),
        "2026-10-12 00:00:00 +0000",
    );
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(list[0].last_used, Some(1_791_763_200));
    assert_eq!(list[0].last_active, None);
    let now = 1_791_763_200 + 7 * 86_400;
    assert_eq!(apps::opened_text(&list[0], now), "Opened: 7 days ago");
}

#[test]
fn app_data_is_a_labelled_fallback() {
    let mac = FakeMac::new();
    let app = mac.app("GarageBand", "com.apple.garageband10", "10.4.13");
    null_date(&mac, &app, "kMDItemLastUsedDate");
    null_date(&mac, &app, "kMDItemLastUsedDate_Ranking");
    // A sandboxed app keeps its preferences in its container.
    let prefs = mac.home().join(
        "Library/Containers/com.apple.garageband10/Data/Library/Preferences/com.apple.garageband10.plist",
    );
    mac.write(&prefs, b"prefs");
    cleancraft_core::testkit::age(&prefs, 5);

    // Without Full Disk Access the container is not looked at (macOS would ask the user).
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(list[0].last_used, None);
    assert_eq!(list[0].last_active, None);
    let now = chrono::Utc::now().timestamp();
    assert_eq!(apps::opened_text(&list[0], now), "Opened: No record");

    // With it, the container's preferences date is shown, clearly labelled.
    mac.write(
        &mac.home()
            .join("Library/Application Support/com.apple.TCC/TCC.db"),
        b"db",
    );
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(list[0].last_used, None, "never counted as opened");
    let t = list[0].last_active.expect("container preferences");
    assert!((now - t - 5 * 86_400).abs() < 120);
    assert_eq!(
        apps::opened_text(&list[0], now),
        "Last active: 5 days ago (app data)"
    );
    assert!(!list[0].unused(now), "a hint never makes an app Unused");

    // A Spotlight date always wins.
    let mac2 = FakeMac::new();
    let a2 = mac2.app("Editor", "com.example.Editor", "1.0");
    cleancraft_core::testkit_apps::FakeMacApps::last_opened(&mac2, &a2, 3);
    let saved = mac2
        .home()
        .join("Library/Saved Application State/com.example.Editor.savedState");
    std::fs::create_dir_all(&saved).unwrap();
    let list = apps::inventory(&mac2.env, true, &|_| {});
    assert!(list[0].last_used.is_some());
    assert_eq!(list[0].last_active, None);
    assert_eq!(
        calls_with(&mac2, "/usr/bin/mdls -name kMDItemLastUsedDate_Ranking"),
        0,
        "the second Spotlight question is only asked when the first has no date"
    );
}

#[test]
fn non_sandboxed_preferences_count_without_full_disk_access() {
    let mac = FakeMac::new();
    let app = mac.app("Live Home 3D", "com.belightsoft.LiveHome3D", "5.0.6");
    null_date(&mac, &app, "kMDItemLastUsedDate");
    let prefs = mac
        .home()
        .join("Library/Preferences/com.belightsoft.LiveHome3D.plist");
    mac.write(&prefs, b"p");
    cleancraft_core::testkit::age(&prefs, 40);
    let list = apps::inventory(&mac.env, true, &|_| {});
    let now = chrono::Utc::now().timestamp();
    assert_eq!(
        apps::opened_text(&list[0], now),
        "Last active: 40 days ago (app data)"
    );
    // Odd bundle ids never build paths outside the Library folders.
    assert!(apps::app_data_paths(&mac.env, "../../etc", true).is_empty());
    assert!(apps::app_data_paths(&mac.env, "", true).is_empty());
}
