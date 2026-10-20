//! Real app icons and file icons on screen (images.rs, file_icons.rs): the placeholder tile
//! while the icon loads, the real icon once it is ready (checked on rendered pixels), failures
//! keep the placeholder, no tool ever runs inside a frame, and every page that shows apps uses
//! it: App Remover list, detail and confirmation, App Updates, Startup Items.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::app_icons;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::testkit_icons::{FakeMacIcons, IconKind, png_bytes};
use cleancraft_ui_egui::file_icons::FileKind;
use cleancraft_ui_egui::images::{self, Mode, Shown};
use common::*;
use egui_kittest::kittest::Queryable;

/// The icon colour the scripted `sips` writes (testkit_icons).
const SIPS_RGB: [u8; 3] = [40, 120, 220];

struct IconMac {
    mac: FakeMac,
    editor: PathBuf,
    notes: PathBuf,
}

/// Editor ships an .icns (sips), Notes Pro only an asset catalog (Quick Look).
fn icon_mac() -> IconMac {
    let mac = FakeMac::new();
    let editor = mac.icon_app("Editor", "com.example.Editor", IconKind::Icns("AppIcon"));
    let notes = mac.icon_app("Notes Pro", "com.maker.NotesPro", IconKind::AssetCatalog);
    mac.last_opened(&editor, 2);
    mac.spotlight_finds_nothing();
    for px in app_icons::SIZES {
        let icns = editor.join("Contents/Resources/AppIcon.icns");
        let out = app_icons::app_icon_path(&mac.env, &editor, px).unwrap();
        mac.sips_writes(&icns, &out, px);
        mac.quicklook_writes(&notes, px);
    }
    IconMac { mac, editor, notes }
}

fn tool_calls(mac: &FakeMac) -> usize {
    mac.env
        .fake_sys()
        .unwrap()
        .calls()
        .iter()
        .filter(|c| {
            c.starts_with("/usr/bin/sips")
                || c.starts_with("/usr/bin/qlmanage")
                || c.starts_with("/usr/bin/osascript -l JavaScript")
        })
        .count()
}

/// The pixel at the centre of the icon tile of the row labelled `name` (list rows put the
/// 38 pt tile 28 pt from the row's left edge).
fn tile_pixel(h: &mut H, name: &str) -> [u8; 3] {
    let r = h.get_by_label(name).rect();
    // The icon sits after the row's checkbox (apps6: multi-select), centred 62 pt in.
    let (x, y) = ((r.left() + 60.0) as u32, r.center().y as u32);
    let img = h.render().expect("render");
    let p = img.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 6)
}

#[test]
fn app_remover_shows_the_placeholder_then_the_real_icon() {
    let m = icon_mac();
    let mut h = harness(&m.mac);
    let ctx = h.ctx.clone();
    images::install(&ctx, &m.mac.env, Mode::Manual);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");

    // Before the work ran: the initial tile, the requests queued, no tool run in a frame.
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Placeholder));
    assert_eq!(images::shown(&ctx, &m.notes), Some(Shown::Placeholder));
    assert!(images::queued(&ctx) >= 2);
    assert_eq!(tool_calls(&m.mac), 0, "the frame never runs icon tools");
    let before = tile_pixel(&mut h, "Editor");
    assert!(!close(before, SIPS_RGB), "placeholder looks like the icon");

    // The background work: sips for Editor, Quick Look for Notes Pro.
    assert!(images::pump(&ctx) >= 2);
    for _ in 0..30 {
        h.step(); // past the fade-in
    }
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Image));
    assert_eq!(images::shown(&ctx, &m.notes), Some(Shown::Image));
    assert_eq!(images::textures(&ctx), 2);
    let after = tile_pixel(&mut h, "Editor");
    assert!(close(after, SIPS_RGB), "icon not painted: {after:?}");
    let calls = m.mac.env.fake_sys().unwrap().calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("/usr/bin/sips -s format png -Z 64"))
    );
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("/usr/bin/qlmanage -t -s 64"))
    );

    // The PNGs are cached: a new loader (next launch) runs no tool.
    let n = tool_calls(&m.mac);
    images::install(&ctx, &m.mac.env, Mode::Sync);
    steps(&mut h);
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Image));
    assert_eq!(tool_calls(&m.mac), n);
}

#[test]
fn a_failed_icon_keeps_the_placeholder_and_is_asked_once() {
    let mac = FakeMac::new();
    let app = mac.icon_app("Broken", "com.example.Broken", IconKind::AssetCatalog);
    mac.quicklook_silent(&app, 64);
    mac.env
        .fake_sys()
        .unwrap()
        .answer("/usr/bin/osascript", "failed\n");
    mac.spotlight_finds_nothing();
    let mut h = harness(&mac);
    let ctx = h.ctx.clone();
    images::install(&ctx, &mac.env, Mode::Sync);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert_eq!(images::shown(&ctx, &app), Some(Shown::Placeholder));
    let n = tool_calls(&mac);
    assert!(n >= 2, "Quick Look then AppKit were tried");
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(tool_calls(&mac), n, "not asked again every frame");
    assert!(has(&h, "Broken"), "the row is still there");
}

#[test]
fn detail_confirmation_and_opened_text_use_the_app() {
    let m = icon_mac();
    let mut h = harness(&m.mac);
    let ctx = h.ctx.clone();
    images::install(&ctx, &m.mac.env, Mode::Sync);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert!(has_text(&h, "Opened: 2 days ago"));
    assert!(has_text(&h, "Opened: No record"), "Notes Pro has no date");
    click(&mut h, "Editor");
    assert!(has(&h, "Back to Apps"));
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Image));
    click(&mut h, "Uninstall");
    assert!(has(&h, "Move to Trash"));
    assert!(has_text(&h, "Uninstall Editor?"));
    // The dialog painted the icon this frame (it is the only Editor icon on top).
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Image));
    click(&mut h, "Cancel");
}

#[test]
fn app_updates_rows_show_real_icons() {
    let mac = FakeMac::new();
    const CAST: &str = "https://example.com/editor/cast.xml";
    let editor = mac.app_spec(&AppSpec {
        build: "200",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.web(CAST, &appcast(&[("210", "2.1", "<p>Faster</p>")]));
    let fs = mac.env.fake_sys().unwrap();
    fs.answer("/usr/sbin/softwareupdate --list", "");
    // No .icns, no asset catalog: Quick Look gives the generic icon.
    mac.quicklook_writes(&editor, 64);
    let mut h = harness(&mac);
    let ctx = h.ctx.clone();
    images::install(&ctx, &mac.env, Mode::Sync);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Updates");
    click(&mut h, "Check for Updates");
    assert!(has(&h, "2.0 -> 2.1"));
    assert_eq!(images::shown(&ctx, &editor), Some(Shown::Image));
}

#[test]
fn startup_items_show_the_app_behind_login_items_and_agents() {
    let m = cleancraft_core::testkit_perf::startup_mac();
    // A login item (LOGIN_ITEMS_SAMPLE names /Applications/Dropbox.app) with an .icns.
    let dropbox = m.icon_app(
        "Dropbox",
        "com.getdropbox.dropbox",
        IconKind::Icns("AppIcon"),
    );
    let icns = dropbox.join("Contents/Resources/AppIcon.icns");
    for px in app_icons::SIZES {
        let out = app_icons::app_icon_path(&m.env, &dropbox, px).unwrap();
        m.sips_writes(&icns, &out, px);
    }
    let mut h = harness(&m);
    let ctx = h.ctx.clone();
    images::install(&ctx, &m.env, Mode::Sync);
    // The tool sits in the "Speed" section of the sidebar.
    click(&mut h, "Speed");
    click(&mut h, "Startup Items");
    assert!(h.state().startup.data.is_some());
    assert_eq!(images::shown(&ctx, &dropbox), Some(Shown::Image));
    // Agents whose program lives inside an app show that app.
    let owners: Vec<PathBuf> = h
        .state()
        .startup
        .data
        .as_ref()
        .unwrap()
        .launch
        .iter()
        .filter_map(|i| i.program.as_deref())
        .filter_map(|p| cleancraft_core::sys::app_bundle_of(Path::new(p)))
        .map(|a| cleancraft_core::apps::resolve(&m.env, &a.to_string_lossy()))
        .collect();
    assert!(!owners.is_empty());
    assert!(
        owners.iter().any(|o| images::shown(&ctx, o).is_some()),
        "no agent row painted its app's icon slot: {owners:?}"
    );
}

/// A one-widget harness for the icon API itself.
fn api_harness(mac: &FakeMac, mode: Mode, draw: fn(&mut egui::Ui, &Path)) -> H2 {
    let env = mac.env.clone();
    let dir = mac.home();
    let mut installed = false;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            move |ui, _s: &mut ()| {
                if !installed {
                    // Fonts first (the placeholder tile uses the bold face): usable next pass.
                    cleancraft_ui_egui::theme::install_fonts(ui.ctx());
                    images::install(ui.ctx(), &env, mode);
                    installed = true;
                    return;
                }
                draw(ui, &dir);
            },
            (),
        );
    h.step();
    h.step();
    h
}

type H2 = egui_kittest::Harness<'static, ()>;

#[test]
fn file_icons_show_previews_for_images_and_drawn_kinds_for_the_rest() {
    let mac = FakeMac::new();
    mac.write(
        &mac.home().join("Pictures/beach.png"),
        &png_bytes(300, 200, [200, 30, 30, 255]),
    );
    mac.home_file("Library/Logs/app.log", 10);
    mac.home_file("Downloads/tools.zip", 10);
    let h = api_harness(&mac, Mode::Sync, |ui, home| {
        for rel in [
            "Pictures/beach.png",
            "Library/Logs/app.log",
            "Downloads/tools.zip",
            "Library/Caches",
        ] {
            let p = home.join(rel);
            images::file_icon(ui, &p, FileKind::of(&p), 32.0);
        }
    });
    let ctx = h.ctx.clone();
    let home = mac.home();
    assert_eq!(
        images::shown(&ctx, &home.join("Pictures/beach.png")),
        Some(Shown::Image)
    );
    // Kinds without a preview never queue work and never get a texture.
    assert_eq!(
        images::shown(&ctx, &home.join("Library/Logs/app.log")),
        None
    );
    assert_eq!(images::textures(&ctx), 1);
    assert!(
        mac.env.fake_sys().unwrap().calls().is_empty(),
        "PNG needs no tool"
    );
}

#[test]
fn thousands_of_rows_queue_a_bounded_amount_of_work() {
    let mac = FakeMac::new();
    let dir = mac.home().join("Pictures/many");
    for i in 0..300 {
        mac.write(&dir.join(format!("{i:04}.jpg")), b"jpeg");
    }
    let h = api_harness(&mac, Mode::Manual, |ui, home| {
        // A list of 300 rows where all 300 ask: the queue keeps only the newest.
        for i in 0..300 {
            let p = home.join(format!("Pictures/many/{i:04}.jpg"));
            images::paint_file_icon(
                ui,
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(16.0, 16.0)),
                &p,
                FileKind::Image,
            );
        }
    });
    let ctx = h.ctx.clone();
    assert!(images::queued(&ctx) <= images::QUEUE_MAX);
    assert!(images::queued(&ctx) > 0);
    // Broken JPEGs: the work fails, the drawn icon stays.
    images::pump(&ctx);
    assert_eq!(images::textures(&ctx), 0);
}

#[test]
fn background_workers_load_without_blocking_the_frame() {
    let m = icon_mac();
    let editor = m.editor.clone();
    let mut h = api_harness(&m.mac, Mode::Threads(2), |ui, home| {
        let app = home
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("Applications/Editor.app");
        images::app_icon(ui, &app, 38.0);
    });
    let ctx = h.ctx.clone();
    let start = std::time::Instant::now();
    while images::shown(&ctx, &editor) != Some(Shown::Image) {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "workers never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.step();
    }
    assert_eq!(images::textures(&ctx), 1);
}

#[test]
fn without_a_loader_everything_is_a_placeholder() {
    // UI tests that do not install one (all the older ones) never run icon tools.
    let m = icon_mac();
    let mut h = harness(&m.mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert_eq!(images::shown(&h.ctx, &m.editor), None);
    assert_eq!(tool_calls(&m.mac), 0);
}

/// `CLEANCRAFT_SNAPSHOTS=1`: renders App Remover with real icons and the file-kind gallery,
/// at 1x and 2x, into docs/screens/v2/ for review. Always checks they render.
#[test]
fn icons_render() {
    let save = std::env::var("CLEANCRAFT_SNAPSHOTS").is_ok_and(|v| v == "1");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2");
    if save {
        std::fs::create_dir_all(&out_dir).unwrap();
    }
    for ppp in [1.0f32, 2.0] {
        let mac = FakeMac::new();
        let colors = [
            ("Editor", [90, 160, 255], [40, 70, 200]),
            ("Notes Pro", [255, 210, 90], [235, 140, 30]),
            ("Mixer", [250, 110, 150], [180, 40, 110]),
            ("Atlas", [90, 220, 160], [20, 140, 100]),
        ];
        for (name, a, b) in colors {
            let app = mac.icon_app(
                name,
                &format!("com.example.{}", name.replace(' ', "")),
                IconKind::Icns("AppIcon"),
            );
            let icns = app.join("Contents/Resources/AppIcon.icns");
            for px in app_icons::SIZES {
                let out = app_icons::app_icon_path(&mac.env, &app, px).unwrap();
                mac.env.fake_sys().unwrap().answer_creating(
                    &cleancraft_core::testkit_icons::sips_line(&mac, &icns, &out, px),
                    "",
                    &out,
                    &cleancraft_core::testkit_icons::fancy_icon(px, a, b),
                );
            }
        }
        mac.app("Plain", "com.example.Plain", "1.0"); // no icon: placeholder tile
        mac.spotlight_finds_nothing();
        let app = cleancraft_ui_egui::CleanApp::new(mac.env.clone()).with_sync();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1060.0, 680.0))
            .with_pixels_per_point(ppp)
            .build_ui_state(
                |ui, app: &mut cleancraft_ui_egui::CleanApp| {
                    app.logic(&ui.ctx().clone());
                    app.ui(ui);
                },
                app,
            );
        for _ in 0..4 {
            h.step();
        }
        images::install(&h.ctx.clone(), &mac.env, Mode::Sync);
        // App Remover sits in the "Apps" section of the sidebar.
        h.get_by_label("Apps").click();
        h.run();
        h.get_by_label("App Remover").click();
        for _ in 0..6 {
            h.step();
        }
        let img = h.render().expect("render");
        if save {
            img.save(out_dir.join(format!("icons-app-remover@{ppp}x.png")))
                .unwrap();
        }

        // The file-kind gallery at 16, 24, 32 and 64 points.
        let g = FakeMac::new();
        g.write(
            &g.home().join("Pictures/photo.png"),
            &cleancraft_core::testkit_icons::fancy_icon(128, [120, 190, 255], [250, 160, 90]),
        );
        let mut gh = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 300.0))
            .with_pixels_per_point(ppp)
            .build_ui_state(
                move |ui, n: &mut u32| {
                    *n += 1;
                    if *n == 1 {
                        cleancraft_ui_egui::theme::install_fonts(ui.ctx());
                        return;
                    }
                    ui.painter().rect_filled(
                        ui.max_rect(),
                        0.0,
                        egui::Color32::from_rgb(0x16, 0x1A, 0x26),
                    );
                    for size in [16.0, 24.0, 32.0, 64.0] {
                        ui.horizontal(|ui| {
                            for k in FileKind::ALL {
                                let probe = PathBuf::from(format!("/nowhere/x.{k:?}"));
                                images::file_icon(ui, &probe, k, size);
                            }
                        });
                    }
                },
                0u32,
            );
        for _ in 0..3 {
            gh.step();
        }
        let gi = gh.render().expect("render");
        if save {
            gi.save(out_dir.join(format!("icons-file-kinds@{ppp}x.png")))
                .unwrap();
        }
    }
}

/// A pixel inside the row's icon tile, off the centre (clear of an initial's letter), and the
/// row's own background just right of the tile.
fn tile_and_row_pixels(h: &mut H, name: &str) -> ([u8; 3], [u8; 3]) {
    let r = h.get_by_label(name).rect();
    // The icon sits after the row's checkbox (apps6: multi-select), centred 62 pt in.
    let (cx, cy) = (r.left() + 62.0, r.center().y);
    let img = h.render().expect("render");
    let ppp = h.ctx.pixels_per_point();
    let at = |x: f32, y: f32| {
        let p = img.get_pixel((x * ppp) as u32, (y * ppp) as u32).0;
        [p[0], p[1], p[2]]
    };
    (at(cx - 9.0, cy + 9.0), at(cx + 26.0, cy + 9.0))
}

/// How far a pixel is from grey.
fn spread(p: [u8; 3]) -> u8 {
    p.iter().max().unwrap() - p.iter().min().unwrap()
}

#[test]
fn the_loading_placeholder_is_a_neutral_tile_not_a_coloured_letter() {
    // REVIEW-A 44: while the icon loads, a quiet grey tile; no colour, no letter.
    let m = icon_mac();
    let mut h = harness(&m.mac);
    let ctx = h.ctx.clone();
    images::install(&ctx, &m.mac.env, Mode::Manual);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert_eq!(images::shown(&ctx, &m.editor), Some(Shown::Placeholder));
    for name in ["Editor", "Notes Pro"] {
        let (tile, row) = tile_and_row_pixels(&mut h, name);
        assert!(
            spread(tile) <= spread(row) + 3,
            "{name}: loading tile {tile:?} is coloured (row {row:?})"
        );
        assert!(
            tile.iter().zip(row).all(|(t, r)| *t > r),
            "{name}: loading tile {tile:?} is not visible on the row {row:?}"
        );
    }
}

#[test]
fn an_app_without_an_icon_keeps_its_initial_tile() {
    // When no tool can make an icon, the coloured initial tile tells apps apart.
    let mac = FakeMac::new();
    let app = mac.icon_app("Broken", "com.example.Broken", IconKind::AssetCatalog);
    mac.quicklook_silent(&app, 64);
    mac.env
        .fake_sys()
        .unwrap()
        .answer("/usr/bin/osascript", "failed\n");
    mac.spotlight_finds_nothing();
    let mut h = harness(&mac);
    images::install(&h.ctx.clone(), &mac.env, Mode::Sync);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    let (tile, row) = tile_and_row_pixels(&mut h, "Broken");
    assert!(
        spread(tile) > spread(row) + 20,
        "failed icon shows no initial tile: {tile:?} (row {row:?})"
    );
}

/// `codesign -dv --verbose=2 <app>` names `authority` (stderr, as codesign writes it).
fn signed(mac: &FakeMac, app: &Path, authority: &str) {
    mac.env.fake_sys().unwrap().answer_full(
        &format!("/usr/bin/codesign -dv --verbose=2 {}", mac.env.display(app)),
        cleancraft_core::sys::CmdOut {
            status: 0,
            stdout: String::new(),
            stderr: format!("Executable=x\n{authority}\nTeamIdentifier=ABCDE12345\n"),
        },
    );
}

fn add_copyright(app: &Path, text: &str) {
    cleancraft_core::testkit_icons::set_plist_strings(app, &[("NSHumanReadableCopyright", text)]);
}

/// REVIEW-A 45 on the App Remover list: the real developer names and "Part of macOS" only on
/// Safari and Calculator, never on Apple's App Store apps.
fn metadata_mac() -> FakeMac {
    let mac = FakeMac::new();
    let goland = mac.app("GoLand", "com.jetbrains.goland", "2024.2");
    signed(
        &mac,
        &goland,
        "Authority=Developer ID Application: JetBrains s.r.o. (2ZEFAR8TH3)",
    );
    let frog = mac.app(
        "Screaming Frog SEO Spider",
        "co.uk.screamingfrog.seo.spider",
        "20.0",
    );
    signed(
        &mac,
        &frog,
        "Authority=Developer ID Application: Screaming Frog Ltd (ABCDE12345)",
    );
    for (name, id) in [
        ("iMovie", "com.apple.iMovieApp"),
        ("GarageBand", "com.apple.garageband10"),
    ] {
        let p = mac.app_spec(&AppSpec {
            mas: true,
            ..AppSpec::new(name, id, "10.4")
        });
        signed(&mac, &p, "Authority=Apple Mac OS Application Signing");
        add_copyright(&p, "Copyright © 2001–2024 Apple Inc. All rights reserved.");
    }
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.app_spec(&AppSpec {
        dir: "/System/Applications",
        ..AppSpec::new("Calculator", "com.apple.calculator", "11.0")
    });
    mac.spotlight_finds_nothing();
    mac
}

#[test]
fn app_remover_shows_real_developers_and_part_of_macos_only_for_system_apps() {
    let mac = metadata_mac();
    let mut h = harness(&mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    // Apps of macOS are hidden until asked for (lists7).
    click(&mut h, "Show apps that are part of macOS");
    let row = |name: &str| h.get_by_label(name).rect();
    let info_in_row = |name: &str, dev: &str| {
        let r = row(name);
        h.query_all_by_label_contains(&format!("  ·  {dev}  ·  "))
            .any(|n| r.contains(n.rect().center()))
    };
    for (name, dev) in [
        ("GoLand", "JetBrains s.r.o."),
        ("Screaming Frog SEO Spider", "Screaming Frog Ltd"),
        ("iMovie", "Apple Inc."),
        ("GarageBand", "Apple Inc."),
        ("Safari", "Apple"),
        ("Calculator", "Apple"),
    ] {
        assert!(info_in_row(name, dev), "{name} should show {dev}");
    }
    for bad in ["Jetbrains", "Co", "Apple  ·  App Store"] {
        assert!(!has_text(&h, &format!("  ·  {bad}  ·  ")), "{bad} shown");
    }
    let tags: Vec<egui::Rect> = h
        .query_all_by_label("Part of macOS")
        .map(|n| n.rect())
        .collect();
    let rows_with_tag: Vec<&str> = [
        "GoLand",
        "Screaming Frog SEO Spider",
        "iMovie",
        "GarageBand",
        "Safari",
        "Calculator",
    ]
    .into_iter()
    .filter(|n| {
        let r = row(n);
        tags.iter().any(|t| r.contains(t.center()))
    })
    .collect();
    assert_eq!(rows_with_tag, ["Safari", "Calculator"]);
}

#[test]
fn metadata_render() {
    let save = std::env::var("CLEANCRAFT_SNAPSHOTS").is_ok_and(|v| v == "1");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2");
    for (w, hgt) in [(1060.0, 680.0), (1280.0, 820.0)] {
        for ppp in [1.0f32, 2.0] {
            // Icons on the way (Manual loader, never pumped): the neutral loading tiles.
            let mac = metadata_mac();
            let app = cleancraft_ui_egui::CleanApp::new(mac.env.clone()).with_sync();
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(w, hgt))
                .with_pixels_per_point(ppp)
                .build_ui_state(
                    |ui, app: &mut cleancraft_ui_egui::CleanApp| {
                        app.logic(&ui.ctx().clone());
                        app.ui(ui);
                    },
                    app,
                );
            for _ in 0..4 {
                h.step();
            }
            images::install(&h.ctx.clone(), &mac.env, Mode::Manual);
            // App Remover sits in the "Apps" section of the sidebar.
            h.get_by_label("Apps").click();
            h.run();
            h.get_by_label("App Remover").click();
            for _ in 0..6 {
                h.step();
            }
            let img = h.render().expect("render");
            if save {
                std::fs::create_dir_all(&out_dir).unwrap();
                img.save(out_dir.join(format!("icons3-app-remover-meta-{}@{ppp}x.png", w as u32)))
                    .unwrap();
            }
        }
    }
}
