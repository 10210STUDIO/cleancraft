//! Every animation CleanCraft runs, with its duration and easing, in one place.
//!
//! - [`Easing`]: the timing functions (cubic Bezier solved like browsers do, closed-form
//!   springs, a few polynomial curves).
//! - [`Motion`]: one transition (duration, easing, delay). The named ones live in [`tr`].
//! - [`Loop`]: a repeating animation (period, easing).
//! - The flat `f32` durations (`PAGE`, `HOVER`, ...) and the helpers `ease_out`, `ease_in_out`,
//!   `progress`, `looped` keep the names `theme::motion` had, so the theme can re-export this
//!   module unchanged.
//! - [`animate`], [`animate_bool`]: drive a transition from any widget; the state lives in
//!   egui's temp memory keyed by the caller's `Id` and a repaint is requested while moving.
//!
//! With reduced motion ([`reduced`], from macOS "Reduce motion" via
//! [`detect_reduce_motion`]) every transition is instant and every loop stands still.

use egui::{Context, Id};
use std::sync::atomic::{AtomicBool, Ordering};

/// Compile-time switch kept for older call sites; use [`reduced`], which also follows the
/// macOS setting.
pub const REDUCE_MOTION: bool = false;

static REDUCED: AtomicBool = AtomicBool::new(false);

/// True when movement is off: transitions jump to their end, loops hold their first frame.
pub fn reduced() -> bool {
    REDUCE_MOTION || REDUCED.load(Ordering::Relaxed)
}

/// Turn movement off or on for the whole app.
pub fn set_reduced(on: bool) {
    REDUCED.store(on, Ordering::Relaxed);
}

/// `defaults read com.apple.universalaccess reduceMotion` prints `1` when the user turned on
/// System Settings > Accessibility > Display > Reduce motion.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    matches!(stdout.trim(), "1" | "true" | "YES")
}

/// Ask macOS (through `env.sys`, so tests answer it) whether to reduce motion, and apply it.
/// Returns the setting. A missing key (never changed) means motion stays on.
pub fn detect_reduce_motion(sys: &dyn cleancraft_core::sys::Sys) -> bool {
    let on = sys
        .run(
            "/usr/bin/defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .map(|o| o.ok() && parse_reduce_motion(&o.stdout))
        .unwrap_or(false);
    set_reduced(on);
    on
}

// ---- durations (seconds), the names the theme always had ----

/// Page change: fade and short slide.
pub const PAGE: f32 = 0.18;
/// Distance of the page slide (points).
pub const PAGE_SLIDE: f32 = 10.0;
/// Hover highlight in and out.
pub const HOVER: f32 = 0.12;
/// Button press scale.
pub const PRESS: f32 = 0.08;
pub const PRESS_SCALE: f32 = 0.965;
/// One breath of the primary action's halo.
pub const PULSE: f32 = 2.6;
/// One turn of the progress ring's sweep.
pub const SPIN: f32 = 1.4;
/// The check mark drawn on the Done screen.
pub const CHECK: f32 = 0.6;
/// Slow drift of the illustration sparkles.
pub const DRIFT: f32 = 9.0;

/// A timing function: maps time fraction 0..1 to progress (0..1, springs may overshoot).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    /// CSS `cubic-bezier(x1, y1, x2, y2)`.
    CubicBezier(f32, f32, f32, f32),
    /// `1 - (1 - t)^3`.
    CubicOut,
    /// Half a cosine wave, for loops that breathe.
    SineInOut,
    /// A damped spring from 0 to 1; the time fraction spans its settle time.
    Spring(Spring),
}

/// Our standard curve: quick start, long soft landing.
pub const GLIDE: Easing = Easing::CubicBezier(0.2, 0.0, 0.0, 1.0);
/// Things appearing (decelerate only).
pub const ENTER: Easing = Easing::CubicBezier(0.0, 0.0, 0.2, 1.0);
/// Things leaving (accelerate only).
pub const EXIT: Easing = Easing::CubicBezier(0.4, 0.0, 1.0, 1.0);
/// Symmetric, for state swaps.
pub const SWAP: Easing = Easing::CubicBezier(0.45, 0.0, 0.55, 1.0);
/// A playful overshoot (back-out).
pub const POP: Easing = Easing::CubicBezier(0.34, 1.56, 0.64, 1.0);

impl Easing {
    /// Progress at time fraction `t` (clamped to 0..1).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::CubicOut => 1.0 - (1.0 - t).powi(3),
            Easing::SineInOut => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
            Easing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, t),
            Easing::Spring(s) => {
                if t >= 1.0 {
                    1.0
                } else {
                    s.value(t * s.settle_time())
                }
            }
        }
    }
}

/// Solve `cubic-bezier()` for `x = t` and return `y` (Newton steps, then bisection, the method
/// browsers use), so a curve sampled here matches the same curve in a design tool.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (x1, y1, x2, y2) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let slope_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let x = x as f64;
    let mut t = x;
    let mut solved = false;
    for _ in 0..8 {
        let err = sample_x(t) - x;
        if err.abs() < 1e-7 {
            solved = true;
            break;
        }
        let d = slope_x(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    if !solved {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        t = x;
        for _ in 0..60 {
            let v = sample_x(t);
            if (v - x).abs() < 1e-7 {
                break;
            }
            if x > v {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) / 2.0;
        }
    }
    sample_y(t) as f32
}

/// A damped harmonic spring (mass 1) released at 0 with no velocity, pulled to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub const fn new(stiffness: f32, damping: f32) -> Spring {
        Spring { stiffness, damping }
    }

    /// Damping ratio (1 = critical, below 1 bounces).
    pub fn ratio(&self) -> f32 {
        self.damping / (2.0 * self.stiffness.max(1e-6).sqrt())
    }

    /// Position at `t` seconds (closed form for under, critical and over damping).
    pub fn value(&self, t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        let w0 = self.stiffness.max(1e-6).sqrt();
        let z = self.ratio();
        if (z - 1.0).abs() < 1e-4 {
            1.0 - (-w0 * t).exp() * (1.0 + w0 * t)
        } else if z < 1.0 {
            let wd = w0 * (1.0 - z * z).sqrt();
            1.0 - (-z * w0 * t).exp() * ((wd * t).cos() + (z * w0 / wd) * (wd * t).sin())
        } else {
            let s = (z * z - 1.0).sqrt();
            let r1 = -w0 * (z - s);
            let r2 = -w0 * (z + s);
            // x = 1 + A e^{r1 t} + B e^{r2 t}, x(0) = 0, x'(0) = 0.
            let a = r2 / (r1 - r2);
            let b = -r1 / (r1 - r2);
            1.0 + a * (r1 * t).exp() + b * (r2 * t).exp()
        }
    }

    /// Time after which the spring stays within 0.1 % of its target.
    pub fn settle_time(&self) -> f32 {
        let step = 0.002;
        let mut last_out = 0.0;
        let mut t = 0.0;
        while t < 10.0 {
            if (self.value(t) - 1.0).abs() > 1e-3 {
                last_out = t;
            }
            t += step;
        }
        (last_out + step).min(10.0)
    }
}

/// One transition: duration in seconds plus its timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub duration: f32,
    pub easing: Easing,
    /// Delay before it starts, in seconds.
    pub delay: f32,
}

impl Motion {
    pub const fn new(duration: f32, easing: Easing) -> Motion {
        Motion {
            duration,
            easing,
            delay: 0.0,
        }
    }

    /// The same transition starting `delay` seconds later (for staggering).
    pub const fn delayed(self, delay: f32) -> Motion {
        Motion { delay, ..self }
    }

    /// A transition that lasts exactly as long as `spring` takes to settle.
    pub fn spring(spring: Spring) -> Motion {
        Motion::new(spring.settle_time(), Easing::Spring(spring))
    }

    /// Progress at `elapsed` seconds since the change (delay included).
    pub fn progress(&self, elapsed: f32) -> f32 {
        if reduced() || self.duration <= 0.0 {
            return 1.0;
        }
        self.easing
            .apply(((elapsed - self.delay) / self.duration).clamp(0.0, 1.0))
    }

    /// Total time until the end, delay included.
    pub fn total(&self) -> f32 {
        self.delay + self.duration
    }
}

/// The named transitions.
pub mod tr {
    use super::*;
    pub const PAGE: Motion = Motion::new(super::PAGE, Easing::CubicOut);
    pub const HOVER: Motion = Motion::new(super::HOVER, GLIDE);
    pub const PRESS: Motion = Motion::new(super::PRESS, GLIDE);
    pub const CHECK: Motion = Motion::new(super::CHECK, Easing::CubicOut);
    /// Number tweens (totals counting up).
    pub const COUNT: Motion = Motion::new(0.6, GLIDE);
    /// Cards and rows appearing; stagger each by [`STAGGER`].
    pub const APPEAR: Motion = Motion::new(0.32, ENTER);
    pub const STAGGER: f32 = 0.035;
    /// Overlays and dialogs fading in.
    pub const OVERLAY: Motion = Motion::new(0.2, ENTER);
    /// Hero switching between idle, scanning and done (glow and speed blend).
    pub const HERO_PHASE: Motion = Motion::new(0.7, SWAP);
    /// Hero settling into its done pose.
    pub const HERO_SETTLE: Spring = Spring::new(120.0, 13.0);
    /// The burst of particles when a scan finishes.
    pub const HERO_BURST: Motion = Motion::new(1.1, ENTER);
    /// Hero fading in the first time it shows.
    pub const HERO_ENTER: Motion = Motion::new(0.55, POP);
}

/// A repeating animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub period: f32,
    pub easing: Easing,
}

impl Loop {
    pub const fn new(period: f32, easing: Easing) -> Loop {
        Loop { period, easing }
    }

    /// Raw phase 0..1 at time `t` seconds (constant with reduced motion).
    pub fn phase(&self, t: f64) -> f32 {
        if reduced() || self.period <= 0.0 {
            return 0.0;
        }
        (t / self.period as f64).rem_euclid(1.0) as f32
    }

    /// Eased 0..1..0 wave (up for half the period, down for the other half).
    pub fn wave(&self, t: f64) -> f32 {
        let p = self.phase(t);
        self.easing.apply(1.0 - (2.0 * p - 1.0).abs())
    }

    /// Angle in radians, one turn per period.
    pub fn angle(&self, t: f64) -> f32 {
        if reduced() || self.period <= 0.0 {
            return 0.0;
        }
        ((t / self.period as f64) * std::f64::consts::TAU) as f32
    }
}

/// The named loops.
pub mod lp {
    use super::*;
    pub const PULSE: Loop = Loop::new(super::PULSE, Easing::CubicOut);
    pub const SPIN: Loop = Loop::new(super::SPIN, Easing::Linear);
    pub const DRIFT: Loop = Loop::new(super::DRIFT, Easing::SineInOut);
    /// Hero turning slowly while idle (seconds per turn).
    pub const HERO_IDLE_TURN: Loop = Loop::new(16.0, Easing::Linear);
    /// Hero turning while a scan runs.
    pub const HERO_SCAN_TURN: Loop = Loop::new(4.5, Easing::Linear);
    /// Hero floating up and down.
    pub const HERO_FLOAT: Loop = Loop::new(4.4, Easing::SineInOut);
    /// Particles orbiting a scanning hero.
    pub const HERO_ORBIT: Loop = Loop::new(2.8, Easing::Linear);
    /// A glint crossing the hero.
    pub const HERO_GLINT: Loop = Loop::new(5.2, Easing::SineInOut);
    /// Glow breathing while scanning.
    pub const HERO_BREATH: Loop = Loop::new(1.6, Easing::SineInOut);
}

// ---- the helpers the theme had ----

/// Ease-out cubic: fast start, gentle landing.
pub fn ease_out(x: f32) -> f32 {
    Easing::CubicOut.apply(x)
}

/// Ease-in-out sine, for loops.
pub fn ease_in_out(x: f32) -> f32 {
    Easing::SineInOut.apply(x)
}

/// Linear progress (0..=1) of an animation of `duration` that started at `start` (seconds).
pub fn progress(now: f64, start: f64, duration: f32) -> f32 {
    if reduced() || duration <= 0.0 {
        return 1.0;
    }
    (((now - start) as f32) / duration).clamp(0.0, 1.0)
}

/// Phase (0..1) of a loop of `period` seconds; constant with reduced motion.
pub fn looped(now: f64, period: f32) -> f32 {
    Loop::new(period, Easing::Linear).phase(now)
}

/// `a + (b - a) * t`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ---- driving transitions in egui ----

#[derive(Clone, Copy, Debug)]
struct Track {
    from: f32,
    to: f32,
    start: f64,
}

fn sample(m: &Motion, tr: &Track, now: f64) -> f32 {
    tr.from + (tr.to - tr.from) * m.progress((now - tr.start) as f32)
}

/// Animate towards `target`: when the target changes mid-flight the new transition starts
/// from the value on screen. Returns the value to draw this frame.
pub fn animate(ctx: &Context, id: Id, target: f32, m: Motion) -> f32 {
    let now = ctx.input(|i| i.time);
    let key = id.with("cleancraft-motion");
    let Some(mut track) = ctx.data(|d| d.get_temp::<Track>(key)) else {
        ctx.data_mut(|d| {
            d.insert_temp(
                key,
                Track {
                    from: target,
                    to: target,
                    start: now,
                },
            )
        });
        return target;
    };
    if track.to != target {
        track = Track {
            from: sample(&m, &track, now),
            to: target,
            start: now,
        };
        ctx.data_mut(|d| d.insert_temp(key, track));
    }
    if !reduced() && now - track.start < m.total() as f64 {
        ctx.request_repaint();
    }
    sample(&m, &track, now)
}

/// 0..1 for a boolean state (hover, selected, open).
pub fn animate_bool(ctx: &Context, id: Id, on: bool, m: Motion) -> f32 {
    animate(ctx, id, if on { 1.0 } else { 0.0 }, m)
}

#[derive(Clone, Copy, Debug)]
struct Count {
    from: f64,
    to: f64,
    start: f64,
}

/// A number that counts up (or down) to `value` over `m` when it changes, e.g. a total size
/// arriving with the results. The first value shows at once; the end value is exact.
pub fn count_to(ctx: &Context, id: Id, value: f64, m: Motion) -> f64 {
    let now = ctx.input(|i| i.time);
    let key = id.with("cleancraft-count");
    let mut c = ctx.data(|d| d.get_temp::<Count>(key)).unwrap_or(Count {
        from: value,
        to: value,
        start: now,
    });
    let at = |c: &Count| {
        let k = m.progress((now - c.start) as f32) as f64;
        if k >= 1.0 {
            c.to
        } else {
            c.from + (c.to - c.from) * k
        }
    };
    if c.to != value {
        c = Count {
            from: at(&c),
            to: value,
            start: now,
        };
    }
    ctx.data_mut(|d| d.insert_temp(key, c));
    let v = at(&c);
    if v != c.to {
        ctx.request_repaint();
    }
    v
}

/// Entrance of the `index`-th item of a list that first showed under `id`: 0..1, staggered by
/// [`tr::STAGGER`] per item (capped so long lists finish quickly). Use it for opacity and a
/// few points of rise.
pub fn appear(ctx: &Context, id: Id, index: usize) -> f32 {
    let t = since(ctx, id, 0) as f32;
    let m = tr::APPEAR.delayed(tr::STAGGER * index.min(12) as f32);
    let k = m.progress(t);
    if k < 1.0 {
        ctx.request_repaint();
    }
    k
}

/// Repaint for an ambient loop (idle heroes, halos) at about 30 frames a second, and not at
/// all while the window is in the background or motion is reduced.
pub fn ambient_repaint(ctx: &Context) {
    if reduced() || window_hidden(ctx) {
        return;
    }
    if ctx.input(|i| i.viewport().focused) == Some(false) {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(AMBIENT_FRAME_MS));
}

/// The window is minimised or fully covered by other windows.
pub fn window_hidden(ctx: &Context) -> bool {
    ctx.input(|i| {
        let v = i.viewport();
        v.minimized == Some(true) || v.occluded == Some(true)
    })
}

/// A spinner or progress ring: about 30 fps, nothing while the window is hidden.
pub fn spinner_repaint(ctx: &Context) {
    if window_hidden(ctx) {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(AMBIENT_FRAME_MS));
}

/// Frame interval of ambient loops (about 30 fps).
pub const AMBIENT_FRAME_MS: u64 = 33;

/// Seconds since `key` was first seen with this `state` value (restarts when it changes).
/// Pages use it to know how long a screen has shown, e.g. for the hero's done burst.
pub fn since(ctx: &Context, id: Id, state: u64) -> f64 {
    let now = ctx.input(|i| i.time);
    let key = id.with("cleancraft-since");
    let (s, start) = ctx
        .data(|d| d.get_temp::<(u64, f64)>(key))
        .unwrap_or((state, now));
    let start = if s == state { start } else { now };
    ctx.data_mut(|d| d.insert_temp(key, (state, start)));
    now - start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 2e-3
    }

    #[test]
    fn cubic_bezier_matches_browser_values() {
        // CSS `ease` (.25,.1,.25,1) at x = .25/.5/.75 from WebKit's UnitBezier.
        let ease = Easing::CubicBezier(0.25, 0.1, 0.25, 1.0);
        assert!(close(ease.apply(0.25), 0.4085), "{}", ease.apply(0.25));
        assert!(close(ease.apply(0.5), 0.8024), "{}", ease.apply(0.5));
        assert!(close(ease.apply(0.75), 0.9604), "{}", ease.apply(0.75));
        // ease-in / ease-out.
        assert!(close(
            Easing::CubicBezier(0.42, 0.0, 1.0, 1.0).apply(0.5),
            0.3153
        ));
        assert!(close(
            Easing::CubicBezier(0.0, 0.0, 0.58, 1.0).apply(0.5),
            0.6847
        ));
        // Symmetric curve passes through the middle.
        assert!(close(SWAP.apply(0.5), 0.5));
        for e in [GLIDE, ENTER, EXIT, SWAP, POP] {
            assert_eq!(e.apply(0.0), 0.0);
            assert_eq!(e.apply(1.0), 1.0);
        }
    }

    #[test]
    fn curves_are_monotonic_where_they_should_be() {
        for e in [
            GLIDE,
            ENTER,
            EXIT,
            SWAP,
            Easing::CubicOut,
            Easing::SineInOut,
        ] {
            let mut last = 0.0;
            for i in 0..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!(v + 1e-5 >= last, "{e:?} at {i}");
                last = v;
            }
        }
        // POP overshoots then lands.
        let peak = (0..=100)
            .map(|i| POP.apply(i as f32 / 100.0))
            .fold(0.0f32, f32::max);
        assert!(peak > 1.05, "{peak}");
        // Enter is ahead of exit at the midpoint.
        assert!(ENTER.apply(0.5) > 0.7 && EXIT.apply(0.5) < 0.35);
    }

    #[test]
    fn polynomial_easings() {
        assert!(close(Easing::CubicOut.apply(0.5), 0.875));
        assert!(close(Easing::SineInOut.apply(0.5), 0.5));
        assert!(close(ease_out(0.5), 0.875));
        assert!(close(ease_in_out(0.25), 0.1464));
    }

    #[test]
    fn springs_settle_and_bounce_by_ratio() {
        let bouncy = Spring::new(200.0, 10.0);
        assert!(bouncy.ratio() < 1.0);
        let peak = (0..2000)
            .map(|i| bouncy.value(i as f32 * 0.001))
            .fold(0.0f32, f32::max);
        assert!(peak > 1.2, "under-damped overshoots: {peak}");
        let critical = Spring::new(100.0, 20.0);
        assert!(close(critical.ratio(), 1.0));
        let over = Spring::new(100.0, 40.0);
        for s in [critical, over] {
            let peak = (0..3000)
                .map(|i| s.value(i as f32 * 0.001))
                .fold(0.0f32, f32::max);
            assert!(peak <= 1.0 + 1e-3, "{s:?} does not overshoot: {peak}");
        }
        for s in [bouncy, critical, over, tr::HERO_SETTLE] {
            assert_eq!(s.value(0.0), 0.0);
            let st = s.settle_time();
            assert!(st > 0.05 && st < 5.0, "{s:?} settles in {st}");
            assert!((s.value(st) - 1.0).abs() <= 1e-3);
            assert!((s.value(st * 1.5) - 1.0).abs() <= 1e-3);
            assert_eq!(Easing::Spring(s).apply(1.0), 1.0);
        }
        // Critical damping: 1 - e^{-wt}(1 + wt) at w = 10, t = 0.1 -> 1 - 2/e.
        assert!(close(critical.value(0.1), 1.0 - 2.0 / std::f32::consts::E));
    }

    #[test]
    fn named_timings_are_deliberate() {
        assert_eq!(tr::PAGE.duration, PAGE);
        assert_eq!(tr::HOVER.duration, 0.12);
        assert_eq!(tr::PRESS.duration, 0.08);
        assert_eq!(tr::CHECK.duration, 0.6);
        const {
            assert!(tr::HERO_BURST.duration > 0.8 && tr::HERO_BURST.duration < 1.5);
            assert!(lp::HERO_IDLE_TURN.period > lp::HERO_SCAN_TURN.period);
        }
        assert_eq!(Motion::spring(tr::HERO_SETTLE).progress(10.0), 1.0);
        let staggered = tr::APPEAR.delayed(3.0 * tr::STAGGER);
        assert_eq!(staggered.progress(0.1), 0.0);
        assert!(staggered.progress(0.2) > 0.0);
    }

    #[test]
    fn loops() {
        let l = Loop::new(2.0, Easing::Linear);
        assert!(close(l.phase(0.5), 0.25));
        assert!(close(l.phase(2.5), 0.25));
        assert!(close(l.phase(-0.5), 0.75));
        assert!(close(l.wave(1.0), 1.0));
        assert!(close(l.wave(0.0), 0.0));
        assert!(close(l.angle(1.0), std::f32::consts::PI));
        assert!(close(looped(1.4, SPIN), 0.0));
        assert!(close(progress(1.09, 1.0, PAGE), 0.5));
    }

    fn frame(ctx: &Context, t: f64, mut f: impl FnMut(&Context) -> f32) -> f32 {
        let mut out = 0.0;
        let input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        let mut full = ctx.run_ui(input, |ui| out = f(ui.ctx()));
        full.textures_delta.clear();
        out
    }

    #[test]
    fn animate_restarts_from_current_value() {
        let ctx = Context::default();
        let id = Id::new("t");
        let m = Motion::new(0.2, SWAP);
        assert_eq!(frame(&ctx, 0.0, |c| animate_bool(c, id, false, m)), 0.0);
        assert_eq!(frame(&ctx, 0.01, |c| animate_bool(c, id, true, m)), 0.0);
        let mid = frame(&ctx, 0.11, |c| animate_bool(c, id, true, m));
        assert!(close(mid, SWAP.apply(0.5)), "{mid}");
        // Reverse mid-flight: starts from the value on screen.
        let back = frame(&ctx, 0.11, |c| animate_bool(c, id, false, m));
        assert!(close(back, mid), "{back}");
        assert_eq!(frame(&ctx, 0.5, |c| animate_bool(c, id, false, m)), 0.0);
    }

    #[test]
    fn count_to_lands_exactly() {
        let ctx = Context::default();
        let id = Id::new("c");
        assert_eq!(
            frame(&ctx, 0.0, |c| count_to(c, id, 10.0, tr::COUNT) as f32),
            10.0
        );
        assert_eq!(
            frame(&ctx, 0.1, |c| count_to(c, id, 1234.5, tr::COUNT) as f32),
            10.0
        );
        let mid = frame(&ctx, 0.4, |c| count_to(c, id, 1234.5, tr::COUNT) as f32);
        assert!(mid > 10.0 && mid < 1234.5, "{mid}");
        assert_eq!(
            frame(&ctx, 2.0, |c| count_to(c, id, 1234.5, tr::COUNT) as f32),
            1234.5
        );
    }

    #[test]
    fn appear_staggers_items() {
        let ctx = Context::default();
        let id = Id::new("list");
        frame(&ctx, 0.0, |c| appear(c, id, 0));
        let first = frame(&ctx, 0.1, |c| appear(c, id, 0));
        let later = frame(&ctx, 0.1, |c| appear(c, id, 5));
        assert!(first > later, "{first} {later}");
        assert_eq!(frame(&ctx, 3.0, |c| appear(c, id, 40)), 1.0);
    }

    #[test]
    fn reduce_motion_setting_is_read_from_macos() {
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
        // Ask the fake Mac; the global flag is restored so parallel tests keep their motion.
        let fake = cleancraft_core::sys::FakeSys::new();
        fake.answer(
            "defaults read com.apple.universalaccess reduceMotion",
            "0\n",
        );
        assert!(!detect_reduce_motion(&fake));
        assert!(fake.calls().contains(
            &"/usr/bin/defaults read com.apple.universalaccess reduceMotion".to_string()
        ));
        let none = cleancraft_core::sys::FakeSys::new();
        assert!(!detect_reduce_motion(&none), "no answer: motion stays on");
        set_reduced(false);
    }

    #[test]
    fn since_restarts_on_state_change() {
        let ctx = Context::default();
        let id = Id::new("s");
        assert_eq!(frame(&ctx, 1.0, |c| since(c, id, 1) as f32), 0.0);
        assert!(close(frame(&ctx, 1.5, |c| since(c, id, 1) as f32), 0.5));
        assert_eq!(frame(&ctx, 2.0, |c| since(c, id, 2) as f32), 0.0);
        assert!(close(frame(&ctx, 2.25, |c| since(c, id, 2) as f32), 0.25));
    }
}
