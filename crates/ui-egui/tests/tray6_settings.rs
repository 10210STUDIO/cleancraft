//! tray6, by clicks: Settings > "Menu bar and notifications". The menu bar switch writes and
//! loads the login item, then moves it to the Trash, with Undo; the schedule, disk alert and
//! pause controls are saved and only usable while the menu bar is on; errors are shown.
//! "Open the app at this page" requests switch the page of the running app.

mod common;

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::login_agent::{self, LABEL};
use cleancraft_core::notify;
use cleancraft_core::schedule::ScanEvery;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{Fault, FeatureId, Settings};
use cleancraft_ui_egui::Page;
use cleancraft_ui_egui::pages::settings::{
    DISK_ALERT_SWITCH, MENU_BAR_SWITCH, NEW_ITEMS_SWITCH, NOTIFY_NOTE, PAUSE_SWITCH,
    TRASH_OFFER_SWITCH,
};
use common::*;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};

fn mac() -> FakeMac {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("launchctl", "");
    fake.answer("sh", "");
    m
}

/// Scroll a control into view, like a user, then click it.
fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

fn disabled(h: &H, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().is_disabled()
}

fn settings_page(m: &FakeMac) -> H {
    let mut h = harness(m);
    click(&mut h, "Settings");
    h.get_by_label(MENU_BAR_SWITCH).scroll_to_me();
    steps(&mut h);
    h
}

#[test]
fn the_menu_bar_switch_adds_and_removes_the_login_item_with_undo() {
    let m = mac();
    let fake = m.env.fake_sys().expect("fake");
    let mut h = settings_page(&m);
    assert!(!login_agent::is_on(&m.env), "off by default");
    assert!(has(&h, MENU_BAR_SWITCH));
    assert!(has_text(
        &h,
        "Off: nothing of CleanCraft runs while the app is closed."
    ));

    reach(&mut h, MENU_BAR_SWITCH);
    let plist = login_agent::plist_path(&m.env);
    assert!(plist.is_file());
    let xml = std::fs::read_to_string(&plist).expect("plist");
    assert!(xml.contains("/Applications/CleanCraft.app/Contents/MacOS/CleanCraft"));
    assert!(xml.contains("<string>agent</string>"));
    let uid = cleancraft_core::features::trash::uid(&m.env);
    assert!(
        fake.calls().contains(&format!(
            "/bin/launchctl bootstrap gui/{uid} {}",
            plist.display()
        )),
        "{:?}",
        fake.calls()
    );
    assert!(has_text(
        &h,
        "CleanCraft is in the menu bar now and starts there at login."
    ));
    assert!(has_text(
        &h,
        "It only looks: it scans, warns and opens CleanCraft"
    ));
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 1);
    assert!(
        matches!(&hist[0].kind, ActionKind::Toggled { what, .. } if what.starts_with(login_agent::ON_TEXT))
    );

    // Off: booted out, the plist goes to the Trash.
    reach(&mut h, MENU_BAR_SWITCH);
    assert!(!plist.exists());
    assert!(m.env.user_trash().join(format!("{LABEL}.plist")).is_file());
    assert!(
        fake.calls()
            .contains(&format!("/bin/launchctl bootout gui/{uid}/{LABEL}"))
    );
    assert!(has_text(&h, "Its login item is in the Trash."));
    // Undo in the notice runs the journaled undo command.
    click(&mut h, "Undo Menu Bar Change");
    assert!(has_text(&h, "The menu bar change was undone."));
    assert!(
        fake.calls()
            .last()
            .is_some_and(|c| c.starts_with("/bin/sh -c /bin/mv -n -- ")),
        "{:?}",
        fake.calls().last()
    );
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 2);
    assert!(hist[1].undone.is_some());
}

#[test]
fn schedule_disk_alert_and_pause_are_saved_and_need_the_menu_bar() {
    let m = mac();
    let mut h = settings_page(&m);
    // Off: the options are there but cannot be used.
    for l in [
        "Every day",
        "Every week",
        DISK_ALERT_SWITCH,
        PAUSE_SWITCH,
        TRASH_OFFER_SWITCH,
        NEW_ITEMS_SWITCH,
    ] {
        assert!(disabled(&h, l), "{l} usable while the menu bar is off");
    }
    click(&mut h, "Every day");
    assert_eq!(Settings::load(&m.env).scan_every, ScanEvery::Off);

    reach(&mut h, MENU_BAR_SWITCH);
    assert!(!disabled(&h, "Every day"));
    reach(&mut h, "Every day");
    let s = Settings::load(&m.env);
    assert_eq!(s.scan_every, ScanEvery::Daily);
    assert_eq!(s.scan_hour, 10);
    assert!(has_text(&h, "Next scan: "));
    assert!(has_text(
        &h,
        "A scheduled scan only looks: nothing moves until you review it in Quick Sweep and confirm."
    ));
    // The time: a click on the slider moves it; saved.
    h.get_by_role_and_label(Role::Slider, "Scan time")
        .scroll_to_me();
    steps(&mut h);
    h.get_by_role_and_label(Role::Slider, "Scan time").click();
    steps(&mut h);
    let hour = Settings::load(&m.env).scan_hour;
    assert_eq!(hour, h.state().settings.scan_hour);
    assert_ne!(hour, 10);
    assert!(has_text(&h, &format!("At: {hour:02}:00")));

    // Weekly: the day choice appears; Friday is saved.
    assert!(!has(&h, "Friday"));
    reach(&mut h, "Every week");
    reach(&mut h, "Friday");
    let s = Settings::load(&m.env);
    assert_eq!((s.scan_every, s.scan_weekday), (ScanEvery::Weekly, 5));
    assert!(has_text(&h, "Next scan: "));

    // Disk alert: on at 10% by default; off saves 0; on again 10.
    assert!(has_text(&h, "When free space is under: 10%"));
    reach(&mut h, DISK_ALERT_SWITCH);
    assert_eq!(Settings::load(&m.env).disk_low_percent, 0);
    assert!(!has_text(&h, "When free space is under"));
    reach(&mut h, DISK_ALERT_SWITCH);
    assert_eq!(Settings::load(&m.env).disk_low_percent, 10);

    // Pause notifications (the same setting as the menu bar menu's Pause).
    reach(&mut h, PAUSE_SWITCH);
    assert!(Settings::load(&m.env).notifications_paused);
    reach(&mut h, PAUSE_SWITCH);
    assert!(!Settings::load(&m.env).notifications_paused);
    assert!(has(&h, NOTIFY_NOTE));

    // The watchers: off by default, each saved.
    let s = Settings::load(&m.env);
    assert!(!s.watch_trash_offer && !s.watch_new_items);
    reach(&mut h, TRASH_OFFER_SWITCH);
    assert!(Settings::load(&m.env).watch_trash_offer);
    reach(&mut h, NEW_ITEMS_SWITCH);
    assert!(Settings::load(&m.env).watch_new_items);
    reach(&mut h, TRASH_OFFER_SWITCH);
    reach(&mut h, NEW_ITEMS_SWITCH);
    let s = Settings::load(&m.env);
    assert!(!s.watch_trash_offer && !s.watch_new_items);

    // Off again: back to the Off radio is still possible while on.
    reach(&mut h, "Off");
    assert_eq!(Settings::load(&m.env).scan_every, ScanEvery::Off);
    assert!(!has(&h, "Friday"));
    // Nothing of this cleaned anything.
    assert!(
        executor::history(&m.env)
            .iter()
            .all(|r| matches!(r.kind, ActionKind::Toggled { .. }))
    );
}

#[test]
fn a_launchctl_failure_and_an_unwritable_log_are_shown() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer_full(
        "launchctl",
        CmdOut {
            status: 5,
            stdout: String::new(),
            stderr: "Bootstrap failed: 5: Input/output error".into(),
        },
    );
    let mut h = settings_page(&m);
    reach(&mut h, MENU_BAR_SWITCH);
    assert!(has_text(
        &h,
        "It starts at your next login; launchctl did not start it now: Bootstrap failed: 5: Input/output error"
    ));
    assert!(login_agent::is_on(&m.env));

    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer("launchctl", "");
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    let mut h = settings_page(&m);
    reach(&mut h, MENU_BAR_SWITCH);
    assert!(has_text(
        &h,
        "Could not write the action log, nothing changed"
    ));
    assert!(!login_agent::is_on(&m.env));
    assert!(m.env.fake_sys().expect("fake").calls().is_empty());
}

#[test]
fn open_requests_switch_the_page_of_the_running_app() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer("open", "");
    let mut h = harness(&m);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
    notify::request_open(&m.env, "history").expect("request");
    steps(&mut h);
    assert_eq!(h.state().page, Page::History);
    // Read once: going elsewhere is not undone by the same request.
    click(&mut h, "Settings");
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.state().page, Page::Settings);
    // The scheduled scan's route opens Quick Sweep; a link's route, the module it names.
    notify::request_open(&m.env, "scan:quick_sweep").expect("request");
    steps(&mut h);
    wait_page(&mut h, Page::Feature(FeatureId::SmartCare));
    let route = notify::route_from_url("cleancraft://open/system-junk").expect("route");
    notify::request_open(&m.env, &route).expect("request");
    wait_page(&mut h, Page::Feature(FeatureId::SystemJunk));
    // An unknown page leaves the page as it is.
    notify::request_open(&m.env, "no_such_page").expect("request");
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.state().page, Page::Feature(FeatureId::SystemJunk));
}

/// Requests are read twice a second (egui time, a quarter second per test step): step
/// until the page changes.
fn wait_page(h: &mut H, want: Page) {
    for _ in 0..20 {
        if h.state().page == want {
            return;
        }
        h.step();
    }
    assert_eq!(h.state().page, want);
}

#[test]
fn an_old_request_does_not_move_an_app_started_later() {
    let m = FakeMac::new();
    std::fs::create_dir_all(m.env.data_dir()).expect("dir");
    std::fs::write(
        notify::request_path(&m.env),
        format!(
            r#"{{"route":"history","time":{},"seq":1}}"#,
            chrono::Utc::now().timestamp() - 600
        ),
    )
    .expect("write");
    let h = harness(&m);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
}
