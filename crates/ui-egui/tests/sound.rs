//! Interface sounds: the right cue plays once through `afplay` (recorded by FakeSys, nothing
//! is heard), settings turn them off, and the built-in WAVs are clean.

use cleancraft_core::Settings;
use cleancraft_core::testkit::FakeMac;
use cleancraft_ui_egui::sound::{self, Cue};

fn spawned(mac: &FakeMac) -> Vec<String> {
    mac.env.fake_sys().expect("fake").spawned()
}

fn played(mac: &FakeMac) -> Vec<String> {
    spawned(mac)
        .iter()
        .map(|l| {
            assert!(l.starts_with("/usr/bin/afplay -v "), "{l}");
            l.rsplit('/').next().unwrap_or_default().to_string()
        })
        .collect()
}

#[test]
fn scan_start_and_done_each_play_their_cue_once() {
    let mac = FakeMac::sample();
    sound::apply(&mac.env, &Settings::default());
    assert!(sound::play(&mac.env, Cue::ScanStart));
    // The scanning screen redraws many times; a repeated start in the same burst is dropped.
    for _ in 0..20 {
        sound::play(&mac.env, Cue::ScanStart);
    }
    assert!(sound::play(&mac.env, Cue::ScanDone));
    sound::play(&mac.env, Cue::ScanDone);
    assert_eq!(played(&mac), vec!["scan-start.wav", "scan-done.wav"]);
    // Played from the app's own data folder in the fake Mac, at the default volume.
    let dir = sound::cache_dir(&mac.env);
    assert!(dir.starts_with(mac.env.data_dir()));
    let first = &spawned(&mac)[0];
    let gain = format!(
        "{:.2}",
        sound::afplay_gain(Settings::default().sound_volume)
    );
    assert_eq!(
        first,
        &format!(
            "/usr/bin/afplay -v {gain} {}",
            dir.join("scan-start.wav").display()
        )
    );
    assert_eq!(
        std::fs::read(dir.join("scan-done.wav")).expect("cached"),
        Cue::ScanDone.wav()
    );
    // Nothing went through the waiting `run` path.
    assert!(
        mac.env
            .fake_sys()
            .expect("fake")
            .calls()
            .iter()
            .all(|c| !c.contains("afplay"))
    );
}

#[test]
fn sounds_off_in_settings_play_nothing() {
    let mac = FakeMac::sample();
    let s = Settings {
        sounds: false,
        ..Settings::default()
    };
    s.save(&mac.env).expect("save");
    // Read from the settings file on first use.
    for cue in Cue::ALL {
        assert!(!sound::play(&mac.env, cue), "{cue:?} played while off");
    }
    assert!(spawned(&mac).is_empty());
    assert!(!sound::cache_dir(&mac.env).exists());
    // Volume zero is silent too.
    sound::apply(
        &mac.env,
        &Settings {
            sound_volume: 0.0,
            ..Settings::default()
        },
    );
    assert!(!sound::play(&mac.env, Cue::Error));
    assert!(spawned(&mac).is_empty());
    // Back on.
    sound::set_volume(&mac.env, 1.0);
    assert!(sound::play(&mac.env, Cue::Error));
    assert_eq!(played(&mac), vec!["error.wav"]);
    assert!(spawned(&mac)[0].starts_with("/usr/bin/afplay -v 1.00 "));
    sound::set_enabled(&mac.env, false);
    assert!(!sound::play(&mac.env, Cue::CleanDone));
    assert_eq!(spawned(&mac).len(), 1);
}

#[test]
fn tap_ticks_are_off_by_default() {
    let mac = FakeMac::sample();
    sound::apply(&mac.env, &Settings::default());
    assert!(!sound::play(&mac.env, Cue::Tap));
    sound::set_ticks(&mac.env, true);
    assert!(sound::play(&mac.env, Cue::Tap));
    assert_eq!(played(&mac), vec!["tap.wav"]);
}

#[test]
fn preview_plays_at_the_slider_volume_even_when_off() {
    let mac = FakeMac::sample();
    sound::apply(
        &mac.env,
        &Settings {
            sounds: false,
            ..Settings::default()
        },
    );
    assert!(sound::preview(&mac.env, 0.5));
    assert_eq!(spawned(&mac).len(), 1);
    assert!(spawned(&mac)[0].starts_with("/usr/bin/afplay -v 0.25 "));
    assert!(!sound::preview(&mac.env, 0.0));
}

#[test]
fn nothing_plays_off_macos() {
    let mac = FakeMac::sample();
    mac.env.fake_sys().expect("fake").with(|s| s.macos = false);
    sound::apply(&mac.env, &Settings::default());
    assert!(!sound::play(&mac.env, Cue::ScanDone));
    assert!(spawned(&mac).is_empty());
}

#[test]
fn a_damaged_cached_file_is_rewritten() {
    let mac = FakeMac::sample();
    sound::apply(&mac.env, &Settings::default());
    let p = sound::cache_dir(&mac.env).join("trash.wav");
    std::fs::create_dir_all(p.parent().expect("dir")).expect("dir");
    std::fs::write(&p, b"not a wav").expect("write");
    assert!(sound::play(&mac.env, Cue::Trash));
    assert_eq!(std::fs::read(&p).expect("read"), Cue::Trash.wav());
}

#[test]
fn the_wavs_are_clean() {
    let mut total = 0;
    for cue in Cue::ALL {
        let bytes = cue.wav();
        total += bytes.len();
        let (rate, s) = sound::decode_wav(bytes)
            .unwrap_or_else(|| panic!("{cue:?} is not a 16-bit mono PCM WAV"));
        assert_eq!(rate, 48_000, "{cue:?}");
        let secs = s.len() as f32 / rate as f32;
        let (min, max) = if cue == Cue::Tap {
            (0.03, 0.15)
        } else {
            (0.15, 1.2)
        };
        assert!((min..=max).contains(&secs), "{cue:?} lasts {secs:.3} s");
        let peak = s.iter().map(|v| (*v as i32).abs()).max().unwrap_or(0);
        let peak_db = 20.0 * (peak as f32 / 32768.0).log10();
        assert!(peak_db < -1.0, "{cue:?} peaks at {peak_db:.1} dBFS");
        assert!(
            peak_db > -30.0,
            "{cue:?} is nearly silent ({peak_db:.1} dBFS)"
        );
        // No click at either end.
        for (name, v) in [("first", s[0]), ("last", s[s.len() - 1])] {
            assert!(v.abs() <= 16, "{cue:?} {name} sample is {v}");
        }
        // Fades: the first 0.5 ms (the start of the fade in) and the last 2 ms (no cut-off
        // tail) stay under -40 dBFS.
        let (head, tail) = ((rate / 2000) as usize, (rate / 500) as usize);
        for v in s[..head].iter().chain(&s[s.len() - tail..]) {
            assert!(v.abs() < 328, "{cue:?} edge sample {v}");
        }
        // No sample-to-sample jump a speaker would click on.
        let jump = s
            .windows(2)
            .map(|w| (w[1] as i32 - w[0] as i32).abs())
            .max()
            .unwrap_or(0);
        assert!(jump < 16_000, "{cue:?} jumps by {jump}");
    }
    assert!(total < 1_000_000, "sounds take {total} bytes");
}
