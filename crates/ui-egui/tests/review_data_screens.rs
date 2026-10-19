//! Data-qa review (reviewer "data") by clicks: numbers and counts on screens, recalculated from
//! the fake disk or the sample tool output. Ignored tests prove a defect (`DAT-n`).

mod common;

use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use common::*;
use egui_kittest::kittest::Queryable;

/// A fake Mac whose System Junk finds exactly one item.
fn one_cache() -> (FakeMac, std::path::PathBuf) {
    let mac = FakeMac::new();
    mac.home_file("Library/Caches/com.example.Solo/cache.db", 1_200_000);
    let p = mac.env.home_path("Library/Caches/com.example.Solo");
    (mac, p)
}

#[test]
fn one_item_is_singular_on_every_step() {
    let (mac, _) = one_cache();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let plan = h.state().plans[&cleancraft_core::FeatureId::SystemJunk]
        .plan
        .clone()
        .expect("plan");
    assert_eq!(plan.items.len(), 1, "{:?}", plan.items);
    let mut wrong = Vec::new();
    if has_text(&h, "1 items") {
        wrong.push("results: 'found in 1 items'");
    }
    click(&mut h, "Review and Clean");
    if has_text(&h, "Move 1 items") {
        wrong.push("confirmation: 'Move 1 items'");
    }
    click(&mut h, "Move to Trash");
    if has_text(&h, "1 items moved") {
        wrong.push("done: '1 items moved'");
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[test]
fn done_and_history_sizes_match_the_disk() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    let plan = h.state().plans[&cleancraft_core::FeatureId::SystemJunk]
        .plan
        .clone()
        .expect("plan");
    let sel: Vec<_> = plan.selected().map(|i| i.path.clone()).collect();
    let expect: u64 = sel.iter().map(|p| tree_size(p).0).sum();
    // Bottom bar: count and size of the selection, recalculated.
    assert!(has(
        &h,
        &format!("{} selected, {}", sel.len(), human_size(expect))
    ));
    click(&mut h, "Review and Clean");
    assert!(has_text(
        &h,
        &format!("Move {} items ({})", sel.len(), human_size(expect))
    ));
    click(&mut h, "Move to Trash");
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(expect))
    ));
    // The journal records exactly what sits in the Trash now.
    let hist = cleancraft_core::executor::history(&mac.env);
    assert_eq!(hist.len(), sel.len());
    let mut stored_sum = 0;
    for r in &hist {
        if let cleancraft_core::ActionKind::Moved { stored, .. } = &r.kind {
            assert_eq!(r.size, tree_size(stored).0);
            stored_sum += r.size;
        }
    }
    assert_eq!(stored_sum, expect);
}

#[test]
fn moving_to_the_trash_is_not_called_freed() {
    let (mac, cache) = one_cache();
    let trash_before = tree_size(&mac.env.user_trash()).0;
    let size = tree_size(&cache).0;
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(!cache.exists());
    // Recalculated: the Trash grew by exactly the moved size, so nothing was freed yet.
    let trash_after = tree_size(&mac.env.user_trash()).0;
    assert_eq!(trash_after - trash_before, size);
    assert!(
        !has(&h, &format!("{} freed", human_size(size))),
        "the Done screen claims {} freed; it is in the Trash until the Trash is emptied",
        human_size(size)
    );
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
}

const PMSET_CHARGING_EST: &str = "Now drawing from 'AC Power'
 -InternalBattery-0 (id=4653155)\t87%; charging; 1:30 remaining present: true
";

#[test]
fn charging_estimate_is_time_until_full() {
    let mac = FakeMac::new();
    let f = mac.env.fake_sys().unwrap();
    answer_all(f);
    f.answer("pmset", PMSET_CHARGING_EST);
    let mut h = harness(&mac);
    h.get_by_label("Live Monitor").scroll_to_me();
    steps(&mut h);
    click(&mut h, "Live Monitor");
    assert!(has(&h, "Charging"), "pmset state in sentence case");
    assert!(has(&h, "1 h 30 min"));
    assert!(
        !has(&h, "Time left"),
        "87% and charging: 1:30 is the time until full, shown as 'Time left'"
    );
    assert!(has(&h, "Time to full"));
}
