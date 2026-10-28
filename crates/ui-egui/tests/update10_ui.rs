//! CleanCraft's own updates, by clicks: Settings > Updates (switches saved, version, last
//! check, Check Now with its results and a calm offline message), the "Update available"
//! banner with the release notes, Later, Install with the four step progress bar, the swap
//! (old version in the Trash, History line with Undo, relaunch helper), and Install refused
//! while a cleanup runs.

mod common;

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::self_update::{self, UpdateState};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_update::*;
use cleancraft_core::{FeatureId, Settings};
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::updates::{
    CHECK_NOW, CHECK_SWITCH, INSTALL_BUTTON, INSTALL_OFF_NOTE, INSTALL_ON_NOTE, INSTALL_SWITCH,
    LATER,
};
use common::*;
use egui_kittest::kittest::Queryable;

const CUR: &str = "0.1.2";

fn mac() -> FakeMac {
    let m = FakeMac::new();
    install_app(&m, CUR);
    m
}

fn app(m: &FakeMac) -> H {
    let mut h = harness(m);
    h.state_mut().updates.current = CUR.into();
    steps(&mut h);
    h
}

fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

fn settings(m: &FakeMac) -> H {
    let mut h = app(m);
    click(&mut h, "Settings");
    h.get_by_label(CHECK_SWITCH).scroll_to_me();
    steps(&mut h);
    h
}

/// The last check found `version` (as the menu bar agent would have saved it).
fn found(m: &FakeMac, version: &str, notes: &str) {
    answer_check(m, &release_json(version, true, notes));
    let res = self_update::check_against(&m.env, CUR);
    let mut st = UpdateState::load(&m.env);
    st.record(&res, chrono::Utc::now().timestamp(), CUR);
    st.save(&m.env).expect("save");
}

#[test]
fn the_updates_section_saves_its_switches_and_shows_version_and_last_check() {
    let m = mac();
    let mut h = settings(&m);
    assert!(has(&h, "CleanCraft 0.1.2"));
    assert!(has(&h, "Never"));
    assert!(has_text(&h, INSTALL_OFF_NOTE));

    reach(&mut h, INSTALL_SWITCH);
    assert!(Settings::load(&m.env).update_install_auto);
    assert!(has_text(&h, INSTALL_ON_NOTE));
    reach(&mut h, CHECK_SWITCH);
    assert!(!Settings::load(&m.env).update_check_auto);
    reach(&mut h, CHECK_SWITCH);
    reach(&mut h, INSTALL_SWITCH);
    let s = Settings::load(&m.env);
    assert!(s.update_check_auto && !s.update_install_auto);
}

#[test]
fn check_now_finds_a_new_version_and_the_banner_shows_its_notes() {
    let m = mac();
    answer_check(
        &m,
        &release_json("0.1.3", true, "Faster scans.\nA new Disk Map."),
    );
    let mut h = settings(&m);
    reach(&mut h, CHECK_NOW);
    assert!(has(&h, "CleanCraft 0.1.3 is available. You have 0.1.2."));
    assert!(!has(&h, "Never"), "the last check time is shown");
    assert!(has(&h, "Install CleanCraft 0.1.3"));
    assert!(has(
        &h,
        "Update available: CleanCraft 0.1.3 (you have 0.1.2)."
    ));
    assert!(has_text(&h, "Faster scans."));
    assert!(has_text(&h, "A new Disk Map."));
    assert_eq!(
        UpdateState::load(&m.env)
            .available(CUR)
            .map(|r| r.version.clone())
            .as_deref(),
        Some("0.1.3")
    );
    // Later puts the banner away; the Settings button stays.
    click(&mut h, LATER);
    assert!(!has(&h, INSTALL_BUTTON));
    assert!(has(&h, "Install CleanCraft 0.1.3"));
    assert_eq!(
        installed_version(&m).as_deref(),
        Some(CUR),
        "nothing installed"
    );
}

#[test]
fn check_now_offline_same_and_ahead_are_calm_lines_without_a_banner() {
    let m = mac();
    fail_check(&m);
    let mut h = settings(&m);
    reach(&mut h, CHECK_NOW);
    assert!(has(
        &h,
        "Could not check for updates (the update server could not be found (no internet?)). CleanCraft will try again later."
    ));
    answer_check(&m, &release_json(CUR, true, ""));
    reach(&mut h, CHECK_NOW);
    assert!(has(&h, "CleanCraft 0.1.2 is the latest version."));
    answer_check(&m, &release_json("0.1.1", true, ""));
    reach(&mut h, CHECK_NOW);
    assert!(has_text(&h, "You are ahead of the latest release"));
    assert!(!has(&h, INSTALL_BUTTON));
    assert!(executor::history(&m.env).is_empty());
}

#[test]
fn install_from_the_banner_swaps_the_app_and_relaunches() {
    let m = mac();
    found(&m, "0.1.3", "Faster scans.");
    answer_install(&m, &Script::good("0.1.3"));
    let mut h = app(&m);
    assert!(has(&h, INSTALL_BUTTON), "the banner shows on any page");
    click(&mut h, INSTALL_BUTTON);
    assert_eq!(installed_version(&m).as_deref(), Some("0.1.3"));
    assert!(m.env.user_trash().join("CleanCraft 0.1.2.app").is_dir());
    assert!(has_text(
        &h,
        "CleanCraft 0.1.3 is installed. The previous version (0.1.2) is in the Trash; History can put it back. Restarting CleanCraft."
    ));
    let app_path = m.env.sys_path("/Applications/CleanCraft.app");
    assert_eq!(
        h.state().updates.relaunching.as_deref(),
        Some(app_path.as_path())
    );
    let sp = m.env.fake_sys().expect("fake").spawned();
    assert!(
        sp.iter().any(|s| s.starts_with("/bin/sh -c ")
            && s.ends_with(&format!(
                " sh {} {}",
                std::process::id(),
                app_path.display()
            ))),
        "{sp:?}"
    );
    assert!(!has(&h, INSTALL_BUTTON), "no banner after the install");
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 1);
    assert!(hist[0].can_undo());
    assert!(matches!(&hist[0].kind,
        ActionKind::Toggled { what, .. } if what.starts_with("Updated CleanCraft from 0.1.2 to 0.1.3")));
}

#[test]
fn install_is_refused_while_a_cleanup_runs() {
    let m = mac();
    found(&m, "0.1.3", "");
    answer_install(&m, &Script::good("0.1.3"));
    let mut h = app(&m);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase =
        cleancraft_ui_egui::pages::plan_page::Phase::Cleaning(Job::waiting("Moving"));
    steps(&mut h);
    click(&mut h, INSTALL_BUTTON);
    assert!(has(
        &h,
        "A cleanup is running in System Junk. Wait for it to finish, then install the update."
    ));
    assert_eq!(installed_version(&m).as_deref(), Some(CUR));
    let calls = m.env.fake_sys().expect("fake").calls();
    assert!(
        !calls.iter().any(|c| c.contains(" -o ")),
        "nothing downloaded"
    );
}

#[test]
fn a_failed_install_says_why_and_changes_nothing() {
    let m = mac();
    found(&m, "0.1.3", "");
    answer_install(
        &m,
        &Script {
            dmg_id: "com.example.other",
            ..Script::good("0.1.3")
        },
    );
    let mut h = app(&m);
    click(&mut h, INSTALL_BUTTON);
    assert!(has_text(
        &h,
        "is not CleanCraft (its id is com.example.other)"
    ));
    assert_eq!(installed_version(&m).as_deref(), Some(CUR));
    assert!(executor::history(&m.env).is_empty());
    assert!(h.state().updates.relaunching.is_none());
}

#[test]
fn the_install_shows_its_four_steps_and_the_progress_bar() {
    let m = mac();
    found(&m, "0.1.3", "");
    let mut h = app(&m);
    let job = Job::waiting("Checking the download");
    job.meter.set_fraction(0.3);
    h.state_mut().updates.install = Some(("0.1.3".into(), job));
    steps(&mut h);
    assert!(has(&h, "Installing CleanCraft 0.1.3"));
    assert!(has(&h, "Download done"));
    assert!(has(&h, "Verify"));
    assert!(has(&h, "Install"));
    assert!(has(&h, "Relaunch"));
    assert!(has(&h, "30%"));
    assert!(has(&h, "Checking the download"));
    assert!(!has(&h, INSTALL_BUTTON), "no second Install while one runs");
}

#[test]
fn with_install_automatically_on_the_launch_check_installs() {
    let m = mac();
    answer_check(&m, &release_json("0.1.3", true, ""));
    answer_install(&m, &Script::good("0.1.3"));
    Settings {
        update_install_auto: true,
        setup_done: true,
        ..Settings::default()
    }
    .save(&m.env)
    .expect("save");
    let mut h = app(&m);
    h.state_mut().settings = Settings::load(&m.env);
    cleancraft_ui_egui::pages::updates::launch_check(h.state_mut());
    steps(&mut h);
    assert_eq!(installed_version(&m).as_deref(), Some("0.1.3"));
    // A second launch the same day does not ask GitHub again.
    let before = m.env.fake_sys().expect("fake").calls().len();
    cleancraft_ui_egui::pages::updates::launch_check(h.state_mut());
    assert_eq!(m.env.fake_sys().expect("fake").calls().len(), before);
}
