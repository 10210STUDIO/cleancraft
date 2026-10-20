//! `cleancraft agent`: the menu bar agent process (started at login by the LaunchAgent that
//! Settings > "Show CleanCraft in the menu bar" writes).
//!
//! Two threads: the worker (this file, no platform code) samples CPU, memory, disk and Trash
//! every few seconds, runs the schedule and alert checks of [`cleancraft_core::schedule`] and
//! carries out menu choices; the main thread runs the status item ([`crate::tray`], the only
//! macOS code). They talk over two channels: menu models one way, menu actions the other.
//!
//! The agent never cleans or moves anything: it scans (look only), notifies and opens the app.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use cleancraft_core::monitor::{self, Sampler, Snapshot};
use cleancraft_core::schedule::{self, Agent, CheckReport, MenuAction, MenuModel};
use cleancraft_core::sweep::Sources;
use cleancraft_core::{Env, Settings, login_agent, notify};

/// A new reading this often.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// Schedule and alert checks this often.
pub const CHECK_EVERY: Duration = Duration::from_secs(60);

/// The worker's state, driven one round at a time (tests call [`Worker::round`] directly).
pub struct Worker {
    env: Env,
    agent: Agent,
    sampler: Sampler,
    snap: Option<Snapshot>,
    last_check: Option<Instant>,
}

impl Worker {
    pub fn new(env: Env, sources: Sources) -> Worker {
        let agent = Agent::new(&env, sources);
        Worker {
            env,
            agent,
            sampler: Sampler::new(),
            snap: None,
            last_check: None,
        }
    }

    /// One round: a new reading, the checks when [`CHECK_EVERY`] passed, and the menu to show.
    /// Settings are read again each round, so changes in the app apply within seconds.
    pub fn round(&mut self, mono: Instant, local: NaiveDateTime) -> (MenuModel, CheckReport) {
        let settings = Settings::load(&self.env);
        self.snap = Some(self.sampler.sample(&self.env, monitor::now_secs()));
        let mut rep = CheckReport::default();
        if self
            .last_check
            .is_none_or(|t| mono.duration_since(t) >= CHECK_EVERY)
        {
            self.last_check = Some(mono);
            rep = self
                .agent
                .check(&self.env, &settings, self.snap.as_ref(), local);
            log_report(&rep);
        }
        (self.agent.menu(self.snap.as_ref(), &settings), rep)
    }

    /// A menu choice: true when the agent should quit. Errors are logged (there is no window
    /// to show them in).
    pub fn act(&mut self, a: MenuAction) -> (bool, MenuModel) {
        let mut settings = Settings::load(&self.env);
        let quit = match self.agent.act(&self.env, &mut settings, a) {
            Ok(q) => q,
            Err(e) => {
                eprintln!("CleanCraft agent: {e}");
                false
            }
        };
        (quit, self.agent.menu(self.snap.as_ref(), &settings))
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }
}

fn log_report(rep: &CheckReport) {
    if let Some(b) = rep.scanned {
        eprintln!("CleanCraft agent: scheduled scan found {b} bytes ready to clean");
    }
    for f in &rep.failed {
        eprintln!("CleanCraft agent: {f}");
    }
}

/// Run the agent until Quit. Refuses when another agent runs.
pub fn run(env: &Env) -> anyhow::Result<()> {
    if !login_agent::is_on(env) {
        anyhow::bail!(
            "The menu bar is off. Turn on Settings > \"Show CleanCraft in the menu bar\" in CleanCraft."
        );
    }
    notify::claim_agent(env, std::process::id()).map_err(anyhow::Error::msg)?;
    let (to_ui, models) = mpsc::channel::<MenuModel>();
    let (actions, from_ui) = mpsc::channel::<MenuAction>();
    let worker_env = env.clone();
    std::thread::spawn(move || {
        worker_loop(Worker::new(worker_env, Sources::registered()), &to_ui, &from_ui)
    });
    crate::tray::run(models, actions)
}

/// Rounds every [`SAMPLE_EVERY`]; menu choices are handled as they come. Ends (and with it
/// the status item, whose channel closes) on Quit or when the menu bar is turned off: off
/// means off, also for an agent that launchctl did not stop.
fn worker_loop(mut w: Worker, to_ui: &Sender<MenuModel>, from_ui: &Receiver<MenuAction>) {
    loop {
        if !login_agent::is_on(&w.env) {
            return;
        }
        let (model, _) = w.round(Instant::now(), schedule::local_now());
        if to_ui.send(model).is_err() {
            return;
        }
        let until = Instant::now() + SAMPLE_EVERY;
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match from_ui.recv_timeout(left) {
                Ok(a) => {
                    let (quit, model) = w.act(a);
                    if quit || to_ui.send(model).is_err() {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::schedule::{QUICK_SWEEP, ScanEvery};
    use cleancraft_core::testkit_monitor::{answer_all, sweep_mac, sweep_sources};

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap_or_default()
    }

    #[test]
    fn a_round_shows_cpu_ram_and_the_four_lines_and_cleans_nothing() {
        let m = sweep_mac();
        let fake = m.env.fake_sys().expect("fake");
        answer_all(fake);
        let mut w = Worker::new(m.env.clone(), sweep_sources());
        let (model, rep) = w.round(Instant::now(), at("2026-10-20 09:00"));
        assert!(model.title.starts_with("CPU 17% RAM "), "{}", model.title);
        assert!(!model.disk_low);
        assert_eq!(model.info[0], "CPU: 17%");
        assert!(model.info[2].starts_with("Free space: "), "{:?}", model.info);
        assert!(model.info[3].starts_with("Trash: "), "{:?}", model.info);
        assert_eq!(rep.scanned, None, "scans are off by default");
        assert!(cleancraft_core::executor::history(&m.env).is_empty());
    }

    #[test]
    fn menu_choices_open_the_app_and_pause() {
        let m = sweep_mac();
        let fake = m.env.fake_sys().expect("fake");
        answer_all(fake);
        fake.answer("open", "");
        let mut w = Worker::new(m.env.clone(), sweep_sources());
        w.round(Instant::now(), at("2026-10-20 09:00"));
        let (quit, _) = w.act(MenuAction::QuickSweep);
        assert!(!quit);
        assert!(fake.calls().contains(&"/usr/bin/open -b com.cristianizzo.cleancraft".to_string()));
        let mut inbox = notify::OpenInbox::default();
        assert_eq!(
            inbox.poll(&m.env, chrono::Utc::now().timestamp()).as_deref(),
            Some("quick_sweep"),
            "{QUICK_SWEEP} opens Quick Sweep"
        );
        let (_, model) = w.act(MenuAction::TogglePause);
        assert!(model.paused);
        assert!(Settings::load(&m.env).notifications_paused);
        assert_eq!(model.pause_label(), schedule::RESUME);
        let (quit, _) = w.act(MenuAction::Quit);
        assert!(quit);
    }

    #[test]
    fn the_schedule_runs_from_rounds_once_per_check() {
        let m = sweep_mac();
        let fake = m.env.fake_sys().expect("fake");
        answer_all(fake);
        fake.answer("osascript", "");
        let s = Settings {
            scan_every: ScanEvery::Daily,
            scan_hour: 10,
            ..Settings::default()
        };
        s.save(&m.env).expect("save");
        let mut w = Worker::new(m.env.clone(), sweep_sources());
        let t0 = Instant::now();
        let (_, rep) = w.round(t0, at("2026-10-20 09:00"));
        assert_eq!(rep.scanned, None, "armed at 09:00, nothing due yet");
        // Next round within a minute: no check at all.
        let (_, rep) = w.round(t0 + Duration::from_secs(5), at("2026-10-20 10:30"));
        assert_eq!(rep.scanned, None);
        let (model, rep) = w.round(t0 + CHECK_EVERY, at("2026-10-20 10:31"));
        assert!(rep.scanned.is_some_and(|b| b > 0), "{rep:?}");
        assert_eq!(model.reviews.len(), 1);
        assert!(cleancraft_core::executor::history(&m.env).is_empty());
    }
}
