//! integ6: renders of the round 6 pages at 1280 x 820 and 1600 x 1000, written to
//! `docs/screens/v2/r6/` with CLEANCRAFT_SNAPSHOTS=1 (CLEANCRAFT_SNAPSHOT_ONLY=<part of a
//! name> narrows it). Without the variable nothing is rendered.

use std::path::{Path, PathBuf};

use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::testkit_trash::{answer_account_names, mail_trash};
use cleancraft_core::{Fault, Settings};
use cleancraft_ui_egui::CleanApp;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness(mac: &FakeMac, w: f32, h: f32, fda: Access) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = fda;
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

fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    settle(h);
}

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

fn dev_mac() -> FakeMac {
    let m = FakeMac::new();
    let p = m.env.home_path("Projects");
    for (name, days, rust) in [
        ("alpha", 90, true),
        ("beta", 120, false),
        ("gamma", 0, true),
    ] {
        let d = p.join(name);
        if rust {
            m.write(&d.join("Cargo.toml"), b"[package]\n");
            m.write(&d.join("src/main.rs"), b"fn main() {}\n");
            m.write(&d.join("target/debug/app"), &vec![1u8; 3_000_000]);
        } else {
            m.write(&d.join("package.json"), b"{}");
            m.write(&d.join("node_modules/a/index.js"), &vec![2u8; 1_200_000]);
        }
        age_tree(&d, days);
    }
    m
}

fn settings_mac() -> FakeMac {
    let m = FakeMac::new();
    let code = m.env.home_path("Work/code");
    std::fs::create_dir_all(&code).unwrap();
    Settings {
        dev_roots: vec![code],
        ..Default::default()
    }
    .save(&m.env)
    .unwrap();
    m
}

fn fda_missing_mac() -> FakeMac {
    let m = FakeMac::new();
    m.home_file("Library/Safari/Bookmarks.plist", 10);
    m.env.inject_fault(
        &m.env.home_path("Library/Safari/Bookmarks.plist"),
        Fault::PermissionDenied,
    );
    m
}

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";

fn trash_mac() -> FakeMac {
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    m.home_file(".Trash/old report.pdf", 1_400_000);
    mail_trash(&m, UUID, "Deleted Messages", 4, 30_000);
    mail_trash(&m, UUID, "Trash", 1, 20_000);
    answer_account_names(&m.env, &format!("{UUID}|iCloud|name@icloud.com\n"));
    m
}

fn apps_mac() -> FakeMac {
    let mac = FakeMac::new();
    let editor = mac.app("Editor", "com.example.Editor", "2.0");
    let viewer = mac.app("Viewer", "com.maker.Viewer", "1.0");
    mac.app("Jotter", "org.jot.Jotter", "3.1");
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.write(
        &viewer.join("Contents/Resources/r.bin"),
        &vec![1u8; 3_000_000],
    );
    mac.write(
        &editor.join("Contents/Resources/r.bin"),
        &vec![1u8; 2_000_000],
    );
    mac.spotlight_finds_nothing();
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Caches/com.maker.Viewer/c", 120_000);
    mac.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    mac
}

const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1s1 488245288 300000000 188245288 62% /
/dev/disk5s1 1000000 400000 600000 40% /Volumes/My Backup
";

const DISKUTIL: &str = r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>AllDisks</key><array><string>disk5</string><string>disk5s1</string></array><key>AllDisksAndPartitions</key><array><dict><key>DeviceIdentifier</key><string>disk5</string><key>Partitions</key><array><dict><key>DeviceIdentifier</key><string>disk5s1</string><key>MountPoint</key><string>/Volumes/My Backup</string><key>VolumeName</key><string>My Backup</string></dict></array></dict></array></dict></plist>"#;

fn map_mac() -> FakeMac {
    let m = FakeMac::new();
    m.home_file("Movies/film.mov", 2_000_000);
    m.sys_file("/Volumes/My Backup/Movies/old.mov", 700_000);
    let f = m.env.fake_sys().unwrap();
    f.answer("/bin/df -Pk", DF);
    f.answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 488245288 300000000 188245288 62% /\n",
    );
    f.answer("/usr/sbin/diskutil list -plist external", DISKUTIL);
    m
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
    m.home_file("Downloads/big.mov", 51_000_000);
    let old = m.home_file("Downloads/old.pdf", 3_000_000);
    age(&old, 400);
    let mid = m.home_file("Downloads/mid.zip", 2_500_000);
    age(&mid, 120);
    m.home_file("Movies/clip.mov", 4_000_000);
    m
}

struct Scene {
    name: &'static str,
    mac: fn() -> FakeMac,
    fda: Access,
    go: fn(&mut H),
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "mac-health",
            mac: cleancraft_core::testkit_health::troubled_mac_now,
            fda: Access::Granted,
            go: |h| {
                h.state_mut().navigate("health");
                settle(h);
            },
        },
        Scene {
            name: "developer-clutter",
            mac: dev_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Clean");
                click(h, "Developer Clutter");
                click(h, "Scan");
            },
        },
        Scene {
            name: "settings-developer-clutter",
            mac: settings_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Settings");
                reach(h, "Projects count as inactive after (days)");
            },
        },
        Scene {
            name: "settings-ai-agents",
            mac: settings_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Settings");
                reach(h, "Connect AI Apps...");
            },
        },
        Scene {
            name: "settings-full-disk-access",
            mac: fda_missing_mac,
            fda: Access::Missing,
            go: |h| {
                h.state_mut().navigate("full_disk_access");
                settle(h);
            },
        },
        Scene {
            name: "trash-bins",
            mac: trash_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Clean");
                click(h, "Trash Bins");
                click(h, "Scan");
            },
        },
        Scene {
            name: "app-remover-multi-select",
            mac: apps_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Apps");
                click(h, "App Remover");
                for n in ["Editor", "Viewer", "Jotter"] {
                    click(h, &format!("Select {n}"));
                }
                for _ in 0..20 {
                    h.step();
                }
            },
        },
        Scene {
            name: "disk-map-location-picker",
            mac: map_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Files");
                click(h, "Disk Map");
                click(h, "Home folder");
            },
        },
        Scene {
            name: "large-old-groups",
            mac: large_old_mac,
            fda: Access::Granted,
            go: |h| {
                click(h, "Files");
                click(h, "Large & Old Files");
                click(h, "Scan");
                click(h, "Group by: Kind");
                click(h, "Size");
            },
        },
        Scene {
            name: "welcome-connect-ai",
            mac: cleancraft_core::testkit_health::ai_mac,
            fda: Access::Granted,
            go: |h| {
                h.state_mut().navigate("connect_ai");
                settle(h);
            },
        },
    ]
}

#[test]
fn round6_pages_render() {
    if std::env::var("CLEANCRAFT_SNAPSHOTS").is_err() {
        return;
    }
    if std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_err() {
        eprintln!("No wgpu adapter here: rendering skipped.");
        return;
    }
    let only = std::env::var("CLEANCRAFT_SNAPSHOT_ONLY").unwrap_or_default();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2/r6");
    std::fs::create_dir_all(&dir).unwrap();
    for (w, hh) in [(1280.0, 820.0), (1600.0, 1000.0)] {
        for s in scenes() {
            if !s.name.contains(only.as_str()) {
                continue;
            }
            let m = (s.mac)();
            let mut h = harness(&m, w, hh, s.fda);
            (s.go)(&mut h);
            settle(&mut h);
            let img = h.render().expect("render");
            let file = dir.join(format!("{}-{}x{}.png", s.name, w, hh));
            img.save(&file).expect("save png");
            eprintln!("wrote {}", file.display());
        }
    }
}
