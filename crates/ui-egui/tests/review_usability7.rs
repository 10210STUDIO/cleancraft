//! Usability review of round 6 (docs/design/REVIEW-U7.md): the new features driven like a
//! first-time Mac user would, by clicks and keys on a fake Mac.
//!
//! Two kinds of tests:
//! - `task_*`: the walkthrough of one task. Passes today; prints every step with
//!   `cargo test -p cleancraft-ui-egui --test review_usability7 -- --ignored --nocapture task_`.
//! - `u7_##_*`: proof of one finding U7-##. All fixed by fix7 (their `#[ignore]` removed).
//!
//! With `CLEANCRAFT_REVIEW_U7_RENDERS=1` the tests write their evidence crops to
//! `docs/design/review-u7/`. With `CLEANCRAFT_REVIEW_U7_DUMP=1` every screen's texts and
//! buttons are printed too.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::notify;
use cleancraft_core::permissions::Access;
use cleancraft_core::route::AppRoute;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::testkit_health as th;
use cleancraft_core::testkit_quit::FakeMacQuit;
use cleancraft_core::testkit_trash::{answer_account_names, erase_call, mail_trash};
use cleancraft_core::{FeatureId, Settings};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::app::Page;
use egui_kittest::kittest::{NodeT, Queryable};

type H = egui_kittest::Harness<'static, CleanApp>;

const W: f32 = 1280.0;
const HGT: f32 = 820.0;

// ---------- harness and a step log ----------

fn harness(mac: &FakeMac) -> H {
    harness_fda(mac, Access::Granted)
}

fn harness_fda(mac: &FakeMac, fda: Access) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = fda;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(W, HGT))
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

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn has_text(h: &H, needle: &str) -> bool {
    h.query_all_by_label_contains(needle).next().is_some()
}

fn disabled(h: &H, label: &str) -> bool {
    h.get_all_by_label(label)
        .next()
        .is_some_and(|n| n.accesskit_node().is_disabled())
}

/// Every visible, enabled control label (what a user can press now).
fn buttons(h: &H) -> Vec<String> {
    use egui::accesskit::Role;
    let mut v: Vec<String> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            (!a.is_hidden()
                && !a.is_disabled()
                && matches!(
                    a.role(),
                    Role::Button
                        | Role::CheckBox
                        | Role::Switch
                        | Role::ComboBox
                        | Role::RadioButton
                ))
            .then(|| a.label().unwrap_or_default())
        })
        .filter(|l| !l.is_empty())
        .collect();
    v.dedup();
    v
}

/// Every visible text (labels of any role).
fn texts(h: &H) -> Vec<String> {
    let mut v: Vec<String> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            (!a.is_hidden()).then(|| a.label().or(a.value()).unwrap_or_default())
        })
        .filter(|l| !l.is_empty())
        .collect();
    v.dedup();
    v
}

fn dump(h: &H, tag: &str) {
    if std::env::var("CLEANCRAFT_REVIEW_U7_DUMP").is_ok_and(|v| v == "1") {
        eprintln!("----- {tag}: texts");
        for t in texts(h) {
            eprintln!("      | {t}");
        }
        eprintln!("----- {tag}: buttons {:?}", buttons(h));
    }
}

/// With `CLEANCRAFT_REVIEW_U7_RENDERS=1`, save the window (or a crop of it, in points) as
/// docs/design/review-u7/<name>.png.
fn snap(h: &mut H, name: &str, crop: Option<egui::Rect>) {
    if !std::env::var("CLEANCRAFT_REVIEW_U7_RENDERS").is_ok_and(|v| v == "1") {
        return;
    }
    if std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_err() {
        eprintln!("No wgpu adapter: {name} not rendered.");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/review-u7");
    std::fs::create_dir_all(&dir).expect("evidence dir");
    let img = h.render().expect("render");
    let img = match crop {
        Some(r) => {
            let s = img.width() as f32 / W;
            let x = (r.min.x * s).max(0.0) as u32;
            let y = (r.min.y * s).max(0.0) as u32;
            let w = ((r.width() * s) as u32).min(img.width() - x);
            let hh = ((r.height() * s) as u32).min(img.height() - y);
            image::imageops::crop_imm(&img, x, y, w, hh).to_image()
        }
        None => img,
    };
    img.save(dir.join(format!("{name}.png"))).expect("save png");
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<egui::Rect> {
    Some(egui::Rect::from_min_max(
        egui::pos2(x0, y0),
        egui::pos2(x1, y1),
    ))
}

/// One task: a step counter and a printed log of what the user did and where they doubted.
struct Task {
    name: &'static str,
    steps: usize,
    doubts: usize,
}

impl Task {
    fn new(name: &'static str) -> Task {
        eprintln!("\n=== TASK: {name}");
        Task {
            name,
            steps: 0,
            doubts: 0,
        }
    }

    fn click(&mut self, h: &mut H, label: &str) {
        self.click_nth(h, label, 0);
    }

    fn click_nth(&mut self, h: &mut H, label: &str, n: usize) {
        let Some(node) = h.query_all_by_label(label).nth(n) else {
            panic!(
                "[{}] step {}: no control '{label}'. Buttons now: {:?}",
                self.name,
                self.steps + 1,
                buttons(h)
            );
        };
        let r = node.rect();
        if !(r.min.y >= 0.0 && r.max.y <= HGT + 0.5) {
            // Off screen: the user scrolls to it first (one step).
            node.scroll_to_me();
            settle(h);
            self.steps += 1;
            eprintln!("  {:>2}. scroll to '{label}'", self.steps);
        }
        let node = h.query_all_by_label(label).nth(n).expect("still there");
        node.click();
        settle(h);
        self.steps += 1;
        eprintln!("  {:>2}. click '{label}'", self.steps);
    }

    fn type_in(&mut self, h: &mut H, field: &str, text: &str) {
        let node = h.get_by_label(field);
        node.focus();
        node.type_text(text);
        settle(h);
        self.steps += 1;
        eprintln!("  {:>2}. type '{text}' in '{field}'", self.steps);
    }

    fn outside(&mut self, what: &str) {
        self.steps += 1;
        eprintln!("  {:>2}. (outside the window) {what}", self.steps);
    }

    fn see(&self, h: &H, what: &str) -> bool {
        let ok = has_text(h, what);
        eprintln!("      sees '{what}': {ok}");
        ok
    }

    fn doubt(&mut self, finding: &str, why: &str) {
        self.doubts += 1;
        eprintln!("      ?  {finding}: {why}");
    }

    fn end(self, done: bool) {
        eprintln!(
            "=== RESULT {}: {} in {} steps, {} doubts",
            self.name,
            if done { "SUCCESS" } else { "FAIL" },
            self.steps,
            self.doubts
        );
    }
}

/// Requests from the menu bar agent are read twice a second (a quarter second per step).
fn wait_page(h: &mut H, want: Page) -> bool {
    for _ in 0..24 {
        if h.state().page == want {
            settle(h);
            return true;
        }
        h.step();
    }
    h.state().page == want
}

// ---------- fake Macs ----------

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("open", "");
    f.answer("sh", "");
    f.answer("launchctl", "");
    f.answer("id -u", "501\n");
}

struct Apps {
    mac: FakeMac,
    editor: PathBuf,
    viewer: PathBuf,
    jotter: PathBuf,
}

fn apps_mac() -> Apps {
    let mac = FakeMac::new();
    let editor = mac.app("Editor", "com.example.Editor", "2.0");
    let viewer = mac.app("Viewer", "com.maker.Viewer", "1.0");
    let jotter = mac.app("Jotter", "org.jot.Jotter", "3.1");
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
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Library/Caches/com.maker.Viewer/c", 120_000);
    mac.home_file("Library/Preferences/org.jot.Jotter.plist", 2_000);
    base(&mac);
    Apps {
        mac,
        editor,
        viewer,
        jotter,
    }
}

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

/// ~/Projects: alpha (Rust, quiet 90 days), gamma (Rust, changed today), beta (Node, 120 days).
fn dev_mac() -> (FakeMac, PathBuf, PathBuf) {
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
    base(&m);
    (m, p.join("alpha/target"), p.join("gamma/target"))
}

const UUID: &str = "BDEB6609-9DCF-4859-898F-019D1CA8E2D0";
const MAIL_GROUP: &str = "Mail trash, iCloud (name@icloud.com)";

fn trash_mac() -> FakeMac {
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.user_trash()).unwrap();
    m.home_file(".Trash/old report.pdf", 1_400_000);
    mail_trash(&m, UUID, "Deleted Messages", 4, 30_000);
    mail_trash(&m, UUID, "Trash", 1, 20_000);
    answer_account_names(&m.env, &format!("{UUID}|iCloud|name@icloud.com\n"));
    base(&m);
    m
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
    m.sys_file("/Volumes/My Backup/Archive 2019/disk.img", 900_000);
    base(&m);
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
    let half = m.home_file("Movies/trip.mov", 4_000_000);
    age(&half, 200);
    base(&m);
    m
}

fn blob(seed: u8, n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

fn dl_mac() -> (FakeMac, Vec<PathBuf>) {
    let m = FakeMac::new();
    let a = blob(3, 400_000);
    let v: Vec<PathBuf> = [
        "Downloads/photo.jpg",
        "Downloads/photo (1).jpg",
        "Downloads/photo (2).jpg",
    ]
    .iter()
    .map(|r| {
        let p = m.env.home_path(r);
        m.write(&p, &a);
        p
    })
    .collect();
    base(&m);
    (m, v)
}

fn monitor_mac() -> FakeMac {
    let mac = FakeMac::new();
    let f = mac.env.fake_sys().unwrap();
    cleancraft_core::testkit_monitor::answer_all(f);
    f.answer(
        "ps",
        "  512  71.0 812000 Brave Browser Helper (Renderer)\n  513   9.0 100000 Brave Browser Helper (GPU)\n  514   5.0 300000 Brave Browser\n  801  47.0  50000 WindowServer\n  930  38.0 600000 com.docker.backend\n  900  23.0  40000 CleanCraft\n  901   1.0  40000 mds_stores\n",
    );
    mac
}

// ======================================================================================
// Task walkthroughs
// ======================================================================================

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_01_first_launch_welcome_and_ai_step_not_now() {
    let mac = th::ai_mac();
    base(&mac);
    let mut h = harness_fda(&mac, Access::Missing);
    h.state_mut().launch_checks();
    settle(&mut h);
    let mut t = Task::new("first launch: welcome, Full Disk Access, AI apps step, Not Now");
    dump(&h, "first screen");
    if h.state().page == Page::ConnectAi {
        t.doubt("U7-5", "the first thing a new user sees is an AI connection page");
    }
    t.see(&h, "Welcome to CleanCraft");
    t.click(&mut h, "Continue");
    dump(&h, "second screen");
    if t.see(&h, "Let CleanCraft see all of your Mac") {
        t.click(&mut h, "Skip");
    }
    if !has(&h, "Use CleanCraft from your AI apps?") {
        t.doubt("U7-9", "the AI page title is not announced");
    }
    if has(&h, "Done") && has(&h, "Not Now") {
        t.doubt("U7-9", "Done and Not Now both close the step with nothing connected");
    }
    t.click(&mut h, "Not Now");
    dump(&h, "after Not Now");
    eprintln!("      page: {:?}", h.state().page);
    let done = h.state().page == Page::Feature(FeatureId::SmartCare);
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_02_uninstall_three_apps_one_running() {
    let m = apps_mac();
    m.mac.user_id(501);
    m.mac
        .run_program(&m.editor.join("Contents/MacOS/Editor"), 4242, 501);
    let f = m.mac.env.fake_sys().unwrap();
    f.answer("/usr/bin/osascript", "");
    let quit_key = std::iter::once("/usr/bin/osascript".to_string())
        .chain(cleancraft_core::app_quit::quit_args("com.example.Editor"))
        .collect::<Vec<_>>()
        .join(" ");
    f.answer(&quit_key, "");
    m.mac.stops(&quit_key, &[4242]);
    let mut h = harness(&m.mac);
    let mut t = Task::new("uninstall 3 apps at once, one running");
    t.click(&mut h, "Apps");
    t.click(&mut h, "App Remover");
    dump(&h, "list");
    for n in ["Editor", "Viewer", "Jotter"] {
        t.click(&mut h, &format!("Select {n}"));
    }
    dump(&h, "3 ticked");
    if t.see(&h, "Each app's file\u{2026}") {
        t.doubt("U7-18", "the bar hint is cut; 9.11 MB selected is more than the 9.09 MB the header gives for all 4 apps");
    }
    t.click(&mut h, "Review and Uninstall");
    dump(&h, "review");
    if t.see(&h, "Uninstall 2 Apps") {
        t.doubt("(fine)", "header says 3 apps, the button 2: explained by 'Running apps stay until you quit them' and Quit Editor in Editor's group");
    }
    t.click(&mut h, "Quit Editor");
    for _ in 0..3 {
        settle(&mut h);
    }
    dump(&h, "after quit");
    let btn = buttons(&h)
        .into_iter()
        .find(|b| b.starts_with("Uninstall ") && b.ends_with("Apps"))
        .unwrap_or_else(|| "Uninstall 3 Apps".into());
    t.click(&mut h, &btn);
    dump(&h, "confirm");
    t.click(&mut h, "Move to Trash");
    dump(&h, "done");
    let done = !m.editor.exists() && !m.viewer.exists() && !m.jotter.exists();
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_03_dragged_app_leftovers_from_the_notification_route() {
    let m = apps_mac();
    let trashed = m.mac.env.user_trash().join("Viewer.app");
    std::fs::create_dir_all(m.mac.env.user_trash()).unwrap();
    std::fs::rename(&m.viewer, &trashed).unwrap();
    let mut h = harness(&m.mac);
    let mut t = Task::new("remove a dragged app's leftovers from the notification");
    t.outside("Settings: turns on Show CleanCraft in the menu bar and Offer to clean up after an app goes to the Trash (both off by default)");
    t.outside("drags Viewer to the Trash in Finder");
    t.outside(
        "reads the notification 'Viewer was moved to the Trash'; clicking it opens Script Editor",
    );
    t.doubt("U7-2", "the notification says 'Review them in App Remover'; clicking it opens Script Editor, not CleanCraft; nothing says to use the menu bar item");
    t.outside(
        "opens the CleanCraft menu bar item, chooses 'Review: Viewer was moved to the Trash'",
    );
    notify::request_open(
        &m.mac.env,
        &AppRoute::UninstallApp(trashed.clone()).to_arg(),
    )
    .expect("request");
    let landed = wait_page(&mut h, Page::Feature(FeatureId::Uninstaller));
    eprintln!("      landed on App Remover: {landed}");
    dump(&h, "landing");
    snap(&mut h, "u7-02-trash-offer-landing", None);
    t.click(&mut h, "Remove Leftovers");
    dump(&h, "confirm");
    t.click(&mut h, "Move to Trash");
    dump(&h, "done");
    let done = !m
        .mac
        .env
        .home_path("Library/Caches/com.maker.Viewer")
        .exists();
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_04_clean_old_rust_targets_keep_active_project() {
    let (m, alpha, gamma) = dev_mac();
    let mut h = harness(&m);
    let mut t = Task::new("clean old Rust target folders, keep the active project");
    t.click(&mut h, "Clean");
    dump(&h, "clean section");
    t.click(&mut h, "Developer Clutter");
    dump(&h, "dev start");
    t.click(&mut h, "Scan");
    dump(&h, "dev results");
    if h.query_all_by_label("target").count() > 1 {
        t.doubt(
            "U7-10",
            "two rows named 'target': the project is only in the dim path and the reason",
        );
    }
    t.see(&h, "active project");
    t.click(&mut h, "Review and Clean");
    dump(&h, "confirm");
    if t.see(&h, "Scroll the list to see every one") {
        t.doubt(
            "U7-11",
            "'Scroll the list to see every one' under a list of 2 rows that are all in view",
        );
    }
    t.click(&mut h, "Move to Trash");
    dump(&h, "done");
    let done = !alpha.exists() && gamma.exists();
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_05_set_code_folders_in_settings() {
    let m = FakeMac::new();
    base(&m);
    let code = m.env.home_path("Work/code/shop");
    m.write(&code.join("Cargo.toml"), b"[package]\n");
    m.write(&code.join("target/debug/app"), &vec![1u8; 2_000_000]);
    age_tree(&code, 60);
    let mut h = harness(&m);
    let mut t = Task::new("set code folders in Settings");
    t.click(&mut h, "Clean");
    t.click(&mut h, "Developer Clutter");
    t.click(&mut h, "Scan");
    dump(&h, "nothing found");
    t.click(&mut h, "Details");
    if t.see(&h, "add your own in Settings") {
        t.doubt(
            "U7-6",
            "the way forward is a sentence under Details; no button opens the code folder setting",
        );
    }
    t.click(&mut h, "Settings");
    t.click(&mut h, "Add Code Folder");
    dump(&h, "settings dev group");
    t.type_in(&mut h, "Path of a code folder", "~/Work/code");
    t.click(&mut h, "Add Code Folder");
    dump(&h, "after add");
    t.click(&mut h, "Clean");
    t.click(&mut h, "Developer Clutter");
    dump(&h, "back on dev");
    if t.see(&h, "Nothing to clean here") {
        t.doubt(
            "U7-6",
            "the page still shows the old empty result; the notice said 'on its next scan'",
        );
    }
    t.click(&mut h, "Scan Again");
    t.see(&h, "Work/code/shop/target");
    t.end(Settings::load(&m.env).dev_roots.len() == 1);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_06_menu_bar_and_weekly_scan() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    let mut t = Task::new("turn on the menu bar and a weekly scan");
    t.click(&mut h, "Settings");
    h.get_by_label(cleancraft_ui_egui::pages::settings::MENU_BAR_SWITCH)
        .scroll_to_me();
    settle(&mut h);
    t.steps += 1;
    eprintln!("  {:>2}. scroll to the menu bar group", t.steps);
    dump(&h, "menu bar group off");
    if t.see(&h, "When free space is under: 10%") {
        t.doubt("U7-15", "the disk alert switch shows On (greyed) while the menu bar is off: is it warning me or not?");
    }
    t.click(&mut h, cleancraft_ui_egui::pages::settings::MENU_BAR_SWITCH);
    dump(&h, "menu bar on");
    t.click(&mut h, "Every week");
    dump(&h, "weekly");
    t.see(&h, "Next scan: Monday at 10:00");
    t.doubt("U7-15", "'Notifications' in the label column next to a 'Pause notifications' switch: On means paused");
    let s = Settings::load(&m.env);
    eprintln!(
        "      saved: {:?} weekday {} hour {}",
        s.scan_every, s.scan_weekday, s.scan_hour
    );
    t.end(cleancraft_core::login_agent::is_on(&m.env));
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_07_read_mac_health_and_follow_two_recommendations() {
    let mac = th::troubled_mac_now();
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("read Mac Health, follow two recommendations");
    if !has_text(&h, "Mac Health") {
        t.doubt("U7-4", "no Mac Health in the sidebar or on Quick Sweep");
    }
    dump(&h, "start");
    t.click(&mut h, "Live Monitor");
    t.click(&mut h, "Mac Health");
    dump(&h, "health");
    t.click(&mut h, "Review in Threat Check");
    eprintln!("      page: {:?}", h.state().page);
    dump(&h, "after threat rec");
    if t.see(&h, "Scan")
        && h.state()
            .plans
            .get(&FeatureId::Malware)
            .and_then(|s| s.plan.as_ref())
            .is_none()
    {
        t.doubt("U7-3", "'Review in Threat Check' lands on the start screen: the item it found is not shown, one more scan");
    }
    t.click(&mut h, "Live Monitor");
    dump(&h, "back via sidebar");
    if !has(&h, "Review in System Junk") {
        t.click(&mut h, "Mac Health");
    }
    t.click(&mut h, "Review in System Junk");
    eprintln!("      page: {:?}", h.state().page);
    dump(&h, "after junk rec");
    if h.state()
        .plans
        .get(&FeatureId::SystemJunk)
        .and_then(|s| s.plan.as_ref())
        .is_none()
    {
        t.doubt(
            "U7-3",
            "'Review in System Junk' (2.30 GB found 4 days ago) lands on the start screen too",
        );
    }
    // fix7: the scan started by itself; its list is on screen.
    t.end(
        h.state()
            .plans
            .get(&FeatureId::SystemJunk)
            .and_then(|s| s.plan.as_ref())
            .is_some(),
    );
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_08_find_what_grew() {
    let mac = th::troubled_mac_now();
    base(&mac);
    mac.home_file("Movies/film.mov", 2_000_000);
    let mut h = harness(&mac);
    // The real app's launch work (tests do not run it by themselves).
    h.state_mut().launch_checks();
    settle(&mut h);
    let mut t = Task::new("find what grew on the disk");
    t.click(&mut h, "Live Monitor");
    t.click(&mut h, "Mac Health");
    t.click(&mut h, "Open Disk Map");
    dump(&h, "disk map from health");
    if t.see(&h, "Map Home Folder") {
        t.doubt("U7-1", "Open Disk Map lands on the start screen; mapping now cannot say what grew in the last 7 days: Health needs two maps days apart");
    }
    let mapped = h.state().disk_map.tree().is_some();
    eprintln!("      Disk Map measured the home folder at once: {mapped}");
    t.click(&mut h, "Live Monitor");
    // fix7: a light home measure is taken at launch; tomorrow's gives the comparison.
    let first = t.see(&h, "Come back tomorrow") || t.see(&h, "First measure");
    eprintln!("      page: {:?}", h.state().page);
    t.end(mapped && (first || h.state().health.growth.is_some()));
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_09_map_an_external_drive() {
    let m = map_mac();
    let mut h = harness(&m);
    let mut t = Task::new("map an external drive");
    t.click(&mut h, "Files");
    t.click(&mut h, "Disk Map");
    dump(&h, "disk map start");
    t.click(&mut h, "Home folder");
    dump(&h, "picker");
    let label = buttons(&h)
        .into_iter()
        .find(|b| b.starts_with("My Backup"))
        .expect("drive in picker");
    t.click(&mut h, &label);
    dump(&h, "drive chosen");
    t.click(&mut h, "Map My Backup");
    dump(&h, "mapped");
    t.see(&h, "locked: a top folder of this disk");
    t.click(&mut h, "Select Archive 2019");
    dump(&h, "select top folder");
    if h.state().disk_map.selected.is_empty() {
        t.doubt("U7-8", "Archive 2019 on my own backup drive cannot be removed as a folder; I must open it and tick what is inside, and the empty folder stays");
    }
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_10_group_large_and_old_by_age() {
    let m = large_old_mac();
    let mut h = harness(&m);
    let mut t = Task::new("group Large & Old by age");
    t.click(&mut h, "Files");
    t.click(&mut h, "Large & Old Files");
    t.click(&mut h, "Scan");
    dump(&h, "results");
    t.click(&mut h, "Group by: Kind");
    dump(&h, "group menu");
    t.click(&mut h, "Age");
    dump(&h, "by age");
    t.doubt(
        "U7-17",
        "groups read 3 to 6 months, 6 to 12 months, over a year, last 3 months: not in age order",
    );
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_11_erase_mail_trash_with_and_without_accessibility() {
    let m = trash_mac();
    let f = m.env.fake_sys().unwrap();
    f.answer_full(
        &erase_call(Some("iCloud")),
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr:
                "System Events got an error: osascript is not allowed assistive access. (-1719)"
                    .into(),
        },
    );
    let mut h = harness(&m);
    let mut t = Task::new("erase Mail trash, Accessibility off then on");
    t.click(&mut h, "Clean");
    t.click(&mut h, "Trash Bins");
    t.click(&mut h, "Scan");
    dump(&h, "trash bins");
    if t.see(&h, "1.59 MB in the Trash") {
        t.doubt("U7-13", "the bar says 1.59 MB next to Empty Trash in Finder, which frees 1.40 MB: the Mail trash is counted in");
    }
    t.click(&mut h, &format!("Erase in Mail... {MAIL_GROUP}"));
    dump(&h, "confirm");
    t.click(&mut h, "Erase in Mail");
    dump(&h, "refused");
    t.click(&mut h, "Open Accessibility Settings");
    t.outside("turns on CleanCraft in Privacy & Security > Accessibility, comes back");
    f.answer(&erase_call(Some("iCloud")), "");
    dump(&h, "came back");
    if !has(&h, "Try Again") {
        t.doubt(
            "U7-14",
            "no Try Again: Erase in Mail... and its confirmation again",
        );
    }
    t.click(&mut h, &format!("Erase in Mail... {MAIL_GROUP}"));
    t.click(&mut h, "Erase in Mail");
    t.see(&h, "Mail is erasing");
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_12_duplicate_downloads_from_quick_sweep() {
    let (m, copies) = dl_mac();
    let mut h = harness(&m);
    let mut t = Task::new("clean Duplicate Downloads from Quick Sweep");
    dump(&h, "quick sweep start");
    t.click(&mut h, "Scan");
    dump(&h, "tiles");
    if h.query_all_by_label("Include").count() > 1 {
        t.doubt(
            "U7-12",
            "Include boxes on tiles with nothing to clean; five controls all named Include",
        );
    }
    t.click(&mut h, "Review and Clean");
    dump(&h, "sweep confirm");
    if !t.see(&h, "Downloads/photo.jpg") {
        t.doubt("U7-11", "the confirmation lists the two copies that go, never the one that stays; it also mentions the threat quarantine and a scroll for 2 rows");
    }
    t.click(&mut h, "Move to Trash");
    dump(&h, "sweep done");
    t.end(copies.iter().filter(|c| c.exists()).count() == 1);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_13_understand_a_process_in_live_monitor() {
    let mac = monitor_mac();
    let mut h = harness(&mac);
    let mut t = Task::new("understand a process in Live Monitor");
    t.click(&mut h, "Live Monitor");
    dump(&h, "monitor");
    t.see(&h, "Part of macOS: draws everything on your screen");
    if t.see(&h, "17%") && t.see(&h, "85%") {
        t.doubt(
            "U7-7",
            "CPU 17% in all, Brave Browser 85%: which number is true?",
        );
    }
    if !has_text(&h, "Docker") {
        t.doubt(
            "U7-7",
            "com.docker.backend at 38% has no explanation and no way to find out more",
        );
    }
    t.end(true);
}

// ======================================================================================
// Proofs of the findings (fail today; drop the #[ignore] when fixed)
// ======================================================================================

fn open_health(h: &mut H) {
    h.get_all_by_label("Live Monitor")
        .next()
        .expect("sidebar")
        .click();
    settle(h);
    h.get_by_label("Mac Health").click();
    settle(h);
}

/// U7-1 (S1). A first-time user cannot find what grew: nothing records a baseline until the
/// user maps the same folder twice, days apart. Launching the app should record one.
#[test]
fn u7_01_a_baseline_for_what_grew_is_recorded_without_the_user() {
    let mac = th::troubled_mac_now();
    base(&mac);
    mac.home_file("Movies/film.mov", 2_000_000);
    let mut h = harness(&mac);
    h.state_mut().launch_checks();
    settle(&mut h);
    open_health(&mut h);
    snap(&mut h, "u7-01-what-grew", rect(820.0, 280.0, 1260.0, 780.0));
    assert!(
        !cleancraft_core::health::map_summaries(&mac.env).is_empty(),
        "no baseline: What grew can only answer after two Disk Map scans days apart"
    );
}

/// U7-1 (S1). "Open Disk Map" from Mac Health lands on Disk Map's start screen: one more
/// click, and still no comparison.
#[test]
fn u7_01_open_disk_map_from_health_starts_measuring() {
    let mac = th::troubled_mac_now();
    base(&mac);
    mac.home_file("Movies/film.mov", 2_000_000);
    let mut h = harness(&mac);
    open_health(&mut h);
    h.get_all_by_label("Open Disk Map").next().unwrap().click();
    settle(&mut h);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SpaceLens));
    assert!(
        h.state().disk_map.tree().is_some(),
        "Disk Map waits on its start screen (Map Home Folder)"
    );
}

/// U7-3 (S2). "Review in Threat Check" and "Review in System Junk" open start screens: the
/// item the card talks about is not shown, the user must scan again.
#[test]
fn u7_03_review_buttons_show_what_the_card_talks_about() {
    let mac = th::troubled_mac_now();
    base(&mac);
    let mut h = harness(&mac);
    open_health(&mut h);
    h.get_by_label("Review in Threat Check").click();
    settle(&mut h);
    snap(&mut h, "u7-03-threat-landing", None);
    let threat = h
        .state()
        .plans
        .get(&FeatureId::Malware)
        .and_then(|s| s.plan.as_ref())
        .is_some();
    open_health(&mut h);
    h.get_by_label("Review in System Junk").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Review in System Junk").click();
    settle(&mut h);
    let junk = h
        .state()
        .plans
        .get(&FeatureId::SystemJunk)
        .and_then(|s| s.plan.as_ref())
        .is_some();
    assert!(
        threat && junk,
        "Threat Check shows a list: {threat}; System Junk shows a list: {junk}"
    );
}

/// U7-4 (S2). Mac Health is only a segment inside Live Monitor; the start page says nothing
/// about it.
#[test]
fn u7_04_mac_health_is_visible_from_the_start_page() {
    let mac = th::troubled_mac_now();
    base(&mac);
    let mut h = harness(&mac);
    snap(&mut h, "u7-04-start-page", None);
    assert!(
        has_text(&h, "Mac Health") || has_text(&h, "Needs attention"),
        "nothing on the first page leads to Mac Health"
    );
}

/// U7-5 (S2). First launch opens on the AI apps step; Full Disk Access is not part of it.
#[test]
fn u7_05_first_launch_welcomes_and_asks_for_full_disk_access_first() {
    let mac = th::ai_mac();
    base(&mac);
    let mut h = harness_fda(&mac, Access::Missing);
    h.state_mut().launch_checks();
    settle(&mut h);
    snap(&mut h, "u7-05-first-launch", None);
    assert!(
        has_text(&h, "Full Disk Access"),
        "first screen: {:?}",
        h.state().page
    );
    assert_ne!(h.state().page, Page::ConnectAi, "AI apps come first");
}

/// U7-6 (S2). Developer Clutter finds nothing: the way to add a code folder is a sentence
/// under Details, with no button.
#[test]
fn u7_06_nothing_found_offers_to_add_a_code_folder() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    h.get_by_label("Clean").click();
    settle(&mut h);
    h.get_by_label("Developer Clutter").click();
    settle(&mut h);
    h.get_by_label("Scan").click();
    settle(&mut h);
    snap(&mut h, "u7-06-dev-nothing-found", None);
    let b = buttons(&h);
    assert!(
        b.iter().any(|l| l.contains("Code Folder")),
        "buttons: {b:?}"
    );
}

/// U7-6 (S2). After adding a code folder in Settings, Developer Clutter still shows its old
/// "Nothing to clean here".
#[test]
fn u7_06_a_new_code_folder_is_looked_in_at_once() {
    let m = FakeMac::new();
    base(&m);
    let code = m.env.home_path("Work/code/shop");
    m.write(&code.join("Cargo.toml"), b"[package]\n");
    m.write(&code.join("target/debug/app"), &vec![1u8; 2_000_000]);
    age_tree(&code, 60);
    let mut h = harness(&m);
    for l in ["Clean", "Developer Clutter", "Scan", "Settings"] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    let field = h.get_by_label("Path of a code folder");
    field.scroll_to_me();
    settle(&mut h);
    let field = h.get_by_label("Path of a code folder");
    field.focus();
    field.type_text("~/Work/code");
    settle(&mut h);
    h.get_by_label("Add Code Folder").click();
    settle(&mut h);
    for l in ["Clean", "Developer Clutter"] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    assert!(
        has_text(&h, "Work/code/shop/target"),
        "the page still says: Nothing to clean here"
    );
}

/// U7-7 (S2). Live Monitor: the CPU card says 17% and lists Brave at 85% (of one core),
/// with nothing saying the units differ.
#[test]
fn u7_07_process_cpu_is_on_the_same_scale_as_the_total() {
    let mac = monitor_mac();
    let mut h = harness(&mac);
    h.get_all_by_label("Live Monitor").next().unwrap().click();
    settle(&mut h);
    snap(
        &mut h,
        "u7-07-monitor-cpu",
        rect(260.0, 80.0, 1260.0, 520.0),
    );
    assert!(
        !(has(&h, "17%") && has(&h, "85%")) || has_text(&h, "one core"),
        "CPU 17% in all, Brave Browser 85%: no unit told"
    );
}

/// U7-7 (S2). A process CleanCraft does not know gets no words at all.
#[test]
fn u7_07_unknown_process_names_its_app() {
    let mac = monitor_mac();
    let mut h = harness(&mac);
    h.get_all_by_label("Live Monitor").next().unwrap().click();
    settle(&mut h);
    assert!(has(&h, "com.docker.backend"));
    assert!(
        has_text(&h, "Docker"),
        "com.docker.backend: no explanation and no way to find out what it is"
    );
}

/// U7-8 (S2). On an external drive every top folder is locked; the checkbox still looks
/// active and a click does nothing.
#[test]
fn u7_08_external_drive_top_folder_can_be_selected() {
    let m = map_mac();
    let mut h = harness(&m);
    for l in ["Files", "Disk Map", "Home folder"] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    let label = buttons(&h)
        .into_iter()
        .find(|b| b.starts_with("My Backup"))
        .unwrap();
    h.get_by_label(&label).click();
    settle(&mut h);
    h.get_by_label("Map My Backup").click();
    settle(&mut h);
    let enabled = !disabled(&h, "Select Archive 2019");
    h.get_by_label("Select Archive 2019").click();
    settle(&mut h);
    snap(&mut h, "u7-08-external-top-folder", None);
    let picked = !h.state().disk_map.selected.is_empty();
    assert!(
        picked || !enabled,
        "the checkbox is enabled ({enabled}) but clicking it selects nothing"
    );
    assert!(
        picked,
        "Archive 2019 on My Backup cannot be removed as a whole"
    );
}

/// U7-9 (S3). One setting, two names; the page title is not announced; Done and Not Now
/// both close the first launch step.
#[test]
fn u7_09_ai_step_names_and_buttons() {
    use cleancraft_ui_egui::pages::{settings, welcome};
    let mac = th::ai_mac();
    base(&mac);
    let mut h = harness(&mac);
    h.state_mut().launch_checks();
    settle(&mut h);
    // fix7: the AI step now follows the welcome (U7-5); Full Disk Access is on here.
    h.get_by_label("Continue").click();
    settle(&mut h);
    let mut problems = Vec::new();
    if welcome::CLEAN_SWITCH != settings::AGENTS_SWITCH {
        problems.push(format!(
            "'{}' vs '{}'",
            welcome::CLEAN_SWITCH,
            settings::AGENTS_SWITCH
        ));
    }
    if !has(&h, "Use CleanCraft from your AI apps?") {
        problems.push("title not announced".into());
    }
    if has(&h, "Done") && has(&h, "Not Now") {
        problems.push("Done and Not Now with nothing connected".into());
    }
    assert!(problems.is_empty(), "{problems:?}");
}

/// U7-10 (S3). Developer Clutter rows are all titled "target" or "node_modules".
#[test]
fn u7_10_rows_lead_with_the_project() {
    let (m, _, _) = dev_mac();
    let mut h = harness(&m);
    for l in ["Clean", "Developer Clutter", "Scan"] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    snap(&mut h, "u7-10-dev-rows", rect(560.0, 230.0, 1250.0, 420.0));
    assert_eq!(
        h.query_all_by_label("target").count(),
        0,
        "two rows titled 'target'"
    );
}

/// U7-11 (S3). Confirmations: a scroll hint for a 2-item list, the quarantine sentence with
/// no threat, and the copy that stays is not named.
#[test]
fn u7_11_quick_sweep_confirmation_says_what_stays() {
    let (m, _) = dl_mac();
    let mut h = harness(&m);
    h.get_by_label("Scan").click();
    settle(&mut h);
    h.get_by_label("Review and Clean").click();
    settle(&mut h);
    snap(&mut h, "u7-11-sweep-confirm", None);
    let mut problems = Vec::new();
    if has_text(&h, "Scroll the list") {
        problems.push("scroll hint for 2 items");
    }
    if has_text(&h, "Threats go to the CleanCraft quarantine") {
        problems.push("quarantine sentence with no threat");
    }
    if !has_text(&h, "Downloads/photo.jpg") {
        problems.push("the kept copy is not named");
    }
    assert!(problems.is_empty(), "{problems:?}");
}

/// U7-12 (S3). Quick Sweep: the subtitle leaves out the new tiles; tiles with nothing to
/// clean still have Include; six "Include" and eight "Review" with the same name.
#[test]
fn u7_12_quick_sweep_tiles_are_named_and_only_offer_what_applies() {
    let (m, _) = dl_mac();
    let mut h = harness(&m);
    let subtitle_ok = has_text(&h, "duplicate") && has_text(&h, "developer");
    h.get_by_label("Scan").click();
    settle(&mut h);
    snap(&mut h, "u7-12-sweep-tiles", None);
    let includes = h.query_all_by_label("Include").count();
    let reviews = h.query_all_by_label("Review").count();
    assert!(
        subtitle_ok && includes <= 1 && reviews <= 1,
        "subtitle names the new tiles: {subtitle_ok}; 'Include' x{includes}; 'Review' x{reviews}"
    );
}

/// U7-13 (S3). Trash Bins: next to "Empty Trash in Finder" the bar adds the Mail trash,
/// which Finder does not empty.
#[test]
fn u7_13_trash_bar_counts_what_finder_empties() {
    let m = trash_mac();
    let mut h = harness(&m);
    for l in ["Clean", "Trash Bins", "Scan"] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    snap(&mut h, "u7-13-trash-bar", rect(260.0, 700.0, 1260.0, 800.0));
    assert!(
        !has_text(&h, "1.59 MB in the Trash"),
        "1.59 MB includes 184 KB of Mail trash"
    );
}

/// U7-14 (S3). After Open Accessibility Settings, coming back needs Erase in Mail... and the
/// confirmation again: no Try Again.
#[test]
fn u7_14_mail_erase_can_be_tried_again() {
    let m = trash_mac();
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
    let mut h = harness(&m);
    for l in [
        "Clean",
        "Trash Bins",
        "Scan",
        &format!("Erase in Mail... {MAIL_GROUP}"),
        "Erase in Mail",
        "Open Accessibility Settings",
    ] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    snap(&mut h, "u7-14-accessibility", None);
    assert!(has(&h, "Try Again"), "buttons: {:?}", buttons(&h));
}

/// U7-15 (S3). With the menu bar off, "Warn when the startup disk is almost full" shows On.
#[test]
fn u7_15_options_that_cannot_run_do_not_show_on() {
    use cleancraft_ui_egui::pages::settings::DISK_ALERT_SWITCH;
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    h.get_by_label("Settings").click();
    settle(&mut h);
    h.get_by_label(DISK_ALERT_SWITCH).scroll_to_me();
    settle(&mut h);
    snap(&mut h, "u7-15-settings-menu-bar", None);
    let n = h.get_by_label(DISK_ALERT_SWITCH);
    let a = n.accesskit_node();
    let on = a.toggled() == Some(egui::accesskit::Toggled::True);
    assert!(
        !(a.is_disabled() && on),
        "disabled (menu bar off) and shown On"
    );
}

/// U7-16 (S3). Mac Health: the battery card's button opens Live Monitor although its text
/// points to System Settings > Battery; the folders under What grew cannot be opened.
#[test]
fn u7_16_health_buttons_go_where_the_text_says() {
    use cleancraft_core::health::MapSummary;
    let mac = th::troubled_mac_now();
    base(&mac);
    let home = mac.env.home().to_path_buf();
    let mk = |at, movies| MapSummary {
        at,
        root: home.clone(),
        entries: vec![(home.clone(), 100 * th::GB), (home.join("Movies"), movies)],
    };
    std::fs::write(
        mac.env.data_dir().join("disk-map-history.json"),
        serde_json::to_vec(&vec![
            mk(th::now() - 86_400 * 3, th::GB),
            mk(th::now(), 9 * th::GB),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut h = harness(&mac);
    open_health(&mut h);
    let battery = h
        .state()
        .health
        .cards
        .iter()
        .find(|c| c.headline.starts_with("The battery"))
        .map(|c| c.action.clone())
        .unwrap_or_default();
    let grew_link = buttons(&h).iter().any(|b| b.contains("Movies"));
    assert!(
        battery != "Open Live Monitor" && grew_link,
        "battery button: {battery}; a What grew folder can be opened: {grew_link}"
    );
}

/// U7-17 (S3). Large & Old by age: the groups are not in age order.
#[test]
fn u7_17_age_groups_in_age_order() {
    let m = large_old_mac();
    let mut h = harness(&m);
    for l in [
        "Files",
        "Large & Old Files",
        "Scan",
        "Group by: Kind",
        "Age",
    ] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    snap(&mut h, "u7-17-age-groups", rect(260.0, 220.0, 560.0, 520.0));
    let t = texts(&h);
    let at = |p: &str| {
        t.iter()
            .position(|x| x.starts_with(p))
            .unwrap_or(usize::MAX)
    };
    let order = [
        at("Not opened for over a year"),
        at("Not opened in 6 to 12 months"),
        at("Not opened in 3 to 6 months"),
        at("Opened in the last 3 months"),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

/// U7-18 (S3). App Remover, three apps ticked: 9.11 MB selected out of "9.09 MB in 4 apps",
/// and the bar's hint is cut.
#[test]
fn u7_18_selection_bar_numbers_and_hint() {
    let m = apps_mac();
    let mut h = harness(&m.mac);
    for l in [
        "Apps",
        "App Remover",
        "Select Editor",
        "Select Viewer",
        "Select Jotter",
    ] {
        h.get_all_by_label(l).next().unwrap().click();
        settle(&mut h);
    }
    snap(&mut h, "u7-18-apps-bar", rect(260.0, 700.0, 1260.0, 800.0));
    let more_than_all = has(&h, "9.09 MB") && has_text(&h, "9.11 MB with their leftovers");
    let cut = has_text(&h, "\u{2026}");
    assert!(
        !more_than_all && !cut,
        "selected more than the total: {more_than_all}; hint cut: {cut}"
    );
}
