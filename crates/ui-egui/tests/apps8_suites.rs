//! App Remover on a copy of the user's Mac, by clicks: AutoCAD 2026 nested in
//! `/Applications/Autodesk` is listed with its helpers tied to it, its maker's uninstaller is
//! offered first (CleanCraft's own uninstall stays available after a question that says why),
//! its extensions' containers and shared framework or macOS ids are never offered as
//! leftovers, and a really removed app's leftovers still are.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::executor;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use common::*;
use egui_kittest::kittest::Queryable;

const SUITE: &str = "/Applications/Autodesk/AutoCAD 2026";

struct Mac {
    mac: FakeMac,
    acad: PathBuf,
    remove: PathBuf,
    plot: PathBuf,
    never: Vec<PathBuf>,
    gone: Vec<PathBuf>,
    maybe: PathBuf,
}

fn mac() -> Mac {
    let mac = FakeMac::new();
    mac.spotlight_finds_nothing();
    let app = |name: &str, id: &str| {
        mac.app_spec(&AppSpec {
            dir: SUITE,
            ..AppSpec::new(name, id, "26.0")
        })
    };
    let acad = app("AutoCAD 2026", "com.autodesk.AutoCAD2026");
    let remove = app("Remove AutoCAD 2026", "com.autodesk.uninstaller.acad2026");
    let plot = app(
        "AutoCAD Plot Style Editor 2026",
        "com.autodesk.PlotStyleEditor2026",
    );
    for x in ["AcQuickLookPreviewer", "AcQuickLookThumbnailer"] {
        mac.embed_bundle(
            &acad,
            &format!("Contents/PlugIns/{x}.appex/Contents/Info.plist"),
            &format!("com.autodesk.AutoCAD2026.{x}"),
        );
    }
    mac.app("FreeCAD", "org.freecad.FreeCAD", "1.0");
    let mut never: Vec<PathBuf> = [
        "Library/Containers/com.autodesk.AutoCAD2026.AcQuickLookThumbnailer/Data/x",
        "Library/Preferences/org.cups.PrintingPrefs.plist",
        "Library/Preferences/org.sparkle-project.Sparkle.Autoupdate.plist",
        "Library/Preferences/net.java.openjdk.java.plist",
        "Library/Preferences/com.freecad.FreeCAD.plist",
        "Library/Containers/io.macfuse.preferencepane/Data/x",
        "Library/Caches/com.crashlytics.data/x",
        "Library/Application Support/com.firebase.FIRInstallations/x",
        "Library/Application Support/com.segment.storage.oai/x",
        "Library/Caches/org.python.python/x",
    ]
    .iter()
    .map(|r| mac.home_file(r, 6_000))
    .collect();
    // The container of AutoCAD's own extension goes with AutoCAD, never as a leftover.
    never.push(mac.home_file(
        "Library/Containers/com.autodesk.AutoCAD2026.AcQuickLookPreviewer/Data/x",
        6_000,
    ));
    let gone = vec![
        mac.home_file("Library/Containers/com.gone.Writer/Data/db", 40_000),
        mac.home_file("Library/Preferences/com.gone.Writer.plist", 2_000),
    ];
    let maybe = mac.home_file("Library/Caches/com.solo.Painter/c", 9_000);
    Mac {
        mac,
        acad,
        remove,
        plot,
        never,
        gone,
        maybe,
    }
}

/// The top Library entry a file is in (what the scan lists).
fn top(m: &Mac, p: &Path) -> PathBuf {
    let lib = m.mac.env.home_path("Library");
    let rel = p.strip_prefix(&lib).unwrap();
    let mut c = rel.components();
    lib.join(c.next().unwrap()).join(c.next().unwrap())
}

fn shown(m: &Mac, p: &Path) -> String {
    m.mac.env.display(p)
}

fn click_contains(h: &mut H, needle: &str) {
    let n = h.query_all_by_label_contains(needle).count();
    assert_eq!(n, 1, "'{needle}' matches {n} widgets");
    h.get_by_label_contains(needle).click();
    steps(h);
}

fn remover(m: &Mac) -> H {
    let mut h = harness(&m.mac);
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    h
}

#[test]
fn suite_app_is_listed_with_its_helpers_tied_to_it() {
    let m = mac();
    let h = remover(&m);
    for n in [
        "AutoCAD 2026",
        "AutoCAD Plot Style Editor 2026",
        "Remove AutoCAD 2026",
    ] {
        assert!(has(&h, n), "{n} not listed");
    }
    assert_eq!(h.query_all_by_label("Part of AutoCAD 2026").count(), 2);
    assert!(has(&h, &human_size(tree_size(&m.acad).0)));
}

#[test]
fn leftovers_group_offers_only_the_removed_apps() {
    let m = mac();
    let mut h = remover(&m);
    let listed: Vec<PathBuf> = vec![top(&m, &m.gone[0]), m.gone[1].clone(), top(&m, &m.maybe)];
    let size: u64 = listed.iter().map(|p| tree_size(p).0).sum();
    assert!(
        has_text(&h, &format!("Leftovers  3  ·  {}", human_size(size))),
        "leftover count and size"
    );
    click_contains(&mut h, "Leftovers  ");
    for p in &listed {
        assert!(has(&h, &shown(&m, p)), "{} not offered", p.display());
    }
    for p in &m.never {
        let t = if p.starts_with(m.mac.env.home_path("Library/Preferences")) {
            p.clone()
        } else {
            top(&m, p)
        };
        assert!(!has(&h, &shown(&m, &t)), "{} offered", t.display());
    }
    assert!(has_text(&h, "Possibly from an app that is gone"));
    // Never ticked for you: pick them, they move, undo puts them back.
    assert!(has_text(&h, "0 selected"), "nothing preselected");
    for p in &listed {
        click(&mut h, &shown(&m, p));
    }
    assert!(has(&h, &format!("3 selected, {}", human_size(size))));
    click(&mut h, "Remove Selected");
    click(&mut h, "Move to Trash");
    for p in &listed {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    for p in &m.never {
        assert!(p.exists(), "{} moved", p.display());
    }
    assert_eq!(executor::history(&m.mac.env).len(), 3);
}

#[test]
fn vendor_uninstaller_is_recommended_and_ours_still_works() {
    let m = mac();
    let mut h = remover(&m);
    click(&mut h, "AutoCAD 2026");
    assert!(has_text(
        &h,
        "AutoCAD 2026 comes with its own uninstaller, Remove AutoCAD 2026."
    ));
    click(&mut h, "Open Autodesk Uninstaller");
    let open = format!("/usr/bin/open {}", shown(&m, &m.remove));
    assert!(
        m.mac.env.fake_sys().unwrap().spawned().contains(&open),
        "{:?}",
        m.mac.env.fake_sys().unwrap().spawned()
    );
    assert!(has_text(
        &h,
        "Remove AutoCAD 2026 is open. When it finishes, click Scan Again"
    ));
    // The helpers and the extension's container are in the plan, selected.
    let cont = top(&m, m.never.last().unwrap());
    for p in [&m.plot, &m.remove, &cont] {
        assert!(has(&h, &shown(&m, p)), "{} not in the plan", p.display());
    }
    // Uninstall asks first which uninstaller to use, and says why.
    click(&mut h, "Uninstall");
    assert!(has_text(&h, "Use Remove AutoCAD 2026 instead?"));
    assert!(has_text(&h, "licensing services"));
    click(&mut h, "Cancel");
    assert!(!has_text(&h, "Use Remove AutoCAD 2026 instead?"));
    assert!(m.acad.exists());
    click(&mut h, "Uninstall");
    click(&mut h, "Uninstall with CleanCraft");
    assert!(has_text(&h, "Uninstall AutoCAD 2026? "));
    click(&mut h, "Move to Trash");
    for p in [&m.acad, &m.plot, &m.remove, &cont] {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    click(&mut h, "Undo");
    for p in [&m.acad, &m.plot, &m.remove, &cont] {
        assert!(p.exists(), "{} not put back", p.display());
    }
}
