//! lists7: renders of the pages with the shared list toolbar at 1280 x 820, written to
//! `docs/screens/v2/r7/` with CLEANCRAFT_SNAPSHOTS=1 (CLEANCRAFT_SNAPSHOT_ONLY=<part of a
//! name> narrows it). Without the variable nothing is rendered.

use std::path::PathBuf;

use cleancraft_core::executor::{self, ActionKind, ActionRecord, Method};
use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::{FeatureId, Settings, testkit_cleanup, testkit_monitor, testkit_perf};
use cleancraft_ui_egui::CleanApp;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness(mac: &FakeMac, w: f32, h: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = Access::Granted;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
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
    for _ in 0..8 {
        h.step();
    }
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

fn type_into(h: &mut H, label: &str, text: &str) {
    let n = h.get_by_label(label);
    n.focus();
    n.type_text(text);
    settle(h);
}

/// Pick a filter and close its pop-up.
fn filter(h: &mut H, entry: &str) {
    click(h, "Filter");
    click(h, entry);
    h.key_press(egui::Key::Escape);
    settle(h);
}

fn apps_mac() -> FakeMac {
    let mac = FakeMac::new();
    for (name, id, mas, dir, size, days) in [
        (
            "Editor",
            "com.example.Editor",
            false,
            "/Applications",
            3_000_000,
            2,
        ),
        (
            "Viewer",
            "com.maker.Viewer",
            true,
            "/Applications",
            40_000_000,
            300,
        ),
        (
            "Jotter",
            "org.jot.Jotter",
            false,
            "/Applications/Setapp",
            1_000_000,
            20,
        ),
        (
            "Sketch Pad",
            "com.pad.Sketch",
            true,
            "/Applications",
            9_000_000,
            400,
        ),
        (
            "Mailer",
            "com.post.Mailer",
            false,
            "/Applications",
            6_000_000,
            1,
        ),
    ] {
        let p = mac.app_spec(&AppSpec {
            mas,
            dir,
            size,
            ..AppSpec::new(name, id, "1.0")
        });
        mac.last_opened(&p, days);
    }
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.app("Calculator", "com.apple.calculator", "11.0");
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.spotlight_finds_nothing();
    mac
}

fn updates_mac() -> FakeMac {
    let mac = FakeMac::new();
    for (name, id, feed) in [
        ("Alpha", "com.a.Alpha", "https://a.example/cast.xml"),
        ("Beta", "com.b.Beta", "https://b.example/cast.xml"),
        ("Gamma", "com.g.Gamma", "https://g.example/cast.xml"),
    ] {
        mac.app_spec(&AppSpec {
            feed: Some(feed),
            ..AppSpec::new(name, id, "1.0")
        });
        mac.web(feed, &appcast(&[("2.0", "2.0", "<p>Fixes</p>")]));
    }
    mac.spotlight_finds_nothing();
    mac
}

fn large_old_mac() -> FakeMac {
    let m = FakeMac::new();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&m.env)
    .unwrap();
    for (p, size, days) in [
        ("Downloads/big.zip", 2_500_000, 0),
        ("Downloads/old.pdf", 1_200_000, 400),
        ("Movies/clip.mov", 3_000_000, 10),
        ("Music/song.wav", 2_200_000, 100),
        ("Pictures/raw.tif", 4_000_000, 60),
    ] {
        let f = m.home_file(p, size);
        if days > 0 {
            age(&f, days);
        }
    }
    m
}

fn map_mac() -> FakeMac {
    let mac = FakeMac::sample();
    mac.home_file("Movies/film.mov", 3_000_000);
    mac.home_file("Projects/app/src/main.rs", 40_000);
    mac.home_file("notes.txt", 20_000);
    mac.home_file("zeta.log", 10_000);
    mac.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    mac
}

fn history_mac() -> FakeMac {
    let mac = FakeMac::new();
    for (id, f, days, size, name) in [
        ("a", FeatureId::SystemJunk, 0, 1_000_000, "today.cache"),
        ("b", FeatureId::LargeOldFiles, 3, 9_000_000, "week.zip"),
        ("c", FeatureId::SystemJunk, 20, 5_000_000, "month.log"),
        ("d", FeatureId::Duplicates, 90, 2_000_000, "old.bin"),
    ] {
        let t = chrono::Utc::now().timestamp() - days * 86_400;
        executor::append_journal(
            &mac.env,
            &ActionRecord {
                id: id.into(),
                plan_id: format!("plan-{id}"),
                feature: Some(f),
                time: t,
                size,
                kind: ActionKind::Moved {
                    original: mac.env.home_path(&format!("Downloads/{name}")),
                    stored: mac.env.home_path(&format!(".Trash/{name}")),
                    method: Method::Trash,
                    stored_id: None,
                },
                undone: None,
            },
        )
        .unwrap();
    }
    mac
}

fn monitor_mac() -> FakeMac {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().expect("fake sys"));
    m
}

struct Scene {
    name: &'static str,
    mac: fn() -> FakeMac,
    go: fn(&mut H),
}

fn remover(h: &mut H) {
    click(h, "Apps");
    click(h, "App Remover");
    for _ in 0..10 {
        h.step();
    }
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "app-remover-toolbar",
            mac: apps_mac,
            go: remover,
        },
        Scene {
            name: "app-remover-filter-menu",
            mac: apps_mac,
            go: |h| {
                remover(h);
                click(h, "Filter");
                click(h, "App Store (2)");
            },
        },
        Scene {
            name: "app-remover-filtered-hidden-selected",
            mac: apps_mac,
            go: |h| {
                remover(h);
                click(h, "Select Editor");
                click(h, "Select Jotter");
                filter(h, "App Store (2)");
                filter(h, "Unused (2)");
                for _ in 0..20 {
                    h.step();
                }
            },
        },
        Scene {
            name: "app-remover-sort-menu",
            mac: apps_mac,
            go: |h| {
                remover(h);
                click(h, "Sort");
            },
        },
        Scene {
            name: "app-remover-macos-shown",
            mac: apps_mac,
            go: |h| {
                remover(h);
                click(h, "Show apps that are part of macOS");
            },
        },
        Scene {
            name: "app-updates-toolbar",
            mac: updates_mac,
            go: |h| {
                remover(h);
                click(h, "App Updates");
                click(h, "Check for Updates");
                type_into(h, "Search updates", "gam");
            },
        },
        Scene {
            name: "startup-items-filtered",
            mac: testkit_perf::startup_mac,
            go: |h| {
                h.state_mut().navigate(FeatureId::LoginItems.as_str());
                settle(h);
                click(h, "Filter");
            },
        },
        Scene {
            name: "system-junk-filtered",
            mac: testkit_cleanup::junk_mac,
            go: |h| {
                click(h, "Clean");
                click(h, "System Junk");
                click(h, "Scan");
                type_into(h, "Search", "example");
                let sel = h.state().plans[&FeatureId::SystemJunk]
                    .plan
                    .as_ref()
                    .map(|p| p.selected_count())
                    .unwrap_or(0);
                filter(h, &format!("Selected ({sel})"));
            },
        },
        Scene {
            name: "large-old-filter-menu",
            mac: large_old_mac,
            go: |h| {
                click(h, "Files");
                click(h, "Large & Old Files");
                click(h, "Scan");
                click(h, "Filter");
                click(h, "~/Downloads (2)");
            },
        },
        Scene {
            name: "disk-map-list-toolbar",
            mac: map_mac,
            go: |h| {
                click(h, "Files");
                click(h, "Disk Map");
                click(h, "Map Home Folder");
                click(h, "Select Movies");
                filter(h, "Files (2)");
            },
        },
        Scene {
            name: "history-filtered",
            mac: history_mac,
            go: |h| {
                h.state_mut().navigate("history");
                settle(h);
                filter(h, "System Junk (2)");
            },
        },
        Scene {
            name: "monitor-sort-menu",
            mac: monitor_mac,
            go: |h| {
                h.state_mut().navigate("monitor");
                for _ in 0..3 {
                    h.state_mut().monitor.interval = std::time::Duration::ZERO;
                    settle(h);
                }
                h.state_mut().monitor.interval = std::time::Duration::from_secs(3600);
                settle(h);
                h.get_all_by_label("Sort").next().expect("sort").click();
                settle(h);
            },
        },
    ]
}

#[test]
fn lists7_pages_render() {
    if std::env::var("CLEANCRAFT_SNAPSHOTS").is_err() {
        return;
    }
    if std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_err() {
        eprintln!("No wgpu adapter here: rendering skipped.");
        return;
    }
    let only = std::env::var("CLEANCRAFT_SNAPSHOT_ONLY").unwrap_or_default();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2/r7");
    std::fs::create_dir_all(&dir).unwrap();
    let (w, hh) = (1280.0, 820.0);
    for s in scenes() {
        if !s.name.contains(only.as_str()) {
            continue;
        }
        let m = (s.mac)();
        let mut h = harness(&m, w, hh);
        (s.go)(&mut h);
        settle(&mut h);
        let img = h.render().expect("render");
        let file = dir.join(format!("{}-{}x{}.png", s.name, w, hh));
        img.save(&file).expect("save png");
        eprintln!("wrote {}", file.display());
    }
}
