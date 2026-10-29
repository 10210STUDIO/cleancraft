//! Writes CleanCraft's interface sounds to `assets/sounds/*.wav` (16-bit, 44.1 kHz, mono).
//!
//! The "Deep Ocean" set (docs/design/BRAND.md): every sound is synthesized here from scratch
//! with water in mind: pitch-swept sines for drops and bubbles, resonant band-passed noise for
//! swells and waves, soft sine pings for sonar, and a small Freeverb-style room for the tails.
//! Nothing is sampled. The output is deterministic (fixed noise seeds), so running this again
//! gives byte-identical files unless the recipes change.
//!
//! `cargo run -p cleancraft-ui-egui --example make_sounds`

use std::f32::consts::{PI, TAU};
use std::path::Path;

const SR: f32 = 44_100.0;

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

/// A scan starts: a soft underwater swell. Noise through a resonant band that rises from
/// 220 Hz to 900 Hz as the swell breathes in, a slow wave-like sway in its level, and a deep
/// sine glide (A2 to E3) under it. Low-passed, so it stays gentle.
fn scan_start() -> Vec<f32> {
    let len = 0.8;
    let mut b = buf(len);
    let swell = |t: f32| {
        if t < 0.45 {
            (t / 0.45 * PI / 2.0).sin().powi(2)
        } else {
            (-(t - 0.45) / 0.14).exp()
        }
    };
    let mut water = noise(len, 11);
    bandpass_sweep(
        &mut water,
        |t| 220.0 * (900.0f32 / 220.0).powf(smooth(t / 0.5)),
        3.0,
    );
    lowpass(&mut water, 1800.0);
    lowpass(&mut water, 2400.0);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // The sway: a slow wave in the level, 3 Hz.
        let sway = 0.82 + 0.18 * (TAU * 3.0 * t).sin();
        *s += 1.0 * swell(t) * sway * water[i];
    }
    glide(&mut b, 0.0, 110.0, 164.81, 0.5, 0.20, 0.05, swell);
    finish(b, 0.30, -8.0, 0.30)
}

/// A scan finished: two soft sonar pings a fifth apart (A5, then E6), each a pure sine that
/// sinks a few cents as it rings, with a long, soft room tail.
fn scan_done() -> Vec<f32> {
    let mut b = buf(1.15);
    ping(&mut b, 0.0, 880.0, 0.75, 0.30);
    ping(&mut b, 0.17, 1318.51, 0.55, 0.34);
    // A barely-there low A for body on small speakers.
    sine_note(&mut b, 0.0, 220.0, 0.08, 0.02, 0.30);
    lowpass(&mut b, 6000.0);
    finish(b, 0.42, -6.0, 0.35)
}

/// A cleanup finished: a wave washes in and draws back (noise through a band that sweeps up
/// to 2.4 kHz and settles), then one bright, clean water drop "plink" with a soft note under
/// it as the wave goes out.
fn clean_done() -> Vec<f32> {
    let len = 1.2;
    let mut b = buf(len);
    let mut wave = noise(len, 23);
    bandpass_sweep(
        &mut wave,
        |t| {
            if t < 0.32 {
                380.0 * (2400.0f32 / 380.0).powf(smooth(t / 0.32))
            } else {
                2400.0 * (700.0f32 / 2400.0).powf(smooth(((t - 0.32) / 0.4).min(1.0)))
            }
        },
        1.6,
    );
    lowpass(&mut wave, 4000.0);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let e = if t < 0.3 {
            (t / 0.3 * PI / 2.0).sin().powi(2)
        } else {
            (-(t - 0.3) / 0.16).exp()
        };
        // Foam: a fast shimmer on the crest.
        let foam = 0.85 + 0.15 * (TAU * 11.0 * t).sin();
        *s += 0.70 * e * foam * wave[i];
    }
    // The drop: a quick upward chirp that rings briefly.
    drop_plink(&mut b, 0.50, 1250.0, 2600.0, 0.55, 0.075);
    // A soft note under it (E6 into a low E5), the "clean" landing.
    ping(&mut b, 0.52, 1318.51, 0.20, 0.32);
    ping(&mut b, 0.52, 659.26, 0.16, 0.36);
    lowpass(&mut b, 8000.0);
    finish(b, 0.34, -6.0, 0.30)
}

/// Items move to the Trash: a short bubble gulp. Two sine bubbles whose pitch rises as they
/// form (190 to 540 Hz, then a smaller one 320 to 860 Hz), with a little low water noise.
fn trash() -> Vec<f32> {
    let len = 0.36;
    let mut b = buf(len);
    bubble(&mut b, 0.0, 190.0, 540.0, 0.13, 0.80);
    bubble(&mut b, 0.075, 320.0, 860.0, 0.08, 0.40);
    let mut n = noise(len, 37);
    bandpass_sweep(&mut n, |t| 500.0 + 900.0 * smooth(t / 0.15), 2.0);
    for (i, s) in b.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s += 0.22 * attack(t, 0.006) * (-t / 0.05).exp() * n[i];
    }
    lowpass(&mut b, 5000.0);
    finish(b, 0.14, -8.0, 0.35)
}

/// Something went wrong: a low, muted double tone (G3, then E3), round sines with a hint of
/// a second partial behind a low-pass, as if heard through water. Calm, never a buzzer.
fn error() -> Vec<f32> {
    let mut b = buf(0.62);
    for (at, f) in [(0.0, 196.0), (0.17, 164.81)] {
        let start = (at * SR) as usize;
        let mut ph = [0.0f32; 2];
        for (n, out) in b[start..].iter_mut().enumerate() {
            let t = n as f32 / SR;
            let e = attack(t, 0.012) * (-t / 0.13).exp();
            let mut v = 0.0;
            for (k, (r, a)) in [(1.0f32, 1.0f32), (2.0, 0.18)].iter().enumerate() {
                ph[k] = (ph[k] + TAU * f * r / SR) % TAU;
                v += a * ph[k].sin();
            }
            *out += 0.7 * e * v;
        }
    }
    lowpass(&mut b, 1200.0);
    lowpass(&mut b, 2000.0);
    finish(b, 0.16, -8.0, 0.35)
}

/// Tap: one tiny water-drop tick, a 6 ms chirp from 1.9 to 3.4 kHz that dies at once. Mostly
/// felt. Off unless the user turns it on.
fn tap() -> Vec<f32> {
    let mut b = buf(0.06);
    drop_plink(&mut b, 0.0, 1900.0, 3400.0, 1.0, 0.009);
    finish(b, 0.0, -18.0, 0.30)
}

// ---------------------------------------------------------------------------------------------
// Voices

/// A sonar ping: a sine with a 3 ms attack, a little of its octave, sinking 6 cents over its
/// ring, decaying with time constant `tau`.
fn ping(b: &mut [f32], at: f32, f: f32, amp: f32, tau: f32) {
    let start = (at * SR) as usize;
    let mut ph = [0.0f32; 2];
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        let e = attack(t, 0.003) * (-t / tau).exp();
        if e < 1e-5 && t > 0.01 {
            break;
        }
        let fr = f * 2f32.powf(-6.0 / 1200.0 * (1.0 - (-t / tau).exp()));
        ph[0] = (ph[0] + TAU * fr / SR) % TAU;
        ph[1] = (ph[1] + TAU * fr * 2.0 / SR) % TAU;
        *out += amp * e * (ph[0].sin() + 0.08 * (-t / (tau * 0.3)).exp() * ph[1].sin());
    }
}

/// A water drop: a sine whose pitch rises fast from `f0` to `f1` (the "plink"), decaying with
/// time constant `tau`.
fn drop_plink(b: &mut [f32], at: f32, f0: f32, f1: f32, amp: f32, tau: f32) {
    let start = (at * SR) as usize;
    let mut ph = 0.0f32;
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        let e = attack(t, 0.0012) * (-t / tau).exp();
        if e < 1e-5 && t > 0.01 {
            break;
        }
        let fr = f0 * (f1 / f0).powf(1.0 - (-t / (tau * 0.5)).exp());
        ph = (ph + TAU * fr / SR) % TAU;
        *out += amp * e * ph.sin();
    }
}

/// A bubble: a sine rising from `f0` to `f1` over `dur` as it forms, swelling in and fading
/// out with it.
fn bubble(b: &mut [f32], at: f32, f0: f32, f1: f32, dur: f32, amp: f32) {
    let start = (at * SR) as usize;
    let mut ph = 0.0f32;
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        if t > dur * 1.6 {
            break;
        }
        let x = (t / dur).min(1.0);
        let fr = f0 * (f1 / f0).powf(x * x);
        let e = attack(t, 0.006) * (-(t / dur).powi(2) * 2.2).exp();
        ph = (ph + TAU * fr / SR) % TAU;
        *out += amp * e * ph.sin();
    }
}

/// A sine gliding from `f0` to `f1` over `secs`, shaped by `env(t)`.
#[allow(clippy::too_many_arguments)]
fn glide(
    b: &mut [f32],
    at: f32,
    f0: f32,
    f1: f32,
    secs: f32,
    amp: f32,
    att: f32,
    env: impl Fn(f32) -> f32,
) {
    let start = (at * SR) as usize;
    let mut ph = 0.0f32;
    for (n, out) in b[start..].iter_mut().enumerate() {
        let t = n as f32 / SR;
        let fr = f0 * (f1 / f0).powf(smooth(t / secs));
        ph = (ph + TAU * fr / SR) % TAU;
        *out += amp * attack(t, att) * env(t) * ph.sin();
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

/// 16-bit 44.1 kHz mono PCM WAV, with TPDF dither.
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
