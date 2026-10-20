//! Final checker: where the sidebar rows are drawn at the default window size (FEATURES U1
//! says every row lies inside the window).

mod common;

use cleancraft_core::testkit::FakeMac;
use common::*;
use egui_kittest::kittest::Queryable;

#[test]
fn check_sidebar_rows_positions() {
    let mac = FakeMac::sample();
    let h = harness(&mac);
    let mut outside = Vec::new();
    // One row per section.
    for (name, _) in cleancraft_ui_egui::app::GROUPS {
        let r = h.get_all_by_label(name).next().unwrap().rect();
        println!("{name:<20} {r:?}");
        if r.max.y > HGT + 0.5 {
            outside.push(name.to_string());
        }
    }
    for l in ["History", "Settings"] {
        let r = h.get_all_by_label(l).next().unwrap().rect();
        println!("{l:<20} {r:?}");
        if r.max.y > HGT + 0.5 {
            outside.push(l.to_string());
        }
    }
    assert!(
        outside.is_empty(),
        "rows below the {HGT} px window: {outside:?}"
    );
}
