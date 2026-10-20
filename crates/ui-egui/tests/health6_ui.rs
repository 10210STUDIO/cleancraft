//! Mac Health, the AI apps step, the agent guide, routes, and the repaint budget, driven by
//! clicks on fake Macs.

mod common;

use cleancraft_core::health::{Facts, Level};
use cleancraft_core::monitor::BatteryReading;
use cleancraft_core::route::AppRoute;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_health::{self as th, GB};
use cleancraft_core::{FeatureId, mcp_setup};
use cleancraft_ui_egui::app::Page;
use cleancraft_ui_egui::pages::health::{self, Read};
use common::{H, click, harness, has, has_text, steps};
use egui_kittest::kittest::{NodeT, Queryable};

/// Click the first widget with this label (the sidebar entry before the switcher segment).
fn click_first(h: &mut H, label: &str) {
    h.get_all_by_label(label).next().expect(label).click();
    steps(h);
}

/// The app in a tall window, so every card of a long page is on screen.
fn tall(mac: &FakeMac) -> H {
    let app = cleancraft_ui_egui::CleanApp::new(mac.env.clone()).with_sync();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 2400.0))
        .build_ui_state(
            |ui, app: &mut cleancraft_ui_egui::CleanApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app,
        );
    steps(&mut h);
    h
}

fn open_health(h: &mut H) {
    click_first(h, "Live Monitor");
    click(h, "Mac Health");
    assert_eq!(h.state().page, Page::Health);
    steps(h);
}

#[test]
fn health_is_a_tool_of_live_monitor_and_reads_good() {
    let mac = th::calm_mac_now();
    let mut h = harness(&mac);
    // Seven sidebar entries, as before.
    for name in [
        "Quick Sweep",
        "Clean",
        "Protect",
        "Speed",
        "Apps",
        "Files",
        "Live Monitor",
    ] {
        assert!(has(&h, name), "{name}");
    }
    open_health(&mut h);
    assert!(has(&h, "Good"));
    assert!(has_text(&h, "No rule that needs attention applies"));
    assert!(h.state().health.cards.is_empty(), "no cards on a calm Mac");
    assert!(has_text(&h, "Nothing to do right now"));
    assert!(has_text(&h, "macOS 15.1"));
    assert!(has_text(&h, "Free space"));
    // The switcher goes back to Live Monitor, and the sidebar remembers Mac Health.
    // The switcher's Live Monitor segment (the second node with that name).
    h.get_all_by_label("Live Monitor").nth(1).unwrap().click();
    steps(&mut h);
    assert_eq!(h.state().page, Page::Feature(FeatureId::Monitor));
    click(&mut h, "Mac Health");
    click(&mut h, "Quick Sweep");
    click_first(&mut h, "Live Monitor");
    assert_eq!(h.state().page, Page::Health);
}

/// Each card appears for its condition, says why the state is what it is, and opens the
/// right page.
#[test]
fn every_card_opens_its_page() {
    let cases: Vec<(&str, &str, AppRoute)> = vec![
        (
            "Only 45.0 GB free",
            "See What Takes Space",
            AppRoute::Feature(FeatureId::SpaceLens),
        ),
        (
            "Free space fell by",
            "Open Disk Map",
            AppRoute::Feature(FeatureId::SpaceLens),
        ),
        (
            "Threat Check found 1 item",
            "Review in Threat Check",
            AppRoute::Feature(FeatureId::Malware),
        ),
        (
            "1 startup item is not signed",
            "Review Startup Items",
            AppRoute::Feature(FeatureId::LoginItems),
        ),
        (
            "Memory pressure is elevated",
            "Open Live Monitor",
            AppRoute::Feature(FeatureId::Monitor),
        ),
        (
            "The battery holds 75%",
            "Open Live Monitor",
            AppRoute::Feature(FeatureId::Monitor),
        ),
        (
            "2.30 GB of junk found 4 days ago",
            "Review in System Junk",
            AppRoute::Feature(FeatureId::SystemJunk),
        ),
        (
            "3 apps not opened for 6 months",
            "Review in App Remover",
            AppRoute::UnusedApps,
        ),
        (
            "Updates not checked in the last 14 days",
            "Check in App Updates",
            AppRoute::Feature(FeatureId::Updater),
        ),
    ];
    for (headline, action, route) in cases {
        let mac = th::troubled_mac_now();
        let mut h = tall(&mac);
        open_health(&mut h);
        assert!(has(&h, "Needs attention"), "state");
        assert!(has_text(&h, headline), "card {headline}");
        let cards = h.state().health.cards.clone();
        let idx = cards
            .iter()
            .position(|c| c.headline.starts_with(headline))
            .unwrap_or_else(|| panic!("{headline} in {cards:#?}"));
        assert_eq!(cards[idx].route, route);
        // Its button: the n-th with that label, in card order.
        let n = cards[..idx].iter().filter(|c| c.action == action).count();
        let node = h.get_all_by_label(action).nth(n).expect("button");
        node.click();
        steps(&mut h);
        let want = match &route {
            AppRoute::Feature(f) => Page::Feature(*f),
            AppRoute::UnusedApps => Page::Feature(FeatureId::Uninstaller),
            _ => unreachable!(),
        };
        assert_eq!(h.state().page, want, "{action}");
        if route == AppRoute::UnusedApps {
            assert_eq!(
                h.state().apps.group,
                cleancraft_ui_egui::pages::apps::Group::Unused
            );
        }
        // Nothing was cleaned or run: the action log is empty.
        assert!(cleancraft_core::executor::history(&mac.env).is_empty());
    }
}

#[test]
fn why_lists_the_deciding_cards_and_rules_are_shown() {
    let mac = th::troubled_mac_now();
    let mut h = harness(&mac);
    open_health(&mut h);
    let why = h
        .query_all_by_label_contains("Why: ")
        .next()
        .expect("why line")
        .accesskit_node()
        .label()
        .or(None)
        .map(|s| s.to_string())
        .or_else(|| {
            h.query_all_by_label_contains("Why: ")
                .next()
                .and_then(|n| n.accesskit_node().value().map(|v| v.to_string()))
        })
        .unwrap_or_default();
    for c in h.state().health.cards.clone() {
        assert_eq!(
            why.contains(&c.headline),
            c.level >= Level::Attention,
            "{} in {why}",
            c.headline
        );
    }
    click(&mut h, "How this is decided");
    for r in cleancraft_core::health::RULES {
        assert!(has(&h, r.text), "rule shown: {}", r.text);
    }
    click(&mut h, "Hide how this is decided");
    assert!(!has(&h, cleancraft_core::health::RULES[0].text));
}

#[test]
fn stale_junk_card_starts_a_scan_that_only_looks() {
    let mac = th::calm_mac_now();
    th::note(&mac.env, FeatureId::SystemJunk, 45, 0, 0, th::now());
    mac.home_file("Library/Caches/com.example.App/blob", 400_000);
    let mut h = harness(&mac);
    open_health(&mut h);
    click(&mut h, "Scan System Junk");
    assert_eq!(h.state().page, Page::Feature(FeatureId::SystemJunk));
    let st = h
        .state()
        .plans
        .get(&FeatureId::SystemJunk)
        .expect("scanned");
    assert!(st.plan.is_some(), "the scan ran and shows its list");
    assert!(
        mac.env
            .home_path("Library/Caches/com.example.App/blob")
            .exists()
    );
    // The new scan is noted for Mac Health.
    let notes = cleancraft_core::health::scan_notes(&mac.env);
    assert!(notes[&FeatureId::SystemJunk].bytes >= 400_000);
}

#[test]
fn trash_card_from_facts() {
    let mac = th::calm_mac_now();
    let mut h = harness(&mac);
    open_health(&mut h);
    let mut f = h.state().health.facts.clone().unwrap();
    f.trash = Some(6 * GB);
    let history = h.state().health.history.clone();
    h.state_mut().health.set(Read {
        facts: f,
        history,
        growth: None,
    });
    steps(&mut h);
    assert!(has_text(&h, "The Trash holds 6.00 GB"));
    click(&mut h, "Review in Trash Bins");
    assert_eq!(h.state().page, Page::Feature(FeatureId::TrashBins));
}

#[test]
fn unknown_facts_say_unknown() {
    let rows = health::fact_rows(&Facts::default());
    for (k, v) in &rows {
        if *k == "Change in 7 days" {
            assert_eq!(v, "Not enough readings yet");
        } else if ["Last junk scan", "Last Threat Check", "Updates"].contains(k) {
            assert_eq!(v, "Never");
        } else {
            assert_eq!(v, "Unknown", "{k}");
        }
    }
    let f = Facts {
        battery: Some(BatteryReading::None),
        ..Default::default()
    };
    assert!(
        health::fact_rows(&f)
            .iter()
            .any(|(_, v)| v == "No battery (desktop Mac)")
    );
}

#[test]
fn chart_ranges_switch_and_count_readings() {
    let mac = th::calm_mac_now();
    let mut h = harness(&mac);
    open_health(&mut h);
    assert!(has_text(&h, "readings in the last 7 days"));
    let n7 = health::chart_points(&h.state().health.history, th::now(), 7).len();
    click(&mut h, "90 days");
    assert_eq!(h.state().health.range, 2);
    assert!(has_text(&h, "in the last 90 days"));
    let n90 = health::chart_points(&h.state().health.history, th::now(), 90).len();
    assert!(n90 > n7 * 10, "{n7} {n90}");
    click(&mut h, "30 days");
    assert_eq!(h.state().health.range, 1);
}

#[test]
fn chart_with_no_history_says_collecting() {
    let mac = th::calm_mac(th::now());
    let mut h = harness(&mac);
    open_health(&mut h);
    assert!(has_text(&h, "No readings yet"));
}

#[test]
fn what_grew_after_two_disk_map_scans() {
    use cleancraft_core::health::MapSummary;
    let mac = th::calm_mac_now();
    let home = mac.env.home().to_path_buf();
    let mk = |at, movies| MapSummary {
        at,
        root: home.clone(),
        entries: vec![(home.clone(), 100 * GB), (home.join("Movies"), movies)],
    };
    std::fs::write(
        mac.env.data_dir().join("disk-map-history.json"),
        serde_json::to_vec(&vec![mk(th::now() - 86_400 * 3, GB), mk(th::now(), 9 * GB)]).unwrap(),
    )
    .unwrap();
    let mut h = harness(&mac);
    open_health(&mut h);
    assert!(has_text(&h, "+8.00 GB"));
    assert!(has_text(&h, "Disk Map scans of"));
}

#[test]
fn disk_map_scan_is_noted_for_what_grew() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    h.state_mut().navigate("disk_map");
    steps(&mut h);
    // Map the home folder (Disk Map's own start button).
    if let Some(n) = h.query_all_by_label("Map Home Folder").next() {
        n.click();
    } else {
        h.get_by_label("Scan").click();
    }
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(cleancraft_core::health::map_summaries(&mac.env).len(), 1);
}

#[test]
fn routes_open_their_pages() {
    let mac = th::ai_mac();
    let mut h = harness(&mac);
    for (arg, page) in [
        ("health", Page::Health),
        ("guide:ai_agents", Page::AgentGuide),
        ("connect_ai", Page::ConnectAi),
        ("settings", Page::Settings),
        ("history", Page::History),
        ("system_junk", Page::Feature(FeatureId::SystemJunk)),
        ("unused_apps", Page::Feature(FeatureId::Uninstaller)),
    ] {
        h.state_mut().navigate(arg);
        steps(&mut h);
        assert_eq!(h.state().page, page, "{arg}");
    }
    let before = h.state().page;
    h.state_mut().navigate("nowhere");
    assert_eq!(h.state().page, before);
}

#[test]
fn guide_page_renders_and_links() {
    let mac = th::ai_mac();
    let mut h = harness(&mac);
    h.state_mut().open_route(&AppRoute::AgentGuide);
    steps(&mut h);
    assert!(has(&h, "Connect an AI agent to CleanCraft"));
    assert!(has_text(&h, "execute_plan"));
    click(&mut h, "Connect AI Apps");
    assert_eq!(h.state().page, Page::ConnectAi);
    h.state_mut().open_route(&AppRoute::AgentGuide);
    steps(&mut h);
    click(&mut h, "Back to Settings");
    assert_eq!(h.state().page, Page::Settings);
}

// ---------------------------------------------------------------------------------------
// The AI apps step
// ---------------------------------------------------------------------------------------

#[test]
fn first_launch_asks_once_and_not_now_is_remembered() {
    let mac = th::ai_mac();
    let mut h = harness(&mac);
    // Tests do not run launch work by themselves; the real app does at its first frame.
    h.state_mut().launch_checks();
    steps(&mut h);
    assert_eq!(h.state().page, Page::ConnectAi);
    assert!(has_text(&h, "AI apps such as Claude can ask CleanCraft"));
    assert!(has_text(&h, "Claude Desktop"));
    assert!(has_text(&h, "Claude Code"));
    click(&mut h, "Not Now");
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
    assert!(!mcp_setup::welcome_due(&mac.env));
    // Next launch: not asked again.
    let mut h2 = harness(&mac);
    h2.state_mut().launch_checks();
    steps(&mut h2);
    assert_eq!(h2.state().page, Page::Feature(FeatureId::SmartCare));
    // A sample of free space was taken at launch.
    assert_eq!(
        cleancraft_core::space_history::load(&mac.env)
            .history
            .samples
            .len(),
        1
    );
}

#[test]
fn connect_desktop_with_undo_and_the_separate_clean_switch() {
    let mac = th::ai_mac();
    let mut h = harness(&mac);
    h.state_mut().open_route(&AppRoute::ConnectAi);
    steps(&mut h);
    assert!(!h.state().settings.agents_may_clean);
    click(&mut h, "Connect Claude Desktop");
    assert!(has_text(&h, "Quit and reopen Claude Desktop"));
    let cfg = std::fs::read_to_string(mcp_setup::desktop_config(&mac.env)).unwrap();
    assert!(cfg.contains("\"cleancraft\"") && cfg.contains("\"files\""));
    assert!(has(&h, "Connected"));
    // Connecting never turned on cleaning.
    assert!(!h.state().settings.agents_may_clean);
    // Undo runs the journaled command (the fake shell answers it).
    h.get_by_label("Undo Connect Claude Desktop").click();
    steps(&mut h);
    assert!(
        mac.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("mcp-setup remove"))
    );
    assert!(has_text(&h, "no longer connected"));
    // The separate switch, off by default.
    // The switch (its visible label is a second node with the same name).
    h.get_all_by_label(cleancraft_ui_egui::pages::welcome::CLEAN_SWITCH)
        .next()
        .unwrap()
        .click();
    steps(&mut h);
    assert!(h.state().settings.agents_may_clean);
    assert!(
        cleancraft_core::Settings::load_checked(&mac.env)
            .0
            .agents_may_clean
    );
    click(&mut h, "Done");
    assert!(!mcp_setup::welcome_due(&mac.env));
}

#[test]
fn connect_claude_code_through_its_command() {
    let mac = th::ai_mac();
    let mut h = harness(&mac);
    h.state_mut().open_route(&AppRoute::ConnectAi);
    steps(&mut h);
    click(&mut h, "Connect Claude Code");
    assert!(
        mac.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("mcp add --scope user cleancraft"))
    );
    assert!(has_text(&h, "Claude Code is connected"));
}

#[test]
fn invalid_desktop_config_is_refused_and_can_be_opened() {
    let mac = th::ai_mac();
    let p = mcp_setup::desktop_config(&mac.env);
    std::fs::write(&p, "{ not json").unwrap();
    let mut h = harness(&mac);
    h.state_mut().open_route(&AppRoute::ConnectAi);
    steps(&mut h);
    assert!(!has(&h, "Connect Claude Desktop"));
    assert!(has_text(&h, "not valid JSON"));
    click(&mut h, "Open the File");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
    let spawned = mac.env.fake_sys().unwrap().spawned();
    assert!(spawned.iter().any(|c| c.contains("open -t")), "{spawned:?}");
}

#[test]
fn not_installed_shows_why_and_no_connect_button() {
    let mac = th::ai_mac();
    std::fs::remove_file(mac.env.sys_path(mcp_setup::APP_BINARY)).unwrap();
    let mut h = harness(&mac);
    h.state_mut().open_route(&AppRoute::ConnectAi);
    steps(&mut h);
    assert!(has_text(&h, "not installed in Applications"));
    assert!(!has(&h, "Connect Claude Desktop"));
}

// ---------------------------------------------------------------------------------------
// Live Monitor: plain words for processes, sampling rate
// ---------------------------------------------------------------------------------------

fn monitor_mac() -> FakeMac {
    let mac = FakeMac::new();
    let f = mac.env.fake_sys().unwrap();
    cleancraft_core::testkit_monitor::answer_all(f);
    f.answer(
        "ps",
        "  512  71.0 812000 Brave Browser Helper (Renderer)\n  513   9.0 100000 Brave Browser Helper (GPU)\n  514   5.0 300000 Brave Browser\n  801  47.0  50000 WindowServer\n  900  23.0  40000 CleanCraft\n  901   1.0  40000 mds_stores\n",
    );
    mac
}

#[test]
fn monitor_groups_helpers_and_explains_macos_processes() {
    let mac = monitor_mac();
    let mut h = harness(&mac);
    click_first(&mut h, "Live Monitor");
    steps(&mut h);
    assert!(has(&h, "Brave Browser (3 processes)"));
    assert!(has_text(
        &h,
        "Part of macOS: draws everything on your screen"
    ));
    click_nth(&mut h, "Show Each Process", 0);
    assert!(has(&h, "Brave Browser Helper (Renderer)"));
    assert!(has_text(&h, "A tab or page in Brave Browser."));
    click_nth(&mut h, "Group by App", 0);
    assert!(has(&h, "Brave Browser (3 processes)"));
}

fn click_nth(h: &mut H, label: &str, n: usize) {
    h.get_all_by_label(label).nth(n).expect(label).click();
    steps(h);
}

#[test]
fn macos_rows_never_offer_quit_app_rows_link_to_startup_items() {
    let mac = monitor_mac();
    let mut h = tall(&mac);
    click_first(&mut h, "Live Monitor");
    // Hover the WindowServer row of the CPU card: no quit link.
    let r = h.get_all_by_label("WindowServer").next().unwrap().rect();
    h.hover_at(r.center());
    steps(&mut h);
    let links = h.query_all_by_label("Quit in Startup Items...").count();
    assert_eq!(links, 0);
    let r = h
        .get_all_by_label("Brave Browser (3 processes)")
        .next()
        .unwrap()
        .rect();
    h.hover_at(r.center());
    steps(&mut h);
    let link = h
        .get_all_by_label("Quit in Startup Items...")
        .next()
        .expect("link");
    link.click();
    steps(&mut h);
    assert_eq!(h.state().page, Page::Feature(FeatureId::LoginItems));
}

/// Commands per second while Live Monitor shows, over 20 s of simulated time; none while
/// another page shows or the window is minimised.
#[test]
fn sampler_runs_at_most_every_two_seconds_and_only_when_visible() {
    let mac = monitor_mac();
    let mut h = harness(&mac);
    click_first(&mut h, "Live Monitor");
    let calls = |mac: &FakeMac| mac.env.fake_sys().unwrap().calls().len();
    let start = calls(&mac);
    let readings_before = h.state().monitor.samples;
    let t0 = h.ctx.input(|i| i.time);
    let mut t = t0;
    // 60 frames a second for 20 s (the worst case: something else keeps the window busy).
    for _ in 0..1200 {
        t += 1.0 / 60.0;
        h.input_mut().time = Some(t);
        h.step();
    }
    let readings = h.state().monitor.samples - readings_before;
    let commands = calls(&mac) - start;
    eprintln!("Live Monitor: {readings} readings and {commands} commands in 20 s");
    assert!((9..=11).contains(&readings), "{readings} readings in 20 s");
    assert!(
        commands <= 11 * (cleancraft_core::monitor::COMMANDS_PER_READING + 2),
        "{commands} commands in 20 s"
    );
    // Another page: no readings.
    click(&mut h, "Quick Sweep");
    let before = calls(&mac);
    for _ in 0..300 {
        t += 1.0 / 60.0;
        h.input_mut().time = Some(t);
        h.step();
    }
    assert_eq!(calls(&mac), before, "no commands off the page");
    // Minimised on the Monitor page: no readings.
    click_first(&mut h, "Live Monitor");
    let before = calls(&mac);
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .minimized = Some(true);
    for _ in 0..300 {
        t += 1.0 / 60.0;
        h.input_mut().time = Some(t);
        h.input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .minimized = Some(true);
        h.step();
    }
    assert!(calls(&mac) - before <= cleancraft_core::monitor::COMMANDS_PER_READING + 2);
}

// ---------------------------------------------------------------------------------------
// Repaint budget
// ---------------------------------------------------------------------------------------

/// Frames a second an idle page asks for, measured like an event loop at 60 Hz: after each
/// frame the next one comes when the page asked for it (at the next vsync, after a delay, or
/// never: then a 10 s wait stands for "no frame"). Simulated over `secs` seconds after the
/// page settled.
fn frames_per_second(h: &mut H, secs: f64) -> f64 {
    use std::sync::{Arc, Mutex};
    // Every repaint request, with the delay asked for (egui's own clock is the wall clock,
    // which a slow test frame would use up: the requests themselves are what counts).
    let asked: Arc<Mutex<Option<std::time::Duration>>> = Arc::default();
    let a = asked.clone();
    h.ctx.set_request_repaint_callback(move |info| {
        if info.viewport_id == egui::ViewportId::ROOT {
            let mut g = a.lock().unwrap();
            *g = Some(g.map_or(info.delay, |d| d.min(info.delay)));
        }
    });
    let mut t = h.ctx.input(|i| i.time);
    // Settle: entrance animations finish.
    for _ in 0..240 {
        t += 1.0 / 60.0;
        h.input_mut().time = Some(t);
        h.step();
    }
    let end = t + secs;
    let mut frames = 0u32;
    while t < end {
        *asked.lock().unwrap() = None;
        h.input_mut().time = Some(t);
        h.step();
        frames += 1;
        let delay = asked.lock().unwrap().unwrap_or(std::time::Duration::MAX);
        if frames < 4 && std::env::var("BUDGET_DEBUG").is_ok() {
            eprintln!(
                "frame {frames}: delay {delay:?} causes {:?}",
                h.ctx.repaint_causes()
            );
        }
        // egui asks to start the next frame one frame early (the delay has the predicted
        // frame time taken off), so the frame it wants lands one frame later.
        let d = (delay.as_secs_f64() + 1.0 / 60.0).clamp(1.0 / 60.0, 10.0);
        t += d;
    }
    frames as f64 / secs
}

/// The app with egui predicting 60 Hz frames, like eframe on a 60 Hz display (kittest's
/// default prediction is 4 frames a second, which turns every short delay into "now").
fn harness60(mac: &FakeMac) -> H {
    let app = cleancraft_ui_egui::CleanApp::new(mac.env.clone()).with_sync();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(common::W, common::HGT))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, app: &mut cleancraft_ui_egui::CleanApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app,
        );
    steps(&mut h);
    h
}

#[test]
fn idle_pages_stay_within_the_repaint_budget() {
    let mac = th::troubled_mac_now();
    cleancraft_core::testkit_monitor::answer_all(mac.env.fake_sys().unwrap());
    let mut h = harness60(&mac);
    let mut report = Vec::new();
    // (page, budget in frames a second)
    let pages: Vec<(&str, f64)> = vec![
        ("history", 1.0),
        ("settings", 1.0),
        ("health", 1.0),
        ("guide:ai_agents", 1.0),
        // Live Monitor: one reading every 2 s: the tick, then the finished reading wakes the
        // window (egui draws twice for an immediate request): at most 3 frames a reading.
        ("monitor", 1.6),
        // Start screens with an ambient hero: capped at 30.
        ("system_junk", 31.0),
    ];
    for (page, budget) in pages {
        h.state_mut().navigate(page);
        h.remove_cursor();
        let fps = frames_per_second(&mut h, 20.0);
        report.push(format!("{page}: {fps:.1} frames/s"));
        assert!(
            fps <= budget,
            "{page}: {fps:.1} frames a second (budget {budget}); {}; causes {:?}",
            report.join(", "),
            h.ctx.repaint_causes()
        );
    }
    // Unfocused or minimised: ambient loops stop.
    h.state_mut().navigate("system_junk");
    let vp = |h: &mut H| {
        h.input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .focused = Some(false);
    };
    vp(&mut h);
    let mut t = h.ctx.input(|i| i.time);
    for _ in 0..120 {
        t += 1.0 / 60.0;
        h.input_mut().time = Some(t);
        vp(&mut h);
        h.step();
    }
    let delay = h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    report.push(format!("system_junk unfocused: next frame in {delay:?}"));
    assert!(
        delay.as_secs_f64() >= 1.0,
        "unfocused start screen repaints after {delay:?}"
    );
    eprintln!("{}", report.join("\n"));
}
