//! fix7 (user requirement): the welcome, Full Disk Access and AI apps steps show only the very
//! first time the app opens; from the second launch on it opens on Quick Sweep, whatever
//! happened the first time. An older settings file counts as set up.

mod common;

use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_health as th;
use cleancraft_core::{FeatureId, Settings};
use cleancraft_ui_egui::app::Page;
use common::*;

fn launch(m: &FakeMac, fda: Access) -> H {
    let mut h = harness(m);
    h.state_mut().fda = fda;
    h.state_mut().launch_checks();
    steps(&mut h);
    h
}

#[test]
fn first_launch_shows_the_steps_then_never_again() {
    let m = th::ai_mac();
    let mut h = launch(&m, Access::Missing);
    assert_eq!(h.state().page, Page::Welcome);
    assert!(has(&h, "Welcome to CleanCraft"));
    assert!(
        Settings::load(&m.env).setup_done,
        "seen as soon as it shows"
    );
    click(&mut h, "Continue");
    if h.state().fda == Access::Missing {
        assert!(has(&h, "Let CleanCraft see all of your Mac"));
        click(&mut h, "Skip");
    }
    assert_eq!(h.state().page, Page::ConnectAi);
    click(&mut h, "Not Now");
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
    drop(h);
    let h2 = launch(&m, Access::Missing);
    assert_eq!(h2.state().page, Page::Feature(FeatureId::SmartCare));
}

#[test]
fn quitting_in_the_middle_still_counts_as_seen() {
    let m = th::ai_mac();
    let h = launch(&m, Access::Missing);
    assert_eq!(h.state().page, Page::Welcome);
    drop(h);
    let h2 = launch(&m, Access::Missing);
    assert_eq!(h2.state().page, Page::Feature(FeatureId::SmartCare));
}

#[test]
fn an_older_settings_file_counts_as_set_up() {
    let m = th::ai_mac();
    std::fs::create_dir_all(m.env.data_dir()).unwrap();
    std::fs::write(Settings::path(&m.env), br#"{"old_days": 200}"#).unwrap();
    let h = launch(&m, Access::Granted);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
}

#[test]
fn a_damaged_settings_file_counts_as_set_up() {
    let m = th::ai_mac();
    std::fs::create_dir_all(m.env.data_dir()).unwrap();
    std::fs::write(Settings::path(&m.env), b"{ not json").unwrap();
    let h = launch(&m, Access::Granted);
    assert_eq!(h.state().page, Page::Feature(FeatureId::SmartCare));
}

#[test]
fn the_steps_stay_reachable_from_settings() {
    let m = th::ai_mac();
    let mut h = launch(&m, Access::Granted);
    click(&mut h, "Continue");
    click(&mut h, "Not Now");
    h.state_mut()
        .open_route(&cleancraft_core::route::AppRoute::ConnectAi);
    steps(&mut h);
    assert!(has(&h, "Use CleanCraft from your AI apps?"));
    h.state_mut()
        .open_route(&cleancraft_core::route::AppRoute::FullDiskAccess);
    steps(&mut h);
    assert_eq!(h.state().page, Page::Settings);
}
