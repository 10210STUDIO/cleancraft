//! Live Monitor: CPU, memory, storage, battery and network cards, refreshed every two seconds
//! by a background job (never on the UI thread), alerts, and Free Up RAM with confirmation.
//! Every number is formatted straight from the last [`Snapshot`]; a value that could not be
//! read shows "Unknown".

use std::time::{Duration, Instant};

use cleancraft_core::FeatureId;
use cleancraft_core::fsutil::{human_size, memory_size};
use cleancraft_core::monitor::{
    self, BatteryReading, PurgeOutcome, Sampler, Snapshot, UNKNOWN, pct, rate_text, uptime_text,
};
use egui::{Color32, RichText, Sense, Ui, vec2};

use crate::app::{CleanApp, Page};
use crate::icons::Icon;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

pub struct State {
    job: Option<Job<(Sampler, Snapshot)>>,
    sampler: Option<Sampler>,
    /// The last reading shown on screen.
    pub last: Option<Snapshot>,
    /// CPU load of recent readings, for the history line.
    pub history: Vec<f64>,
    /// Readings taken since the page opened.
    pub samples: u32,
    last_start: Option<Instant>,
    /// Time between readings (tests may change it).
    pub interval: Duration,
    confirm_purge: bool,
    purge: Option<Job<PurgeOutcome>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            job: None,
            sampler: Some(Sampler::new()),
            last: None,
            history: Vec::new(),
            samples: 0,
            last_start: None,
            interval: Duration::from_secs(2),
            confirm_purge: false,
            purge: None,
        }
    }
}

fn start_sample(app: &mut CleanApp) {
    let env = app.env.clone();
    let mut s = app.monitor.sampler.take().unwrap_or_default();
    app.monitor.last_start = Some(Instant::now());
    app.monitor.job = Some(Job::start(app.sync, move |_| {
        let snap = s.sample(&env, monitor::now_secs());
        (s, snap)
    }));
}

/// Finish background work and start the next reading while the page is open. Returns true
/// while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    if let Some(job) = app.monitor.purge.as_mut() {
        if let Some(out) = job.poll() {
            app.monitor.purge = None;
            match out {
                PurgeOutcome::Done => app.tell("Memory freed. Inactive memory went back to macOS."),
                PurgeOutcome::Cancelled => app.tell("Cancelled. Nothing was run."),
                PurgeOutcome::Failed(e) => app.warn(format!("Could not free memory: {e}")),
            }
            // Read again right away.
            app.monitor.last_start = None;
        } else if job.is_dead() {
            app.monitor.purge = None;
            app.warn("Freeing memory stopped unexpectedly.");
        } else {
            busy = true;
        }
    }
    if app.page == Page::Feature(FeatureId::Monitor)
        && app.monitor.job.is_none()
        && app
            .monitor
            .last_start
            .is_none_or(|t| t.elapsed() >= app.monitor.interval)
    {
        start_sample(app);
    }
    let st = &mut app.monitor;
    if let Some(job) = st.job.as_mut() {
        if let Some((s, snap)) = job.poll() {
            st.history = s.cpu_history.clone();
            st.sampler = Some(s);
            st.last = Some(snap);
            st.samples += 1;
            st.job = None;
        } else if job.is_dead() {
            st.job = None;
            st.sampler = Some(Sampler::new());
        } else {
            busy = true;
        }
    }
    busy
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let (icon, _) = kit::feature_look(&t, f);
    kit::header(
        ui,
        &t,
        icon,
        f.title(),
        "What your Mac is doing right now. Updated every two seconds.",
    );
    ui.ctx().request_repaint_after(app.monitor.interval);
    let Some(snap) = app.monitor.last.clone() else {
        kit::progress(ui, &t, f, "Reading...", "", 0, false);
        return;
    };
    egui::ScrollArea::vertical()
        .id_salt("monitor")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for a in monitor::alerts(&snap) {
                kit::strip(ui, Icon::Warning, t.warning, &a.text);
                ui.add_space(4.0);
            }
            ui.add_space(2.0);
            ui.spacing_mut().item_spacing.x = space::GUTTER;
            // Three columns that stack their cards, so no column ends in a hole.
            ui.columns(3, |cols| {
                cpu_card(app, &mut cols[0], &snap);
                memory_card(app, &mut cols[1], &snap);
                cols[1].add_space(space::GUTTER - 8.0);
                network_card(app, &mut cols[1], &snap);
                storage_card(app, &mut cols[2], &snap);
                cols[2].add_space(space::GUTTER - 8.0);
                battery_card(app, &mut cols[2], &snap);
            });
            if !snap.problems.is_empty() {
                ui.add_space(8.0);
                kit::dim(ui, &t, &format!("Not read: {}", snap.problems.join("; ")));
            }
            ui.add_space(space::M);
        });
    if app.monitor.confirm_purge {
        confirm_purge(app, ui);
    }
}

fn card(app: &CleanApp, ui: &mut Ui, icon: Icon, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = app.t;
    kit::card(&t)
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            kit::card_head(ui, &t, icon, t.sec_a, title);
            ui.add_space(2.0);
            add(ui);
        });
}

fn big(app: &CleanApp, ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(font(size::NUMBER, Weight::Bold))
                .color(app.t.text),
        )
        .truncate(),
    );
}

/// A semicircle gauge with the value inside it.
fn gauge_big(app: &CleanApp, ui: &mut Ui, frac: f32, color: Color32, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 78.0), Sense::hover());
    let c = egui::pos2(r.left() + 56.0, r.bottom() - 8.0);
    kit::gauge(ui.painter(), c, 48.0, frac, color);
    let fid = font(size::TITLE, Weight::Bold);
    let tr = egui::Rect::from_center_size(c - vec2(0.0, 16.0), vec2(88.0, 30.0));
    kit::text_at(ui, tr, text, text, fid, app.t.text, egui::Align::Center);
}

/// A label pair on one line: name on the left, value on the right.
fn row(app: &CleanApp, ui: &mut Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                RichText::new(name)
                    .font(font(size::CALLOUT, Weight::Regular))
                    .color(app.t.text_dim),
            )
            .truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .font(font(size::CALLOUT, Weight::Medium))
                    .color(app.t.text),
            );
        });
    });
}

fn sparkline(ui: &mut Ui, values: &[f64], color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
    kit::sparkline(ui.painter(), r, values, monitor::CPU_HISTORY, color);
}

/// Warm colours as a value nears its limit.
fn level_color(app: &CleanApp, frac: f32) -> Color32 {
    let t = app.t;
    if frac > 0.9 {
        t.danger
    } else if frac > 0.75 {
        t.warning
    } else {
        t.sec_a
    }
}

fn cpu_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let history = app.monitor.history.clone();
    card(app, ui, Icon::Monitor, "CPU", |ui| match &s.cpu {
        Some(c) => {
            let frac = (c.load / 100.0) as f32;
            gauge_big(app, ui, frac, level_color(app, frac), &pct(c.load));
            sparkline(ui, &history, app.t.sec_a);
            if let Some(l) = c.load_avg {
                row(
                    app,
                    ui,
                    "Load average",
                    &format!("{:.2}, {:.2}, {:.2}", l[0], l[1], l[2]),
                );
            }
            row(
                app,
                ui,
                "Uptime",
                &s.uptime_secs
                    .map(uptime_text)
                    .unwrap_or_else(|| UNKNOWN.into()),
            );
            ui.add_space(4.0);
            for a in &c.top {
                row(app, ui, &a.name, &pct(a.cpu));
            }
        }
        None => {
            big(app, ui, UNKNOWN);
            row(
                app,
                ui,
                "Uptime",
                &s.uptime_secs
                    .map(uptime_text)
                    .unwrap_or_else(|| UNKNOWN.into()),
            );
        }
    });
}

fn memory_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let t = app.t;
    let mut purge = false;
    let running = app.monitor.purge.is_some();
    card(app, ui, Icon::Gear, "Memory", |ui| {
        match &s.memory {
            Some(m) => {
                big(
                    app,
                    ui,
                    &format!("{} of {}", memory_size(m.used), memory_size(m.total)),
                );
                let frac = m.used_fraction() as f32;
                let w = ui.available_width();
                kit::bar(ui, &t, frac, level_color(app, frac), w);
                row(app, ui, "Available", &memory_size(m.available()));
                row(
                    app,
                    ui,
                    "Pressure",
                    m.pressure.map(|p| p.label()).unwrap_or(UNKNOWN),
                );
                row(
                    app,
                    ui,
                    "Swap used",
                    &m.swap_used
                        .map(memory_size)
                        .unwrap_or_else(|| UNKNOWN.into()),
                );
                ui.add_space(4.0);
                for a in &m.top {
                    row(app, ui, &a.name, &memory_size(a.memory));
                }
            }
            None => big(app, ui, UNKNOWN),
        }
        ui.add_space(6.0);
        if running {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 14.0, &t, time);
                ui.label("Freeing memory...");
            });
            ui.ctx().request_repaint();
        } else if kit::secondary(ui, &t, "Free Up RAM") {
            purge = true;
        }
    });
    if purge {
        app.act();
        app.monitor.confirm_purge = true;
    }
}

fn storage_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let t = app.t;
    let mut go = false;
    card(app, ui, Icon::DiskMap, "Storage", |ui| {
        let st = &s.storage;
        match (st.free, st.total) {
            (Some(free), Some(total)) => {
                big(app, ui, &format!("{} free", human_size(free)));
                let w = ui.available_width();
                let used = 1.0 - free as f32 / total.max(1) as f32;
                kit::bar(ui, &t, used, level_color(app, used), w);
                row(app, ui, "Disk size", &human_size(total));
            }
            _ => big(app, ui, UNKNOWN),
        }
        row(
            app,
            ui,
            "Trash",
            &st.trash.map(human_size).unwrap_or_else(|| UNKNOWN.into()),
        );
        if let Some(n) = &st.trash_note {
            kit::dim(ui, &t, n);
        }
        ui.add_space(6.0);
        if kit::secondary(ui, &t, "Free Up Space") {
            go = true;
        }
    });
    if go {
        app.act();
        app.navigate(FeatureId::SmartCare.as_str());
    }
}

fn battery_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    card(app, ui, Icon::Startup, "Battery", |ui| match &s.battery {
        Some(BatteryReading::Present(b)) => {
            let frac = b.percent as f32 / 100.0;
            let color = if frac < 0.2 {
                app.t.danger
            } else {
                app.t.success
            };
            gauge_big(app, ui, frac, color, &format!("{}%", b.percent));
            row(app, ui, "State", &b.state);
            row(
                app,
                ui,
                b.estimate_label(),
                &b.remaining_minutes
                    .map(|m| uptime_text(u64::from(m) * 60))
                    .unwrap_or_else(|| "No estimate".into()),
            );
            row(
                app,
                ui,
                "Health",
                &b.health
                    .map(|h| format!("{h}%"))
                    .unwrap_or_else(|| UNKNOWN.into()),
            );
            row(
                app,
                ui,
                "Cycles",
                &b.cycles
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| UNKNOWN.into()),
            );
        }
        Some(BatteryReading::None) => {
            big(app, ui, "No battery");
            kit::dim(ui, &app.t, "This Mac runs on mains power.");
        }
        None => big(app, ui, UNKNOWN),
    });
}

fn network_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let measuring = app.monitor.samples < 2
        && !s
            .problems
            .iter()
            .any(|p| p.contains("netstat") || p.contains("network"));
    card(app, ui, Icon::Updates, "Network", |ui| match s.network {
        Some(n) => {
            row(app, ui, "Download", &rate_text(n.down));
            row(app, ui, "Upload", &rate_text(n.up));
        }
        None if measuring => {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 14.0, &app.t, time);
                ui.label("Measuring speed...");
            });
            ui.ctx().request_repaint();
        }
        None => big(app, ui, UNKNOWN),
    });
}

fn confirm_purge(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut go = false;
    let mut close = false;
    kit::modal(egui::Id::new("confirm-purge"), &t).show(ui.ctx(), |ui| {
        ui.set_width(480.0);
        kit::dialog_head(
            ui,
            &t,
            Icon::Gear,
            t.sec_a,
            "Free up memory?",
            "CleanCraft runs the macOS purge command, which clears memory used as disk cache. \
             macOS asks for an administrator password. Apps may feel slower for a moment.",
        );
        ui.add_space(space::L);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::primary(ui, &t, "Free Up RAM Now") {
                go = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
    if go {
        app.act();
        app.monitor.confirm_purge = false;
        let env = app.env.clone();
        app.monitor.purge = Some(Job::start(app.sync, move |_| monitor::free_up_ram(&env)));
    } else if close {
        app.act();
        app.monitor.confirm_purge = false;
    }
}
