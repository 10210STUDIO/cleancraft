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
                t.ends_with(" KB") || t.ends_with(" MB") || t.ends_with(" GB") || t.ends_with(" bytes")
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
    eprintln!("{what}: buttons {buttons:?} steps {steps:?}");
    assert!(
        !buttons.is_empty() || !steps.is_empty(),
        "{what}: every item is locked ({:?}) and the screen offers no enabled action and no step",
        p.items.iter().filter_map(|i| i.locked.clone()).collect::<Vec<_>>()
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
