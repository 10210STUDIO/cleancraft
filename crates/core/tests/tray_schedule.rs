//! tray6: the menu bar agent's brain on a fake Mac. Scheduled scans (daily, weekly, missed
//! runs after sleep run once, a changed schedule re-arms), scan-only proof (the journal stays
//! empty, the files stay), notifications (exact osascript line, escaping, paused), the disk
//! alert at most once a day, and the menu model.

use chrono::NaiveDateTime;
use cleancraft_core::executor::history;
use cleancraft_core::monitor::{Sampler, Snapshot};
use cleancraft_core::notify::{self, Notification};
use cleancraft_core::schedule::*;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::{answer_all, sweep_mac, sweep_sources};
use cleancraft_core::{Env, Settings};

fn at(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").expect("time")
}

// 2026-10-20 is a Tuesday.

#[test]
fn daily_slots() {
    let d = ScanEvery::Daily;
    assert_eq!(
        last_slot(d, 10, 1, at("2026-10-20 09:59")),
        Some(at("2026-10-19 10:00"))
    );
    assert_eq!(
        last_slot(d, 10, 1, at("2026-10-20 10:00")),
        Some(at("2026-10-20 10:00"))
    );
    assert_eq!(
        next_slot(d, 10, 1, at("2026-10-20 10:00")),
        Some(at("2026-10-21 10:00"))
    );
    assert_eq!(
        next_slot(d, 10, 1, at("2026-10-20 09:00")),
        Some(at("2026-10-20 10:00"))
    );
    assert_eq!(
        last_slot(ScanEvery::Off, 10, 1, at("2026-10-20 10:00")),
        None
    );
    assert_eq!(
        next_slot(ScanEvery::Off, 10, 1, at("2026-10-20 10:00")),
        None
    );
}

#[test]
fn weekly_slots() {
    let w = ScanEvery::Weekly;
    // Tuesday 09:00, scans on Tuesdays at 10: the last one was a week ago.
    assert_eq!(
        last_slot(w, 10, 2, at("2026-10-20 09:00")),
        Some(at("2026-10-13 10:00"))
    );
    assert_eq!(
        last_slot(w, 10, 2, at("2026-10-20 11:00")),
        Some(at("2026-10-20 10:00"))
    );
    // Mondays: yesterday. Sundays: two days ago.
    assert_eq!(
        last_slot(w, 10, 1, at("2026-10-20 09:00")),
        Some(at("2026-10-19 10:00"))
    );
    assert_eq!(
        last_slot(w, 10, 7, at("2026-10-20 09:00")),
        Some(at("2026-10-18 10:00"))
    );
    // Wednesday: last week's.
    assert_eq!(
        last_slot(w, 10, 3, at("2026-10-20 09:00")),
        Some(at("2026-10-14 10:00"))
    );
    assert_eq!(
        next_slot(w, 10, 3, at("2026-10-20 09:00")),
        Some(at("2026-10-21 10:00"))
    );
}

#[test]
fn due_after_the_slot_once_and_missed_runs_after_sleep_run_once() {
    let d = ScanEvery::Daily;
    let armed = Some(at("2026-10-20 08:00"));
    // Before 10:00: not due; after: due.
    assert!(!due(d, 10, 1, at("2026-10-20 09:59"), None, armed));
    assert!(due(d, 10, 1, at("2026-10-20 10:00"), None, armed));
    // Scanned at 10:01: not due again until tomorrow 10:00.
    let last = Some(at("2026-10-20 10:01"));
    assert!(!due(d, 10, 1, at("2026-10-20 23:00"), last, armed));
    assert!(due(d, 10, 1, at("2026-10-21 10:00"), last, armed));
    // The Mac slept from Tuesday 09:00 to Friday 15:00: three slots missed, one scan, then
    // nothing more until Saturday.
    let woke = at("2026-10-23 15:00");
    assert!(due(d, 10, 1, woke, Some(at("2026-10-19 10:00")), armed));
    assert!(!due(d, 10, 1, at("2026-10-23 15:05"), Some(woke), armed));
    assert!(due(d, 10, 1, at("2026-10-24 10:00"), Some(woke), armed));
    // Never armed: never due (the agent arms a schedule when it first sees it).
    assert!(!due(d, 10, 1, woke, None, None));
    // A schedule set at 15:00 does not scan for the 10:00 slot of the same day.
    assert!(!due(
        d,
        10,
        1,
        at("2026-10-20 15:01"),
        None,
        Some(at("2026-10-20 15:00"))
    ));
}

#[test]
fn next_scan_text() {
    let s = Settings {
        scan_every: ScanEvery::Weekly,
        scan_hour: 9,
        scan_weekday: 5,
        ..Settings::default()
    };
    assert_eq!(
        next_text(&s, at("2026-10-20 12:00")).as_deref(),
        Some("Next scan: Friday at 09:00")
    );
    let daily = Settings {
        scan_every: ScanEvery::Daily,
        scan_hour: 18,
        ..Settings::default()
    };
    assert_eq!(
        next_text(&daily, at("2026-10-20 12:00")).as_deref(),
        Some("Next scan: today at 18:00")
    );
    assert_eq!(
        next_text(&daily, at("2026-10-20 19:00")).as_deref(),
        Some("Next scan: tomorrow at 18:00")
    );
    assert_eq!(
        next_text(&Settings::default(), at("2026-10-20 12:00")),
        None
    );
}

fn daily(hour: u8) -> Settings {
    Settings {
        scan_every: ScanEvery::Daily,
        scan_hour: hour,
        ..Settings::default()
    }
}

/// The command line FakeSys records for a notification.
fn line(title: &str, body: &str, sub: Option<&str>) -> String {
    let n = Notification {
        title: title.into(),
        subtitle: sub.map(str::to_string),
        body: body.into(),
        route: None,
    };
    format!(
        "/usr/bin/osascript {}",
        notify::osascript_args(&n).join(" ")
    )
}

fn osascripts(env: &Env) -> Vec<String> {
    env.fake_sys()
        .expect("fake")
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("/usr/bin/osascript"))
        .collect()
}

#[test]
fn a_scheduled_scan_only_looks_then_notifies() {
    let m = sweep_mac();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("osascript", "");
    let before: Vec<_> = [
        "Library/Mail Downloads/a/report.pdf",
        "Library/Mail Downloads/b.zip",
        ".Trash/old.dmg",
    ]
    .iter()
    .map(|r| m.env.home_path(r))
    .collect();
    let s = daily(10);
    let mut a = Agent::new(&m.env, sweep_sources());
    // First sight arms the schedule at 09:00: nothing yet.
    let r = a.check(&m.env, &s, None, at("2026-10-20 09:00"));
    assert_eq!(r.scanned, None);
    let r = a.check(&m.env, &s, None, at("2026-10-20 10:00"));
    let found = r.scanned.expect("the 10:00 scan ran");
    assert!(found > 0);
    // Scan only: nothing journaled, nothing moved, no administrator command, the plans saved.
    assert!(history(&m.env).is_empty(), "{:?}", history(&m.env));
    for p in &before {
        assert!(p.exists(), "{} was moved", p.display());
    }
    assert!(fake.admin_calls().is_empty());
    assert!(
        std::fs::read_dir(cleancraft_core::plans::dir(&m.env))
            .map(|d| d.count())
            .unwrap_or(0)
            >= 2
    );
    // The exact notification line.
    let text = found_text(found);
    assert!(text.starts_with("CleanCraft found ") && text.ends_with(" to clean"));
    assert_eq!(
        osascripts(&m.env),
        vec![line(
            "CleanCraft",
            &format!("{text}. {}", notify::MENU_HINT),
            None
        )]
    );
    assert_eq!(r.shown.len(), 1);
    assert_eq!(r.shown[0].route.as_deref(), Some(SWEEP_ROUTE));
    // The menu offers to review it.
    let menu = a.menu(None, &s);
    assert_eq!(menu.reviews.len(), 1);
    assert_eq!(menu.reviews[0].text, format!("Review: {text}"));
    // Not again the same day, also after a restart of the agent (state on disk).
    let mut again = Agent::new(&m.env, sweep_sources());
    let r = again.check(&m.env, &s, None, at("2026-10-20 18:00"));
    assert_eq!(r.scanned, None);
    assert_eq!(osascripts(&m.env).len(), 1);
    let r = again.check(&m.env, &s, None, at("2026-10-21 10:00"));
    assert!(r.scanned.is_some());
    assert!(history(&m.env).is_empty());
}

#[test]
fn changing_the_schedule_re_arms_it() {
    let m = sweep_mac();
    m.env.fake_sys().expect("fake").answer("osascript", "");
    let mut a = Agent::new(&m.env, sweep_sources());
    a.check(&m.env, &daily(10), None, at("2026-10-20 09:00"));
    // At 09:30 the person moves the time to 09:00: today's 09:00 is before the change.
    let r = a.check(&m.env, &daily(9), None, at("2026-10-20 09:30"));
    assert_eq!(r.scanned, None);
    let r = a.check(&m.env, &daily(9), None, at("2026-10-21 09:00"));
    assert!(r.scanned.is_some());
    // Off: nothing, and the state forgets the arming.
    let r = a.check(&m.env, &Settings::default(), None, at("2026-10-22 09:00"));
    assert_eq!(r.scanned, None);
    assert_eq!(a.state.armed, None);
}

#[test]
fn nothing_found_means_no_notification() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer("osascript", "");
    let mut a = Agent::new(&m.env, sweep_sources());
    a.check(&m.env, &daily(10), None, at("2026-10-20 09:00"));
    let r = a.check(&m.env, &daily(10), None, at("2026-10-20 10:00"));
    assert_eq!(r.scanned, Some(0));
    assert!(osascripts(&m.env).is_empty());
    assert!(a.menu(None, &daily(10)).reviews.is_empty());
}

#[test]
fn paused_notifications_are_not_shown_but_kept_in_the_menu() {
    let m = sweep_mac();
    m.env.fake_sys().expect("fake").answer("osascript", "");
    let s = Settings {
        notifications_paused: true,
        ..daily(10)
    };
    let mut a = Agent::new(&m.env, sweep_sources());
    a.check(&m.env, &s, None, at("2026-10-20 09:00"));
    let r = a.check(&m.env, &s, None, at("2026-10-20 10:00"));
    assert!(r.scanned.is_some());
    assert!(r.shown.is_empty());
    assert!(osascripts(&m.env).is_empty());
    let menu = a.menu(None, &s);
    assert!(menu.paused);
    assert_eq!(menu.pause_label(), RESUME);
    assert_eq!(menu.reviews.len(), 1);
}

#[test]
fn a_notification_that_fails_is_reported() {
    let m = sweep_mac();
    m.env.fake_sys().expect("fake").answer_full(
        "osascript",
        cleancraft_core::sys::CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "execution error: Notifications are off (-1)".into(),
        },
    );
    let mut a = Agent::new(&m.env, sweep_sources());
    a.check(&m.env, &daily(10), None, at("2026-10-20 09:00"));
    let r = a.check(&m.env, &daily(10), None, at("2026-10-20 10:00"));
    assert!(r.shown.is_empty());
    assert_eq!(r.failed.len(), 1);
    assert!(
        r.failed[0].contains("Notifications are off"),
        "{:?}",
        r.failed
    );
}

fn snapshot(m: &FakeMac, free_percent: f64) -> Snapshot {
    answer_all(m.env.fake_sys().expect("fake"));
    let mut s = Sampler::new().sample(&m.env, 1_760_100_000.0);
    let total = 500_000_000_000u64;
    s.storage.total = Some(total);
    s.storage.free = Some((total as f64 * free_percent / 100.0) as u64);
    s
}

#[test]
fn disk_low_alert_at_most_once_a_day_and_the_badge() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer("osascript", "");
    let s = Settings::default();
    assert_eq!(s.disk_low_percent, 10);
    let mut a = Agent::new(&m.env, sweep_sources());
    let low = snapshot(&m, 8.0);
    let ok = snapshot(&m, 12.0);
    assert_eq!(disk_low(&low, 10), Some(true));
    assert_eq!(disk_low(&ok, 10), Some(false));
    assert_eq!(disk_low(&low, 0), None, "0 turns it off");

    let r = a.check(&m.env, &s, Some(&low), at("2026-10-20 09:00"));
    assert_eq!(r.shown.len(), 1);
    assert_eq!(r.shown[0].title, "Your startup disk is almost full");
    assert_eq!(
        osascripts(&m.env),
        vec![line(
            "Your startup disk is almost full",
            &format!(
                "40.0 GB free. Quick Sweep can show what to clean. {}",
                notify::MENU_HINT
            ),
            None
        )]
    );
    // Same day, later and after a restart: no second alert.
    a.check(&m.env, &s, Some(&low), at("2026-10-20 18:00"));
    let mut b = Agent::new(&m.env, sweep_sources());
    b.check(&m.env, &s, Some(&low), at("2026-10-20 23:59"));
    assert_eq!(osascripts(&m.env).len(), 1);
    // The next day: one more.
    b.check(&m.env, &s, Some(&low), at("2026-10-21 00:01"));
    assert_eq!(osascripts(&m.env).len(), 2);
    // Enough space: no alert; turned off: no alert.
    b.check(&m.env, &s, Some(&ok), at("2026-10-22 09:00"));
    let off = Settings {
        disk_low_percent: 0,
        ..Settings::default()
    };
    b.check(&m.env, &off, Some(&low), at("2026-10-23 09:00"));
    assert_eq!(osascripts(&m.env).len(), 2);

    // The menu bar title carries the badge while the disk is low.
    let menu = b.menu(Some(&low), &s);
    assert!(menu.disk_low);
    assert!(menu.title.ends_with(" Disk low"), "{}", menu.title);
    assert!(menu.title.starts_with("CPU 17% RAM "), "{}", menu.title);
    let menu = b.menu(Some(&ok), &s);
    assert!(!menu.disk_low);
    assert!(!menu.title.contains("Disk low"));
    assert!(history(&m.env).is_empty());
}

#[test]
fn menu_lines_and_unknown_values() {
    let m = FakeMac::new();
    let a = Agent::new(&m.env, sweep_sources());
    let menu = a.menu(None, &Settings::default());
    assert_eq!(menu.title, "CPU ? RAM ?");
    assert_eq!(
        menu.info,
        [
            "CPU: Unknown".to_string(),
            "Memory: Unknown".into(),
            "Free space: Unknown".into(),
            "Trash: Unknown".into()
        ]
    );
    let snap = snapshot(&m, 50.0);
    let menu = a.menu(Some(&snap), &Settings::default());
    assert_eq!(menu.info[0], "CPU: 17%");
    assert!(menu.info[1].starts_with("Memory: ") && menu.info[1].ends_with(" used"));
    assert_eq!(
        menu.info[2],
        format!(
            "Free space: {}",
            cleancraft_core::fsutil::human_size(250_000_000_000)
        )
    );
}

#[test]
fn watcher_notices_are_shown_and_open_their_page_from_the_menu() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().expect("fake");
    fake.answer("osascript", "");
    fake.answer("open", "");
    let mut a = Agent::new(&m.env, sweep_sources());
    let n = Notification {
        title: "\"Editor\" went to the Trash".into(),
        subtitle: Some("Its leftovers take 120 MB".into()),
        body: "Remove them with App Remover.\nNothing moves without your OK.".into(),
        route: Some("uninstall:/Users/tester/.Trash/Editor.app".into()),
    };
    let r = a.notice(&m.env, &Settings::default(), n);
    assert_eq!(r.shown.len(), 1);
    assert_eq!(
        osascripts(&m.env),
        vec![line(
            "\"Editor\" went to the Trash",
            "Remove them with App Remover. Nothing moves without your OK.",
            Some("Its leftovers take 120 MB")
        )]
    );
    let mut settings = Settings::default();
    assert_eq!(a.menu(None, &settings).reviews.len(), 1);
    assert_eq!(
        a.act(&m.env, &mut settings, MenuAction::Review(0)),
        Ok(false)
    );
    let mut inbox = notify::OpenInbox::default();
    assert_eq!(
        inbox
            .poll(&m.env, chrono::Utc::now().timestamp())
            .as_deref(),
        Some("uninstall:/Users/tester/.Trash/Editor.app")
    );
    assert!(
        a.menu(None, &settings).reviews.is_empty(),
        "opened, so gone"
    );
    assert!(a.act(&m.env, &mut settings, MenuAction::Review(0)).is_err());
    assert!(history(&m.env).is_empty());
}

#[test]
fn reviews_keep_the_newest_three() {
    let m = FakeMac::new();
    m.env.fake_sys().expect("fake").answer("osascript", "");
    let mut a = Agent::new(&m.env, sweep_sources());
    for i in 0..5 {
        a.notice(
            &m.env,
            &Settings::default(),
            Notification {
                title: format!("n{i}"),
                subtitle: None,
                body: "b".into(),
                route: Some(format!("r{i}")),
            },
        );
    }
    let texts: Vec<String> = a
        .menu(None, &Settings::default())
        .reviews
        .into_iter()
        .map(|r| r.text)
        .collect();
    assert_eq!(texts, ["Review: n4", "Review: n3", "Review: n2"]);
}
