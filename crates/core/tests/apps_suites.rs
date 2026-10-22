//! A1, A2, A5 on a copy of the user's Mac: a suite app nested in `/Applications/Autodesk`,
//! bundles embedded in apps, shared framework and macOS ids in the Library, and an app that is
//! really gone.

use std::path::{Path, PathBuf};

use cleancraft_core::apps;
use cleancraft_core::features::uninstaller::{self, CAT_APP, CAT_HIGH};
use cleancraft_core::features::{self, leftovers};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::{FeatureId, Settings};

pub const SUITE: &str = "/Applications/Autodesk/AutoCAD 2026";

struct Mac {
    mac: FakeMac,
    acad: PathBuf,
    remove: PathBuf,
    plot: PathBuf,
    editor: PathBuf,
}

/// The apps and Library of the user's Mac, as far as this fix is concerned.
fn mac() -> Mac {
    let mac = FakeMac::new();
    mac.spotlight_finds_nothing();
    let acad = mac.app_spec(&AppSpec {
        dir: SUITE,
        ..AppSpec::new("AutoCAD 2026", "com.autodesk.AutoCAD2026", "26.0")
    });
    let remove = mac.app_spec(&AppSpec {
        dir: SUITE,
        ..AppSpec::new(
            "Remove AutoCAD 2026",
            "com.autodesk.uninstaller.acad2026",
            "1.0",
        )
    });
    let plot = mac.app_spec(&AppSpec {
        dir: SUITE,
        ..AppSpec::new(
            "AutoCAD Plot Style Editor 2026",
            "com.autodesk.PlotStyleEditor2026",
            "26.0",
        )
    });
    // Quick Look extensions inside the AutoCAD bundle.
    for x in ["AcQuickLookPreviewer", "AcQuickLookThumbnailer"] {
        mac.embed_bundle(
            &acad,
            &format!("Contents/PlugIns/{x}.appex/Contents/Info.plist"),
            &format!("com.autodesk.AutoCAD2026.{x}"),
        );
    }
    // An app embedding bundles of another maker: a framework, an XPC service, a login item.
    let editor = mac.app("Editor", "com.example.Editor", "2.0");
    mac.embed_bundle(
        &editor,
        "Contents/Frameworks/EngineKit.framework/Versions/A/Resources/Info.plist",
        "com.thirdparty.EngineKit",
    );
    mac.embed_bundle(
        &editor,
        "Contents/Frameworks/EngineKit.framework/Resources/Info.plist",
        "com.thirdparty.EngineKit",
    );
    mac.embed_bundle(
        &editor,
        "Contents/XPCServices/Renderer.xpc/Contents/Info.plist",
        "net.renderco.Renderer",
    );
    mac.embed_bundle(
        &editor,
        "Contents/Library/LoginItems/Launcher.app/Contents/Info.plist",
        "io.launchy.Launcher",
    );
    mac.embed_bundle(
        &editor,
        "Contents/Library/QuickLook/Peek.qlgenerator/Contents/Info.plist",
        "org.peekers.Peek",
    );
    mac.app("FreeCAD", "org.freecad.FreeCAD", "1.0");
    // macFUSE is a settings pane, not an app.
    mac.embed_bundle(
        &mac.env
            .sys_path("/Library/PreferencePanes/macFUSE.prefPane"),
        "Contents/Info.plist",
        "io.macfuse.preferencepane",
    );
    Mac {
        mac,
        acad,
        remove,
        plot,
        editor,
    }
}

/// What the user's Mac had in the Library, none of which may be offered.
const NEVER: &[&str] = &[
    "Library/Containers/com.autodesk.AutoCAD2026.AcQuickLookPreviewer/Data/x",
    "Library/Containers/com.autodesk.AutoCAD2026.AcQuickLookThumbnailer/Data/x",
    "Library/Preferences/org.cups.PrintingPrefs.plist",
    "Library/Preferences/org.sparkle-project.Sparkle.Autoupdate.plist",
    "Library/Preferences/net.java.openjdk.java.plist",
    "Library/Preferences/com.freecad.FreeCAD.plist",
    "Library/Containers/io.macfuse.preferencepane/Data/x",
    "Library/Containers/com.github.osxfuse.OSXFUSEPrefPane/Data/x",
    "Library/Caches/com.crashlytics.data/x",
    "Library/Application Support/com.firebase.FIRInstallations/x",
    "Library/Application Support/com.google.firebase.crashlytics/x",
    "Library/Application Support/com.segment.storage.oai/x",
    "Library/Caches/org.python.python/x",
    "Library/Caches/io.sentry.cache/x",
    "Library/Application Support/org.mozilla.updater/x",
    "Library/Preferences/com.microsoft.autoupdate2.plist",
    "Library/Caches/com.thirdparty.EngineKit/x",
    "Library/Containers/net.renderco.Renderer/Data/x",
    "Library/Containers/io.launchy.Launcher/Data/x",
    "Library/Caches/org.peekers.Peek.extra/x",
    "Library/Preferences/com.apple.finder.plist",
];

fn scan(mac: &FakeMac) -> cleancraft_core::Plan {
    features::scan(&mac.env, &Settings::default(), FeatureId::Uninstaller).unwrap()
}

#[test]
fn nested_suite_app_is_listed_with_its_helpers_and_uninstaller() {
    let m = mac();
    // Utilities is not a suite, a folder 4 levels down is not searched, an app bundle is
    // never searched inside, and a link out of the app folders is not followed.
    let util = m.mac.app_spec(&AppSpec {
        dir: "/Applications/Utilities",
        ..AppSpec::new("Utilities", "com.tools.Utilities", "1.0")
    });
    let other_util = m.mac.app_spec(&AppSpec {
        dir: "/Applications/Utilities",
        ..AppSpec::new("Disk Tool", "com.tools.DiskTool", "1.0")
    });
    m.mac.app_spec(&AppSpec {
        dir: "/Applications/A/B/C",
        ..AppSpec::new("Deep", "com.deep.Deep", "1.0")
    });
    m.mac.app_spec(&AppSpec {
        dir: "/Applications/Editor.app/Contents/Applications",
        ..AppSpec::new("Inner", "com.example.Inner", "1.0")
    });
    let outside = m.mac.env.sys_path("/Volumes/Ext");
    m.mac.app_spec(&AppSpec {
        dir: "/Volumes/Ext",
        ..AppSpec::new("Outside", "com.outside.App", "1.0")
    });
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, m.mac.env.sys_path("/Applications/Linked")).unwrap();

    let list = apps::inventory(&m.mac.env, false, &|_| {});
    let get = |p: &Path| list.iter().find(|a| a.path == p).cloned();
    let acad = get(&m.acad).expect("AutoCAD 2026 listed");
    assert_eq!(acad.name, "AutoCAD 2026");
    assert_eq!(acad.part_of, None);
    assert_eq!(acad.vendor_uninstaller.as_deref(), Some(m.remove.as_path()));
    assert_eq!(
        apps::vendor_uninstaller_label(&acad).as_deref(),
        Some("Open Autodesk Uninstaller")
    );
    for helper in [&m.plot, &m.remove] {
        let h = get(helper).expect("helper listed");
        assert_eq!(h.part_of.as_deref(), Some(m.acad.as_path()), "{}", h.name);
        assert_eq!(h.vendor_uninstaller, None);
    }
    for u in [&util, &other_util] {
        let a = get(u).unwrap();
        assert_eq!(a.part_of, None);
        assert_eq!(a.vendor_uninstaller, None);
    }
    for gone in ["Deep", "Inner", "Outside"] {
        assert!(!list.iter().any(|a| a.name == gone), "{gone} listed");
    }
    assert!(get(&m.editor).is_some());
}

#[test]
fn uninstallers_by_name() {
    assert_eq!(
        apps::uninstaller_target("Remove AutoCAD 2026").as_deref(),
        Some("autocad 2026")
    );
    assert_eq!(
        apps::uninstaller_target("Uninstall Foo").as_deref(),
        Some("foo")
    );
    assert_eq!(
        apps::uninstaller_target("Foo Uninstaller").as_deref(),
        Some("foo")
    );
    assert_eq!(apps::uninstaller_target("Uninstaller").as_deref(), Some(""));
    assert_eq!(apps::uninstaller_target("Removal Tool"), None);
    assert_eq!(apps::uninstaller_target("AutoCAD 2026"), None);
    // A top-level "Uninstall Foo.app" goes with Foo only.
    let mac = FakeMac::new();
    let foo = mac.app("Foo", "com.foo.Foo", "1");
    let un = mac.app("Uninstall Foo", "com.foo.Uninstaller", "1");
    let bar = mac.app("Bar", "com.bar.Bar", "1");
    let list = apps::inventory(&mac.env, false, &|_| {});
    let get = |p: &Path| list.iter().find(|a| a.path == p).cloned().unwrap();
    assert_eq!(get(&foo).vendor_uninstaller.as_deref(), Some(un.as_path()));
    assert_eq!(get(&bar).vendor_uninstaller, None);
    assert_eq!(get(&un).part_of, None);
}

#[test]
fn embedded_bundles_count_as_installed() {
    let m = mac();
    let list = apps::inventory(&m.mac.env, false, &|_| {});
    let ids = apps::installed_ids(&m.mac.env, &list);
    for id in [
        "com.autodesk.autocad2026",
        "com.autodesk.autocad2026.acquicklookpreviewer",
        "com.autodesk.autocad2026.acquicklookthumbnailer",
        "com.thirdparty.enginekit",
        "net.renderco.renderer",
        "io.launchy.launcher",
        "org.peekers.peek",
        "io.macfuse.preferencepane",
    ] {
        assert!(ids.contains(id), "{id} missing from {ids:?}");
    }
}

#[test]
fn shared_and_installed_ids_are_never_offered_and_a_gone_app_still_is() {
    let m = mac();
    for rel in NEVER {
        m.mac.home_file(rel, 4_000);
    }
    // A really removed app: a container and settings (sure), and one with a lone cache
    // (medium).
    let sure = [
        m.mac
            .home_file("Library/Containers/com.gone.Writer/Data/db", 30_000),
        m.mac
            .home_file("Library/Preferences/com.gone.Writer.plist", 1_000),
    ];
    let maybe = m.mac.home_file("Library/Caches/com.solo.Painter/c", 9_000);
    let plan = scan(&m.mac);
    let orphans: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.category == leftovers::CAT_ORPHANS)
        .collect();
    for rel in NEVER {
        let p = m.mac.env.home_path(rel);
        assert!(
            !orphans.iter().any(|i| p.starts_with(&i.path)),
            "{rel} offered: {orphans:#?}"
        );
    }
    assert_eq!(orphans.len(), 3, "{orphans:#?}");
    assert!(orphans.iter().all(|i| !i.selected));
    for p in &sure {
        let it = orphans.iter().find(|i| p.starts_with(&i.path)).unwrap();
        assert!(
            it.reason
                .ends_with("of com.gone.Writer, an app that is no longer installed"),
            "{}",
            it.reason
        );
        assert!(!it.reason.contains("Possibly"));
    }
    let it = orphans.iter().find(|i| maybe.starts_with(&i.path)).unwrap();
    assert_eq!(
        it.reason,
        "Cache of com.solo.Painter. Possibly from an app that is gone"
    );
    // Spotlight was never asked about a denied or installed id.
    let calls = m.mac.env.fake_sys().unwrap().calls();
    assert!(!calls.iter().any(|c| c.contains("org.cups")));
    assert!(!calls.iter().any(|c| c.contains("AcQuickLook")));
}

#[test]
fn suite_uninstall_takes_its_helpers_and_extensions_data() {
    let m = mac();
    let cont = m.mac.home_file(
        "Library/Containers/com.autodesk.AutoCAD2026.AcQuickLookPreviewer/Data/x",
        4_000,
    );
    let plan =
        uninstaller::uninstall_plan(&m.mac.env, &Settings::default(), &m.acad, false).unwrap();
    let get = |p: &Path| plan.items.iter().find(|i| p.starts_with(&i.path)).cloned();
    assert_eq!(get(&m.acad).unwrap().category, CAT_APP);
    for h in [&m.plot, &m.remove] {
        let it = get(h).expect("helper in the plan");
        assert_eq!(it.category, CAT_HIGH);
        assert!(it.selected);
        assert!(
            it.reason.starts_with("App of the same suite: "),
            "{}",
            it.reason
        );
    }
    let c = get(&cont).expect("container of its extension");
    assert!(c.selected);
    // Carried out: everything selected moves, and the plan passes the run-time checks.
    let digest = plan.digest();
    let rep = features::run_plan(
        &m.mac.env,
        &Settings::default(),
        &plan,
        &digest,
        &mut |_, _| {},
    )
    .unwrap();
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    for p in [&m.acad, &m.plot, &m.remove, &cont] {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    // Chosen together in a batch, a helper goes with its suite's app (listed once).
    let m = mac();
    let (bp, batch) = uninstaller::batch_plan(
        &m.mac.env,
        &Settings::default(),
        &[m.plot.clone(), m.acad.clone()],
    )
    .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        bp.items.iter().filter(|i| i.path == m.plot).count(),
        1,
        "helper listed once"
    );
}
