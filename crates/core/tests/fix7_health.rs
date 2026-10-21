//! fix7: Mac Health's "What grew" has data without the person mapping anything (U7-1): a
//! light home measure at most once a day, kept and thinned per day and week, compared over
//! the chart's range; a Threat Check note keeps what it found so the card can select it (U7-3).

use cleancraft_core::health::{self, MapSummary};
use cleancraft_core::plan::{FeatureId, Plan, Risk};
use cleancraft_core::route::AppRoute;
use cleancraft_core::testkit::FakeMac;

const DAY: i64 = 86_400;
const NOW: i64 = 1_790_000_000;

#[test]
fn a_light_home_measure_once_a_day() {
    let m = FakeMac::new();
    m.home_file("Movies/film.mov", 2_000_000);
    m.home_file("Documents/a/b.txt", 10_000);
    assert!(health::record_baseline(&m.env, NOW).unwrap());
    let all = health::map_summaries(&m.env);
    assert_eq!(all.len(), 1);
    let s = &all[0];
    assert_eq!(s.root, m.env.home());
    let movies = m.env.home_path("Movies");
    let size = s.entries.iter().find(|e| e.0 == movies).unwrap().1;
    assert!(size >= 2_000_000, "{size}");
    assert!(
        s.entries
            .iter()
            .any(|e| e.0 == m.env.home_path("Documents/a"))
    );
    // Within a day: not again.
    assert!(!health::record_baseline(&m.env, NOW + 3_600).unwrap());
    // A day later (and something grew): a second one, and What grew answers.
    m.home_file("Movies/more.mov", 3_000_000);
    assert!(health::record_baseline(&m.env, NOW + DAY).unwrap());
    let all = health::map_summaries(&m.env);
    assert_eq!(all.len(), 2);
    assert_eq!(health::measures(&all), (2, Some(NOW)));
    let g = health::what_grew_in(&all, NOW + DAY, 7).unwrap();
    assert_eq!((g.from, g.to), (NOW, NOW + DAY));
}

fn summary(m: &FakeMac, at: i64, movies: u64) -> MapSummary {
    let home = m.env.home().to_path_buf();
    MapSummary {
        at,
        root: home.clone(),
        entries: vec![(home.clone(), movies + 1), (home.join("Movies"), movies)],
    }
}

#[test]
fn kept_thinned_and_compared_over_the_range() {
    let m = FakeMac::new();
    // One a day for 60 days, growing 1 GB a day.
    for d in (0..60).rev() {
        health::keep_map_summary(
            &m.env,
            summary(&m, NOW - d * DAY, (60 - d) as u64 * 1_000_000_000),
        )
        .unwrap();
    }
    let all = health::map_summaries(&m.env);
    assert!(all.len() <= health::MAP_PER_ROOT, "{}", all.len());
    let oldest = all.iter().map(|s| s.at).min().unwrap();
    assert!(NOW - oldest >= 50 * DAY, "weekly ones reach back");
    // 7 days: compared with the measure about a week ago.
    let g = health::what_grew_in(&all, NOW, 7).unwrap();
    assert!(
        (NOW - 7 * DAY - g.from).abs() <= DAY,
        "{}",
        (NOW - g.from) / DAY
    );
    let g30 = health::what_grew_in(&all, NOW, 30).unwrap();
    assert!((NOW - 30 * DAY - g30.from).abs() <= 6 * DAY);
    assert!(g30.grew[0].after - g30.grew[0].before > g.grew[0].after - g.grew[0].before);
}

#[test]
fn a_threat_note_keeps_its_items_so_the_card_selects_one() {
    let m = FakeMac::new();
    let mut p = Plan::new(FeatureId::Malware);
    p.add_category("x", "X", "x", Risk::High);
    let bad = m.env.home_path("Library/LaunchAgents/com.bad.plist");
    p.push(bad.clone(), 10, "x", "bad".to_string(), Risk::High, true);
    health::note_plan(&m.env, &p).unwrap();
    let n = health::scan_notes(&m.env)[&FeatureId::Malware].clone();
    assert_eq!(n.paths, vec![bad.clone()]);
    let f = health::Facts {
        now: n.at,
        threats: Some(n),
        ..Default::default()
    };
    let c = health::recommendations(&f)
        .into_iter()
        .find(|c| c.rule == "threats")
        .unwrap();
    assert_eq!(c.route, AppRoute::ThreatReview(bad));
    assert!(
        c.landing
            .unwrap()
            .starts_with("Scanning again to show what Threat Check found")
    );
}

#[test]
fn the_battery_card_opens_battery_settings() {
    use cleancraft_core::monitor::{Battery, BatteryReading};
    let f = health::Facts {
        now: NOW,
        battery: Some(BatteryReading::Present(Battery {
            percent: 50,
            state: "discharging".into(),
            on_ac: false,
            remaining_minutes: None,
            cycles: Some(1200),
            health: Some(70),
        })),
        ..Default::default()
    };
    let c = health::recommendations(&f)
        .into_iter()
        .find(|c| c.rule == "battery")
        .unwrap();
    assert_eq!(c.action, "Open Battery Settings");
    assert_eq!(c.route, AppRoute::BatterySettings);
}
