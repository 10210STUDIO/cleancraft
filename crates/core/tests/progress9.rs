//! Real progress of running jobs on a fake Mac: Quick Sweep's scan (modules done of the
//! modules selected) and run (items handled of the items selected), the file walkers (Large &
//! Old, Duplicates), the Disk Map measure, and a stopped scan. The bar is known from the
//! start where the total is known, never goes back, and stays under 100% until the job ends.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cleancraft_core::features::{self, ScanCtx};
use cleancraft_core::progress::Progress;
use cleancraft_core::space::{self, MapLocation};
use cleancraft_core::sweep::{self, Sources};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Plan, Settings, testkit_monitor};

/// Every share seen by the progress callback, in order.
fn monotonic(seen: &[Option<f32>]) -> bool {
    let known: Vec<f32> = seen.iter().flatten().copied().collect();
    known.windows(2).all(|w| w[0] <= w[1])
}

#[test]
fn quick_sweep_scan_counts_modules_and_stops_at_99() {
    let m = testkit_monitor::sweep_mac();
    let s = Settings::default();
    let src = Sources::registered();
    let n = sweep::scan_ids(&s, &src).len();
    assert!(n >= 3, "a few modules to count");
    let meter = Progress::new();
    let seen = Mutex::new(Vec::new());
    let steps = Mutex::new(Vec::new());
    let r = sweep::scan_metered(
        &m.env,
        &s,
        &src,
        &AtomicBool::new(false),
        &|st| {
            seen.lock().unwrap().push(meter.fraction());
            steps.lock().unwrap().push(st.to_string());
        },
        &meter,
    );
    assert_eq!(r.tiles.len(), n);
    let seen = seen.into_inner().unwrap();
    assert!(
        seen.iter().all(Option::is_some),
        "the module count is known from the first step"
    );
    assert!(monotonic(&seen));
    assert_eq!(seen[0], Some(0.0));
    assert!(
        seen.iter().flatten().any(|f| *f > 0.0 && *f < 1.0),
        "it moves while the scan runs"
    );
    // Every module done, but the job has not ended: 99%, never 100.
    assert_eq!(meter.percent(), Some(99));
    assert!(!meter.is_finished());
    meter.finish();
    assert_eq!(meter.percent(), Some(100));
    // Each module's title is a step.
    let steps = steps.into_inner().unwrap();
    for id in sweep::scan_ids(&s, &src) {
        assert!(steps.iter().any(|x| x == sweep::tile_title(id)));
    }
}

#[test]
fn quick_sweep_run_counts_items_moved() {
    let m = testkit_monitor::sweep_mac();
    let s = Settings::default();
    let src = Sources::registered();
    let r = sweep::scan(&m.env, &s, &src, &AtomicBool::new(false), &|_| {});
    let confirmed: Vec<(Plan, String)> = r
        .to_run(&[])
        .into_iter()
        .map(|p| (p.clone(), p.digest()))
        .collect();
    let total: usize = confirmed.iter().map(|(p, _)| p.selected_count()).sum();
    assert!(total >= 2, "something to move");
    let meter = Progress::new();
    let seen = Mutex::new(Vec::new());
    let runs = sweep::run_metered(&m.env, &s, &src, &confirmed, &|_| {}, &meter);
    assert!(runs.iter().all(|r| r.result.is_ok()));
    seen.lock().unwrap().push(meter.fraction());
    assert_eq!(
        meter.percent(),
        Some(99),
        "all items handled, job not ended"
    );
    assert_eq!(meter.step(), format!("Moving {total} of {total} items"));
}

#[test]
fn a_stopped_sweep_never_reaches_the_end() {
    let m = testkit_monitor::sweep_mac();
    let s = Settings::default();
    let src = Sources::registered();
    let n = sweep::scan_ids(&s, &src).len();
    let cancel = AtomicBool::new(false);
    let meter = Progress::new();
    let r = sweep::scan_metered(
        &m.env,
        &s,
        &src,
        &cancel,
        &|st| {
            // Stop as the second module starts.
            if st == sweep::tile_title(sweep::scan_ids(&s, &src)[1]) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &meter,
    );
    assert!(r.tiles.len() < n, "later modules were not scanned");
    let f = meter.fraction().unwrap();
    assert!(
        f <= 2.0 / n as f32 + 1e-6,
        "the bar stays at the modules really done: {f}"
    );
    assert!(meter.percent().unwrap() < 100);
    assert!(!meter.is_finished());
}

fn files_mac() -> FakeMac {
    let m = FakeMac::sample();
    for d in [
        "Movies",
        "Music/Old",
        "Downloads",
        "Pictures/2020",
        "Projects/a/b",
    ] {
        for i in 0..3 {
            m.home_file(&format!("{d}/file-{i}.bin"), 60_000_000 + i * 1000);
        }
    }
    m
}

#[test]
fn file_walkers_report_folders_done() {
    let m = files_mac();
    let s = Settings::default();
    for id in [FeatureId::LargeOldFiles, FeatureId::Duplicates] {
        let f = features::get(id).unwrap();
        let meter = Progress::new();
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let ctx = ScanCtx {
            env: &m.env,
            settings: &s,
            cancel: &cancel,
            progress: &|_| seen.lock().unwrap().push(meter.fraction()),
            meter: &meter,
        };
        features::scan_with(f.as_ref(), &ctx).unwrap();
        let seen = seen.into_inner().unwrap();
        assert!(monotonic(&seen), "{id:?} never goes back");
        assert_eq!(meter.percent(), Some(99), "{id:?} done but not ended");
    }
}

#[test]
fn disk_map_measure_reports_folders_done() {
    let m = files_mac();
    let meter = Progress::new();
    let samples = Mutex::new(Vec::new());
    let t = space::build_location_metered(
        &m.env,
        &MapLocation::Home,
        &AtomicBool::new(false),
        &|_, _| samples.lock().unwrap().push(meter.fraction()),
        &meter,
    );
    assert!(t.root.is_some());
    assert!(monotonic(&samples.into_inner().unwrap()));
    let f = meter.fraction().unwrap();
    assert!(f > 0.9 && f < 1.0, "every top folder finished: {f}");
    // Stopped at once: unknown or near zero, never the end.
    let stopped = Progress::new();
    space::build_location_metered(
        &m.env,
        &MapLocation::Home,
        &AtomicBool::new(true),
        &|_, _| {},
        &stopped,
    );
    assert!(stopped.fraction().unwrap_or(0.0) < 0.5);
}
