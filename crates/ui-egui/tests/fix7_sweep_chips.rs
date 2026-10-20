//! fix7 (user request): Quick Sweep's start screen chips choose what the scan includes. All
//! on by default; a chip turned off leaves its module out of the scan; the choice is
//! Settings::sweep_skip, so it stays after a relaunch and matches each result tile's Include;
//! none on disables Scan with a hint.

mod common;

use cleancraft_core::sweep::tile_title;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{FeatureId, Settings};
use common::*;
use egui::accesskit::Toggled;
use egui_kittest::kittest::{NodeT, Queryable};

fn open(m: &FakeMac) -> H {
    let mut h = harness(m);
    h.state_mut().sweep.sources = Some(sweep_sources());
    steps(&mut h);
    h
}

fn chip_on(h: &H, id: FeatureId) -> bool {
    let n = h.get_by_label(tile_title(id));
    n.accesskit_node().toggled() == Some(Toggled::True)
}

fn ids() -> Vec<FeatureId> {
    sweep_sources().ids()
}

fn tiles(h: &H) -> Vec<FeatureId> {
    h.state()
        .sweep
        .result
        .as_ref()
        .map(|r| r.tiles.iter().map(|t| t.id).collect())
        .unwrap_or_default()
}

#[test]
fn all_on_by_default_and_scan_names_everything() {
    let m = sweep_mac();
    let h = open(&m);
    for id in ids() {
        assert!(chip_on(&h, id), "{id:?} on by default");
    }
    assert!(has(&h, "Scan"));
}

#[test]
fn a_chip_turned_off_leaves_its_module_out_and_stays_after_relaunch() {
    let m = sweep_mac();
    let mut h = open(&m);
    let n = ids().len();
    click(&mut h, tile_title(FeatureId::MailAttachments));
    assert!(!chip_on(&h, FeatureId::MailAttachments));
    assert!(
        Settings::load(&m.env)
            .sweep_skip
            .contains(&FeatureId::MailAttachments)
    );
    let label = format!("Scan {} of {n}", n - 1);
    assert!(has(&h, &label), "the Scan button says what it scans");
    // Keyboard: focus the chip and press Space to turn it back on, then off again.
    h.get_by_label(tile_title(FeatureId::MailAttachments))
        .focus();
    steps(&mut h);
    h.key_press(egui::Key::Space);
    steps(&mut h);
    assert!(chip_on(&h, FeatureId::MailAttachments), "Space toggles");
    click(&mut h, tile_title(FeatureId::MailAttachments));
    click(&mut h, &label);
    let got = tiles(&h);
    assert!(!got.contains(&FeatureId::MailAttachments), "{got:?}");
    assert!(got.contains(&FeatureId::SystemJunk));
    // Relaunch: still off.
    let h2 = open(&m);
    assert!(!chip_on(&h2, FeatureId::MailAttachments));
    assert!(chip_on(&h2, FeatureId::SystemJunk));
}

#[test]
fn the_result_tiles_include_box_is_the_same_choice() {
    let m = sweep_mac();
    let mut h = open(&m);
    click(&mut h, "Scan");
    click(
        &mut h,
        &format!("Include {}", tile_title(FeatureId::SystemJunk)),
    );
    assert!(
        Settings::load(&m.env)
            .sweep_skip
            .contains(&FeatureId::SystemJunk)
    );
    let h2 = open(&m);
    assert!(!chip_on(&h2, FeatureId::SystemJunk));
}

#[test]
fn none_on_disables_scan_with_a_hint() {
    let m = sweep_mac();
    let mut h = open(&m);
    for id in ids() {
        click(&mut h, tile_title(id));
    }
    let label = format!("Scan 0 of {}", ids().len());
    let n = h.get_by_label(&label);
    assert!(n.accesskit_node().is_disabled());
    assert!(has_text(&h, "Choose at least one module above to scan."));
    n.click();
    steps(&mut h);
    assert!(h.state().sweep.result.is_none(), "nothing scanned");
}

/// Lead's extra: a file that stayed shows its next step, and Try Again moves it after the
/// same confirmation.
#[test]
fn done_shows_the_next_step_and_try_again_moves_what_stayed() {
    let m = sweep_mac();
    let mut h = open(&m);
    click(&mut h, "Scan");
    let b = m.env.home_path("Library/Mail Downloads/b.zip");
    m.env.fake_sys().unwrap().set_open(&b);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(b.exists());
    assert!(has_text(&h, "Quit the app that uses it, then Try Again."));
    let label = format!("Try Again {}", tile_title(FeatureId::MailAttachments));
    // The app was quit.
    m.env.fake_sys().unwrap().with(|s| s.open_files.clear());
    click(&mut h, &label);
    assert!(has_text(&h, "b.zip"), "the confirmation lists what stayed");
    click(&mut h, "Move to Trash");
    assert!(!b.exists());
}
