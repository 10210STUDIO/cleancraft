//! Background work. Scans and cleanups run on a thread so the window stays responsive; the UI
//! polls each frame. Tests set `sync` so a click finishes its work before the next frame.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

pub use cleancraft_core::progress::Progress;

pub struct Job<T> {
    rx: Option<Receiver<T>>,
    done: Option<T>,
    /// How far the work is (the progress bar) and its last step line. Work started with
    /// [`Job::start`] only reports steps (the bar stays a moving stripe); work started with
    /// [`Job::start_metered`] reports real units. Reaches 100% when the work returns.
    pub meter: Progress,
    /// When the work started (timeouts such as "the app has not quit yet").
    pub started: Instant,
    /// Keeps a `waiting` job's channel open.
    _hold: Option<Sender<T>>,
}

impl<T: Send + 'static> Job<T> {
    /// Start `work`, which reports its step lines. With `sync` it runs right here.
    pub fn start(
        sync: bool,
        work: impl FnOnce(&(dyn Fn(&str) + Sync)) -> T + Send + 'static,
    ) -> Job<T> {
        Self::start_metered(sync, move |m: &Progress| {
            let m2 = m.clone();
            work(&move |s: &str| m2.set_step(s))
        })
    }

    /// Start `work`, which reports into the job's [`Progress`] (units done and step lines).
    pub fn start_metered(sync: bool, work: impl FnOnce(&Progress) -> T + Send + 'static) -> Job<T> {
        let meter = Progress::new();
        if sync {
            let v = work(&meter);
            meter.finish();
            return Job {
                rx: None,
                done: Some(v),
                meter,
                started: Instant::now(),
                _hold: None,
            };
        }
        let (tx, rx) = channel();
        let m = meter.clone();
        std::thread::spawn(move || {
            let v = work(&m);
            m.finish();
            let _ = tx.send(v);
        });
        Job {
            rx: Some(rx),
            done: None,
            meter,
            started: Instant::now(),
            _hold: None,
        }
    }

    /// Work that never finishes, showing `status`: for screenshots of progress screens and
    /// for tests of the Stop button. The bar is a moving stripe until a test sets a share on
    /// [`Job::meter`].
    pub fn waiting(status: &str) -> Job<T> {
        let (tx, rx) = channel();
        let meter = Progress::new();
        meter.set_step(status);
        Job {
            rx: Some(rx),
            done: None,
            meter,
            started: Instant::now(),
            _hold: Some(tx),
        }
    }

    /// Whole seconds since the work started.
    pub fn elapsed_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// The result, once, when the work has finished.
    pub fn poll(&mut self) -> Option<T> {
        if let Some(v) = self.done.take() {
            return Some(v);
        }
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(v) => {
                self.rx = None;
                Some(v)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
        }
    }

    /// The work ended without a result (it panicked).
    pub fn is_dead(&self) -> bool {
        self.rx.is_none() && self.done.is_none()
    }

    /// The last step line reported by the work.
    pub fn status(&self) -> String {
        self.meter.step()
    }
}
