//! Free space over time: one small sample (free and total bytes of the startup disk, Trash
//! size) per app launch and per menu bar agent tick, at most one per [`MIN_GAP_SECS`], kept
//! [`KEEP_DAYS`] days in `space-history.json` in CleanCraft's data folder.
//!
//! Numbers only, never a path or a file name. A damaged file is kept aside as
//! `space-history.damaged.json` and the history starts again; it never stops the app.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::Env;

/// At most one sample per hour.
pub const MIN_GAP_SECS: i64 = 3600;
/// Samples older than this are dropped.
pub const KEEP_DAYS: i64 = 90;
/// Hard cap on the number of samples (90 days of one per hour), so the file stays small even
/// if the clock jumps around.
pub const MAX_SAMPLES: usize = (KEEP_DAYS as usize) * 24;
/// A trend needs samples spread over at least two days.
pub const MIN_TREND_SECS: i64 = 2 * 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since the epoch.
    pub at: i64,
    /// Free bytes on the startup disk.
    pub free: u64,
    /// Size of the startup disk, when known.
    #[serde(default)]
    pub total: Option<u64>,
    /// Bytes in the Trash, when it could be read.
    #[serde(default)]
    pub trash: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub version: u32,
    /// Oldest first.
    pub samples: Vec<Sample>,
}

/// What reading the file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub history: History,
    /// The file could not be read as a history: it was kept aside and the history starts
    /// again. The text says so in plain words.
    pub damaged: Option<String>,
}

pub fn path(env: &Env) -> PathBuf {
    env.data_dir().join("space-history.json")
}

/// Read the history. A missing file is an empty history; a damaged one is copied next to it
/// as `space-history.damaged.json` and reported, and the history starts empty.
pub fn load(env: &Env) -> Loaded {
    let p = path(env);
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Loaded {
                history: History::default(),
                damaged: None,
            };
        }
        Err(e) => {
            return Loaded {
                history: History::default(),
                damaged: Some(format!("The free space history could not be read: {e}")),
            };
        }
    };
    match serde_json::from_slice::<History>(&bytes) {
        Ok(mut h) => {
            h.samples.sort_by_key(|s| s.at);
            Loaded {
                history: h,
                damaged: None,
            }
        }
        Err(_) => {
            let copy = p.with_extension("damaged.json");
            let _ = std::fs::write(&copy, &bytes);
            Loaded {
                history: History::default(),
                damaged: Some(
                    "The free space history was damaged and starts again from now.".into(),
                ),
            }
        }
    }
}

fn save(env: &Env, h: &History) -> io::Result<()> {
    let p = path(env);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    let body = serde_json::to_vec(h).map_err(io::Error::other)?;
    std::fs::write(&tmp, body)?;
    // One rename: the file is never seen half written.
    std::fs::rename(&tmp, &p)
}

/// Drop samples older than [`KEEP_DAYS`] before `now` and keep at most [`MAX_SAMPLES`].
pub fn prune(h: &mut History, now: i64) {
    let oldest = now - KEEP_DAYS * 86_400;
    h.samples.retain(|s| s.at >= oldest);
    if h.samples.len() > MAX_SAMPLES {
        let cut = h.samples.len() - MAX_SAMPLES;
        h.samples.drain(..cut);
    }
}

/// Whether a sample is due at `now`: none yet, or the last one is an hour old or more. A last
/// sample dated in the future (the clock was set back) does not block new ones.
pub fn due(h: &History, now: i64) -> bool {
    match h.samples.last() {
        None => true,
        Some(last) => {
            let gap = now - last.at;
            !(0..MIN_GAP_SECS).contains(&gap)
        }
    }
}

/// Take a sample now if one is due (at most one per hour). Safe to call on every launch and
/// every agent tick. Returns true when a sample was written.
pub fn record(env: &Env) -> io::Result<bool> {
    record_at(env, chrono::Utc::now().timestamp())
}

/// [`record`] at a given time (tests).
pub fn record_at(env: &Env, now: i64) -> io::Result<bool> {
    let mut h = load(env).history;
    if !due(&h, now) {
        return Ok(false);
    }
    let disk = crate::space::disk_use(env, env.home());
    let Some(free) = disk.free else {
        return Err(io::Error::other("free space could not be read"));
    };
    let (trash, _) = crate::monitor::trash_size(env);
    h.version = 1;
    // A sample dated in the future (clock set back) is replaced, so the list stays in order.
    h.samples.retain(|s| s.at < now);
    h.samples.push(Sample {
        at: now,
        free,
        total: disk.total,
        trash,
    });
    prune(&mut h, now);
    save(env, &h)?;
    Ok(true)
}

/// Samples of the last `days` days before `now`, oldest first.
pub fn window(h: &History, now: i64, days: i64) -> Vec<Sample> {
    let from = now - days * 86_400;
    h.samples
        .iter()
        .filter(|s| s.at >= from && s.at <= now)
        .copied()
        .collect()
}

/// Change of free space over about `days` days: (latest free minus the free space of the
/// oldest sample in the window, seconds covered). None with fewer than two samples or when
/// they cover less than [`MIN_TREND_SECS`] (too short to call a trend).
pub fn change(h: &History, now: i64, days: i64) -> Option<(i64, i64)> {
    let w = window(h, now, days);
    let (first, last) = (w.first()?, w.last()?);
    let span = last.at - first.at;
    if w.len() < 2 || span < MIN_TREND_SECS {
        return None;
    }
    Some((last.free as i64 - first.free as i64, span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::FakeMac;

    const T0: i64 = 1_760_000_000;

    fn mac(free: u64) -> FakeMac {
        let m = FakeMac::new();
        m.env
            .fake_sys()
            .unwrap()
            .with(|s| s.free_space = Some(free));
        m
    }

    #[test]
    fn one_sample_per_hour() {
        let m = mac(50_000_000_000);
        assert!(record_at(&m.env, T0).unwrap());
        assert!(!record_at(&m.env, T0 + 60).unwrap());
        assert!(!record_at(&m.env, T0 + MIN_GAP_SECS - 1).unwrap());
        assert!(record_at(&m.env, T0 + MIN_GAP_SECS).unwrap());
        assert_eq!(load(&m.env).history.samples.len(), 2);
    }

    #[test]
    fn clock_set_back_does_not_block_or_disorder() {
        let m = mac(1);
        assert!(record_at(&m.env, T0).unwrap());
        assert!(record_at(&m.env, T0 - 7200).unwrap());
        let h = load(&m.env).history;
        assert_eq!(h.samples.len(), 1);
        assert_eq!(h.samples[0].at, T0 - 7200);
    }

    #[test]
    fn keeps_ninety_days() {
        let m = mac(10);
        for d in 0..100 {
            record_at(&m.env, T0 + d * 86_400).unwrap();
        }
        let h = load(&m.env).history;
        let last = T0 + 99 * 86_400;
        assert!(h.samples.iter().all(|s| s.at >= last - KEEP_DAYS * 86_400));
        assert_eq!(h.samples.len(), 91);
    }

    #[test]
    fn cap_bounds_the_file() {
        let mut h = History::default();
        for i in 0..(MAX_SAMPLES as i64 + 50) {
            h.samples.push(Sample {
                at: T0 + i,
                free: 1,
                total: None,
                trash: None,
            });
        }
        prune(&mut h, T0);
        assert_eq!(h.samples.len(), MAX_SAMPLES);
    }

    #[test]
    fn damaged_file_is_kept_aside_and_history_restarts() {
        let m = mac(42);
        std::fs::create_dir_all(m.env.data_dir()).unwrap();
        std::fs::write(path(&m.env), b"{\"version\":1,\"samples\":[{\"at\":").unwrap();
        let l = load(&m.env);
        assert!(l.damaged.is_some());
        assert!(l.history.samples.is_empty());
        assert!(path(&m.env).with_extension("damaged.json").exists());
        assert!(record_at(&m.env, T0).unwrap());
        assert_eq!(load(&m.env).history.samples[0].free, 42);
    }

    #[test]
    fn unknown_free_space_writes_nothing() {
        let m = FakeMac::new();
        m.env.fake_sys().unwrap().with(|s| s.free_space = None);
        assert!(record_at(&m.env, T0).is_err());
        assert!(!path(&m.env).exists());
    }

    #[test]
    fn samples_hold_numbers_only() {
        let m = mac(5);
        m.home_file(".Trash/secret-name.txt", 1000);
        record_at(&m.env, T0).unwrap();
        let body = std::fs::read_to_string(path(&m.env)).unwrap();
        assert!(!body.contains("secret"), "{body}");
        assert!(!body.contains('/'), "{body}");
        assert!(
            load(&m.env).history.samples[0]
                .trash
                .is_some_and(|t| t >= 1000)
        );
    }

    #[test]
    fn change_needs_enough_span() {
        let mut h = History::default();
        let s = |at, free| Sample {
            at,
            free,
            total: None,
            trash: None,
        };
        h.samples.push(s(T0, 100));
        h.samples.push(s(T0 + 3600, 90));
        assert_eq!(change(&h, T0 + 3600, 7), None);
        h.samples.push(s(T0 + 5 * 86_400, 40));
        assert_eq!(change(&h, T0 + 5 * 86_400, 7), Some((-60, 5 * 86_400)));
    }
}
