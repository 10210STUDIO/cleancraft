//! Usability review (docs/design/REVIEW-U.md): real tasks driven like a first-time Mac user
//! would, by clicks and keys on a fake Mac.
//!
//! Two kinds of tests, all `#[ignore]`d so the suite stays green:
//! - `task_*`: the walkthrough of one task. Passes today; prints every step with
//!   `cargo test -p cleancraft-ui-egui --test review_usability -- --ignored --nocapture task_`.
//! - `u##_*`: proof of one finding U##. Fails today; remove the `#[ignore]` when it is fixed.

mod common;

use std::path::PathBuf;

use cleancraft_core::executor::{self, Method};
use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::testkit_perf as tp;
use cleancraft_core::{Fault, FeatureId, Settings, testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
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
    for _ in 0..6 {
        h.step();
    }
}

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn has_text(h: &H, needle: &str) -> bool {
    h.query_all_by_label_contains(needle).next().is_some()
}

fn count(h: &H, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

/// With `CLEANCRAFT_REVIEW_U_RENDERS=1`, save the window as docs/design/review-u/<name>.png
/// (the evidence crops in REVIEW-U.md).
fn snap(h: &mut H, name: &str) {
    if std::env::var("CLEANCRAFT_REVIEW_U_RENDERS").is_ok_and(|v| v == "1") {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/review-u");
        std::fs::create_dir_all(&dir).expect("evidence dir");
        let img = h.render().expect("render");
        img.save(dir.join(format!("{name}.png"))).expect("save png");
    }
}

/// Every visible, enabled button label (what a user can press now).
fn buttons(h: &H) -> Vec<String> {
    use egui::accesskit::Role;
    h.root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            (!a.is_hidden()
                && !a.is_disabled()
                && matches!(
                    a.role(),
                    Role::Button | Role::CheckBox | Role::Switch | Role::ComboBox
                ))
            .then(|| a.label().unwrap_or_default())
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// Every visible text (labels of any role).
fn texts(h: &H) -> Vec<String> {
    h.root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            (!a.is_hidden()).then(|| a.label().or(a.value()).unwrap_or_default())
        })
        .filter(|l| !l.is_empty())
        .collect()
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
        let Some(node) = h.query_all_by_label(label).next() else {
            panic!(
                "[{}] step {}: no control '{label}'. Buttons now: {:?}",
                self.name,
                self.steps + 1,
                buttons(h)
            );
        };
        let r = node.rect();
        assert!(
            r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5,
            "[{}] '{label}' is outside the window: {r:?}",
            self.name
        );
        node.click();
        settle(h);
        self.steps += 1;
        eprintln!("  {:>2}. click '{label}'", self.steps);
    }

    fn click_contains(&mut self, h: &mut H, needle: &str) {
        let node = h
            .query_all_by_label_contains(needle)
            .next()
            .unwrap_or_else(|| panic!("[{}] nothing contains '{needle}'", self.name));
        let label = node.accesskit_node().label().unwrap_or_default();
        node.click();
        settle(h);
        self.steps += 1;
        eprintln!("  {:>2}. click '{label}'", self.steps);
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

// ---------- fake Macs ----------

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
}

fn junk() -> FakeMac {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    m
}

fn sweep() -> FakeMac {
    let m = testkit_monitor::sweep_mac();
    base(&m);
    m
}

struct Apps {
    mac: FakeMac,
    editor: PathBuf,
    leftovers: Vec<PathBuf>,
}

fn apps_mac() -> Apps {
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
    let leftovers = vec![
        mac.home_file(
            "Library/Application Support/com.example.Editor/db.sqlite",
            400_000,
        ),
        mac.home_file("Library/Caches/com.example.Editor/blob", 900_000),
        mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000),
    ];
    base(&mac);
    Apps {
        mac,
        editor,
        leftovers,
    }
}

fn files_mac() -> (FakeMac, PathBuf) {
    let m = FakeMac::sample();
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
    .save(&m.env)
    .expect("save settings");
    m.home_file("Downloads/big.zip", 2_500_000);
    let old = m.home_file("Downloads/old-video.mov", 4_200_000);
    age(&old, 400);
    m.home_file("Movies/clip.mov", 3_000_000);
    let dup: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(7919) >> 3) as u8)
        .collect();
    for p in ["Downloads/a.bin", "Music/old/a.bin", "Projects/a.bin"] {
        m.write(&m.env.home_path(p), &dup);
    }
    m.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    base(&m);
    (m, old)
}

/// Section, then tool (the sidebar route a user takes).
fn go(t: &mut Task, h: &mut H, section: &str, tool: Option<&str>) {
    t.click(h, section);
    if let Some(tool) = tool {
        t.click(h, tool);
    }
}

// ======================================================================================
// Task walkthroughs (pass today; the log is the evidence in REVIEW-U.md)
// ======================================================================================

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_01_free_space_quickly_with_quick_sweep() {
    let mac = sweep();
    let mut h = harness(&mac);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    let mut t = Task::new("free space quickly (Quick Sweep)");
    t.click(&mut h, "Scan");
    t.see(&h, "ready to clean");
    if t.see(&h, "Emptied from its own page") {
        t.doubt(
            "U-trash-tile",
            "the Trash tile has no Include: emptying the Trash is not part of the sweep",
        );
    }
    t.click(&mut h, "Review and Clean");
    t.click(&mut h, "Move to Trash");
    let done = t.see(&h, "moved to the Trash");
    if t.see(&h, "The space is freed when the Trash is emptied") {
        t.doubt(
            "U-space-not-freed",
            "free space did not change; the Done screen offers no way to empty the Trash",
        );
    }
    eprintln!("      buttons on Done: {:?}", buttons(&h));
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_02_clean_system_junk_but_keep_one_item() {
    let mac = junk();
    let mut h = harness(&mac);
    let keep = mac.env.home_path("Library/Caches/com.example.Browser");
    let mut t = Task::new("clean System Junk, keep one item");
    go(&mut t, &mut h, "Clean", Some("System Junk"));
    t.click(&mut h, "Scan");
    // Finding the item: search by name, then untick it.
    let node = h.get_by_label("Search");
    node.focus();
    node.type_text("Browser");
    settle(&mut h);
    t.steps += 1;
    eprintln!("  {:>2}. type 'Browser' in Search", t.steps);
    let shown = mac.env.display(&keep);
    eprintln!(
        "      rows now: {:?}",
        texts(&h)
            .iter()
            .filter(|s| s.contains("Browser"))
            .collect::<Vec<_>>()
    );
    t.click_contains(&mut h, "com.example.Browser");
    eprintln!(
        "      after click: {:?}",
        h.state_mut()
            .plan_state(FeatureId::SystemJunk)
            .plan
            .as_ref()
            .map(|p| p
                .items
                .iter()
                .filter(|i| i.path == keep)
                .map(|i| i.selected)
                .collect::<Vec<_>>())
    );
    t.click(&mut h, "Review and Clean");
    let listed = has_text(&h, &shown);
    eprintln!("      kept item in confirmation: {listed}");
    t.click(&mut h, "Move to Trash");
    let done = keep.exists() && t.see(&h, "moved to the Trash");
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_03_empty_trash_bins() {
    let mac = testkit_cleanup::trash_mac();
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("empty Trash Bins");
    go(&mut t, &mut h, "Clean", Some("Trash Bins"));
    t.click(&mut h, "Scan");
    if t.see(&h, "Tick the items to clean.") {
        t.doubt(
            "U-trash-dead-end",
            "every row is locked, yet the bar says to tick items and Select All is offered",
        );
    }
    eprintln!("      buttons: {:?}", buttons(&h));
    t.click(&mut h, "Empty Trash in Finder");
    eprintln!("      dialog buttons: {:?}", buttons(&h));
    t.click(&mut h, "Empty Trash");
    let done = true;
    eprintln!(
        "      after: {:?}",
        texts(&h).into_iter().take(12).collect::<Vec<_>>()
    );
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_04_find_and_remove_a_big_old_file() {
    let (mac, old) = files_mac();
    let mut h = harness(&mac);
    let mut t = Task::new("find and remove a big old file");
    go(&mut t, &mut h, "Files", Some("Large & Old Files"));
    t.click(&mut h, "Scan");
    eprintln!(
        "      sort choices: {:?}",
        h.query_all_by_label_contains("Sort")
            .map(|n| n.accesskit_node().value().unwrap_or_default())
            .collect::<Vec<_>>()
    );
    let shown = mac.env.display(&old);
    eprintln!(
        "      old file row present: {}",
        has_text(&h, "old-video.mov")
    );
    t.click_contains(&mut h, "old-video.mov");
    t.click(&mut h, "Review and Clean");
    eprintln!("      confirm lists it: {}", has_text(&h, &shown));
    t.click(&mut h, "Move to Trash");
    let done = !old.exists();
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_05_remove_duplicates_keeping_the_right_one() {
    let (mac, _) = files_mac();
    let keep = mac.env.home_path("Projects/a.bin");
    let mut h = harness(&mac);
    let mut t = Task::new("remove duplicate copies, keep the one in Projects");
    go(&mut t, &mut h, "Files", Some("Duplicates"));
    t.click(&mut h, "Scan");
    eprintln!("      buttons: {:?}", buttons(&h));
    let sel: Vec<(String, bool)> = h
        .state_mut()
        .plan_state(FeatureId::Duplicates)
        .plan
        .as_ref()
        .map(|p| {
            p.items
                .iter()
                .map(|i| (mac.env.display(&i.path), i.selected))
                .collect()
        })
        .unwrap_or_default();
    eprintln!("      preselection: {sel:?}");
    t.end(true);
    let _ = keep;
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_06_uninstall_an_app_with_leftovers() {
    let a = apps_mac();
    let mut h = harness(&a.mac);
    let mut t = Task::new("uninstall Editor with its leftovers");
    go(&mut t, &mut h, "Apps", Some("App Remover"));
    t.click(&mut h, "Editor");
    t.click(&mut h, "Uninstall");
    t.click(&mut h, "Move to Trash");
    let gone = !a.editor.exists() && a.leftovers.iter().all(|p| !p.exists());
    eprintln!("      gone: {gone}; buttons on Done: {:?}", buttons(&h));
    t.click(&mut h, "Done");
    eprintln!(
        "      after Done: {:?}",
        texts(&h).into_iter().take(20).collect::<Vec<_>>()
    );
    if has_text(&h, "could not be read") || has(&h, "Back to Apps") {
        t.doubt(
            "U-after-uninstall",
            "Done leaves the user on the removed app's page",
        );
    }
    t.end(gone);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_07_uninstall_a_running_app() {
    let a = apps_mac();
    a.mac.run_app(&a.editor);
    let mut h = harness(&a.mac);
    let mut t = Task::new("uninstall Editor while it is running");
    go(&mut t, &mut h, "Apps", Some("App Remover"));
    t.click(&mut h, "Editor");
    t.see(&h, "Quit Editor first");
    eprintln!("      buttons: {:?}", buttons(&h));
    let way = buttons(&h).iter().any(|b| b.starts_with("Quit"));
    if !way {
        t.doubt(
            "U-running-dead-end",
            "no Quit button: the user must leave CleanCraft",
        );
    }
    t.end(way);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_08_reset_an_app() {
    let a = apps_mac();
    let mut h = harness(&a.mac);
    let mut t = Task::new("reset Editor");
    go(&mut t, &mut h, "Apps", Some("App Remover"));
    t.click(&mut h, "Editor");
    t.click(&mut h, "Reset");
    eprintln!(
        "      dialog: {:?}",
        texts(&h)
            .iter()
            .filter(|s| s.contains("Reset"))
            .collect::<Vec<_>>()
    );
    t.click(&mut h, "Move to Trash");
    let done = a.editor.exists() && a.leftovers.iter().all(|p| !p.exists());
    t.end(done);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_09_update_apps() {
    const CAST: &str = "https://example.com/editor/cast.xml";
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        build: "200",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.web(
        CAST,
        &cleancraft_core::testkit_apps::appcast(&[("210", "2.1", "<ul><li>Faster</li></ul>")]),
    );
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("update apps");
    go(&mut t, &mut h, "Apps", Some("App Updates"));
    t.click(&mut h, "Check for Updates");
    eprintln!("      buttons: {:?}", buttons(&h));
    eprintln!(
        "      texts: {:?}",
        texts(&h)
            .into_iter()
            .filter(|s| s.contains("2.1") || s.contains("Update"))
            .collect::<Vec<_>>()
    );
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_10_turn_off_a_startup_item() {
    let mac = tp::startup_mac();
    base(&mac);
    mac.env.fake_sys().unwrap().answer(
        "osascript -e tell application \"System Events\" to delete login item \"Dropbox\"",
        "",
    );
    let mut h = harness(&mac);
    let mut t = Task::new("turn off the Dropbox login item");
    go(&mut t, &mut h, "Speed", Some("Startup Items"));
    eprintln!("      buttons: {:?}", buttons(&h));
    t.click_contains(&mut h, "Dropbox");
    eprintln!(
        "      after: {:?}",
        texts(&h)
            .into_iter()
            .filter(|s| s.contains("Dropbox"))
            .collect::<Vec<_>>()
    );
    eprintln!("      notice: {:?}", h.state().notice);
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_11_run_a_tune_up_task() {
    let mac = FakeMac::new();
    let f = mac.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("run Free up purgeable space");
    go(&mut t, &mut h, "Speed", Some("Tune-up Tasks"));
    eprintln!("      buttons: {:?}", buttons(&h));
    t.click(&mut h, "Free up purgeable space");
    eprintln!("      buttons: {:?}", buttons(&h));
    t.click(&mut h, "Run Selected");
    eprintln!("      dialog buttons: {:?}", buttons(&h));
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_12_find_what_fills_the_disk() {
    let (mac, _) = files_mac();
    let mut h = harness(&mac);
    let mut t = Task::new("find what fills the disk (Disk Map)");
    go(&mut t, &mut h, "Files", Some("Disk Map"));
    t.click(&mut h, "Map Home Folder");
    t.see(&h, "Downloads");
    t.click(&mut h, "Block Downloads");
    eprintln!(
        "      texts: {:?}",
        texts(&h).into_iter().take(40).collect::<Vec<_>>()
    );
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_13_threats_and_privacy() {
    let mac = testkit_cleanup::privacy_mac();
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("check threats and privacy traces");
    go(&mut t, &mut h, "Protect", Some("Threat Check"));
    t.click(&mut h, "Scan");
    eprintln!(
        "      threat texts: {:?}",
        texts(&h).into_iter().take(30).collect::<Vec<_>>()
    );
    t.click(&mut h, "Privacy Traces");
    t.click(&mut h, "Scan");
    eprintln!("      privacy buttons: {:?}", buttons(&h));
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_14_undo_from_history() {
    let mac = junk();
    let mut h = harness(&mac);
    let mut t = Task::new("undo one cleanup from History");
    go(&mut t, &mut h, "Clean", Some("System Junk"));
    t.click(&mut h, "Scan");
    t.click(&mut h, "Review and Clean");
    t.click(&mut h, "Move to Trash");
    t.click(&mut h, "History");
    let n = count(&h, "Undo");
    eprintln!("      Undo buttons in History for one cleanup: {n}");
    if n > 1 {
        t.doubt(
            "U-history-per-item",
            "one cleanup is one row per file; no Undo for the whole run",
        );
    }
    t.click(&mut h, "Undo");
    t.see(&h, "Put back.");
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_15_change_settings_and_sounds() {
    let mac = FakeMac::new();
    base(&mac);
    let mut h = harness(&mac);
    let mut t = Task::new("change settings: quarantine, sounds off");
    t.click(&mut h, "Settings");
    eprintln!("      controls: {:?}", buttons(&h));
    t.click_contains(&mut h, "CleanCraft quarantine");
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_16_full_disk_access_missing() {
    let mac = junk();
    let mut h = harness_fda(&mac, Access::Missing);
    let mut t = Task::new("work with Full Disk Access missing");
    t.see(&h, "Turn on Full Disk Access");
    t.click(&mut h, "Settings");
    if t.see(&h, "Turn on Full Disk Access") {
        t.doubt(
            "U-fda-everywhere",
            "the banner sits on Settings and History too, and cannot be closed",
        );
    }
    t.end(true);
}

#[test]
#[ignore = "usability walkthrough: run with --ignored --nocapture"]
fn task_17_recover_from_errors() {
    let mac = junk();
    let mut h = harness(&mac);
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    let logs = mac.env.home_path("Library/Logs/Example");
    mac.env.inject_fault(&browser, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&editor.join("index"));
    mac.env.inject_fault(&logs, Fault::DiskFull);
    let mut t = Task::new("recover from permission denied, file in use, disk full");
    go(&mut t, &mut h, "Clean", Some("System Junk"));
    t.click(&mut h, "Scan");
    t.click(&mut h, "Review and Clean");
    t.click(&mut h, "Move to Trash");
    t.see(&h, "permission denied");
    t.see(&h, "open in a running app");
    t.see(&h, "the disk is full");
    eprintln!("      buttons on Done: {:?}", buttons(&h));
    let retry = buttons(&h).iter().any(|b| b.contains("Try Again"));
    if !retry {
        t.doubt(
            "U-no-retry",
            "no way to retry the skipped items after quitting the app",
        );
    }
    t.end(retry);
}

// ======================================================================================
// Findings: each test fails today and passes once U## is fixed (then drop its #[ignore])
// ======================================================================================

fn open_junk_results(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    h.get_by_label("Scan").click();
    settle(&mut h);
    h
}

fn clean_junk(h: &mut H) {
    h.get_by_label("Review and Clean").click();
    settle(h);
    let label = if has(h, "Move to Quarantine") {
        "Move to Quarantine"
    } else {
        "Move to Trash"
    };
    h.get_by_label(label).click();
    settle(h);
}

fn open_editor(a: &Apps) -> H {
    let mut h = harness(&a.mac);
    h.state_mut().navigate("uninstaller");
    settle(&mut h);
    h.get_by_label("Editor").click();
    settle(&mut h);
    h
}

#[test]
fn u01_running_app_offers_quit_in_place() {
    let a = apps_mac();
    a.mac.run_app(&a.editor);
    let mut h = open_editor(&a);
    snap(&mut h, "u01-running-app");
    let b = buttons(&h);
    let Some(quit) = b.iter().find(|l| l.starts_with("Quit Editor")).cloned() else {
        panic!("no Quit button while Editor runs; buttons: {b:?}");
    };
    // The user quits it from here and Editor obeys (the fake stops it as the request goes out).
    testkit_cleanup::clear_running(&a.mac.env);
    h.get_by_label(&quit).click();
    settle(&mut h);
    if let Some(q) = buttons(&h).into_iter().find(|l| l.starts_with("Quit")) {
        // A confirmation dialog, if there is one.
        h.get_by_label(&q).click();
        settle(&mut h);
    }
    for _ in 0..40 {
        h.step();
    }
    let enabled = buttons(&h);
    assert!(
        enabled.iter().any(|l| l == "Uninstall"),
        "Editor quit, yet Uninstall stays disabled; buttons: {enabled:?}"
    );
}

#[test]
fn u02_running_helper_is_named() {
    let a = apps_mac();
    // Editor itself is quit; only its login helper inside the bundle runs.
    testkit_cleanup::set_running(
        &a.mac.env,
        "EditorHelper",
        "/Applications/Editor.app/Contents/Library/LoginItems/EditorHelper.app",
    );
    let h = open_editor(&a);
    assert!(
        has_text(&h, "EditorHelper"),
        "the user quit Editor, is told 'Quit Editor first' and never learns that EditorHelper runs"
    );
}

#[test]
fn u03_trash_bins_has_no_dead_selection_controls() {
    let mac = testkit_cleanup::trash_mac();
    base(&mac);
    let mut h = harness(&mac);
    h.state_mut().navigate("trash_bins");
    settle(&mut h);
    h.get_by_label("Scan").click();
    settle(&mut h);
    let b = buttons(&h);
    assert!(
        !has_text(&h, "Tick the items to clean."),
        "hint asks for the impossible"
    );
    assert!(
        !b.iter().any(|l| l == "Select All" || l == "Ignore"),
        "Select All / Ignore offered on a page where nothing can be selected: {b:?}"
    );
}

#[test]
fn u04_done_offers_to_empty_the_trash() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    clean_junk(&mut h);
    assert!(has_text(&h, "moved to the Trash"));
    let b = buttons(&h);
    assert!(
        b.iter().any(|l| l.contains("Empty Trash")),
        "Done screen has no Empty Trash; buttons: {b:?}"
    );
    assert!(
        !has_text(&h, "quarantine is cleared"),
        "quarantine mentioned although Settings uses the Trash"
    );
}

#[test]
fn u05_quarantine_can_be_seen_and_emptied() {
    let mac = junk();
    Settings {
        method: Method::Quarantine,
        ..Default::default()
    }
    .save(&mac.env)
    .expect("save");
    let mut h = open_junk_results(&mac);
    clean_junk(&mut h);
    h.state_mut().navigate("settings");
    settle(&mut h);
    let b = buttons(&h);
    // The lead named the action "Move Quarantine to Trash..." (fix5): CleanCraft never
    // deletes for good, so the quarantine goes to the Trash and Finder empties it.
    assert!(
        b.iter().any(|l| l.contains("Move Quarantine to Trash")),
        "nothing shows or empties the quarantine; buttons: {b:?}"
    );
    let (bytes, items) = executor::quarantine_size(&mac.env);
    assert!(items > 0 && has_text(&h, "The quarantine holds"));
    h.get_by_label("Move Quarantine to Trash...").click();
    settle(&mut h);
    h.get_by_label("Move to Trash").click();
    settle(&mut h);
    assert_eq!(executor::quarantine_size(&mac.env), (0, 0), "{bytes} bytes left");
    assert!(has_text(&h, "The quarantine is empty."));
    assert!(has_text(&h, "moved from the quarantine to the Trash"));
    // Undo in the notice puts it back.
    h.get_by_label("Undo Move to Trash").click();
    settle(&mut h);
    assert_eq!(executor::quarantine_size(&mac.env), (bytes, items));
}

#[test]
fn u06_history_groups_a_cleanup_into_one_undo() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    clean_junk(&mut h);
    let moved = executor::history(&mac.env).len();
    h.state_mut().navigate("history");
    settle(&mut h);
    let undos = count(&h, "Undo");
    assert!(
        undos <= 1 || has(&h, "Undo All"),
        "one cleanup of {moved} files shows {undos} Undo buttons and no Undo All"
    );
}

#[test]
fn u07_history_hides_undo_for_items_no_longer_in_the_trash() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    clean_junk(&mut h);
    // Finder empties the Trash (inside the fake Mac only).
    for r in executor::history(&mac.env) {
        if let cleancraft_core::ActionKind::Moved { stored, .. } = &r.kind {
            let _ = std::fs::remove_dir_all(stored).or_else(|_| std::fs::remove_file(stored));
        }
    }
    h.state_mut().navigate("history");
    settle(&mut h);
    assert_eq!(
        count(&h, "Undo"),
        0,
        "Undo offered for items that are gone; clicking it only says 'Could not put it back'"
    );
}

#[test]
fn u08_ignore_can_be_undone_on_the_spot() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    h.get_all_by_label("Ignore")
        .next()
        .expect("an Ignore")
        .click();
    settle(&mut h);
    snap(&mut h, "u08-after-ignore");
    let note = h.state().notice.clone();
    let b = buttons(&h);
    assert!(
        note.is_some_and(|(m, _)| m.to_lowercase().contains("ignore"))
            && b.iter()
                .any(|l| l == "Undo" || l.starts_with("Stop Ignoring")),
        "no feedback and no way back after Ignore; buttons: {b:?}"
    );
}

#[test]
fn u09_skipped_items_say_what_to_do_and_can_be_retried() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    let editor = mac.env.home_path("Library/Caches/com.example.Editor");
    let browser = mac.env.home_path("Library/Caches/com.example.Browser");
    mac.env.fake_sys().unwrap().set_open(&editor.join("index"));
    mac.env.inject_fault(&browser, Fault::PermissionDenied);
    clean_junk(&mut h);
    snap(&mut h, "u09-not-moved");
    assert!(has(&h, "open in a running app"));
    let b = buttons(&h);
    assert!(
        b.iter().any(|l| l.contains("Try Again")),
        "no Try Again for the skipped items; buttons: {b:?}"
    );
    assert!(
        has_text(&h, "Full Disk Access") || b.iter().any(|l| l == "Open Privacy Settings"),
        "'permission denied' gives no next step"
    );
}

#[test]
fn u10_return_confirms_the_dialog() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    h.get_by_label("Review and Clean").click();
    settle(&mut h);
    assert!(has(&h, "Move to Trash"));
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(
        has_text(&h, "moved to the Trash"),
        "Return in the confirmation did nothing"
    );
}

#[test]
fn u10_standard_shortcuts() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    settle(&mut h);
    h.event(egui::Event::Text("Browser".into()));
    settle(&mut h);
    let searched = h
        .state_mut()
        .plan_state(FeatureId::SystemJunk)
        .search
        .clone();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    settle(&mut h);
    let settings = matches!(h.state().page, cleancraft_ui_egui::app::Page::Settings);
    assert!(
        searched == "Browser" && settings,
        "Cmd+F focused search: {:?}; Cmd+, opened Settings: {settings}",
        searched
    );
}

#[test]
fn u11_duplicates_mark_and_choose_the_kept_copy() {
    let (mac, _) = files_mac();
    let mut h = harness(&mac);
    h.state_mut().navigate("duplicates");
    settle(&mut h);
    h.get_by_label("Scan").click();
    settle(&mut h);
    let b = buttons(&h);
    assert!(
        has_text(&h, "Kept") || b.iter().any(|l| l.starts_with("Keep This")),
        "no 'Kept' mark and no per-row 'Keep This One'; buttons: {b:?}"
    );
}

#[test]
fn u12_ignore_list_has_a_folder_picker() {
    let mac = FakeMac::new();
    base(&mac);
    let mut h = harness(&mac);
    h.state_mut().navigate("settings");
    settle(&mut h);
    let b = buttons(&h);
    assert!(
        b.iter()
            .any(|l| l.starts_with("Add Folder") || l.starts_with("Choose")),
        "no folder picker for the ignore list; buttons: {b:?}"
    );
}

#[test]
fn u13_one_name_for_scanning_again() {
    let names = [
        "Scan Again",
        "Look Again",
        "Check Again",
        "Refresh",
        "Map Again",
    ];
    let mut seen = std::collections::BTreeSet::new();
    // Plan page results.
    let mac = junk();
    let h = open_junk_results(&mac);
    seen.extend(names.iter().filter(|n| has(&h, n)).map(|s| s.to_string()));
    // App Remover, Startup, Tune-up, History, Disk Map.
    let (fm, _) = files_mac();
    let mut h = harness(&fm);
    for page in ["uninstaller", "login_items", "maintenance", "history"] {
        h.state_mut().navigate(page);
        settle(&mut h);
        seen.extend(names.iter().filter(|n| has(&h, n)).map(|s| s.to_string()));
    }
    h.state_mut().navigate("disk_map");
    settle(&mut h);
    h.get_by_label("Map Home Folder").click();
    settle(&mut h);
    seen.extend(names.iter().filter(|n| has(&h, n)).map(|s| s.to_string()));
    assert!(seen.len() <= 1, "rescan is called {seen:?}");
}

#[test]
fn u14_notice_clears_on_page_change() {
    let mac = junk();
    let mut h = open_junk_results(&mac);
    clean_junk(&mut h);
    h.state_mut().navigate("history");
    settle(&mut h);
    h.get_all_by_label("Undo").next().expect("Undo").click();
    settle(&mut h);
    assert!(has_text(&h, "Put back."));
    h.get_by_label("Settings").click();
    settle(&mut h);
    assert!(
        !has_text(&h, "Put back."),
        "History's notice is still shown on Settings"
    );
}

#[test]
fn u14_fda_banner_only_where_needed_and_dismissable() {
    let mac = junk();
    let mut h = harness_fda(&mac, Access::Missing);
    let on_sweep = has_text(&h, "Turn on Full Disk Access");
    let not_now = buttons(&h).iter().any(|l| l == "Not Now" || l == "Dismiss");
    h.state_mut().navigate("settings");
    settle(&mut h);
    let on_settings = has_text(&h, "Turn on Full Disk Access");
    assert!(
        on_sweep && not_now && !on_settings,
        "banner on Quick Sweep {on_sweep}, can be closed {not_now}, also on Settings {on_settings}"
    );
}

#[test]
fn u16_pointing_hand_over_buttons() {
    let mac = junk();
    let mut h = harness(&mac);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    h.get_by_label("Scan").hover();
    settle(&mut h);
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "arrow cursor over the Scan button"
    );
}

#[test]
fn u18_large_old_sorts_by_last_opened() {
    let (mac, _) = files_mac();
    let mut h = harness(&mac);
    h.state_mut().navigate("large_old_files");
    settle(&mut h);
    h.get_by_label("Scan").click();
    settle(&mut h);
    h.get_by_label("Sort").click();
    settle(&mut h);
    assert!(
        has_text(&h, "Sort by date") || has_text(&h, "Sort by last opened"),
        "only size and name: an 'old' file cannot be found by age; texts: {:?}",
        texts(&h)
            .into_iter()
            .filter(|s| s.starts_with("Sort"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn u19_login_item_error_is_in_plain_words() {
    let mac = tp::startup_mac();
    base(&mac);
    mac.env.fake_sys().unwrap().answer_full(
        "osascript -e tell application \"System Events\" to delete login item \"Dropbox\"",
        cleancraft_core::sys::CmdOut {
            status: 1,
            stdout: String::new(),
            stderr:
                "execution error: Not authorized to send Apple events to System Events. (-1743)"
                    .into(),
        },
    );
    let mut h = harness(&mac);
    h.state_mut().navigate("login_items");
    settle(&mut h);
    h.get_by_label("Turn Off Dropbox").click();
    settle(&mut h);
    snap(&mut h, "u19-raw-error");
    let msg = h.state().notice.clone().map(|n| n.0).unwrap_or_default();
    assert!(
        !msg.contains("-1743") && !msg.contains("osascript") && msg.contains("Automation"),
        "raw error shown: {msg}"
    );
}

#[test]
fn u17_quick_sweep_tiles_follow_a_module_cleanup() {
    let mac = sweep();
    let mut h = harness(&mac);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    h.get_by_label("Scan").click();
    settle(&mut h);
    assert!(has(&h, "13.3 MB"), "System Junk tile before");
    // Review on the System Junk tile, clean there, come back.
    h.get_all_by_label("Review").next().expect("Review").click();
    settle(&mut h);
    clean_junk(&mut h);
    h.get_by_label("Done").click();
    settle(&mut h);
    h.get_by_label("Quick Sweep").click();
    settle(&mut h);
    snap(&mut h, "u17-stale-sweep");
    assert!(
        !has(&h, "13.3 MB"),
        "Quick Sweep still offers what was just cleaned; texts: {:?}",
        texts(&h)
            .into_iter()
            .filter(|s| s.contains("MB"))
            .collect::<Vec<_>>()
    );
}
