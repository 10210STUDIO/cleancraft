//! Round 6 safety review (reviewer safety6), by clicks on a FakeMac.
//!
//! Lead rule (COMMON6, 17:25): a locked item always comes with a way forward on the same
//! screen, the reason in plain words AND a button or exact steps. A page state where every
//! listed item is locked and nothing can be done is S1. `way_forward_*` tests walk the page
//! states that lock everything (app running, Trash only Finder empties, protected places) and
//! fail when the screen offers no enabled action and no step.
//!
//! Ignored tests prove open defects (SAF6-n, docs/review/SAFETY-R6.md).
//! Run them: `cargo test -p cleancraft-ui-egui --test review_safety6_ui -- --ignored`.

mod common;

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, mail_mac, privacy_mac, trash_mac};
use cleancraft_core::{FeatureId, Plan};
use common::*;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};

/// Controls that are on every result screen and do not get a locked item out of its lock.
const GENERIC: &[&str] = &[
    "Scan",
    "Scan Again",
    "Stop",
    "Review and Clean",
    "Select All",
    "Select None",
    "Search",
    "Sort",
    "Back",
    "Done",
    "Cancel",
    "Reveal",
    "Ignore",
    "Include",
    "Not Now",
];

/// Words that make a visible text a step the person can follow.
const STEP_WORDS: &[&str] = &[
    "Quit ",
    "System Settings",
    "Open ",
    "Erase ",
    "Empty ",
    "Turn off",
    "Turn on",
    "Grant ",
    "Allow ",
    "Move ",
];

/// The right edge of the sidebar: content controls lie right of it.
fn sidebar_edge(h: &H) -> f32 {
    let ppp = h.ctx.pixels_per_point();
    ["Clean", "Protect", "Speed", "Apps"]
        .iter()
        .filter_map(|l| h.query_all_by_label(l).next())
        .filter_map(|n| n.accesskit_node().bounding_box())
        .map(|r| r.x1 as f32 / ppp)
        .fold(0.0, f32::max)
}

/// What on this screen gets the person past the locks: enabled content buttons that are not
/// generic, and visible texts that name a step.
fn ways_forward(h: &H) -> (Vec<String>, Vec<String>) {
    let ppp = h.ctx.pixels_per_point();
    let edge = sidebar_edge(h);
    let mut buttons = Vec::new();
    let mut steps = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        if a.is_hidden() {
            continue;
        }
        let Some(r) = a.bounding_box() else { continue };
        if (r.x0 as f32 / ppp) < edge || r.y1 <= 0.0 || (r.y0 as f32 / ppp) > HGT {
            continue;
        }
        // AccessKit keeps a Label's text in its value.
        let label = if a.role() == Role::Label {
            a.value().unwrap_or_default()
        } else {
            a.label().unwrap_or_default()
        };
        if label.is_empty() {
            continue;
        }
        // Category rows ("Mail trash  70 KB") only filter the list.
        let category_row = label.contains("  ")
            && label.rsplit("  ").next().is_some_and(|t| {
                t.ends_with(" KB")
                    || t.ends_with(" MB")
                    || t.ends_with(" GB")
                    || t.ends_with(" bytes")
            });
        let clickable = matches!(a.role(), Role::Button | Role::Link | Role::MenuItem);
        if clickable && !a.is_disabled() && !category_row && !GENERIC.contains(&label.as_str()) {
            buttons.push(label.clone());
        }
        if STEP_WORDS.iter().any(|w| label.contains(w)) {
            steps.push(label);
        }
    }
    (buttons, steps)
}

fn plan(h: &H, f: FeatureId) -> Plan {
    h.state().plans[&f].plan.clone().expect("plan after scan")
}

/// The rule itself: when every item of the plan on screen is locked, a way forward shows.
fn assert_way_forward(h: &H, f: FeatureId, what: &str) {
    let p = plan(h, f);
    assert!(!p.items.is_empty(), "{what}: the scenario found nothing");
    let all_locked = p.items.iter().all(|i| i.locked.is_some());
    assert!(all_locked, "{what}: the scenario should lock every item");
    let (buttons, steps) = ways_forward(h);
    assert!(
        !buttons.is_empty() || !steps.is_empty(),
        "{what}: every item is locked ({:?}) and the screen offers no enabled action and no step",
        p.items
            .iter()
            .filter_map(|i| i.locked.clone())
            .collect::<Vec<_>>()
    );
}

fn open(mac: &FakeMac, section: &str, tool: &str) -> H {
    let mut h = harness(mac);
    click(&mut h, section);
    click(&mut h, tool);
    click(&mut h, "Scan");
    h
}

#[test]
fn way_forward_mail_open() {
    let mac = mail_mac();
    kit::set_running(&mac.env, "Mail", "/System/Applications/Mail.app");
    let h = open(&mac, "Clean", "Mail Downloads");
    assert_way_forward(&h, FeatureId::MailAttachments, "Mail Downloads, Mail open");
}

#[test]
fn way_forward_trash_bins() {
    let mac = trash_mac();
    let h = open(&mac, "Clean", "Trash Bins");
    assert_way_forward(&h, FeatureId::TrashBins, "Trash Bins");
}

#[test]
fn way_forward_privacy_every_browser_open() {
    let mac = privacy_mac();
    for (name, app) in [
        ("Safari", "/Applications/Safari.app"),
        ("Google Chrome", "/Applications/Google Chrome.app"),
        ("Brave Browser", "/Applications/Brave Browser.app"),
        ("Firefox", "/Applications/Firefox.app"),
    ] {
        kit::set_running(&mac.env, name, app);
    }
    let h = open(&mac, "Protect", "Privacy Traces");
    let p = plan(&h, FeatureId::Privacy);
    // Items of other kinds (Wi-Fi, permissions) may stay unlocked; the rule is about the
    // browsers' rows when every one of them is locked.
    let locked: Vec<_> = p.items.iter().filter(|i| i.locked.is_some()).collect();
    assert!(!locked.is_empty(), "browsers open lock their rows");
    let (buttons, steps) = ways_forward(&h);
    assert!(
        !buttons.is_empty() || !steps.is_empty(),
        "browsers open: no enabled action and no step on screen"
    );
}

/// Developer Clutter with the person's code folder in Documents (very common): every project
/// is locked as a protected place. The screen must say what to do instead (move the folder
/// out of Documents, or run the project's own clean command), not only "never".
#[test]
fn way_forward_dev_clutter_in_documents() {
    use cleancraft_core::Settings;
    let mac = FakeMac::new();
    let code = mac.env.home_path("Documents/code");
    for name in ["game", "site"] {
        let d = code.join(name);
        mac.write(&d.join("Cargo.toml"), b"[package]\nname = \"x\"\n");
        mac.write(&d.join("target/debug/app"), &vec![7u8; 300_000]);
    }
    let s = Settings {
        dev_roots: vec![code],
        ..Settings::default()
    };
    s.save(&mac.env).unwrap();
    let h = open(&mac, "Clean", "Developer Clutter");
    assert_way_forward(
        &h,
        FeatureId::DevClutter,
        "Developer Clutter, code in Documents",
    );
}

/// H1: on Mac Health every enabled button only opens a page or starts a scan that looks.
/// Clicking each one (on a Mac that needs attention everywhere) writes no journal line, moves
/// no file, and runs no command that changes the system.
#[test]
fn h1_every_health_button_only_looks() {
    use cleancraft_core::testkit_health::{self as th, GB};
    use cleancraft_core::{FeatureId, executor};
    use std::path::PathBuf;
    let now = th::now();
    let mac = th::calm_mac(now);
    th::sample_history(&mac.env, now, 500 * GB, 12 * GB);
    th::set_disk(&mac.env, 500 * GB, 12 * GB);
    th::set_pressure(&mac.env, 4);
    th::launch_agent(&mac, "com.example.unsigned", false);
    th::note(&mac.env, FeatureId::Malware, 1, 2_000_000, 1, now);
    th::note(&mac.env, FeatureId::SystemJunk, 60, 3_000_000_000, 400, now);
    mac.home_file("Library/Caches/com.example.big/blob", 2_000_000);
    let snapshot = |m: &FakeMac| -> Vec<(PathBuf, u64)> {
        let data = m.env.data_dir().to_path_buf();
        let mut out = Vec::new();
        let mut stack = vec![m.env.root().to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.starts_with(&data) {
                    continue;
                }
                match std::fs::symlink_metadata(&p) {
                    Ok(md) if md.is_dir() => stack.push(p),
                    Ok(md) => out.push((p, md.len())),
                    _ => {}
                }
            }
        }
        out.sort();
        out
    };
    let before = snapshot(&mac);
    let mut h = harness(&mac);
    let go_health = |h: &mut H| {
        h.state_mut()
            .open_route(&cleancraft_core::route::AppRoute::Health);
        for _ in 0..6 {
            h.step();
        }
    };
    go_health(&mut h);
    let (buttons, _) = ways_forward(&h);
    assert!(buttons.len() >= 3, "few buttons on Mac Health: {buttons:?}");
    for b in &buttons {
        go_health(&mut h);
        if h.query_all_by_label(b).count() != 1 {
            continue;
        }
        // Below the fold the person scrolls first; the action is the same.
        h.get_by_label(b).click();
        for _ in 0..6 {
            h.step();
        }
        assert!(
            executor::history(&mac.env).is_empty(),
            "'{b}' wrote a journal line"
        );
        let fake = mac.env.fake_sys().unwrap();
        assert!(
            fake.admin_calls().is_empty(),
            "'{b}' asked for the administrator"
        );
        let changing: Vec<_> = fake
            .calls()
            .into_iter()
            .filter(|c| {
                let w: Vec<&str> = c.split_whitespace().collect();
                (w.first().is_some_and(|p| p.ends_with("launchctl"))
                    && w.get(1).is_some_and(|s| {
                        [
                            "bootout",
                            "bootstrap",
                            "unload",
                            "load",
                            "disable",
                            "remove",
                        ]
                        .contains(s)
                    }))
                    || c.contains("/bin/mv")
                    || c.contains("purge")
                    || c.contains("periodic")
            })
            .collect();
        assert!(changing.is_empty(), "'{b}' ran {changing:?}");
    }
    assert_eq!(before, snapshot(&mac), "files changed");
}
