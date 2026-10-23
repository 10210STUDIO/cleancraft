//! Running jobs show a progress bar with the share done ("42%") and the current step, never
//! an elapsed clock; while the total is not known the bar moves without a number; a real
//! scan reaches its result screen. All on a fake Mac.

mod common;
use common::*;

use std::sync::mpsc::channel;

use cleancraft_core::features;
use cleancraft_core::{FeatureId, Settings, testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::{plan_page, sweep};
use egui_kittest::kittest::{NodeT, Queryable};

/// Any label that reads like a clock ("0:03", "1:20").
fn shows_clock(h: &H) -> bool {
    h.query_all_by_label_contains(":")
        .filter_map(|n| n.accesskit_node().label())
        .any(|s| {
            let s = s.trim();
            s.split_once(':').is_some_and(|(m, sec)| {
                !m.is_empty()
                    && m.chars().all(|c| c.is_ascii_digit())
                    && sec.len() == 2
                    && sec.chars().all(|c| c.is_ascii_digit())
            })
        })
}

fn open_junk(h: &mut H) {
    click(h, "Clean");
    click(h, "System Junk");
}

#[test]
fn a_scan_shows_the_share_done_and_the_step_not_a_clock() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    open_junk(&mut h);
    let job = Job::waiting("~/Library/Caches");
    job.meter.set(42, 100);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase = plan_page::Phase::Scanning(job);
    steps(&mut h);
    assert!(has_text(&h, "Scanning System Junk"));
    assert!(has(&h, "42%"), "the share done as a number");
    assert!(has(&h, "Progress 42%"), "the bar itself");
    assert!(
        has_text(&h, "Looking in ~/Library/Caches"),
        "the current step"
    );
    assert!(!shows_clock(&h), "no elapsed time");
    assert!(has(&h, "Stop"), "Stop stays");
}

#[test]
fn an_unknown_total_shows_a_moving_bar_without_a_number() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    open_junk(&mut h);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase =
        plan_page::Phase::Scanning(Job::waiting("~/Library/Logs"));
    steps(&mut h);
    assert!(has(&h, "Progress, working"));
    assert!(
        !has_text(&h, "%"),
        "no percentage before the total is known"
    );
    assert!(has_text(&h, "Looking in ~/Library/Logs"));
    assert!(!shows_clock(&h));
}

#[test]
fn a_real_scan_moves_the_bar_then_reaches_the_results() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    open_junk(&mut h);
    let env = mac.env.clone();
    let (go, wait) = channel::<()>();
    let (reported, at_half) = channel::<()>();
    // A real System Junk scan on a thread, held at a known point so the screen can be read.
    let job = Job::start_metered(false, move |meter| {
        meter.set(1, 2);
        meter.set_step("~/Library/Caches");
        let _ = reported.send(());
        let _ = wait.recv();
        let f = features::get(FeatureId::SystemJunk).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let s = Settings::default();
        let ctx = features::ScanCtx {
            env: &env,
            settings: &s,
            cancel: &cancel,
            progress: &|st| meter.set_step(st),
            meter: &meter.range(0.5, 1.0),
        };
        features::scan_with(f.as_ref(), &ctx).map_err(|e| format!("{e:#}"))
    });
    let meter = job.meter.clone();
    h.state_mut().plan_state(FeatureId::SystemJunk).phase = plan_page::Phase::Scanning(job);
    at_half.recv().unwrap();
    steps(&mut h);
    assert!(has(&h, "50%"));
    assert!(!shows_clock(&h));
    go.send(()).unwrap();
    // Let the thread finish, then the next frames pick up the result.
    for _ in 0..200 {
        if meter.is_finished() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(meter.percent(), Some(100), "100% once the job ended");
    steps(&mut h);
    assert!(matches!(
        h.state_mut().plan_state(FeatureId::SystemJunk).phase,
        plan_page::Phase::Results
    ));
    assert!(has(&h, "Review and Clean"), "the result screen");
}

#[test]
fn a_cleanup_shows_items_moved() {
    let mac = testkit_cleanup::junk_mac();
    let mut h = harness(&mac);
    open_junk(&mut h);
    let job = Job::waiting("Moving 3 of 4 items");
    job.meter.set(2, 4);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase = plan_page::Phase::Cleaning(job);
    steps(&mut h);
    assert!(has(&h, "50%"));
    assert!(has_text(&h, "Moving 3 of 4 items"));
    assert!(!shows_clock(&h));
}

#[test]
fn quick_sweep_shows_the_bar_and_module_ticks_then_its_results() {
    let mac = testkit_monitor::sweep_mac();
    let mut h = harness(&mac);
    let ids = cleancraft_core::sweep::scan_ids(
        &h.state().settings,
        &cleancraft_core::sweep::Sources::registered(),
    );
    assert!(ids.len() >= 3);
    let job = Job::waiting(cleancraft_core::sweep::tile_title(ids[1]));
    // The first module finished, the second half way.
    job.meter.span(0, ids.len()).complete();
    job.meter.span(1, ids.len()).set(1, 2);
    let pct = (150 / ids.len()) as u8;
    h.state_mut().sweep.phase = sweep::Phase::Scanning(job);
    steps(&mut h);
    assert!(has_text(&h, "Sweeping your Mac"));
    assert!(has(&h, &format!("{pct}%")));
    let first = cleancraft_core::sweep::tile_title(ids[0]);
    assert!(
        has(&h, &format!("{first}: done")),
        "a tick on the finished module"
    );
    assert!(!shows_clock(&h));
    assert!(has(&h, "Stop"));
    // A real sweep from the start screen reaches its results.
    h.state_mut().sweep.phase = sweep::Phase::Idle;
    steps(&mut h);
    click(&mut h, "Scan");
    assert!(matches!(h.state().sweep.phase, sweep::Phase::Results));
}
