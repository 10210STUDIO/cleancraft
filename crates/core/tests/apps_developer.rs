//! REVIEW-A 45: the developer shown for each app comes from its code signature, else its
//! Info.plist copyright, never from a bundle id fragment ("Co", "Jetbrains"); "Part of macOS"
//! only for apps in /System and Apple apps installed with macOS (Safari), so Apple's App Store
//! apps (iMovie, GarageBand, Pages, Numbers, Keynote) are removable like any other.

use std::path::{Path, PathBuf};

use cleancraft_core::Settings;
use cleancraft_core::apps::{self, AppInfo, Store};
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};

/// Add Info.plist keys to an app made by the testkit.
fn plist_keys(app: &Path, keys: &[(&str, &str)]) {
    cleancraft_core::testkit_icons::set_plist_strings(app, keys);
}

/// `codesign -dv --verbose=2 <app>` writes `authority` lines to stderr.
fn signed(mac: &FakeMac, app: &Path, authority: &str) {
    let line = format!("/usr/bin/codesign -dv --verbose=2 {}", mac.env.display(app));
    mac.env.fake_sys().unwrap().answer_full(
        &line,
        CmdOut {
            status: 0,
            stdout: String::new(),
            stderr: format!(
                "Executable={}/Contents/MacOS/x\nIdentifier=x\n{authority}TeamIdentifier=ABCDE12345\n",
                mac.env.display(app)
            ),
        },
    );
}

fn devid(name: &str) -> String {
    format!(
        "Authority=Developer ID Application: {name} (ABCDE12345)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\n"
    )
}

const MAS: &str = "Authority=Apple Mac OS Application Signing\nAuthority=Apple Worldwide Developer Relations Certification Authority\nAuthority=Apple Root CA\n";

fn get<'a>(list: &'a [AppInfo], p: &Path) -> &'a AppInfo {
    list.iter().find(|a| a.path == p).unwrap()
}

#[test]
fn developer_names_come_from_the_signature_then_the_copyright() {
    let mac = FakeMac::new();
    // The six apps of REVIEW-A 45, as the real Mac named them.
    let goland = mac.app("GoLand", "com.jetbrains.goland", "2024.2");
    signed(&mac, &goland, &devid("JetBrains s.r.o."));
    let frog = mac.app(
        "Screaming Frog SEO Spider",
        "co.uk.screamingfrog.seo.spider",
        "20.0",
    );
    signed(&mac, &frog, &devid("Screaming Frog Ltd"));
    let freecad = mac.app("FreeCAD", "org.freecad.FreeCAD", "1.0");
    // Ad hoc signed: no authority, the copyright names the holder.
    signed(&mac, &freecad, "Signature=adhoc\n");
    plist_keys(
        &freecad,
        &[(
            "NSHumanReadableCopyright",
            "Copyright (c) 2001-2024 The FreeCAD project association. All rights reserved.",
        )],
    );
    let blender = mac.app("Blender", "org.blenderfoundation.blender", "4.2");
    signed(&mac, &blender, &devid("Stichting Blender Foundation"));
    let chatgpt = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("ChatGPT", "com.openai.chat", "1.2024")
    });
    // App Store: Apple signs it, the copyright names the developer.
    signed(&mac, &chatgpt, MAS);
    plist_keys(&chatgpt, &[("NSHumanReadableCopyright", "© 2024 OpenAI.")]);
    let live = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Live Home 3D", "com.belightsoft.LiveHome3D", "4.9")
    });
    signed(&mac, &live, MAS);
    // Old style: only CFBundleGetInfoString.
    plist_keys(
        &live,
        &[(
            "CFBundleGetInfoString",
            "Live Home 3D 4.9, Copyright 2024 BeLight Software, Ltd.",
        )],
    );
    // Nothing to read: no signature answer, no copyright.
    let bare = mac.app("Bare", "co.x.bare", "1.0");

    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(get(&list, &goland).vendor, "JetBrains s.r.o.");
    assert_eq!(get(&list, &frog).vendor, "Screaming Frog Ltd");
    assert_eq!(
        get(&list, &freecad).vendor,
        "The FreeCAD project association"
    );
    assert_eq!(get(&list, &blender).vendor, "Stichting Blender Foundation");
    assert_eq!(get(&list, &chatgpt).vendor, "OpenAI");
    assert_eq!(get(&list, &live).vendor, "BeLight Software, Ltd.");
    assert_eq!(get(&list, &bare).vendor, apps::UNKNOWN_DEVELOPER);
    for a in &list {
        assert!(!a.apple, "{} is not part of macOS", a.name);
        for bad in [
            "Co",
            "Jetbrains",
            "Freecad",
            "Openai",
            "Blenderfoundation",
            "Belightsoft",
        ] {
            assert_ne!(a.vendor, bad, "{}", a.name);
        }
    }

    // Every codesign ran with a time limit.
    let limits = mac.env.fake_sys().unwrap().with(|s| s.limits.clone());
    let signs: Vec<_> = limits
        .iter()
        .filter(|(l, _)| l.starts_with("/usr/bin/codesign -dv --verbose=2 "))
        .collect();
    assert_eq!(signs.len(), 7);
    assert!(signs.iter().all(|(_, t)| *t == apps::CODESIGN_LIMIT));
}

#[test]
fn quick_listing_reads_the_copyright_without_commands() {
    let mac = FakeMac::new();
    let app = mac.app("Editor", "com.example.Editor", "1.0");
    plist_keys(
        &app,
        &[("NSHumanReadableCopyright", "Copyright © 2024 Example GmbH")],
    );
    let list = apps::inventory(&mac.env, false, &|_| {});
    assert_eq!(get(&list, &app).vendor, "Example GmbH");
    assert!(mac.env.fake_sys().unwrap().calls().is_empty());
}

#[test]
fn a_slow_codesign_falls_back_to_the_copyright() {
    let mac = FakeMac::new();
    let a = mac.app("Alpha", "com.alpha.Alpha", "1.0");
    signed(&mac, &a, &devid("Alpha Labs Inc."));
    let b = mac.app("Beta", "com.beta.Beta", "1.0");
    signed(&mac, &b, &devid("Beta Corp"));
    plist_keys(
        &b,
        &[("NSHumanReadableCopyright", "© 2023 Beta Corporation")],
    );
    mac.env.fake_sys().unwrap().time_out(&format!(
        "/usr/bin/codesign -dv --verbose=2 {}",
        mac.env.display(&b)
    ));
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(get(&list, &a).vendor, "Alpha Labs Inc.");
    assert_eq!(get(&list, &b).vendor, "Beta Corporation");
}

#[test]
fn a_long_list_keeps_each_signature_with_its_app() {
    let mac = FakeMac::new();
    let mut apps_made: Vec<(PathBuf, String)> = Vec::new();
    for i in 0..120 {
        let name = format!("App {i:03}");
        let id = format!("com.maker{i}.app{i}");
        let p = mac.app(&name, &id, "1.0");
        let dev = format!("Maker Number {i} Ltd");
        signed(&mac, &p, &devid(&dev));
        apps_made.push((p, dev));
    }
    let t = std::time::Instant::now();
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert_eq!(list.len(), 120);
    for (p, dev) in &apps_made {
        assert_eq!(&get(&list, p).vendor, dev);
    }
    assert!(t.elapsed() < std::time::Duration::from_secs(20));
}

/// iMovie, GarageBand, Pages, Numbers and Keynote as Apple ships them: in /Applications, with
/// an App Store receipt and Apple's App Store signature. Safari: in /Applications, no receipt,
/// signed as macOS. Calculator: in /System/Applications.
struct AppleMac {
    mac: FakeMac,
    store_apps: Vec<PathBuf>,
    safari: PathBuf,
    calculator: PathBuf,
}

fn apple_mac() -> AppleMac {
    let mac = FakeMac::new();
    let mut store_apps = Vec::new();
    for (name, id) in [
        ("iMovie", "com.apple.iMovieApp"),
        ("GarageBand", "com.apple.garageband10"),
        ("Pages", "com.apple.iWork.Pages"),
        ("Numbers", "com.apple.iWork.Numbers"),
        ("Keynote", "com.apple.iWork.Keynote"),
    ] {
        let p = mac.app_spec(&AppSpec {
            mas: true,
            ..AppSpec::new(name, id, "10.4")
        });
        signed(&mac, &p, MAS);
        plist_keys(
            &p,
            &[(
                "NSHumanReadableCopyright",
                "Copyright © 2001–2024 Apple Inc. All rights reserved.",
            )],
        );
        store_apps.push(p);
    }
    let safari = mac.app("Safari", "com.apple.Safari", "18.0");
    let calculator = mac.app_spec(&AppSpec {
        dir: "/System/Applications",
        ..AppSpec::new("Calculator", "com.apple.calculator", "11.0")
    });
    AppleMac {
        mac,
        store_apps,
        safari,
        calculator,
    }
}

#[test]
fn apple_app_store_apps_are_not_part_of_macos() {
    let m = apple_mac();
    let list = apps::inventory(&m.mac.env, true, &|_| {});
    for p in &m.store_apps {
        let a = get(&list, p);
        assert!(
            !a.apple,
            "{} is an App Store app, not part of macOS",
            a.name
        );
        assert_eq!(a.store, Store::AppStore);
        assert_eq!(a.vendor, "Apple Inc.");
        assert!(uninstaller::app_lock(&m.mac.env, a).is_none(), "{}", a.name);
    }
    assert!(get(&list, &m.safari).apple, "Safari ships with macOS");
    assert_eq!(get(&list, &m.safari).vendor, "Apple");
    assert!(get(&list, &m.calculator).apple);
    assert_eq!(get(&list, &m.calculator).vendor, "Apple");
    assert!(uninstaller::app_lock(&m.mac.env, get(&list, &m.safari)).is_some());
    // The quick listing agrees without running anything.
    let quick = apps::inventory(&m.mac.env, false, &|_| {});
    for a in &quick {
        assert_eq!(a.apple, get(&list, &a.path).apple, "{}", a.name);
    }
    // apps in /System/Library/CoreServices are part of macOS.
    let finder = m
        .mac
        .env
        .sys_path("/System/Library/CoreServices/Finder.app");
    assert!(apps::in_system(&m.mac.env, &finder));
    assert!(!apps::in_system(&m.mac.env, &m.store_apps[0]));
}

#[test]
fn imovie_can_be_uninstalled_and_its_apple_named_files_stay() {
    let m = apple_mac();
    let env = &m.mac.env;
    let imovie = &m.store_apps[0];
    let container = m
        .mac
        .home_file("Library/Containers/com.apple.iMovieApp/Data/x", 10_000);
    let support = m
        .mac
        .home_file("Library/Application Support/iMovie/y", 5_000);
    let settings = Settings::default();
    let plan = uninstaller::uninstall_plan(env, &settings, imovie, false).unwrap();
    let bundle = plan.items.iter().find(|i| &i.path == imovie).unwrap();
    assert!(bundle.selected && bundle.locked.is_none());
    assert!(
        !plan.items.iter().any(|i| container.starts_with(&i.path)),
        "files named after an Apple id are never proposed"
    );
    let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert!(!imovie.exists());
    assert!(env.user_trash().join("iMovie.app").exists());
    assert!(container.exists());
    assert!(support.exists(), "name match is proposed but not selected");

    // Safari and Calculator stay locked.
    for p in [&m.safari, &m.calculator] {
        let plan = uninstaller::uninstall_plan(env, &settings, p, false).unwrap();
        assert!(plan.selected().next().is_none(), "{}", p.display());
    }
}
