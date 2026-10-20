//! integ6: the round 6 pieces joined. Routes from the menu bar agent, the watchers and
//! `cleancraft open` go through the app's one router: Quick Sweep starts its scan, a trashed
//! app opens App Remover on what it left, a Threat Check finding is selected for review, the
//! Full Disk Access route lands on its Settings group. Settings has the Developer Clutter group
//! and the AI agents links. Nothing here ever moves a file.

mod common;

use cleancraft_core::executor;
use cleancraft_core::notify;
use cleancraft_core::route::AppRoute;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_perf::{add_launch_item, codesign_unsigned};
use cleancraft_core::{Fault, FeatureId, Settings};
use cleancraft_ui_egui::Page;
use cleancraft_ui_egui::pages::settings::{
    AGENT_GUIDE_LINK, CONNECT_AI_BUTTON, DEV_ADD, DEV_CHOOSE, FDA_CHECK, FDA_OPEN,
};
use common::*;
use egui_kittest::kittest::Queryable;

fn mac() -> FakeMac {
    let m = FakeMac::new();
    let f = m.env.fake_sys().expect("fake");
    f.answer("id -u", "501\n");
    f.answer("launchctl", "");
    // `open -b` of the app (the request's wake-up) and the shell it may go through.
    f.answer("open", "");
    f.answer("sh", "");
    m
}

/// Requests are read twice a second (a quarter second per test step).
fn wait_page(h: &mut H, want: Page) {
    for _ in 0..20 {
        if h.state().page == want {
            return;
        }
        h.step();
    }
    assert_eq!(h.state().page, want);
}

fn nothing_moved(m: &FakeMac) {
    assert!(executor::history(&m.env).is_empty(), "a route moved files");
    assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
}

fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

#[test]
fn the_scheduled_scan_route_opens_quick_sweep_and_starts_its_scan() {
    let m = mac();
    m.home_file("Library/Caches/com.example.app/blob", 2_000_000);
    let mut h = harness(&m);
    click(&mut h, "Settings");
    assert!(matches!(
        h.state().sweep.phase,
        cleancraft_ui_egui::pages::sweep::Phase::Idle
    ));
    notify::request_open(&m.env, "scan:quick_sweep").expect("request");
    wait_page(&mut h, Page::Feature(FeatureId::SmartCare));
    steps(&mut h);
    assert!(
        !matches!(
            h.state().sweep.phase,
            cleancraft_ui_egui::pages::sweep::Phase::Idle
        ),
        "the scan started"
    );
    // The same route again keeps the results the person may be reviewing.
    let had = h.state().sweep.result.is_some();
    cleancraft_ui_egui::pages::settings::open_route(h.state_mut(), "scan:quick_sweep");
    steps(&mut h);
    assert_eq!(h.state().sweep.result.is_some(), had);
    nothing_moved(&m);
}

#[test]
fn the_trash_offer_route_opens_app_remover_on_what_the_app_left() {
    let m = mac();
    let trashed = m.env.user_trash().join("Editor.app");
    m.write(
        &trashed.join("Contents/Info.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.Editor</string>
<key>CFBundleName</key><string>Editor</string>
</dict></plist>"#,
    );
    m.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    m.home_file("Library/Preferences/com.example.Editor.plist", 4_000);
    let mut h = harness(&m);
    // The agent writes the route the watcher made: the path as found on this Mac.
    let route = AppRoute::UninstallApp(trashed.clone()).to_arg();
    notify::request_open(&m.env, &route).expect("request");
    wait_page(&mut h, Page::Feature(FeatureId::Uninstaller));
    assert_eq!(h.state().apps.selected.as_deref(), Some(trashed.as_path()));
    let plan = h.state().apps.detail.clone().expect("App Remover plan");
    let paths: Vec<_> = plan.items.iter().map(|i| i.path.clone()).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("Library/Caches/com.example.Editor")),
        "{paths:?}"
    );
    assert!(
        !paths.contains(&trashed),
        "the app is in the Trash already: only its leftovers are listed"
    );
    assert!(trashed.exists());
    nothing_moved(&m);
}

#[test]
fn the_threat_route_selects_the_finding_and_says_when_it_is_gone() {
    let m = mac();
    let prog = "/Library/Application Support/Example/helper";
    let plist = add_launch_item(
        &m,
        "~/Library/LaunchAgents",
        "com.example.helper",
        prog,
        Some(codesign_unsigned(prog)),
    );
    let mut h = harness(&m);
    notify::request_open(&m.env, &AppRoute::ThreatReview(plist.clone()).to_arg()).expect("request");
    wait_page(&mut h, Page::Feature(FeatureId::Malware));
    steps(&mut h);
    {
        let app = h.state_mut();
        let st = app.plan_state(FeatureId::Malware);
        let plan = st.plan.as_ref().expect("Threat Check scanned");
        let it = plan.items.iter().find(|i| i.path == plist).expect("listed");
        assert!(it.selected, "the finding is selected for review");
        assert!(app.pending_threat.is_none());
    }
    assert!(has_text(&h, "is selected for review"), "told why");
    assert!(plist.exists(), "selecting never removes");

    // A finding the scan no longer lists: said in words, nothing else selected.
    let gone = m
        .env
        .home_path("Library/LaunchAgents/com.example.gone.plist");
    notify::request_open(&m.env, &AppRoute::ThreatReview(gone).to_arg()).expect("request");
    for _ in 0..10 {
        h.step();
    }
    assert!(
        has_text(&h, "did not list"),
        "the person is told it is gone"
    );
    nothing_moved(&m);
}

#[test]
fn the_full_disk_access_route_lands_on_its_group_with_the_way_forward() {
    let m = mac();
    m.home_file("Library/Safari/Bookmarks.plist", 10);
    m.env.inject_fault(
        &m.env.home_path("Library/Safari/Bookmarks.plist"),
        Fault::PermissionDenied,
    );
    let mut h = harness(&m);
    notify::request_open(&m.env, &AppRoute::FullDiskAccess.to_arg()).expect("request");
    wait_page(&mut h, Page::Settings);
    steps(&mut h);
    assert!(has_text(
        &h,
        "Off: macOS keeps Mail, Safari and the Trash private."
    ));
    // Scrolled into view: the buttons are reachable without scrolling.
    click(&mut h, FDA_OPEN);
    let urls = m.env.fake_sys().unwrap().with(|s| s.opened_urls.clone());
    assert!(
        urls.iter()
            .any(|u| u == cleancraft_core::permissions::FDA_SETTINGS_URL),
        "{urls:?}"
    );
    click(&mut h, FDA_CHECK);
    assert!(has(&h, "Full Disk Access is still off."));
    // Granted meanwhile: Check Again says so and the Open button goes away.
    m.env.clear_faults();
    click(&mut h, FDA_CHECK);
    assert!(has(&h, "Full Disk Access is on."));
    assert!(!has(&h, FDA_OPEN));
    nothing_moved(&m);
}

#[test]
fn developer_clutter_code_folders_are_added_checked_and_removed() {
    let m = mac();
    let code = m.env.home_path("Work/code");
    std::fs::create_dir_all(&code).unwrap();
    m.home_file("notes.txt", 10);
    let mut h = harness(&m);
    click(&mut h, "Settings");
    h.get_by_label(DEV_ADD).scroll_to_me();
    steps(&mut h);
    assert!(has_text(&h, "Projects are looked for in ~/Developer"));
    assert!(has(&h, "No code folders of your own yet."));

    // Empty, missing, a file and the home folder are refused with the reason.
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "Type the path of a code folder first"));
    type_into(&mut h, "Path of a code folder", "~/Nowhere");
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "Nothing is at"));
    clear_field(&mut h);
    type_into(&mut h, "Path of a code folder", "~/notes.txt");
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "is not a folder"));
    clear_field(&mut h);
    type_into(&mut h, "Path of a code folder", "~");
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "holds your whole home folder"));
    assert!(Settings::load(&m.env).dev_roots.is_empty());

    // A real folder is added and saved.
    clear_field(&mut h);
    type_into(&mut h, "Path of a code folder", "~/Work/code");
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "is added"));
    assert_eq!(Settings::load(&m.env).dev_roots, vec![code.clone()]);
    // Twice is refused.
    type_into(&mut h, "Path of a code folder", "~/Work/code");
    click(&mut h, DEV_ADD);
    assert!(has_text(&h, "on the list already"));

    // Choose Folder: the macOS dialog's answer is added; Cancel adds nothing.
    let other = m.env.home_path("Sites");
    std::fs::create_dir_all(&other).unwrap();
    let f = m.env.fake_sys().unwrap();
    f.answer(
        "/usr/bin/osascript",
        &format!("{}/\n", m.env.display(&other)),
    );
    reach(&mut h, DEV_CHOOSE);
    steps(&mut h);
    let roots = Settings::load(&m.env).dev_roots;
    assert_eq!(roots.len(), 2, "{roots:?}");
    assert!(roots[1].ends_with("Sites"), "{roots:?}");

    // Remove takes it off the list.
    let shown = m.env.display(&code);
    reach(&mut h, &format!("Remove code folder {shown}"));
    let roots = Settings::load(&m.env).dev_roots;
    assert_eq!(roots.len(), 1);
    assert!(!roots.contains(&code));
    assert!(
        code.exists(),
        "removing from the list leaves the folder alone"
    );
    nothing_moved(&m);
}

#[test]
fn inactivity_days_slider_is_saved() {
    let m = mac();
    let mut h = harness(&m);
    click(&mut h, "Settings");
    let label = "Projects count as inactive after (days)";
    h.get_by_label(label).scroll_to_me();
    steps(&mut h);
    let node = h.get_by_label(label);
    node.focus();
    steps(&mut h);
    h.key_press(egui::Key::ArrowRight);
    steps(&mut h);
    let days = Settings::load(&m.env).dev_inactive_days;
    assert!(days > 30, "{days}");
}

#[test]
fn ai_agents_group_links_to_the_guide_and_the_connect_step() {
    let m = mac();
    let mut h = harness(&m);
    click(&mut h, "Settings");
    reach(&mut h, CONNECT_AI_BUTTON);
    assert_eq!(h.state().page, Page::ConnectAi);
    click(&mut h, "Settings");
    reach(&mut h, AGENT_GUIDE_LINK);
    assert_eq!(h.state().page, Page::AgentGuide);
}

fn clear_field(h: &mut H) {
    h.state_mut().settings_page.new_dev_root.clear();
    steps(h);
}
