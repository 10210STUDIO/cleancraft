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
    for (_, feats) in cleancraft_ui_egui::app::GROUPS {
        for f in *feats {
            let r = h.get_all_by_label(f.title()).next().unwrap().rect();
            println!("{:<20} {r:?}", f.title());
            if r.max.y > HGT + 0.5 {
                outside.push(f.title().to_string());
            }
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
