//! Look and layout of every page, at the smallest supported window (1060 x 680) and a large
//! one (1280 x 820):
//! - no text uses a character Inter cannot draw (no empty boxes where icons should be);
//! - every control and every text sits inside the window, so nothing is clipped off;
//! - the pages render (wgpu, software rasterizer when there is no GPU).
//!
//! `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_render -- --nocapture`
//! also writes a PNG of every scene to `docs/screens/v1/` for review.

mod common;

use std::path::PathBuf;

use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::testkit_files::{ImageFormat, save_scene};
use cleancraft_core::{FeatureId, Settings, testkit_cleanup, testkit_monitor};
use cleancraft_core::testkit_perf as tp;
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::{plan_page, sweep};
use egui::epaint::Shape;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

const SIZES: [(f32, f32); 2] = [(1280.0, 820.0), (1060.0, 680.0)];

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
}

fn harness(mac: &FakeMac, w: f32, h: f32, ppp: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = Access::Granted;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(ppp)
        .build_ui_state(
            |ui, app: &mut CleanApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                // The whole window, like eframe gives it (the harness adds a margin).
                let mut root = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("window"),
                    egui::UiBuilder::new()
                        .layer_id(egui::LayerId::background())
                        .max_rect(ctx.viewport_rect()),
                );
                app.ui(&mut root);
            },
            app,
        );
    settle(&mut h);
    h
}

fn settle(h: &mut H) {
    for _ in 0..6 {
        h.step();
    }
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

fn files_mac() -> FakeMac {
    let m = FakeMac::sample();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&m.env)
    .expect("save settings");
    m.home_file("Downloads/big.zip", 2_500_000);
    let old = m.home_file("Downloads/old.pdf", 1_200_000);
    age(&old, 400);
    m.home_file("Movies/clip.mov", 3_000_000);
    let dup: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(7919) >> 3) as u8)
        .collect();
    for p in ["Downloads/a.bin", "Music/old/a.bin", "Projects/a.bin"] {
        m.write(&m.env.home_path(p), &dup);
    }
    let pics = m.env.home_path("Pictures");
    save_scene(&pics.join("a.png"), 320, 240, 0, 0, 1, ImageFormat::Png);
    save_scene(&pics.join("a-bright.png"), 400, 300, 0, 5, 3, ImageFormat::Png);
    m.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    base(&m);
    m
}

fn remover_mac() -> FakeMac {
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        size: 3_000_000,
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    let viewer = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Viewer", "com.maker.Viewer", "1.0")
    });
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.last_opened(&editor, 2);
    mac.last_opened(&viewer, 300);
    mac.spotlight_finds_nothing();
    mac.home_file("Library/Application Support/com.example.Editor/db.sqlite", 400_000);
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    base(&mac);
    mac
}

fn monitor_mac() -> FakeMac {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    base(&m);
    m
}

const WAIT_PLACE: &str =
    "~/Library/Caches/com.apple.Safari/WebKitCache/Version 16/Records/9F2A6C1D/Resource";

/// One screen to check: a name, the Mac behind it, and how to reach it.
struct Scene {
    name: &'static str,
    mac: fn() -> FakeMac,
    go: fn(&mut H),
}

fn nav(h: &mut H, page: &str) {
    h.state_mut().navigate(page);
    settle(h);
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "quick-sweep-idle",
            mac: testkit_monitor::sweep_mac,
            go: |h| {
                h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
            },
        },
        Scene {
            name: "quick-sweep-scanning",
            mac: testkit_monitor::sweep_mac,
            go: |h| {
                h.state_mut().sweep.phase = sweep::Phase::Scanning(Job::waiting("System Junk"));
                settle(h);
            },
        },
        Scene {
            name: "quick-sweep-results",
            mac: testkit_monitor::sweep_mac,
            go: |h| {
                h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
                click(h, "Scan");
            },
        },
        Scene {
            name: "quick-sweep-confirm",
            mac: testkit_monitor::sweep_mac,
            go: |h| {
                h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
                click(h, "Scan");
                click(h, "Run");
            },
        },
        Scene {
            name: "quick-sweep-done",
            mac: testkit_monitor::sweep_mac,
            go: |h| {
                h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
                click(h, "Scan");
                click(h, "Run");
                click(h, "Move to Trash");
            },
        },
        Scene {
            name: "system-junk-idle",
            mac: testkit_cleanup::junk_mac,
            go: |h| nav(h, "system_junk"),
        },
        Scene {
            name: "system-junk-scanning",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                nav(h, "system_junk");
                h.state_mut().plan_state(FeatureId::SystemJunk).phase =
                    plan_page::Phase::Scanning(Job::waiting(WAIT_PLACE));
                settle(h);
            },
        },
        Scene {
            name: "system-junk-results",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                nav(h, "system_junk");
                click(h, "Scan");
            },
        },
        Scene {
            name: "system-junk-confirm",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                nav(h, "system_junk");
                click(h, "Scan");
                click(h, "Review and Clean");
            },
        },
        Scene {
            name: "system-junk-done",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                nav(h, "system_junk");
                click(h, "Scan");
                click(h, "Review and Clean");
                click(h, "Move to Trash");
            },
        },
        Scene {
            name: "mail-downloads-idle",
            mac: testkit_cleanup::mail_mac,
            go: |h| nav(h, "mail_attachments"),
        },
        Scene {
            name: "trash-bins-results",
            mac: testkit_cleanup::trash_mac,
            go: |h| {
                nav(h, "trash_bins");
                click(h, "Scan");
            },
        },
        Scene {
            name: "threat-check-idle",
            mac: FakeMac::sample,
            go: |h| nav(h, "malware"),
        },
        Scene {
            name: "privacy-traces-results",
            mac: testkit_cleanup::privacy_mac,
            go: |h| {
                nav(h, "privacy");
                click(h, "Scan");
            },
        },
        Scene {
            name: "tune-up-tasks",
            mac: || {
                let m = FakeMac::new();
                let f = m.env.fake_sys().unwrap();
                f.answer("id -u", "501\n");
                f.answer("sw_vers -productVersion", "14.6.1\n");
                tp::mail_index(&m, 300_000);
                base(&m);
                m
            },
            go: |h| nav(h, "maintenance"),
        },
        Scene {
            name: "startup-items",
            mac: tp::startup_mac,
            go: |h| nav(h, "login_items"),
        },
        Scene {
            name: "app-remover",
            mac: remover_mac,
            go: |h| nav(h, "uninstaller"),
        },
        Scene {
            name: "app-remover-detail",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                click(h, "Editor");
            },
        },
        Scene {
            name: "app-updates-idle",
            mac: FakeMac::new,
            go: |h| nav(h, "updater"),
        },
        Scene {
            name: "large-old-files-idle",
            mac: files_mac,
            go: |h| nav(h, "large_old_files"),
        },
        Scene {
            name: "duplicates-results",
            mac: files_mac,
            go: |h| {
                nav(h, "duplicates");
                click(h, "Scan");
            },
        },
        Scene {
            name: "similar-images-idle",
            mac: files_mac,
            go: |h| nav(h, "similar_photos"),
        },
        Scene {
            name: "disk-map-idle",
            mac: files_mac,
            go: |h| nav(h, "disk_map"),
        },
        Scene {
            name: "disk-map",
            mac: files_mac,
            go: |h| {
                nav(h, "disk_map");
                click(h, "Map Home Folder");
            },
        },
        Scene {
            name: "live-monitor",
            mac: monitor_mac,
            go: |h| {
                nav(h, "monitor");
                for _ in 0..3 {
                    h.state_mut().monitor.interval = std::time::Duration::ZERO;
                    settle(h);
                }
                h.state_mut().monitor.interval = std::time::Duration::from_secs(3600);
                settle(h);
            },
        },
        Scene {
            name: "history",
            mac: || {
                let m = FakeMac::sample();
                base(&m);
                let s = Settings::default();
                let plan = cleancraft_core::features::scan(&m.env, &s, FeatureId::SystemJunk)
                    .expect("scan");
                cleancraft_core::features::run_plan(&m.env, &s, &plan, &plan.digest(), &mut |_, _| {})
                    .expect("clean");
                m
            },
            go: |h| nav(h, "history"),
        },
        Scene {
            name: "settings",
            mac: || {
                let m = FakeMac::new();
                Settings {
                    ignore: vec![m.env.home_path("Library/Caches/com.example.Keep")],
                    ..Settings::default()
                }
                .save(&m.env)
                .expect("save");
                m
            },
            go: |h| nav(h, "settings"),
        },
        Scene {
            name: "full-disk-access-banner",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                h.state_mut().fda = Access::Missing;
                nav(h, "system_junk");
            },
        },
    ]
}

/// Every character on screen with a font family that cannot draw it.
fn missing_glyphs(h: &H, inter: &egui::Context) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<(String, egui::FontId)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                for sec in &t.galley.job.sections {
                    let text = t.galley.job.text[sec.byte_range.start.0..sec.byte_range.end.0].to_string();
                    out.push((text, sec.format.font_id.clone()));
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut texts);
    }
    let mut bad = Vec::new();
    inter.fonts_mut(|f| {
        for (text, fid) in texts {
            // Monospace is egui's own font; CleanCraft never uses it for icons.
            if fid.family == egui::FontFamily::Monospace {
                continue;
            }
            for c in text.chars() {
                if c.is_whitespace() || c.is_control() {
                    continue;
                }
                // Inter's own character map (has_glyph would count the fallback box).
                if !f.fonts.font(&fid.family).characters().contains_key(&c) {
                    bad.push(format!("{c:?} (U+{:04X}) in {text:?}", c as u32));
                }
            }
        }
    });
    bad
}

/// A context whose fonts are Inter only: no fallback to egui's fonts.
fn inter_only() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(cleancraft_ui_egui::theme::inter_only_fonts());
    // Fonts set now are used from the next pass.
    for _ in 0..2 {
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
    }
    ctx
}

/// Text the window edge cuts off: the visible part of a text (inside its clip rectangle, so
/// rows scrolled out of a list do not count) that reaches past the window. Also text cut on
/// the left or right by its own clip rectangle (nothing scrolls sideways, so that is always
/// a layout bug).
fn outside(h: &H, w: f32, hgt: f32) -> Vec<String> {
    fn walk(s: &Shape, clip: egui::Rect, w: f32, hgt: f32, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, clip, w, hgt, out)),
            Shape::Text(t) => {
                let vis = s.visual_bounding_rect().intersect(clip);
                if vis.is_positive()
                    && (vis.min.x < -0.5 || vis.min.y < -0.5 || vis.max.x > w + 0.5 || vis.max.y > hgt + 0.5)
                {
                    out.push(format!("{:?} at {vis:?}", t.galley.job.text));
                }
                let full = s.visual_bounding_rect();
                let rows_visible = full.y_range().intersects(clip.y_range());
                if vis.is_positive()
                    && rows_visible
                    && (full.min.x < clip.min.x - 1.0 || full.max.x > clip.max.x + 1.0)
                {
                    out.push(format!(
                        "{:?} cut sideways: text {full:?}, clip {clip:?}",
                        t.galley.job.text
                    ));
                }
            }
            _ => {}
        }
    }
    let mut bad = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, cs.clip_rect, w, hgt, &mut bad);
    }
    bad
}

#[test]
fn no_page_uses_a_glyph_inter_cannot_draw_and_nothing_leaves_the_window() {
    let inter = inter_only();
    let mut problems = Vec::new();
    for (w, hgt) in SIZES {
        for s in scenes() {
            let mac = (s.mac)();
            let mut h = harness(&mac, w, hgt, 1.0);
            (s.go)(&mut h);
            for g in missing_glyphs(&h, &inter) {
                problems.push(format!("{} at {w}x{hgt}: missing glyph {g}", s.name));
            }
            for o in outside(&h, w, hgt) {
                problems.push(format!("{} at {w}x{hgt}: outside the window: {o}", s.name));
            }
            // Every sidebar entry is fully visible without scrolling.
            let mut names: Vec<&str> = FeatureId::ALL.iter().map(|f| f.title()).collect();
            names.extend(["History", "Settings"]);
            for n in names {
                if let Some(node) = h.query_all_by_label(n).next() {
                    let r = node.rect();
                    if r.max.y > hgt + 0.5 {
                        problems.push(format!("{} at {w}x{hgt}: sidebar row {n} at {r:?}", s.name));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn module_icons_are_shapes_not_text() {
    // Painting an icon never adds text.
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let t = cleancraft_ui_egui::theme::Tokens::dark();
        for f in FeatureId::ALL {
            let (icon, color) = cleancraft_ui_egui::kit::feature_look(&t, f);
            cleancraft_ui_egui::kit::badge(ui, color, icon, 32.0);
        }
        for icon in cleancraft_ui_egui::icons::Icon::ALL {
            let r = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover()).0;
            cleancraft_ui_egui::icons::paint(ui.painter(), r, icon, egui::Color32::WHITE);
        }
    });
    out.textures_delta.clear();
    let mut n_text = 0;
    let mut n_shapes = 0;
    for cs in &out.shapes {
        n_shapes += 1;
        if matches!(cs.shape, Shape::Text(_)) {
            n_text += 1;
        }
    }
    assert_eq!(n_text, 0, "icons must not be drawn with text");
    assert!(n_shapes > 100, "every icon draws shapes ({n_shapes})");
}

/// Render the pages. With `CLEANCRAFT_SNAPSHOTS=1` every scene is saved as a PNG at both sizes
/// (and the Quick Sweep start screen at 2x); otherwise a few scenes are rendered to check the
/// pipeline and that the content area is not an empty rectangle.
#[test]
fn pages_render() {
    let all = std::env::var("CLEANCRAFT_SNAPSHOTS").is_ok_and(|v| v == "1");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v1");
    if all {
        std::fs::create_dir_all(&out_dir).expect("make docs/screens/v1");
    }
    let picked = ["quick-sweep-idle", "system-junk-results"];
    let renderer_ok = std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_ok();
    if !renderer_ok {
        eprintln!("No wgpu adapter here: rendering skipped (the layout test above still runs).");
        return;
    }
    for (w, hgt) in SIZES {
        for s in scenes() {
            if !all && !picked.contains(&s.name) {
                continue;
            }
            let ppps: &[f32] = if all && s.name == "quick-sweep-idle" && w > 1200.0 {
                &[1.0, 2.0]
            } else {
                &[1.0]
            };
            for &ppp in ppps {
                let mac = (s.mac)();
                let mut h = harness(&mac, w, hgt, ppp);
                (s.go)(&mut h);
                settle(&mut h);
                let img = h.render().expect("render");
                assert_eq!(img.width(), (w * ppp) as u32);
                // The content area has a picture, not a flat fill: many distinct colours.
                let mut seen = std::collections::HashSet::new();
                for y in (0..img.height()).step_by(7) {
                    for x in ((260.0 * ppp) as u32..img.width()).step_by(7) {
                        seen.insert(img.get_pixel(x, y).0);
                    }
                }
                assert!(seen.len() > 200, "{} looks flat ({} colours)", s.name, seen.len());
                if all {
                    let suffix = if ppp > 1.0 { "@2x" } else { "" };
                    let file = out_dir.join(format!("{}-{}x{}{suffix}.png", s.name, w, hgt));
                    img.save(&file).expect("save png");
                    eprintln!("wrote {}", file.display());
                }
            }
        }
    }
}
