//! The interface in Italian. Its own test binary: the language is one setting for the whole
//! process, and every other test reads the English texts.

mod common;

use cleancraft_core::testkit::FakeMac;
use common::*;
use egui_kittest::kittest::Queryable;

#[test]
fn italian_shows_on_screen_and_to_voiceover() {
    cleancraft_core::i18n::set("it");
    let m = FakeMac::new();
    let mut h = harness(&m);
    steps(&mut h);
    // The sidebar, then the page it opens, in Italian; nothing is left in English there.
    click(&mut h, "Impostazioni");
    steps(&mut h);
    assert!(h.query_by_label("Settings").is_none());
    h.get_by_label("Lingua").scroll_to_me();
    steps(&mut h);
    assert!(h.query_by_label("Come il Mac").is_some());
    assert!(h.query_by_label("Italiano").is_some());
    // A text built from parts: the template and the name in it are both translated.
    assert_eq!(
        cleancraft_core::i18n::tr("Review: Live Monitor"),
        "Da vedere: Monitor in tempo reale"
    );
    cleancraft_core::i18n::set("en");
}
