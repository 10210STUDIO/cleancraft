//! fix7: renders of the pages touched in round 7 at 1280 x 820, written to
//! `docs/screens/v2/r7/` with CLEANCRAFT_SNAPSHOTS=1 (CLEANCRAFT_SNAPSHOT_ONLY=<part of a
//! name> narrows it). Without the variable nothing is rendered.

use std::path::{Path, PathBuf};

use cleancraft_core::health::MapSummary;
use cleancraft_core::permissions::Access;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::testkit_health as th;
use cleancraft_core::testkit_trash::{answer_account_names, erase_call, mail_trash};
use cleancraft_core::{Fault, FeatureId};
use cleancraft_ui_egui::CleanApp;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness(mac: &FakeMac, fda: Access) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = fda;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
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
    for _ in 0..10 {
        h.step();
    }
}

fn click(h: &mut H, label: &str) {
    let n = h
        .get_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("{label}"));
    n.scroll_to_me();
    settle(h);
    h.get_all_by_label(label).next().unwrap().click();
    settle(h);
}

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().unwrap();
    for k in ["/usr/bin/open", "open", "sh", "launchctl"] {
        f.answer(k, "");
    }
    f.answer("id -u", "501\n");
}

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

fn health_mac() -> FakeMac {
    let m = th::troubled_mac_now();
    base(&m);
    let home = m.env.home().to_path_buf();
    let mk = |at, movies, dev| MapSummary {
        at,
        root: home.clone(),
        entries: vec![
            (home.clone(), 100 * th::GB),
            (home.join("Movies"), movies),
            (home.join("Library/Developer"), dev),
        ],
    };
    std::fs::write(
        m.env.data_dir().join("disk-map-history.json"),
        serde_json::to_vec(&vec![
            mk(th::now() - 86_400 * 6, th::GB, 4 * th::GB),
            mk(th::now(), 9 * th::GB, 11 * th::GB),
        ])
        .unwrap(),
    )
    .unwrap();
    m
}

fn dev_empty_mac() -> FakeMac {
    let m = FakeMac::new();
    base(&m);
    std::fs::create_dir_all(m.env.home_path("Projects")).unwrap();
    m
}

fn dev_mac() -> FakeMac {
    let m = FakeMac::new();
    base(&m);
    let p = m.env.home_path("Projects");
    for (name, days) in [("alpha", 90), ("gamma", 0)] {
        let d = p.join(name);
        m.write(&d.join("Cargo.toml"), b"[package]\n");
        m.write(&d.join("src/main.rs"), b"fn main() {}\n");
        m.write(&d.join("target/debug/app"), &vec![1u8; 3_000_000]);
        age_tree(&d, days);
    }
    m
}

fn monitor_mac() -> FakeMac {
    let mac = FakeMac::new();
    let f = mac.env.fake_sys().unwrap();
    cleancraft_core::testkit_monitor::answer_all(f);
    f.answer(
        "ps",
        "  512  71.0 812000 /Applications/Brave Browser.app/Contents/Frameworks/Brave Browser Framework.framework/Helpers/Brave Browser Helper (Renderer).app/Contents/MacOS/Brave Browser Helper (Renderer)\n  801  47.0  50000 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer\n  930  38.0 600000 /Applications/Docker.app/Contents/MacOS/com.docker.backend\n  900  23.0  40000 /Applications/CleanCraft.app/Contents/MacOS/CleanCraft\n  950  12.0  40000 /opt/homebrew/bin/node\n",
    );
    mac
}

const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1s1 488245288 300000000 188245288 62% /
/dev/disk5s1 1000000 400000 600000 40% /Volumes/My Backup
";
const DISKUTIL: &str = r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>AllDisks</key><array><string>disk5</string><string>disk5s1</string></array><key>AllDisksAndPartitions</key><array><dict><key>DeviceIdentifier</key><string>disk5</string><key>Partitions</key><array><dict><key>DeviceIdentifier</key><string>disk5s1</string><key>MountPoint</key><string>/Volumes/My Backup</string><key>VolumeName</key><string>My Backup</string></dict></array></dict></array></dict></plist>"#;

fn map_mac() -> FakeMac {
    let m = FakeMac::new();
    base(&m);
    m.home_file("Movies/film.mov", 2_000_000);
    m.sys_file("/Volumes/My Backup/Movies/old.mov", 700_000);
    m.sys_file("/Volumes/My Backup/Archive 2019/disk.img", 900_000);
    m.sys_file("/Volumes/My Backup/Archive 2019/notes.txt", 90_000);
    let f = m.env.fake_sys().unwrap();
    f.answer("/bin/df -Pk", DF);
    f.answer("/usr/sbin/diskutil list -plist external", DISKUTIL);
    m
}

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";

fn trash_mac() -> FakeMac {
    let m = FakeMac::new();
    base(&m);
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    m.home_file(".Trash/old report.pdf", 1_400_000);
    mail_trash(&m, UUID, "Deleted Messages", 4, 30_000);
    mail_trash(&m, UUID, "Trash", 1, 20_000);
    answer_account_names(&m.env, &format!("{UUID}|iCloud|name@icloud.com\n"));
    m.env.fake_sys().unwrap().answer_full(
        &erase_call(Some("iCloud")),
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr:
                "System Events got an error: osascript is not allowed assistive access. (-1719)"
                    .into(),
        },
    );
    m
}

fn store_mac() -> (FakeMac, PathBuf) {
    let m = FakeMac::new();
    base(&m);
    let app = m.app_spec(&AppSpec {
        name: "Numbers",
        bundle_id: "com.apple.iWork.Numbers",
        version: "14.0",
        build: "1",
        dir: "/Applications",
        mas: true,
        feed: None,
        size: 200_000,
        binary_plist: false,
    });
    m.env.inject_fault(&app, Fault::PermissionDenied);
    m.home_file("Library/Containers/com.apple.iWork.Numbers/Data/x", 40_000);
    m.spotlight_finds_nothing();
    m.env.fake_sys().unwrap().fail_admin_move(&app);
    (m, app)
}

#[allow(clippy::type_complexity)]
fn scenes() -> Vec<(&'static str, Box<dyn Fn() -> (FakeMac, H)>)> {
    vec![
        (
            "first-launch-welcome",
            Box::new(|| {
                let m = th::ai_mac();
                base(&m);
                let mut h = harness(&m, Access::Missing);
                h.state_mut().launch_checks();
                settle(&mut h);
                (m, h)
            }),
        ),
        (
            "first-launch-full-disk-access",
            Box::new(|| {
                let m = th::ai_mac();
                base(&m);
                m.home_file("Library/Safari/Bookmarks.plist", 10);
                m.env.inject_fault(
                    &m.env.home_path("Library/Safari/Bookmarks.plist"),
                    Fault::PermissionDenied,
                );
                let mut h = harness(&m, Access::Missing);
                h.state_mut().launch_checks();
                settle(&mut h);
                click(&mut h, "Continue");
                click(&mut h, "How to turn it on");
                (m, h)
            }),
        ),
        (
            "first-launch-ai-apps",
            Box::new(|| {
                let m = th::ai_mac();
                base(&m);
                let mut h = harness(&m, Access::Granted);
                h.state_mut().launch_checks();
                settle(&mut h);
                click(&mut h, "Continue");
                (m, h)
            }),
        ),
        (
            "quick-sweep-start-chips",
            Box::new(|| {
                let m = th::troubled_mac_now();
                base(&m);
                let mut h = harness(&m, Access::Granted);
                h.state_mut().launch_checks();
                settle(&mut h);
                click(&mut h, "App Updates");
                (m, h)
            }),
        ),
        (
            "mac-health",
            Box::new(|| {
                let m = health_mac();
                let mut h = harness(&m, Access::Granted);
                click(&mut h, "Live Monitor");
                click(&mut h, "Mac Health");
                (m, h)
            }),
        ),
        (
            "live-monitor-processes",
            Box::new(|| {
                let m = monitor_mac();
                let mut h = harness(&m, Access::Granted);
                click(&mut h, "Live Monitor");
                (m, h)
            }),
        ),
        (
            "developer-clutter-nothing-found",
            Box::new(|| {
                let m = dev_empty_mac();
                let mut h = harness(&m, Access::Granted);
                for l in ["Clean", "Developer Clutter", "Scan"] {
                    click(&mut h, l);
                }
                (m, h)
            }),
        ),
        (
            "developer-clutter-rows",
            Box::new(|| {
                let m = dev_mac();
                let mut h = harness(&m, Access::Granted);
                for l in ["Clean", "Developer Clutter", "Scan"] {
                    click(&mut h, l);
                }
                (m, h)
            }),
        ),
        (
            "disk-map-external-top-folder",
            Box::new(|| {
                let m = map_mac();
                let mut h = harness(&m, Access::Granted);
                for l in ["Files", "Disk Map", "Home folder"] {
                    click(&mut h, l);
                }
                let label = h
                    .root()
                    .children_recursive()
                    .filter_map(|n| egui_kittest::kittest::NodeT::accesskit_node(&n).label())
                    .find(|l| l.starts_with("My Backup"))
                    .unwrap();
                click(&mut h, &label);
                click(&mut h, "Map My Backup");
                click(&mut h, "Select Archive 2019");
                (m, h)
            }),
        ),
        (
            "app-remover-kept-app",
            Box::new(|| {
                let (m, _) = store_mac();
                let mut h = harness(&m, Access::Granted);
                for l in [
                    "Apps",
                    "App Remover",
                    "Numbers",
                    "Uninstall",
                    "Move to Trash",
                ] {
                    click(&mut h, l);
                }
                (m, h)
            }),
        ),
        (
            "trash-bins-try-again",
            Box::new(|| {
                let m = trash_mac();
                let mut h = harness(&m, Access::Granted);
                for l in [
                    "Clean",
                    "Trash Bins",
                    "Scan",
                    "Erase in Mail... Mail trash, iCloud (name@icloud.com)",
                    "Erase in Mail",
                    "Open Accessibility Settings",
                ] {
                    click(&mut h, l);
                }
                (m, h)
            }),
        ),
        (
            "settings-menu-bar",
            Box::new(|| {
                let m = FakeMac::new();
                base(&m);
                let mut h = harness(&m, Access::Missing);
                click(&mut h, "Settings");
                click(
                    &mut h,
                    cleancraft_ui_egui::pages::settings::TRASH_OFFER_SWITCH,
                );
                (m, h)
            }),
        ),
        (
            "quick-sweep-done-skipped",
            Box::new(|| {
                let m = cleancraft_core::testkit_monitor::sweep_mac();
                base(&m);
                let mut h = harness(&m, Access::Granted);
                h.state_mut().sweep.sources =
                    Some(cleancraft_core::testkit_monitor::sweep_sources());
                settle(&mut h);
                click(&mut h, "Scan");
                let b = m.env.home_path("Library/Mail Downloads/b.zip");
                m.env.fake_sys().unwrap().set_open(&b);
                click(&mut h, "Review and Clean");
                click(&mut h, "Move to Trash");
                let _ = FeatureId::SmartCare;
                (m, h)
            }),
        ),
    ]
}

#[test]
fn round7_pages_render() {
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
    for (name, make) in scenes() {
        if !name.contains(only.as_str()) {
            continue;
        }
        let (_m, mut h) = make();
        settle(&mut h);
        let img = h.render().expect("render");
        let file = dir.join(format!("{name}-1280x820.png"));
        img.save(&file).expect("save png");
        eprintln!("wrote {}", file.display());
    }
}
