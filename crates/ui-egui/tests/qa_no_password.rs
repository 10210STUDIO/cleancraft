//! Maintenance without a password: Settings sets it up with one dialog and turns it off with
//! one more. The rule itself is written by the root script, simulated here.

mod common;

use cleancraft_core::admin;
use cleancraft_core::testkit::FakeMac;
use cleancraft_ui_egui::pages::settings::{ADMIN_OFF, ADMIN_SET_UP};
use common::*;
use egui_kittest::kittest::Queryable;

fn reach(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    steps(h);
    click(h, label);
}

#[test]
fn set_up_once_then_ask_every_time_again() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -un", "mario\n");
    let mut h = harness(&m);
    click(&mut h, "Settings");
    assert!(has_text(&h, "Off: macOS asks for your password each time."));
    // Cancelled: nothing changes.
    f.with(|s| s.admin_cancelled = true);
    reach(&mut h, ADMIN_SET_UP);
    steps(&mut h);
    assert!(has(&h, "Cancelled, nothing changed."));
    f.with(|s| s.admin_cancelled = false);
    reach(&mut h, ADMIN_SET_UP);
    steps(&mut h);
    assert!(has(&h, "These tasks no longer ask for the password."));
    assert_eq!(
        f.admin_calls().last().unwrap(),
        &admin::set_up_script("mario")
    );
    m.write(
        &m.env.sys_path(admin::RULE_FILE),
        admin::rule_text("mario").as_bytes(),
    );
    steps(&mut h);
    assert!(has_text(
        &h,
        "On: Free Up RAM, Flush DNS, Reindex Spotlight"
    ));
    reach(&mut h, ADMIN_OFF);
    steps(&mut h);
    assert!(has(&h, "Back to asking for the password every time."));
    assert_eq!(f.admin_calls().last().unwrap(), &admin::turn_off_script());
    assert_eq!(f.admin_calls().len(), 3);
}
