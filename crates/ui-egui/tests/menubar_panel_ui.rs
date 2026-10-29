//! The menu bar panel (panel13): what it shows from a fake Mac's readings, that it refreshes,
//! and that every control sends the right command to the agent, driven by clicks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cleancraft_core::Settings;
use cleancraft_core::menubar::{self, PanelCmd};
use cleancraft_core::schedule::{self, AgentState, Review};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::{
    NETSTAT_2, PMSET_DESKTOP, answer_all, break_tool, sweep_mac,
};
use cleancraft_ui_egui::menubar_panel::{self as mp, MenuPanel};
use egui_kittest::kittest::{NodeT, Queryable};

type H = egui_kittest::Harness<'static, MenuPanel>;

const W: f32 = menubar::PANEL_W as f32;
const HGT: f32 = menubar::PANEL_H as f32;

fn harness(m: &FakeMac) -> H {
    harness_at(m, 1.0)
}

fn harness_at(m: &FakeMac, ppp: f32) -> H {
    let p = MenuPanel::new(m.env.clone()).with_sync();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(W, HGT))
        .with_pixels_per_point(ppp)
        .build_ui_state(
            |ui, p: &mut MenuPanel| {
                p.logic(&ui.ctx().clone());
                // The whole window, with no margin, as eframe gives it.
                let r = ui.ctx().content_rect();
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    ui.set_clip_rect(r);
                    p.ui(ui);
                });
            },
            p,
        );
    steps(&mut h);
    h
}

fn steps(h: &mut H) {
    for _ in 0..4 {
        h.step();
    }
}

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn labels(h: &H) -> Vec<String> {
    h.root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            a.label().or_else(|| a.value())
        })
        .collect()
}

fn mac() -> FakeMac {
    let m = sweep_mac();
    answer_all(m.env.fake_sys().expect("fake"));
    m
}

/// Every control and reading lies inside the 340 x 460 panel (the process list scrolls).
fn all_inside(h: &H) {
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        let Some(text) = a.label().or_else(|| a.value()) else {
            continue;
        };
        // The busiest apps scroll under the waiting items; the rest always shows.
        if text.contains("% CPU") {
            continue;
        }
        let r = n.rect();
        assert!(
            r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5,
            "{:?} is outside the panel: {r:?}",
            n.accesskit_node().label()
        );
    }
}

#[test]
fn the_panel_shows_live_readings_with_plain_process_names() {
    let m = mac();
    let mut h = harness(&m);
    let l = labels(&h);
    assert!(has(&h, "CPU: 17%, 8 cores"), "{l:?}");
    assert!(
        l.iter()
            .any(|x| x.starts_with("Memory: ")
                && x.ends_with(" GB used, of 16.0 GB, pressure normal")),
        "{l:?}"
    );
    assert!(has(&h, "Free space: 81.9 GB free, of 500 GB"), "{l:?}");
    assert!(has(&h, "Battery: 64%, Discharging"), "{l:?}");
    assert!(has(&h, "Network: Measuring"), "{l:?}");
    assert!(
        has(
            &h,
            "Google Chrome, 48% CPU. A helper process of Google Chrome."
        ),
        "{l:?}"
    );
    assert!(has(&h, "Safari, 12% CPU"), "{l:?}");
    assert!(
        l.iter()
            .any(|x| x.starts_with("WindowServer, 2% CPU. Part of macOS")),
        "{l:?}"
    );
    assert!(
        h.query_all_by_label_contains("Xcode").next().is_none(),
        "top 3 only"
    );
    assert!(h.query_by_label_contains(mp::NOTHING_WAITING).is_some());
    assert!(has(&h, mp::QUICK_SWEEP) && has(&h, mp::OPEN_APP) && has(&h, mp::MORE));
    all_inside(&h);

    // A second reading a second later: speeds appear, the busiest list follows ps.
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("netstat", NETSTAT_2);
    fake.answer(
        "ps",
        "  1 0.3 14000 launchd\n  512 3.0 812000 Safari\n  700 95.0 2100000 Xcode\n",
    );
    let now = cleancraft_core::monitor::now_secs();
    h.state_mut().refresh_at(now + 2.0);
    steps(&mut h);
    let l = labels(&h);
    assert!(
        l.iter()
            .any(|x| x.starts_with("Network: Down ") && x.contains(", up ")),
        "{l:?}"
    );
    assert!(
        has(&h, "Xcode, 95% CPU. The Xcode app.")
            || l.iter().any(|x| x.starts_with("Xcode, 95% CPU")),
        "{l:?}"
    );
    assert!(
        h.query_all_by_label_contains("Google Chrome")
            .next()
            .is_none()
    );
    // Nothing was cleaned, moved or run beyond the readings.
    assert!(cleancraft_core::executor::history(&m.env).is_empty());
    assert!(h.state_mut().take_commands().is_empty());
}

#[test]
fn a_mac_without_battery_shows_network_in_its_place_and_failures_say_unknown() {
    let m = mac();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("pmset", PMSET_DESKTOP);
    break_tool(fake, "ps");
    break_tool(fake, "top");
    break_tool(fake, "df");
    let h = harness(&m);
    let l = labels(&h);
    assert!(!l.iter().any(|x| x.starts_with("Battery")), "{l:?}");
    assert!(has(&h, "Network: Measuring"), "{l:?}");
    assert!(has(&h, "CPU: Unknown"), "{l:?}");
    assert!(has(&h, "Free space: Unknown"), "{l:?}");
    assert!(
        h.query_by_label_contains("The process list could not be read.")
            .is_some()
    );
    all_inside(&h);
}

fn with_reviews(m: &FakeMac) {
    let st = AgentState {
        reviews: vec![
            Review {
                text: "Review: CleanCraft found 2.10 GB to clean".into(),
                route: schedule::SWEEP_ROUTE.into(),
            },
            Review {
                text: "Review: Editor went to the Trash".into(),
                route: "uninstall:/Users/me/.Trash/Editor.app".into(),
            },
        ],
        ..AgentState::default()
    };
    st.save(&m.env).expect("save");
}

#[test]
fn waiting_items_are_listed_newest_first_and_open_the_app_there() {
    let m = mac();
    with_reviews(&m);
    let mut h = harness(&m);
    let l = labels(&h);
    let a = l
        .iter()
        .position(|x| x == "Editor went to the Trash")
        .expect("newest");
    let b = l
        .iter()
        .position(|x| x == "CleanCraft found 2.10 GB to clean")
        .expect("older");
    assert!(a < b, "newest first: {l:?}");
    assert!(h.query_by_label_contains(mp::NOTHING_WAITING).is_none());
    assert!(h.state().view().badge(), "the menu bar icon shows its dot");
    all_inside(&h);
    h.get_by_label("CleanCraft found 2.10 GB to clean").click();
    steps(&mut h);
    assert_eq!(h.state_mut().take_commands(), [PanelCmd::Review(1)]);
    assert!(h.state().wants_close());
    // Index 1 newest-first is the agent's oldest item: the same one the agent opens.
    assert_eq!(
        PanelCmd::Review(1).action(),
        schedule::MenuAction::Review(1)
    );
}

#[test]
fn quick_sweep_and_open_ask_the_agent_and_close_the_panel() {
    for (label, cmd) in [
        (mp::QUICK_SWEEP, PanelCmd::QuickSweep),
        (mp::OPEN_APP, PanelCmd::Open),
    ] {
        let m = mac();
        let mut h = harness(&m);
        assert!(!h.state().wants_close());
        h.get_by_label(label).click();
        steps(&mut h);
        assert_eq!(h.state_mut().take_commands(), [cmd]);
        assert!(h.state().wants_close(), "{label}");
    }
}

#[test]
fn more_holds_pause_and_quit() {
    let m = mac();
    let mut h = harness(&m);
    assert!(!has(&h, schedule::PAUSE), "behind More");
    h.get_by_label(mp::MORE).click();
    steps(&mut h);
    h.get_by_label(schedule::PAUSE).click();
    steps(&mut h);
    assert_eq!(h.state_mut().take_commands(), [PanelCmd::TogglePause]);
    assert!(!h.state().wants_close(), "pausing keeps the panel open");
    assert!(has(&h, "CleanCraft, Paused"), "{:?}", labels(&h));
    // The agent saved the setting; the next read keeps it.
    Settings {
        notifications_paused: true,
        ..Settings::default()
    }
    .save(&m.env)
    .expect("save");
    h.state_mut()
        .refresh_at(cleancraft_core::monitor::now_secs());
    steps(&mut h);
    h.get_by_label(mp::MORE).click();
    steps(&mut h);
    assert!(has(&h, schedule::RESUME));
    h.get_by_label(schedule::QUIT).click();
    steps(&mut h);
    assert_eq!(h.state_mut().take_commands(), [PanelCmd::QuitAgent]);
    assert!(h.state().wants_close());
}

#[test]
fn escape_closes_the_panel() {
    let m = mac();
    let mut h = harness(&m);
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(h.state().wants_close());
    assert!(h.state_mut().take_commands().is_empty());
}

/// The panel draws with the GPU renderer (with waiting items and a live reading). Set
/// CLEANCRAFT_SNAPSHOT_DIR to also write menubar-panel.png there for review.
#[test]
fn the_panel_renders() {
    let m = mac();
    with_reviews(&m);
    let mut h = harness_at(&m, 2.0);
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("netstat", NETSTAT_2);
    h.state_mut()
        .refresh_at(cleancraft_core::monitor::now_secs() + 2.0);
    steps(&mut h);
    let img = h.render().expect("render");
    assert_eq!(img.width(), (W * 2.0) as u32);
    if let Ok(dir) = std::env::var("CLEANCRAFT_SNAPSHOT_DIR") {
        std::fs::create_dir_all(&dir).expect("dir");
        img.save(std::path::Path::new(&dir).join("menubar-panel.png"))
            .expect("save png");
    }
}
