//! Background work. Scans and cleanups run on a thread so the window stays responsive; the UI
//! polls each frame. Tests set `sync` so a click finishes its work before the next frame.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

pub struct Job<T> {
    rx: Option<Receiver<T>>,
    done: Option<T>,
    /// Last progress line reported by the work.
    pub status: Arc<Mutex<String>>,
}

impl<T: Send + 'static> Job<T> {
    /// Start `work`. With `sync` it runs right here.
    pub fn start(
        sync: bool,
        work: impl FnOnce(&(dyn Fn(&str) + Sync)) -> T + Send + 'static,
    ) -> Job<T> {
        let status = Arc::new(Mutex::new(String::new()));
        if sync {
            let st = status.clone();
            let v = work(&move |s: &str| {
                if let Ok(mut g) = st.lock() {
                    *g = s.to_string();
                }
            });
            return Job {
                rx: None,
                done: Some(v),
                status,
            };
        }
        let (tx, rx) = channel();
        let st = status.clone();
        std::thread::spawn(move || {
            let v = work(&move |s: &str| {
                if let Ok(mut g) = st.lock() {
                    *g = s.to_string();
                }
            });
            let _ = tx.send(v);
        });
        Job {
            rx: Some(rx),
            done: None,
            status,
        }
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

    pub fn status(&self) -> String {
        self.status.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
