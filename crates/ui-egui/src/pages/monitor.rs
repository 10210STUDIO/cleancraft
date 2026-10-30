//! Live Monitor: CPU, memory, storage, battery and network cards, refreshed every two seconds
//! by a background job (never on the UI thread), alerts, and Free Up RAM with confirmation.
//! Every number is formatted straight from the last [`Snapshot`]; a value that could not be
//! read shows "Unknown".

use std::time::Duration;

use cleancraft_core::FeatureId;
use cleancraft_core::fsutil::{human_size, memory_size};
use cleancraft_core::monitor::{
    self, AppUse, BatteryReading, PurgeOutcome, Sampler, Snapshot, UNKNOWN, pct, rate_text,
    uptime_text,
};
use cleancraft_core::proc_names;
use egui::{Color32, Sense, Ui, vec2};

use crate::app::{CleanApp, Page};
use crate::icons::Icon;
use crate::jobs::Job;
use crate::kit;
use crate::kit_list;
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
    /// When the last reading started (egui's clock, seconds).
    last_start: Option<f64>,
    /// Show every process instead of grouping helpers under their app.
    pub by_process: bool,
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
            by_process: false,
            interval: Duration::from_secs(2),
            confirm_purge: false,
            purge: None,
        }
    }
}

fn start_sample(app: &mut CleanApp) {
    let env = app.env.clone();
    let mut s = app.monitor.sampler.take().unwrap_or_default();
    app.monitor.last_start = Some(app.now);
    // The finished reading wakes the window itself: no frames are drawn while it runs.
    let wake = app.egui_ctx.clone();
    app.monitor.job = Some(Job::start(app.sync, move |_| {
        let snap = s.sample(&env, monitor::now_secs());
        if let Some(ctx) = wake {
            ctx.request_repaint();
        }
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
    // Readings only while the page is on screen, at most one per interval.
    let now = app.now;
    if app.page == Page::Feature(FeatureId::Monitor)
        && !app.window_hidden
        && app.monitor.job.is_none()
        && app
            .monitor
            .last_start
            .is_none_or(|t| now - t >= app.monitor.interval.as_secs_f64() || now < t)
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
        } else if app.egui_ctx.is_none() {
            // Nothing can wake the window: look again shortly.
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
    // The next reading: one wake-up per interval (none while the window is hidden).
    if !app.window_hidden {
        ui.ctx().request_repaint_after(app.monitor.interval);
    }
    let Some(snap) = app.monitor.last.clone() else {
        let meter = app
            .monitor
            .job
            .as_ref()
            .map(|j| j.meter.clone())
            .unwrap_or_default();
        kit::progress(ui, &t, f, "Reading...", "", &meter, false);
        return;
    };
    let out = egui::ScrollArea::vertical()
        .id_salt("monitor")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for a in monitor::alerts(&snap) {
                kit::strip(ui, Icon::Warning, t.warning, &a.text);
                ui.add_space(4.0);
            }
            ui.add_space(2.0);
            ui.spacing_mut().item_spacing.x = space::GUTTER;
            // Three columns that stack their cards; the last card of each column grows so
            // the three end level (REVIEW-C C25), by what was missing on the frame before.
            let key = egui::Id::new("monitor-col-extra");
            let extra: [f32; 3] = ui.data(|d| d.get_temp(key)).unwrap_or([0.0; 3]);
            let mut bottoms = [0.0f32; 3];
            ui.columns(3, |cols| {
                cols[0].data_mut(|d| d.insert_temp(extra_id(), extra[0]));
                cpu_card(app, &mut cols[0], &snap);
                memory_card(app, &mut cols[1], &snap);
                cols[1].add_space(space::GUTTER - 8.0);
                cols[1].data_mut(|d| d.insert_temp(extra_id(), extra[1]));
                network_card(app, &mut cols[1], &snap);
                storage_card(app, &mut cols[2], &snap);
                cols[2].add_space(space::GUTTER - 8.0);
                cols[2].data_mut(|d| d.insert_temp(extra_id(), extra[2]));
                battery_card(app, &mut cols[2], &snap);
                for (i, c) in cols.iter().enumerate() {
                    bottoms[i] = c.min_rect().bottom() - extra[i];
                }
            });
            let low = bottoms.iter().copied().fold(f32::MIN, f32::max);
            let want = bottoms.map(|b| (low - b).max(0.0).round());
            // Settle in a few frames at most: never a loop of frames chasing a pixel.
            let tries_key = egui::Id::new("monitor-col-tries");
            let tries: u8 = ui.data(|d| d.get_temp(tries_key)).unwrap_or(0);
            if want.iter().zip(extra).any(|(a, b)| (a - b).abs() > 1.5) && tries < 3 {
                ui.data_mut(|d| {
                    d.insert_temp(key, want);
                    d.insert_temp(tries_key, tries + 1);
                });
                ui.ctx().request_repaint();
            } else if tries > 0 && want.iter().zip(extra).all(|(a, b)| (a - b).abs() <= 1.5) {
                ui.data_mut(|d| d.insert_temp(tries_key, 0u8));
            }
            if !snap.problems.is_empty() {
                ui.add_space(8.0);
                kit::dim(ui, &t, &format!("Not read: {}", snap.problems.join("; ")));
            }
            ui.add_space(space::M);
        });
    kit::scroll_fade_page(ui, &out, &t);
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
            // The room that makes its column end level with the others (set by the page).
            let extra: f32 = ui.data_mut(|d| d.remove_temp(extra_id())).unwrap_or(0.0);
            if extra > 0.0 {
                ui.add_space(extra);
            }
        });
}

/// Extra height for the next card drawn (the last of its column).
fn extra_id() -> egui::Id {
    egui::Id::new("monitor-card-extra")
}

fn big(app: &CleanApp, ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            crate::rich(text)
                .font(font(size::TITLE, Weight::Bold))
                .color(app.t.text),
        )
        .truncate(),
    );
}

/// A semicircle gauge with the value inside it.
fn gauge_big(app: &CleanApp, ui: &mut Ui, frac: f32, color: Color32, text: &str) {
    // A 128 pt arc with the number inside it: a gauge, not a loading indicator.
    const R: f32 = 64.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), R + 22.0), Sense::hover());
    let c = egui::pos2(r.left() + R + 6.0, r.bottom() - 6.0);
    kit::gauge(ui.painter(), c, R, frac, color);
    let tr = egui::Rect::from_center_size(c - vec2(0.0, 20.0), vec2(R * 1.5, 34.0));
    ui.put(
        tr,
        egui::Label::new(
            crate::rich(text)
                .font(font(size::TITLE, Weight::Bold))
                .color(app.t.text),
        )
        .selectable(false),
    );
}

/// A label pair on one line: name on the left, value on the right.
fn row(app: &CleanApp, ui: &mut Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                crate::rich(name)
                    .font(font(size::CALLOUT, Weight::Regular))
                    .color(app.t.text_dim),
            )
            .truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                crate::rich(value)
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
    let mut act = ProcAction::default();
    card(app, ui, Icon::Monitor, "CPU", |ui| match &s.cpu {
        Some(c) => {
            let frac = (c.load / 100.0) as f32;
            gauge_big(app, ui, frac, level_color(app, frac), &pct(c.load));
            // The history line once there is something to draw (two different readings).
            let varied = history
                .windows(2)
                .any(|w| (w[0] - w[1]).abs() > f64::EPSILON);
            if varied {
                ui.add_space(12.0);
                sparkline(ui, &history, app.t.sec_a);
            } else {
                kit::dim(ui, &app.t, "The load over time shows after a few readings.");
            }
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
            act = proc_rows(app, ui, s, true, &c.top);
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
    apply(app, act);
}

fn memory_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let t = app.t;
    let mut purge = false;
    let running = app.monitor.purge.is_some();
    let mut act = ProcAction::default();
    card(app, ui, Icon::Gear, "Memory", |ui| {
        match &s.memory {
            Some(m) => {
                // "7.78 GB" big and "of 16.0 GB" beside it: the figure is never cut, and
                // VoiceOver hears the whole "7.78 GB of 16.0 GB".
                let used = memory_size(m.used);
                let full = format!("{used} of {}", memory_size(m.total));
                let rest = format!("of {}", memory_size(m.total));
                ui.horizontal(|ui| {
                    let fid = font(size::TITLE, Weight::Bold);
                    let w = kit::galley(ui, &used, fid.clone(), t.text).size().x + 2.0;
                    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                    kit::text_at(ui, r, &used, &full, fid, t.text, egui::Align::Min);
                    let (r2, _) = ui.allocate_exact_size(
                        vec2(ui.available_width().max(10.0), 30.0),
                        Sense::hover(),
                    );
                    ui.painter().text(
                        r2.left_bottom() - vec2(0.0, 6.0),
                        egui::Align2::LEFT_BOTTOM,
                        crate::tr(rest),
                        font(size::CALLOUT, Weight::Medium),
                        t.text_dim,
                    );
                });
                let frac = m.used_fraction() as f32;
                let w = ui.available_width();
                kit::bar(ui, &t, frac, level_color(app, frac), w);
                row(app, ui, "Available", &memory_size(m.available()));
                row(app, ui, "Pressure", &monitor::pressure_text(m));
                row(
                    app,
                    ui,
                    "Swap used",
                    &m.swap_used
                        .map(memory_size)
                        .unwrap_or_else(|| UNKNOWN.into()),
                );
                ui.add_space(4.0);
                act = proc_rows(app, ui, s, false, &m.top);
            }
            None => big(app, ui, UNKNOWN),
        }
        ui.add_space(6.0);
        if running {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 14.0, &t, time);
                ui.label(
                    crate::rich("Freeing memory...")
                        .font(font(size::CALLOUT, Weight::Medium))
                        .color(t.text),
                );
            });
            crate::motion::spinner_repaint(ui.ctx());
        } else {
            // In a row of its own: the columns are justified, and the click area must be
            // the painted pill only.
            ui.horizontal(|ui| {
                if kit::pill(ui, &t, "Free Up RAM", kit::Kind::Secondary, None)
                    .on_hover_text(
                        crate::tr("Clear memory macOS keeps as disk cache (asks for your password). The same as the Free up RAM task in Tune-up, and logged in History the same way."),
                    )
                    .clicked()
                {
                    purge = true;
                }
            });
        }
    });
    if purge {
        app.act();
        app.monitor.confirm_purge = true;
    }
    apply(app, act);
}

fn storage_card(app: &mut CleanApp, ui: &mut Ui, s: &Snapshot) {
    let t = app.t;
    let mut go = false;
    kit::card(&t)
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            kit::card_head(ui, &t, Icon::DiskMap, t.sec_a, "Storage");
            ui.add_space(2.0);
            let st = &s.storage;
            {
                let a: &CleanApp = app;
                match (st.free, st.total) {
                    (Some(free), Some(total)) => {
                        big(a, ui, &format!("{} free", human_size(free)));
                        let w = ui.available_width();
                        let used = 1.0 - free as f32 / total.max(1) as f32;
                        kit::bar(ui, &t, used, level_color(a, used), w);
                        row(a, ui, "Disk size", &human_size(total));
                    }
                    _ => big(a, ui, UNKNOWN),
                }
                row(
                    a,
                    ui,
                    "Trash",
                    &st.trash.map(human_size).unwrap_or_else(|| UNKNOWN.into()),
                );
            }
            if let Some(n) = &st.trash_note {
                kit::dim(ui, &t, n);
            }
            ui.add_space(6.0);
            // Two buttons wrap to two rows in a narrow column (1060 pt window).
            ui.horizontal_wrapped(|ui| {
                if kit::pill(ui, &t, "Free Up Space", kit::Kind::Secondary, None)
                    .on_hover_text(crate::tr("Open Quick Sweep to find what you can remove"))
                    .clicked()
                {
                    go = true;
                }
                // What waits in the Trash comes back with Finder's Empty Trash, right here
                // (REVIEW-U U4, U22).
                if st.trash.is_some_and(|b| b > 0) {
                    super::extras_cleanup::empty_trash(app, ui, "Empty Trash...");
                }
            });
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
            row(app, ui, "State", &sentence_case(&b.state));
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
                ui.label(
                    crate::rich("Measuring speed...")
                        .font(font(size::CALLOUT, Weight::Medium))
                        .color(app.t.text_dim),
                );
            });
            crate::motion::spinner_repaint(ui.ctx());
        }
        None => big(app, ui, UNKNOWN),
    });
}

fn confirm_purge(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut go = false;
    let mut close = false;
    let dlg = kit::modal(egui::Id::new("confirm-purge"), &t).show(ui.ctx(), |ui| {
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
        let (g, c) = kit::dialog_actions(ui, &t, "Free Up RAM Now", kit::Kind::Primary);
        go = g;
        close = c;
    });
    if dlg.should_close() {
        close = true;
    }
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

/// "discharging" as "Discharging": pmset's words start lower case.
fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The top list of a card: apps with their helpers grouped ("Brave Browser, 3 processes"),
/// or each process; each row says in plain words what a known process is (dim second line
/// when there is room, the whole text on hover). macOS processes never get a Quit link; an
/// app's row links to Startup Items, where apps using a lot can be quit with confirmation.
/// What a click in a top list asks for.
#[derive(Default)]
struct ProcAction {
    toggle: bool,
    quit: bool,
    /// A top list's new sort order: (list name, preferences).
    sort: Option<(&'static str, cleancraft_core::settings::ListPrefs)>,
}

const BY_CPU: kit_list::SortKey = kit_list::SortKey {
    key: "cpu",
    label: "Sort by CPU",
    first: "Highest first",
    second: "Lowest first",
    descending: true,
};
const BY_MEMORY: kit_list::SortKey = kit_list::SortKey {
    key: "memory",
    label: "Sort by memory",
    first: "Highest first",
    second: "Lowest first",
    descending: true,
};
/// The CPU card's list orders (its own measure first) and its name in the settings.
pub const CPU_SORTS: [kit_list::SortKey; 3] = [BY_CPU, BY_MEMORY, kit_list::BY_NAME];
pub const CPU_LIST: &str = "monitor.cpu";
/// The Memory card's.
pub const MEMORY_SORTS: [kit_list::SortKey; 3] = [BY_MEMORY, BY_CPU, kit_list::BY_NAME];
pub const MEMORY_LIST: &str = "monitor.memory";

/// Order a card's top list (which processes are in it stays the card's own measure).
fn order_groups(
    groups: &mut [proc_names::Group],
    sorts: &[kit_list::SortKey],
    p: &cleancraft_core::settings::ListPrefs,
) {
    let (key, rev) = kit_list::sort_of(p, sorts);
    groups.sort_by(|a, b| {
        let o = match key.key {
            "cpu" => b.cpu.total_cmp(&a.cpu),
            "memory" => b.memory.cmp(&a.memory),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        if rev { o.reverse() } else { o }
    });
}

fn apply(app: &mut CleanApp, a: ProcAction) {
    if let Some((list, p)) = a.sort {
        app.act();
        if kit_list::store(&mut app.settings, list, p) {
            app.save_settings();
        }
    }
    if a.toggle {
        app.act();
        app.monitor.by_process = !app.monitor.by_process;
    }
    if a.quit {
        app.act();
        app.open_route(&cleancraft_core::route::AppRoute::Feature(
            FeatureId::LoginItems,
        ));
    }
}

fn proc_rows(
    app: &CleanApp,
    ui: &mut Ui,
    s: &Snapshot,
    by_cpu: bool,
    top: &[AppUse],
) -> ProcAction {
    let t = app.t;
    let mut groups: Vec<proc_names::Group> = if s.processes.is_empty() || app.monitor.by_process {
        let list: Vec<AppUse> = if s.processes.is_empty() {
            top.to_vec()
        } else if by_cpu {
            monitor::top_by_cpu(&s.processes, monitor::TOP_APPS)
        } else {
            monitor::top_by_memory(&s.processes, monitor::TOP_APPS)
        };
        list.iter()
            .map(|a| {
                let ex = proc_names::explain_proc(a);
                proc_names::Group {
                    name: a.name.clone(),
                    processes: 1,
                    cpu: a.cpu,
                    memory: a.memory,
                    macos: ex.as_ref().is_some_and(|e| e.macos),
                    what: ex.map(|e| e.what),
                }
            })
            .collect()
    } else if by_cpu {
        proc_names::top_groups_by_cpu(&s.processes, monitor::TOP_APPS)
    } else {
        proc_names::top_groups_by_memory(&s.processes, monitor::TOP_APPS)
    };
    let (list, sorts) = if by_cpu {
        (CPU_LIST, &CPU_SORTS)
    } else {
        (MEMORY_LIST, &MEMORY_SORTS)
    };
    let mut prefs = kit_list::prefs(&app.settings, list);
    order_groups(&mut groups, sorts, &prefs);
    let mut sort = None;
    let mut quit = false;
    let mut toggle = false;
    // The order of the top list, at its top right (its own row: cards are narrow).
    if groups.len() > 1 {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), crate::theme::hit::FIELD),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if kit_list::sort_menu(ui, list, sorts, &mut prefs) {
                    sort = Some((list, prefs.clone()));
                }
            },
        );
        ui.add_space(2.0);
    }
    for (i, g) in groups.iter().enumerate() {
        let value = if by_cpu {
            pct(g.cpu)
        } else {
            memory_size(g.memory)
        };
        let name = if g.processes > 1 {
            format!("{} ({} processes)", g.name, g.processes)
        } else {
            g.name.clone()
        };
        let hover_id = egui::Id::new(("proc-row-hover", by_cpu, i));
        let was_hovered: bool = ui.data(|d| d.get_temp(hover_id)).unwrap_or(false);
        let resp = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                row(app, ui, &name, &value);
                let wide = ui.available_width() >= 240.0;
                if let Some(w) = &g.what
                    && wide
                {
                    // A small faint line under the name; the whole text on hover and for
                    // VoiceOver.
                    let fid = font(size::FOOTNOTE, Weight::Regular);
                    let short = kit::truncate_end(ui.ctx(), w, &fid, ui.available_width());
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            crate::tr(w.as_str()),
                        )
                    });
                    ui.painter().text(
                        r.left_center(),
                        egui::Align2::LEFT_CENTER,
                        crate::tr(short),
                        fid,
                        t.text_faint,
                    );
                    ui.add_space(2.0);
                }
                if was_hovered && !g.macos {
                    ui.push_id(("quit-link", by_cpu, i), |ui| {
                        if kit::ghost(ui, &t, "Quit in Startup Items...", None).clicked() {
                            quit = true;
                        }
                    });
                }
            })
            .response;
        let tip = match (&g.what, g.macos) {
            (Some(w), true) => format!(
                "{w} CleanCraft never offers to quit it. Activity Monitor shows more about it."
            ),
            (Some(w), false) => format!(
                "{w} To quit it, use Startup Items, under apps using a lot. Activity Monitor shows more about it."
            ),
            (None, _) => "To quit it, use Startup Items, under apps using a lot. Activity Monitor shows more about it.".to_string(),
        };
        let hovered = ui.rect_contains_pointer(resp.rect);
        if hovered != was_hovered {
            ui.data_mut(|d| d.insert_temp(hover_id, hovered));
            ui.ctx().request_repaint();
        }
        kit::row_tip(&resp, resp.rect, &tip);
    }
    if by_cpu && !groups.is_empty() {
        // The rows and the gauge are on two scales (U7-7): say so once.
        kit::dim(
            ui,
            &t,
            "Per process: percent of one core, as Activity Monitor shows it. 100% is one core busy, so a row can be higher than the CPU total.",
        );
    }
    if !s.processes.is_empty() {
        let label = if app.monitor.by_process {
            "Group by App"
        } else {
            "Show Each Process"
        };
        ui.push_id(("proc-mode", by_cpu), |ui| {
            if kit::ghost(ui, &t, label, None).clicked() {
                toggle = true;
            }
        });
    }
    ProcAction { toggle, quit, sort }
}
