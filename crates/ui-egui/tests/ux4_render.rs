//! ux4's pages rendered at 1060x680, 1280x820 and 1600x1000 (1x and 2x): App Remover (list,
//! detail, a running app, confirmation, done), App Updates, Startup Items, Tune-up Tasks,
//! Disk Map, Live Monitor, History and Settings.
//!
//! `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux4_render -- --nocapture`
//! writes the PNGs to `docs/screens/v2/` (`CLEANCRAFT_SNAPSHOT_DIR` elsewhere,
//! `CLEANCRAFT_SNAPSHOT_ONLY=app-remover` limits the scenes, `CLEANCRAFT_SNAPSHOT_1X=1` skips
//! 2x). Without it, a few scenes are rendered to check they are not empty, and every scene is
//! laid out at every size to check nothing leaves the window.

use std::path::PathBuf;

use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::testkit_perf as tp;
use cleancraft_core::{FeatureId, Settings, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use egui::epaint::Shape;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

const SIZES: [(f32, f32); 3] = [(1060.0, 680.0), (1280.0, 820.0), (1600.0, 1000.0)];

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

fn nav(h: &mut H, page: &str) {
    h.state_mut().navigate(page);
    settle(h);
}

/// Editor (with leftovers), Viewer (App Store, unused), Safari (part of macOS), and an app
/// with long names everywhere, like the one the user could not remove.
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
    let studio = mac.app_spec(&AppSpec {
        size: 4_500_000,
        ..AppSpec::new("PhotoStudio_Pro", "com.longmaker.PhotoStudioPro5", "5.7.2")
    });
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.last_opened(&editor, 2);
    mac.last_opened(&viewer, 300);
    mac.last_opened(&studio, 1);
    mac.spotlight_finds_nothing();
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Library/Logs/Editor/run.log", 5_000);
    for p in [
        "Library/Caches/com.longmaker.PhotoStudioPro5/data",
        "Library/Caches/com.longmaker.PhotoStudioPro5.Updater/data",
        "Library/Preferences/com.longmaker.PhotoStudioPro5.HealthMonitor.plist",
        "Library/Preferences/com.longmaker.PhotoStudioPro5.Menu.plist",
        "Library/Preferences/com.longmaker.PhotoStudioPro5.plist",
        "Library/Containers/com.longmaker.PhotoStudioPro5.AppIntentsExtension/Data/x",
        "Library/Containers/com.longmaker.PhotoStudioPro5.FinderSyncExtension/Data/x",
        "Library/Saved Application State/com.longmaker.PhotoStudioPro5.savedState/w.data",
    ] {
        mac.home_file(p, 40_000);
    }
    base(&mac);
    mac
}

fn running_mac() -> FakeMac {
    use cleancraft_core::testkit_quit::FakeMacQuit;
    let mac = remover_mac();
    let app = mac.env.sys_path("/Applications/PhotoStudio_Pro.app");
    mac.user_id(501);
    mac.run_program(&app.join("Contents/MacOS/PhotoStudio_Pro"), 4242, 501);
    let menu = mac.helper_app(
        &app,
        "Contents/Library/LoginItems/PhotoStudio Menu.app",
        "com.longmaker.PhotoStudioPro5.Menu",
    );
    mac.run_program(&menu, 4243, 501);
    mac
}

fn updates_mac() -> FakeMac {
    const CAST: &str = "https://example.com/editor/cast.xml";
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
        mas: true,
        ..AppSpec::new("Sketchbook", "com.maker.Sketchbook", "5.0")
    });
    mac.web(
        CAST,
        &appcast(&[("210", "2.1", "<ul><li>Faster search</li></ul>")]),
    );
    mac.web(
        &cleancraft_core::updater::lookup_url("com.maker.NotesPro"),
        r#"{"resultCount":1,"results":[{"version":"3.2","releaseNotes":"Sync fixes"}]}"#,
    );
    mac.web(
        &cleancraft_core::updater::lookup_url("com.maker.Sketchbook"),
        r#"{"resultCount":1,"results":[{"version":"5.0","releaseNotes":""}]}"#,
    );
    let fs = mac.env.fake_sys().unwrap();
    fs.answer(
        "/usr/sbin/softwareupdate --list",
        "* Label: macOS 15.1-24B83\n\tTitle: macOS Sequoia 15.1, Version: 15.1, Size: 2000000KiB, Recommended: YES, Action: restart,\n",
    );
    base(&mac);
    mac
}

fn files_mac() -> FakeMac {
    let m = FakeMac::sample();
    m.home_file("Downloads/big.zip", 2_500_000);
    m.home_file("Movies/clip.mov", 3_000_000);
    m.home_file("Projects/site/build/out.bin", 1_800_000);
    m.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    base(&m);
    m
}

fn monitor_mac() -> FakeMac {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    base(&m);
    m
}

fn history_mac() -> FakeMac {
    let m = FakeMac::sample();
    base(&m);
    let s = Settings::default();
    let plan = cleancraft_core::features::scan(&m.env, &s, FeatureId::SystemJunk).expect("scan");
    cleancraft_core::features::run_plan(&m.env, &s, &plan, &plan.digest(), &mut |_, _| {})
        .expect("clean");
    m
}

struct Scene {
    name: &'static str,
    mac: fn() -> FakeMac,
    go: fn(&mut H),
}

fn scenes() -> Vec<Scene> {
    vec![
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
            name: "app-remover-detail-long",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                click(h, "PhotoStudio_Pro");
            },
        },
        Scene {
            name: "app-remover-running",
            mac: running_mac,
            go: |h| {
                nav(h, "uninstaller");
                click(h, "PhotoStudio_Pro");
            },
        },
        Scene {
            name: "app-remover-system-app",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                // Apps of macOS are listed on request (lists7).
                click(h, "Show apps that are part of macOS");
                click(h, "Safari");
            },
        },
        Scene {
            name: "app-remover-confirm",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                click(h, "PhotoStudio_Pro");
                click(h, "Uninstall");
            },
        },
        Scene {
            name: "app-remover-done",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                click(h, "Editor");
                click(h, "Uninstall");
                click(h, "Move to Trash");
            },
        },
        Scene {
            name: "app-remover-leftovers",
            mac: remover_mac,
            go: |h| {
                nav(h, "uninstaller");
                h.get_all_by_label_contains("Leftovers")
                    .next()
                    .expect("Leftovers group")
                    .click();
                settle(h);
            },
        },
        Scene {
            name: "app-updates-idle",
            mac: FakeMac::new,
            go: |h| nav(h, "updater"),
        },
        Scene {
            name: "app-updates-results",
            mac: updates_mac,
            go: |h| {
                nav(h, "updater");
                click(h, "Check for Updates");
            },
        },
        Scene {
            name: "startup-items",
            mac: tp::startup_mac,
            go: |h| nav(h, "login_items"),
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
            mac: history_mac,
            go: |h| nav(h, "history"),
        },
        Scene {
            name: "history-empty",
            mac: || {
                let m = FakeMac::new();
                base(&m);
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
    ]
}

/// Texts and controls painted outside the window (cut off).
fn outside(h: &H, w: f32, hgt: f32) -> Vec<String> {
    fn walk(s: &Shape, clip: egui::Rect, w: f32, hgt: f32, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, w, hgt, out)),
            Shape::Text(t) => {
                let r = t.visual_bounding_rect().intersect(clip);
                if r.is_positive()
                    && (r.max.x > w + 0.5 || r.max.y > hgt + 0.5 || r.min.x < -0.5)
                    && !t.galley.text().trim().is_empty()
                {
                    out.push(t.galley.text().to_string());
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        walk(&c.shape, c.clip_rect, w, hgt, &mut out);
    }
    out
}

/// Every scene lays out at every size with no text cut off by the window edge.
#[test]
fn ux4_pages_fit_every_size() {
    for (w, hgt) in SIZES {
        for s in scenes() {
            let mac = (s.mac)();
            let mut h = harness(&mac, w, hgt, 1.0);
            (s.go)(&mut h);
            settle(&mut h);
            let out = outside(&h, w, hgt);
            assert!(out.is_empty(), "{} at {w}x{hgt}: outside {out:?}", s.name);
        }
    }
}

#[test]
fn ux4_pages_render() {
    let all = std::env::var("CLEANCRAFT_SNAPSHOTS").is_ok_and(|v| v == "1");
    let only = std::env::var("CLEANCRAFT_SNAPSHOT_ONLY").unwrap_or_default();
    let one = std::env::var("CLEANCRAFT_SNAPSHOT_1X").is_ok_and(|v| v == "1");
    let out_dir = std::env::var("CLEANCRAFT_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2")
        });
    if all {
        std::fs::create_dir_all(&out_dir).expect("make the snapshot folder");
    }
    let picked = ["app-remover-running", "history"];
    if std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_err() {
        eprintln!("No wgpu adapter here: rendering skipped.");
        return;
    }
    for (w, hgt) in SIZES {
        for s in scenes() {
            if !all && !picked.contains(&s.name) {
                continue;
            }
            if all && !s.name.contains(only.as_str()) {
                continue;
            }
            let ppps: &[f32] = if all && !one { &[1.0, 2.0] } else { &[1.0] };
            for &ppp in ppps {
                let mac = (s.mac)();
                let mut h = harness(&mac, w, hgt, ppp);
                (s.go)(&mut h);
                settle(&mut h);
                let img = h.render().expect("render");
                let mut seen = std::collections::HashSet::new();
                for y in (0..img.height()).step_by(7) {
                    for x in ((260.0 * ppp) as u32..img.width()).step_by(7) {
                        seen.insert(img.get_pixel(x, y).0);
                    }
                }
                assert!(seen.len() > 200, "{} looks flat", s.name);
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
