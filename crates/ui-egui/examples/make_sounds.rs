//! Writes CleanCraft's interface sounds to `assets/sounds/*.wav` (16-bit, 48 kHz, mono).
//!
//! Every sound is synthesized here from scratch: additive partials with soft envelopes,
//! band-passed noise for air and paper, and a small Freeverb-style room for the tails. Nothing
//! is sampled. The output is deterministic (fixed noise seeds), so running this again gives
//! byte-identical files unless the recipes change.
//!
//! `cargo run -p cleancraft-ui-egui --example make_sounds`

use std::f32::consts::{PI, TAU};
use std::path::Path;

const SR: f32 = 48_000.0;

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
    std::fs::create_dir_all(&dir)?;
    let cues: [(&str, Vec<f32>); 6] = [
        ("scan-start", scan_start()),
        ("scan-done", scan_done()),
        ("clean-done", clean_done()),
        ("trash", trash()),
        ("error", error()),
        ("tap", tap()),
    ];
    for (name, samples) in cues {
        let path = dir.join(format!("{name}.wav"));
        let bytes = wav(&samples);
        std::fs::write(&path, &bytes)?;
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "{name:<11} {:>5} ms  peak {:>6.1} dBFS  {:>6} bytes",
            samples.len() as f32 / SR * 1000.0,
            20.0 * peak.log10(),
            bytes.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Recipes

/// A soft rising swell: an open fifth (D4, A4, a little D5) gliding up a whole tone while it
/// breathes in, with a band of air rising behind it. Says "working" without a fanfare.
fn scan_start() -> Vec<f32> {
    let len = 0.75;
    let mut b = buf(len);
    let swell = |t: f32| {
        // In over 0.42 s (sine squared), then a gentle fall.
        if t < 0.42 {
            (t / 0.42 * PI / 2.0).sin().powi(2)
        } else {
            (-(t - 0.42) / 0.16).exp()
        }
    };
    let glide = |t: f32| 2f32.powf((-2.0 + 2.0 * smooth(t / 0.5)) / 12.0);
    for (f, amp) in [(293.66, 0.55), (440.0, 0.38), (587.33, 0.16)] {
        let partials = [(1.0, 1.0), (2.0, 0.22), (3.0, 0.07)];
        let mut ph = [0.0f32; 3];
        for (i, s) in b.iter_mut().enumerate() {
            let t = i as f32 / SR;
            let fr = f * glide(t);
            let mut v = 0.0;
            for (k, (r, a)) in partials.iter().enumerate() {
                ph[k] = (ph[k] + TAU * fr * r / SR) % TAU;
                v += a * ph[k].sin();
            }
            *s += amp * swell(t) * v;
        }
    }
    let mut air = noise(len, 11);
    bandpass_sweep(&mut air, |t| 500.0 * 5f32.powf(smooth(t / 0.5)), 1.1);
    lowpass(&mut air, 3500.0);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s += 0.30 * swell(t) * air[i];
    }
    lowpass(&mut b, 7000.0);
    finish(b, 0.24, -10.0, 0.30)
}

/// Scan finished: three soft mallet notes on an open C (C5, G5, C6), a fifth and an octave,
/// so it reads as "done" and never as a jingle.
fn scan_done() -> Vec<f32> {
    let mut b = buf(1.1);
    for (at, f, amp) in [
        (0.0, 523.25, 0.75),
        (0.085, 783.99, 0.66),
        (0.17, 1046.5, 0.42),
    ] {
        bell(&mut b, at, f, amp, 0.42);
    }
    // A barely-there low C under the last note for body on small speakers.
    sine_note(&mut b, 0.17, 261.63, 0.10, 0.012, 0.35);
    lowpass(&mut b, 9000.0);
    finish(b, 0.22, -7.0, 0.35)
}

/// Cleanup finished: a breath of air sweeping up and settling, then a suspended F over C that
/// resolves to E and G, landing on the same open C as "scan done".
fn clean_done() -> Vec<f32> {
    let len = 1.2;
    let mut b = buf(len);
    let mut air = noise(len, 23);
    bandpass_sweep(
        &mut air,
        |t| {
            if t < 0.28 {
                700.0 * (3200.0f32 / 700.0).powf(smooth(t / 0.28))
            } else {
                3200.0 * (1800.0f32 / 3200.0).powf(smooth(((t - 0.28) / 0.3).min(1.0)))
            }
        },
        0.9,
    );
    lowpass(&mut air, 5000.0);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let e = if t < 0.22 {
            (t / 0.22 * PI / 2.0).sin().powi(2)
        } else {
            (-(t - 0.22) / 0.12).exp()
        };
        *s += 0.42 * e * air[i];
    }
    // Suspension...
    bell_soft(&mut b, 0.22, 523.25, 0.45, 0.30);
    bell_soft(&mut b, 0.24, 698.46, 0.40, 0.16);
    // ...and resolution.
    bell_soft(&mut b, 0.36, 659.26, 0.55, 0.50);
    bell_soft(&mut b, 0.41, 783.99, 0.48, 0.50);
    bell_soft(&mut b, 0.48, 1046.5, 0.24, 0.45);
    sine_note(&mut b, 0.36, 261.63, 0.12, 0.02, 0.45);
    lowpass(&mut b, 9000.0);
    finish(b, 0.28, -7.0, 0.32)
}

/// Move to Trash: a short dry swoosh, like a sheet slid off a desk: band-passed noise falling
/// from 4.2 kHz to 1.4 kHz with a slight paper grain, and a soft low tuck as it lands.
fn trash() -> Vec<f32> {
    let len = 0.38;
    let mut b = noise(len, 37);
    bandpass_sweep(
        &mut b,
        |t| 4200.0 * (1400.0f32 / 4200.0).powf(smooth((t / 0.24).min(1.0))),
        1.5,
    );
    // Two more poles above the band: paper, not hiss.
    lowpass(&mut b, 3800.0);
    lowpass(&mut b, 5000.0);
    // Grain: a slowly wandering amplitude (sample and hold every 4 ms, smoothed).
    let mut rng = Rng(91);
    let hold = (0.004 * SR) as usize;
    let mut target = 1.0;
    let mut g = 1.0f32;
    for (i, s) in b.iter_mut().enumerate() {
        if i % hold == 0 {
            target = 1.0 - 0.45 * (rng.next() * 0.5 + 0.5);
        }
        g += (target - g) * 0.02;
        let t = i as f32 / SR;
        let e = if t < 0.05 {
            (t / 0.05 * PI / 2.0).sin().powi(2)
        } else if t < 0.15 {
            1.0 - 0.4 * (t - 0.05) / 0.1
        } else {
            0.6 * (-(t - 0.15) / 0.045).exp()
        };
        *s *= g * e;
    }
    sine_note(&mut b, 0.13, 170.0, 0.16, 0.008, 0.05);
    sine_note(&mut b, 0.13, 340.0, 0.05, 0.008, 0.04);
    finish(b, 0.10, -11.0, 0.35)
}

/// Error: two soft, low, rounded tones stepping down a minor third (E flat 4, C4). Noticeable
/// on a laptop speaker, never alarming.
fn error() -> Vec<f32> {
    let mut b = buf(0.6);
    for (at, f) in [(0.0, 311.13), (0.15, 261.63)] {
        let partials = [(1.0, 1.0), (2.0, 0.30), (3.0, 0.12), (4.0, 0.04)];
        let start = (at * SR) as usize;
        let mut ph = [0.0f32; 4];
        for (n, out) in b[start..].iter_mut().enumerate() {
            let t = n as f32 / SR;
            let e = attack(t, 0.008) * (-t / 0.11).exp();
            let mut v = 0.0;
            for (k, (r, a)) in partials.iter().enumerate() {
                ph[k] = (ph[k] + TAU * f * r / SR) % TAU;
                // Higher partials fade faster: the tone darkens as it decays.
                v += a * ph[k].sin() * (-t * 6.0 * (r - 1.0)).exp();
            }
            *out += 0.6 * e * v;
        }
    }
    lowpass(&mut b, 4000.0);
    finish(b, 0.15, -9.0, 0.35)
}

/// Tap: a tiny damped wooden tick, mostly felt. Off unless the user turns it on.
fn tap() -> Vec<f32> {
    let mut b = buf(0.06);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let a = attack(t, 0.0015);
        *s = a
            * ((TAU * 2400.0 * t).sin() * (-t / 0.006).exp()
                + 0.5 * (TAU * 1250.0 * t).sin() * (-t / 0.009).exp());
    }
    finish(b, 0.0, -22.0, 0.30)
}

// ---------------------------------------------------------------------------------------------
// Voices

/// A soft mallet bell: slightly stretched partials, higher ones dying faster, 4 ms attack,
/// two oscillators 1.5 cents apart for a little warmth.
fn bell(b: &mut [f32], at: f32, f: f32, amp: f32, tau: f32) {
    let partials = [
        (1.0, 1.0, 1.0),
        (2.0, 0.32, 0.55),
        (3.01, 0.10, 0.35),
        (4.07, 0.05, 0.25),
        (5.2, 0.025, 0.18),
    ];
    voice(b, at, f, amp, tau, 0.004, &partials);
}

/// A rounder bell for the resolve: fewer, quieter overtones and a softer 7 ms attack.
fn bell_soft(b: &mut [f32], at: f32, f: f32, amp: f32, tau: f32) {
    let partials = [(1.0, 1.0, 1.0), (2.0, 0.22, 0.5), (3.0, 0.06, 0.3)];
    voice(b, at, f, amp, tau, 0.007, &partials);
}

fn voice(
    b: &mut [f32],
    at: f32,
    f: f32,
    amp: f32,
    tau: f32,
    att: f32,
    partials: &[(f32, f32, f32)],
) {
    let start = (at * SR) as usize;
    let detune = 2f32.powf(1.5 / 1200.0);
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        let a = attack(t, att);
        let mut v = 0.0;
        for (r, pa, d) in partials {
            let e = (-t / (tau * d)).exp();
            if e < 1e-4 {
                continue;
            }
            let w = TAU * f * r * t;
            v += pa * e * 0.5 * (w.sin() + (w * detune).sin());
        }
        *out += amp * a * v;
    }
}

fn sine_note(b: &mut [f32], at: f32, f: f32, amp: f32, att: f32, tau: f32) {
    let start = (at * SR) as usize;
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        *out += amp * attack(t, att) * (-t / tau).exp() * (TAU * f * t).sin();
    }
}

// ---------------------------------------------------------------------------------------------
// Building blocks

fn buf(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * SR) as usize]
}

/// Raised-cosine attack over `secs`.
fn attack(t: f32, secs: f32) -> f32 {
    if t >= secs {
        1.0
    } else {
        0.5 - 0.5 * (PI * t / secs).cos()
    }
}

/// Smoothstep on 0..1.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A small deterministic noise source (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

fn noise(secs: f32, seed: u64) -> Vec<f32> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    (0..(secs * SR) as usize).map(|_| r.next()).collect()
}

/// RBJ band-pass (constant 0 dB peak) whose centre follows `centre(t)`, updated every 16
/// samples.
fn bandpass_sweep(b: &mut [f32], centre: impl Fn(f32) -> f32, q: f32) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut c = [0.0f32; 5];
    for (i, s) in b.iter_mut().enumerate() {
        if i % 16 == 0 {
            let w = TAU * centre(i as f32 / SR) / SR;
            let alpha = w.sin() / (2.0 * q);
            let a0 = 1.0 + alpha;
            c = [
                alpha / a0,
                0.0,
                -alpha / a0,
                -2.0 * w.cos() / a0,
                (1.0 - alpha) / a0,
            ];
        }
        let x = *s;
        let y = c[0] * x + c[1] * x1 + c[2] * x2 - c[3] * y1 - c[4] * y2;
        x2 = x1;
        x1 = x;
        y2 = y1;
        y1 = y;
        *s = y;
    }
}

/// One-pole low-pass.
fn lowpass(b: &mut [f32], hz: f32) {
    let k = 1.0 - (-TAU * hz / SR).exp();
    let mut y = 0.0;
    for s in b.iter_mut() {
        y += k * (*s - y);
        *s = y;
    }
}

/// One-pole high-pass (removes DC and rumble).
fn highpass(b: &mut [f32], hz: f32) {
    let k = (-TAU * hz / SR).exp();
    let (mut x1, mut y1) = (0.0f32, 0.0f32);
    for s in b.iter_mut() {
        let y = k * (y1 + *s - x1);
        x1 = *s;
        y1 = y;
        *s = y;
    }
}

/// A small mono Freeverb-style room: eight damped combs into four all-passes.
fn reverb(dry: &[f32], wet: f32) -> Vec<f32> {
    if wet <= 0.0 {
        return dry.to_vec();
    }
    let scale = SR / 44_100.0;
    let combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    let alls = [556, 441, 341, 225];
    let (feedback, damp) = (0.72f32, 0.38f32);
    let mut comb_bufs: Vec<Vec<f32>> = combs
        .iter()
        .map(|&n| vec![0.0; (n as f32 * scale) as usize])
        .collect();
    let mut comb_idx = [0usize; 8];
    let mut comb_lp = [0.0f32; 8];
    let mut all_bufs: Vec<Vec<f32>> = alls
        .iter()
        .map(|&n| vec![0.0; (n as f32 * scale) as usize])
        .collect();
    let mut all_idx = [0usize; 4];
    // 10 ms pre-delay keeps the attack clean.
    let pre = (0.010 * SR) as usize;
    let mut out = Vec::with_capacity(dry.len());
    for (i, &d) in dry.iter().enumerate() {
        let input = if i >= pre { dry[i - pre] * 0.015 } else { 0.0 };
        let mut acc = 0.0;
        for ((bufk, idx), lp) in comb_bufs
            .iter_mut()
            .zip(comb_idx.iter_mut())
            .zip(comb_lp.iter_mut())
        {
            let y = bufk[*idx];
            *lp = y * (1.0 - damp) + *lp * damp;
            bufk[*idx] = input + *lp * feedback;
            *idx = (*idx + 1) % bufk.len();
            acc += y;
        }
        for (bufk, idx) in all_bufs.iter_mut().zip(all_idx.iter_mut()) {
            let bv = bufk[*idx];
            let y = -acc + bv;
            bufk[*idx] = acc + bv * 0.5;
            *idx = (*idx + 1) % bufk.len();
            acc = y;
        }
        out.push(d + wet * acc * 3.0);
    }
    out
}

/// Room, rumble filter, a release window over the last `release` fraction (so tails fade
/// instead of stopping), a 3 ms fade in, and the peak set to `peak_db` dBFS.
fn finish(b: Vec<f32>, wet: f32, peak_db: f32, release: f32) -> Vec<f32> {
    let mut b = reverb(&b, wet);
    highpass(&mut b, 35.0);
    let n = b.len();
    let fade_in = (0.003 * SR) as usize;
    let fade_out = ((n as f32 * release) as usize).max(1);
    for (i, s) in b.iter_mut().enumerate() {
        if i < fade_in {
            *s *= attack(i as f32 / SR, 0.003);
        }
        let from_end = n - 1 - i;
        if from_end < fade_out {
            let x = from_end as f32 / fade_out as f32;
            *s *= (x * PI / 2.0).sin().powi(2);
        }
    }
    let peak = b.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9);
    let gain = 10f32.powf(peak_db / 20.0) / peak;
    b.iter_mut().for_each(|s| *s *= gain);
    b
}

/// 16-bit 48 kHz mono PCM WAV, with TPDF dither.
fn wav(samples: &[f32]) -> Vec<u8> {
    let mut rng = Rng(0x5EED);
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&(SR as u32).to_le_bytes());
    out.extend_from_slice(&(SR as u32 * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    let last = samples.len().saturating_sub(1);
    for (i, s) in samples.iter().enumerate() {
        // No dither on the very first and last sample: they stay exactly silent.
        let d = if i == 0 || i == last {
            0.0
        } else {
            (rng.next() + rng.next()) * 0.5
        };
        let v = (s * 32767.0 + d).round().clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
