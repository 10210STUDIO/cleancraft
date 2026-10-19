# sound3, round 3: interface sounds

Branch agent3/sound3. Original sounds, synthesized in code (nothing sampled, nothing modelled on another app's sounds).

## Cues

| Cue | File | Length | Peak | What it is |
|---|---|---|---|---|
| ScanStart | scan-start.wav | 0.75 s | -10 dBFS | Soft rising swell. An open fifth (D4, A4, a little D5) glides up a whole tone as it breathes in, with a band of air rising from 0.5 to 2.5 kHz behind it. |
| ScanDone | scan-done.wav | 1.10 s | -7 dBFS | Three soft mallet notes on an open C (C5, G5, C6, 85 ms apart), slightly stretched partials, light room. |
| CleanDone | clean-done.wav | 1.20 s | -7 dBFS | An airy whoosh (noise band 0.7 to 3.2 to 1.8 kHz), then a suspended F over C that resolves to E and G, landing on the same open C as ScanDone. |
| Trash | trash.wav | 0.38 s | -11 dBFS | Short paper swoosh: band-passed noise falling from 4.2 to 1.4 kHz, a slight paper grain and a soft low tuck as it lands. |
| Error | error.wav | 0.60 s | -9 dBFS | Two soft, low, rounded tones stepping down a minor third (E flat 4, C4), darkening as they decay. |
| Tap | tap.wav | 0.06 s | -22 dBFS | A tiny damped wooden tick (2.4 kHz and 1.25 kHz). Off by default. |

All files are 16-bit, 48 kHz, mono PCM WAV, 393 KB in total. Every file has a 3 ms fade-in, a release window over its last 30 to 35 percent (tails fade out, nothing is cut off) and first and last samples of exactly 0. TPDF dither is applied.

## Regenerating

`cargo run -p cleancraft-ui-egui --example make_sounds` (`crates/ui-egui/examples/make_sounds.rs`) rewrites `assets/sounds/*.wav` and prints each file's length, peak and size. The output is deterministic: the noise seeds are fixed, so the same recipes give byte-identical files. The library includes the WAVs with `include_bytes!`, so the example cannot build while the files are missing. In that case, bootstrap with plain rustc (the generator uses only std):

`CARGO_MANIFEST_DIR=$PWD/crates/ui-egui rustc --edition 2024 -O -o /tmp/make_sounds crates/ui-egui/examples/make_sounds.rs && /tmp/make_sounds`

## Playback

- `crates/ui-egui/src/sound.rs` provides `play(&env, Cue)`, `preview(&env, volume)`, `apply(&env, &settings)`, `set_enabled`, `set_volume` and `set_ticks`.
- The first time a cue plays, it is copied to `<data_dir>/Sounds/` (in tests, inside the fake Mac). The copy is checked and rewritten whenever it differs from the built-in file.
- Sounds play through `/usr/bin/afplay -v <gain>` via `env.sys.spawn`, which returns at once.
- The gain is the slider value squared, so the slider feels perceptual.
- Nothing plays off macOS, when sounds are off, or at volume 0.
- Tap plays only when ticks are on.
- If the same cue is asked for again within 0.6 s (Trash 0.25 s, Tap 60 ms), the repeat is dropped.
- State is kept per data folder, so parallel tests never share it.
- Core `sys.rs` gains `Sys::spawn`. The trait default refuses, `RealSys` cleans up the finished child process, and `FakeSys` records the call in `spawned` (kept apart from `calls`).
- Core `settings.rs` gains three fields with serde defaults: `sounds` (true), `sound_volume` (0.6) and `sound_ticks` (false). `Settings` no longer derives `Eq` because of the f32 field. I checked every use in the tree: none needed `Eq`, and the workspace builds and passes clippy.

## Tests

- core `settings::tests`:
  - `a_settings_file_from_before_sounds_still_loads_with_sounds_on`
  - `sound_settings_round_trip`
- core `sys::tests::fake_spawn_is_recorded_apart_from_calls`
- `crates/ui-egui/tests/sound.rs`, all on FakeSys:
  - `scan_start_and_done_each_play_their_cue_once` (repeats dropped, exact afplay line, played from the data folder)
  - `sounds_off_in_settings_play_nothing` (read from settings.json, volume 0, `set_enabled`)
  - `tap_ticks_are_off_by_default`
  - `preview_plays_at_the_slider_volume_even_when_off`
  - `nothing_plays_off_macos`
  - `a_damaged_cached_file_is_rewritten`
  - `the_wavs_are_clean`. It checks the RIFF/PCM header and 48 kHz rate, lengths of 0.15 to 1.2 s (tap 0.03 to 0.15 s), peaks between -30 and -1 dBFS, first and last samples within 16 LSB of zero, the first 0.5 ms and last 2 ms under -40 dBFS, no sample-to-sample jump of 16000 or more, and a total under 1 MB.
- `crates/ui-egui/tests/sound_flow.rs` has two flow tests, `#[ignore]`d until design3 wires the call sites:
  - a System Junk scan plays scan-start and scan-done once each, and the cleanup plays clean-done
  - with sounds off, the same flow plays nothing

## Handed to design3 (BOARD)

The API, the recommended call sites (scan started, scan finished with results, cleanup finished, single moves to the Trash, error banner shown, optional ticks) and the Settings controls: a "Play sounds" switch, a Volume slider with a Preview button, and a "Click sounds on switches" switch. The UI files are design3's, so I did not edit them.

## Needs a real Mac check (hearing them)

The sounds were made and checked here by numbers only: envelopes, spectral centroid (scan-start about 1.5 kHz, scan-done about 1.0 kHz, clean-done about 2.4 kHz, trash about 3.8 kHz, error about 0.4 kHz) and peaks. Nobody has listened to them. On a Mac:

1. `afplay -v 0.36 assets/sounds/<name>.wav` for each file, on the MacBook speakers and on headphones.
2. Check overall loudness against system alert sounds at the default 0.6 slider value.
3. Check that Trash reads as paper and not hiss.
4. Check that Error is noticeable but not alarming.
5. Check that the room tails do not sound boxy.
6. Check that the tap tick is subtle enough.

Level, timing or tone changes are one-line edits in the recipes in make_sounds.rs. afplay startup latency (typically 50 to 150 ms) also needs checking against the UI moment it marks.
