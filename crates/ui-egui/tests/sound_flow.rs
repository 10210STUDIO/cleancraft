//! Interface sounds in the real flow: a scan plays "scan start" then "scan done" once each, a
//! cleanup plays "clean done" once, and with sounds off in Settings nothing plays.
//!
//! Ignored until design3 adds the `sound::play` calls (BOARD, sound3 -> design3); remove the
//! `#[ignore]`s then.

mod common;

use cleancraft_core::Settings;
use cleancraft_core::testkit::FakeMac;
use common::*;

fn played(mac: &FakeMac) -> Vec<String> {
    mac.env
        .fake_sys()
        .expect("fake")
        .spawned()
        .iter()
        .filter(|l| l.starts_with("/usr/bin/afplay "))
        .map(|l| l.rsplit('/').next().unwrap_or_default().to_string())
        .collect()
}

#[test]
#[ignore = "call sites land with design3 (BOARD)"]
fn a_scan_and_a_cleanup_play_their_cues_once() {
    let mac = FakeMac::sample();
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    steps(&mut h);
    assert_eq!(played(&mac), vec!["scan-start.wav", "scan-done.wav"]);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    steps(&mut h);
    assert_eq!(
        played(&mac),
        vec!["scan-start.wav", "scan-done.wav", "clean-done.wav"]
    );
}

#[test]
#[ignore = "call sites land with design3 (BOARD)"]
fn sounds_off_play_nothing_in_the_flow() {
    let mac = FakeMac::sample();
    Settings {
        sounds: false,
        ..Settings::default()
    }
    .save(&mac.env)
    .expect("save");
    let mut h = harness(&mac);
    click(&mut h, "System Junk");
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    steps(&mut h);
    assert!(played(&mac).is_empty(), "{:?}", played(&mac));
}
