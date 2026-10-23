//! How far a running job is: the share of its work done, shared between the thread doing the
//! work and the window drawing the progress bar.
//!
//! The rules the progress bar relies on:
//! - The share is honest: it comes from real units (modules done of the modules selected, items
//!   moved of the items in the plan, folders finished of the folders found). Until the work
//!   knows its total, the share is unknown ([`Progress::fraction`] is `None`) and the screen
//!   shows a moving stripe with the current step instead of a number.
//! - It only grows. A report lower than what was shown before is ignored.
//! - It reaches 100% only when the job ends ([`Progress::finish`]); until then it stops at 99%.
//!
//! A job made of parts gives each part a [`Progress::span`]: a reporter that maps its own
//! 0..1 into its slice of the parent's bar, so a module reports its folders and the whole
//! sweep moves smoothly.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Parts per million: the unit the share is stored in.
const FULL: u32 = 1_000_000;
/// The most a share can show before the job ends.
const ALMOST: u32 = FULL - 1;

struct Inner {
    done: AtomicU32,
    known: AtomicBool,
    finished: AtomicBool,
    step: Mutex<String>,
    started: Instant,
    eta: Mutex<Eta>,
}

#[derive(Default)]
struct Eta {
    /// When the last estimate was taken.
    at: Option<Instant>,
    /// Seconds left at that time.
    last: Option<f64>,
    /// Estimates in a row that agreed with the one before.
    agreeing: u32,
}

/// A thread-safe progress reporter. Clones share the same bar; [`Progress::span`] gives a
/// reporter for a slice of it.
#[derive(Clone)]
pub struct Progress {
    inner: Arc<Inner>,
    lo: f64,
    hi: f64,
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Progress")
            .field("fraction", &self.fraction())
            .field("step", &self.step())
            .finish()
    }
}

impl Progress {
    /// A new bar: nothing done, total unknown.
    pub fn new() -> Progress {
        Progress {
            inner: Arc::new(Inner {
                done: AtomicU32::new(0),
                known: AtomicBool::new(false),
                finished: AtomicBool::new(false),
                step: Mutex::new(String::new()),
                started: Instant::now(),
                eta: Mutex::new(Eta::default()),
            }),
            lo: 0.0,
            hi: 1.0,
        }
    }

    /// The reporter for part `index` of `count` equal parts of this one's slice.
    pub fn span(&self, index: usize, count: usize) -> Progress {
        let count = count.max(1) as f64;
        let i = (index as f64).min(count);
        self.range(i / count, ((i + 1.0) / count).min(1.0))
    }

    /// The reporter for the slice `from..to` (shares of this one's slice, 0..=1).
    pub fn range(&self, from: f64, to: f64) -> Progress {
        let w = self.hi - self.lo;
        let from = from.clamp(0.0, 1.0);
        let to = to.clamp(from, 1.0);
        Progress {
            inner: self.inner.clone(),
            lo: self.lo + w * from,
            hi: self.lo + w * to,
        }
    }

    /// `done` of `total` units of this slice are finished. Ignored while `total` is 0; the
    /// bar never goes back.
    pub fn set(&self, done: u64, total: u64) {
        if total == 0 {
            return;
        }
        self.set_fraction(done.min(total) as f64 / total as f64);
    }

    /// A share of this slice is finished (0..=1). The bar never goes back.
    pub fn set_fraction(&self, f: f64) {
        let f = if f.is_finite() {
            f.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let v = self.lo + (self.hi - self.lo) * f;
        let ppm = ((v * FULL as f64).floor() as u32).min(ALMOST);
        self.inner.known.store(true, Ordering::Relaxed);
        self.inner.done.fetch_max(ppm, Ordering::Relaxed);
    }

    /// This slice is complete (the same as `set(1, 1)`).
    pub fn complete(&self) {
        self.set_fraction(1.0);
    }

    /// The step shown under the bar ("Scanning Caches", "Moving 12 of 40 items").
    pub fn set_step(&self, s: &str) {
        if let Ok(mut g) = self.inner.step.lock()
            && *g != s
        {
            *g = s.to_string();
        }
    }

    /// The job ended: the bar shows 100%.
    pub fn finish(&self) {
        self.inner.known.store(true, Ordering::Relaxed);
        self.inner.finished.store(true, Ordering::Relaxed);
        self.inner.done.store(FULL, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.inner.finished.load(Ordering::Relaxed)
    }

    /// The share of the whole bar done, `None` while the total is not known yet.
    pub fn fraction(&self) -> Option<f32> {
        if !self.inner.known.load(Ordering::Relaxed) {
            return None;
        }
        Some(self.inner.done.load(Ordering::Relaxed) as f32 / FULL as f32)
    }

    /// Whole percent done (0..=99 while running, 100 once finished), `None` while unknown.
    pub fn percent(&self) -> Option<u8> {
        if !self.inner.known.load(Ordering::Relaxed) {
            return None;
        }
        if self.is_finished() {
            return Some(100);
        }
        let d = self.inner.done.load(Ordering::Relaxed) as u64;
        Some(((d * 100) / FULL as u64).min(99) as u8)
    }

    pub fn step(&self) -> String {
        self.inner
            .step
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Time since the bar was made.
    pub fn elapsed(&self) -> Duration {
        self.inner.started.elapsed()
    }

    /// Seconds left, only when the estimate is stable: at least 10% done after at least 5
    /// seconds, and three estimates in a row (taken a second apart) within 25% of each
    /// other. `None` otherwise: no guess is better than a jumping one.
    pub fn eta_secs(&self) -> Option<u64> {
        self.eta_at(self.elapsed())
    }

    fn eta_at(&self, elapsed: Duration) -> Option<u64> {
        let f = self.fraction()? as f64;
        if self.is_finished() || f < 0.10 || elapsed < Duration::from_secs(5) {
            return None;
        }
        let left = elapsed.as_secs_f64() * (1.0 - f) / f;
        let mut g = self.inner.eta.lock().ok()?;
        let now = self.inner.started + elapsed;
        let due =
            g.at.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1));
        if due {
            g.agreeing = match g.last {
                Some(prev) if prev > 0.0 && ((left - prev) / prev).abs() <= 0.25 => g.agreeing + 1,
                _ => 0,
            };
            g.last = Some(left);
            g.at = Some(now);
        }
        (g.agreeing >= 2).then(|| left.round() as u64)
    }
}

/// "about 2 min left", "less than a minute left": words for [`Progress::eta_secs`].
pub fn eta_text(secs: u64) -> String {
    if secs < 60 {
        "Less than a minute left".to_string()
    } else {
        let m = secs.div_ceil(60);
        if m == 1 {
            "About 1 min left".to_string()
        } else {
            format!("About {m} min left")
        }
    }
}

/// The bar as text for a terminal: "[########............]  42%  Scanning Caches", or a
/// moving band ("[....####............]  Scanning Caches") while the total is unknown.
/// `tick` moves the band.
pub fn text_bar(meter: &Progress, width: usize, tick: usize) -> String {
    let width = width.max(4);
    let step = meter.step();
    let mut bar = vec!['.'; width];
    let tail = match meter.fraction() {
        Some(f) => {
            let filled = ((f as f64) * width as f64).floor() as usize;
            for c in bar.iter_mut().take(filled.min(width)) {
                *c = '#';
            }
            format!(" {:>3}%", meter.percent().unwrap_or(0))
        }
        None => {
            let band = (width / 4).max(1);
            let start = tick % (width + band);
            for (i, c) in bar.iter_mut().enumerate() {
                if i + band >= start && i < start {
                    *c = '#';
                }
            }
            String::new()
        }
    };
    let bar: String = bar.into_iter().collect();
    if step.is_empty() {
        format!("[{bar}]{tail}")
    } else {
        format!("[{bar}]{tail}  {step}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_until_a_total_is_set() {
        let p = Progress::new();
        assert_eq!(p.fraction(), None);
        assert_eq!(p.percent(), None);
        p.set(3, 0);
        assert_eq!(p.percent(), None, "a zero total says nothing");
        p.set(0, 4);
        assert_eq!(p.percent(), Some(0));
        p.set(1, 4);
        assert_eq!(p.percent(), Some(25));
    }

    #[test]
    fn never_goes_back_and_stops_at_99_until_finished() {
        let p = Progress::new();
        p.set(3, 4);
        p.set(1, 4);
        assert_eq!(p.percent(), Some(75));
        p.set(10, 10);
        assert_eq!(p.percent(), Some(99));
        assert!(!p.is_finished());
        p.finish();
        assert_eq!(p.percent(), Some(100));
        assert_eq!(p.fraction(), Some(1.0));
    }

    #[test]
    fn spans_map_into_their_slice() {
        let p = Progress::new();
        let second = p.span(1, 4);
        second.set(1, 2);
        assert_eq!(p.percent(), Some(37));
        // A finer slice inside it.
        second.range(0.5, 1.0).set(1, 1);
        assert_eq!(p.percent(), Some(50));
        // An earlier slice reporting late never pulls the bar back.
        p.span(0, 4).set(1, 2);
        assert_eq!(p.percent(), Some(50));
        p.span(3, 4).complete();
        assert_eq!(p.percent(), Some(99));
    }

    #[test]
    fn many_threads_keep_it_monotonic() {
        let p = Progress::new();
        let mut seen = Vec::new();
        std::thread::scope(|s| {
            for t in 0..4u64 {
                let p = p.clone();
                s.spawn(move || {
                    for i in 0..=1000u64 {
                        p.set((i * 7 + t * 13) % 1000, 1000);
                    }
                });
            }
            for _ in 0..200 {
                seen.push(p.fraction().unwrap_or(0.0));
            }
        });
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "never goes back");
        assert!(p.percent().unwrap() <= 99);
    }

    #[test]
    fn text_bar_for_terminals() {
        let p = Progress::new();
        p.set_step("Logs");
        let unknown = text_bar(&p, 20, 7);
        assert!(!unknown.contains('%'), "{unknown}");
        assert!(unknown.contains('#') && unknown.ends_with("  Logs"));
        p.set(1, 4);
        assert_eq!(text_bar(&p, 20, 0), "[#####...............]  25%  Logs");
        p.finish();
        assert_eq!(text_bar(&p, 4, 0), "[####] 100%  Logs");
    }

    #[test]
    fn steps_are_shared() {
        let p = Progress::new();
        p.span(2, 3).set_step("Scanning Caches");
        assert_eq!(p.step(), "Scanning Caches");
    }

    #[test]
    fn eta_only_when_stable() {
        let p = Progress::new();
        p.set(1, 100);
        assert_eq!(p.eta_at(Duration::from_secs(10)), None, "under 10% done");
        p.set(20, 100);
        assert_eq!(p.eta_at(Duration::from_secs(2)), None, "too early");
        // 20% after 20s, 25% after 25s, 30% after 30s: steady, about 80 to 70 s left.
        p.set(20, 100);
        assert_eq!(p.eta_at(Duration::from_secs(20)), None);
        p.set(25, 100);
        assert_eq!(p.eta_at(Duration::from_secs(25)), None);
        p.set(30, 100);
        assert_eq!(p.eta_at(Duration::from_secs(30)), Some(70));
        assert_eq!(eta_text(70), "About 2 min left");
        assert_eq!(eta_text(40), "Less than a minute left");
        // A jump in speed resets it.
        p.set(90, 100);
        assert_eq!(p.eta_at(Duration::from_secs(31)), None);
        p.finish();
        assert_eq!(p.eta_at(Duration::from_secs(40)), None);
    }
}
