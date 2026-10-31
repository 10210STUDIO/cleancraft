//! The menu bar panel's logic, with no platform code: what the panel shows, where it opens,
//! how clicks on the status item open and close it, and the one-line commands the panel sends
//! back to the agent.
//!
//! The menu bar item is an icon only. A click opens a small live panel under it (CPU, memory,
//! free space, battery, network, the top processes, what waits for review, and buttons). The
//! panel is its own short-lived process (`cleancraft panel`, see `apps/cleancraft/src/panel.rs`)
//! so the agent that runs all day keeps no renderer in memory. Everything here is plain data
//! and arithmetic so it is tested on Linux.

use std::time::{Duration, Instant};

use crate::fsutil::{human_size, memory_size};
use crate::monitor::{self, BatteryReading, Snapshot, UNKNOWN};
use crate::proc_names;
use crate::schedule::{self, MenuAction, Review};
use crate::settings::Settings;

// ---------------------------------------------------------------------------------------------
// Size and place.

/// The panel's size in points.
pub const PANEL_W: f64 = 340.0;
pub const PANEL_H: f64 = 520.0;
/// Space between the menu bar item and the panel's top edge, in points.
pub const GAP: f64 = 6.0;
/// The panel stays this far from the screen's left and right edges, in points.
pub const EDGE: f64 = 8.0;

/// A rectangle in physical pixels of the desktop (origin at the top left of the main
/// display, y down), as tray-icon and winit report them on macOS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A display: its rectangle in physical pixels and its scale factor (2 on Retina).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub rect: PhysRect,
    pub scale: f64,
}

/// A rectangle in points (logical pixels), the unit windows are placed in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PtRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PhysRect {
    fn to_points(self, scale: f64) -> PtRect {
        let s = if scale > 0.0 { scale } else { 1.0 };
        PtRect {
            x: self.x / s,
            y: self.y / s,
            w: self.w / s,
            h: self.h / s,
        }
    }
}

impl PtRect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// The menu bar item's rectangle and its display's rectangle, both in points. Each physical
/// rectangle was made from points with its own display's scale, so the right display is the
/// one whose scale turns the item's centre into a point inside it. `None` without displays.
pub fn locate(icon: PhysRect, screens: &[Screen]) -> Option<(PtRect, PtRect)> {
    let fit = screens.iter().find_map(|s| {
        let i = icon.to_points(s.scale);
        let r = s.rect.to_points(s.scale);
        r.contains(i.x + i.w / 2.0, i.y + i.h / 2.0)
            .then_some((i, r))
    });
    // None fits (a display list that changed under us): the first display decides.
    fit.or_else(|| {
        screens
            .first()
            .map(|s| (icon.to_points(s.scale), s.rect.to_points(s.scale)))
    })
}

/// The panel's top-left corner in points: centred under the menu bar item, `GAP` below it,
/// kept `EDGE` inside the display's left and right edges and never below its bottom.
pub fn panel_origin(icon: PhysRect, screens: &[Screen], w: f64, h: f64) -> (f64, f64) {
    let Some((i, scr)) = locate(icon, screens) else {
        return (icon.x, icon.y + icon.h + GAP);
    };
    let mut x = i.x + i.w / 2.0 - w / 2.0;
    let (lo, hi) = (scr.x + EDGE, scr.x + scr.w - w - EDGE);
    x = if hi < lo {
        scr.x + (scr.w - w) / 2.0
    } else {
        x.clamp(lo, hi)
    };
    let mut y = i.y + i.h + GAP;
    let bottom = scr.y + scr.h - EDGE;
    if y + h > bottom {
        y = (bottom - h).max(scr.y);
    }
    (x.round(), y.round())
}

/// `--at` of `cleancraft panel`: "x,y" in points.
pub fn at_arg(x: f64, y: f64) -> String {
    format!("{},{}", x.round() as i64, y.round() as i64)
}

/// Read `--at`.
pub fn parse_at(s: &str) -> Option<(f32, f32)> {
    let (x, y) = s.split_once(',')?;
    let (x, y) = (x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

// ---------------------------------------------------------------------------------------------
// Clicks on the status item.

/// A click that comes this soon after the panel closed itself is the click that closed it
/// (the panel loses focus to the menu bar first), so it does not open the panel again.
pub const REOPEN_GUARD: Duration = Duration::from_millis(400);

/// What a click on the menu bar item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickDo {
    Open,
    Close,
    Nothing,
}

/// The panel's open or closed state as the agent sees it.
#[derive(Clone, Debug, Default)]
pub struct PanelToggle {
    open: bool,
    closed_at: Option<Instant>,
}

impl PanelToggle {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// A left click on the item.
    pub fn click(&mut self, now: Instant) -> ClickDo {
        if self.open {
            return ClickDo::Close;
        }
        if self
            .closed_at
            .is_some_and(|t| now.saturating_duration_since(t) < REOPEN_GUARD)
        {
            return ClickDo::Nothing;
        }
        self.open = true;
        ClickDo::Open
    }

    /// The panel process ended (closed itself, was closed, or failed to start).
    pub fn closed(&mut self, now: Instant) {
        self.open = false;
        self.closed_at = Some(now);
    }
}

// ---------------------------------------------------------------------------------------------
// What the panel sends back: one line per command on its standard output.

/// The panel's quick actions: a name, the word under its icon (a quarter of the panel fits
/// about 9 letters) and the app page it opens ([`crate::route::AppRoute`]).
/// A scan route only looks, and Free Up RAM waits for its button on the Live Monitor, so the
/// person still confirms everything in the app and the panel never cleans.
pub const QUICK_ACTIONS: [(&str, &str, &str); 4] = [
    ("Free Up RAM", "Free RAM", "task:free_ram"),
    ("Empty Trash", "Trash", "scan:trash_bins"),
    ("Threat Check", "Threats", "scan:malware"),
    ("App Updates", "Updates", "scan:updater"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelCmd {
    /// Open the n-th waiting item (newest first, as the panel lists them).
    Review(usize),
    /// The n-th of [`QUICK_ACTIONS`].
    Quick(usize),
    Open,
    QuickSweep,
    TogglePause,
    QuitAgent,
}

impl PanelCmd {
    pub fn line(&self) -> String {
        match self {
            PanelCmd::Review(i) => format!("review {i}"),
            PanelCmd::Quick(i) => format!("quick {i}"),
            PanelCmd::Open => "open".into(),
            PanelCmd::QuickSweep => "sweep".into(),
            PanelCmd::TogglePause => "pause".into(),
            PanelCmd::QuitAgent => "quit".into(),
        }
    }

    pub fn parse(line: &str) -> Option<PanelCmd> {
        let l = line.trim();
        if let Some(n) = l.strip_prefix("review ") {
            return n.trim().parse().ok().map(PanelCmd::Review);
        }
        if let Some(n) = l.strip_prefix("quick ") {
            let i: usize = n.trim().parse().ok()?;
            return (i < QUICK_ACTIONS.len()).then_some(PanelCmd::Quick(i));
        }
        match l {
            "open" => Some(PanelCmd::Open),
            "sweep" => Some(PanelCmd::QuickSweep),
            "pause" => Some(PanelCmd::TogglePause),
            "quit" => Some(PanelCmd::QuitAgent),
            _ => None,
        }
    }

    /// The agent's action for it.
    pub fn action(&self) -> MenuAction {
        match self {
            PanelCmd::Review(i) => MenuAction::Review(*i),
            PanelCmd::Quick(i) => MenuAction::Route(QUICK_ACTIONS[*i].2),
            PanelCmd::Open => MenuAction::Open,
            PanelCmd::QuickSweep => MenuAction::QuickSweep,
            PanelCmd::TogglePause => MenuAction::TogglePause,
            PanelCmd::QuitAgent => MenuAction::Quit,
        }
    }

    /// Does the panel close after it? Everything but Pause leads elsewhere.
    pub fn closes_panel(&self) -> bool {
        !matches!(self, PanelCmd::TogglePause)
    }
}

// ---------------------------------------------------------------------------------------------
// Refresh.

/// A new reading about this often while the panel is open.
pub const REFRESH: Duration = Duration::from_secs(1);
/// Processes in the panel's top list.
pub const TOP_N: usize = 3;

// ---------------------------------------------------------------------------------------------
// What the panel shows.

/// One row of the top list.
#[derive(Clone, Debug, PartialEq)]
pub struct TopRow {
    /// "Safari", or "Google Chrome (3 processes)".
    pub name: String,
    pub cpu: String,
    /// What it is in plain words, when CleanCraft knows.
    pub what: Option<String>,
}

/// One reading of a meter: its value text, a small line under it, and how full its bar is.
#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub value: String,
    pub detail: String,
    /// 0 to 1; `None` draws no bar.
    pub fill: Option<f32>,
    /// Shown in the warning colour.
    pub warn: bool,
}

/// Everything on the panel, as text, from one reading.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelView {
    /// No reading yet: the meters say "Reading".
    pub reading: bool,
    pub cpu: Meter,
    pub memory: Meter,
    pub disk: Meter,
    /// `None` on a Mac without a battery: no tile.
    pub battery: Option<Meter>,
    /// "Down 1.2 MB/s, up 30 KB/s".
    pub network: String,
    pub top: Vec<TopRow>,
    /// The waiting items, newest first.
    pub reviews: Vec<String>,
    pub paused: bool,
}

pub const READING: &str = "Reading";
pub const MEASURING: &str = "Measuring";

impl PanelView {
    pub fn pause_label(&self) -> &'static str {
        if self.paused {
            schedule::RESUME
        } else {
            schedule::PAUSE
        }
    }

    /// Show the badge on the menu bar icon? Only when something waits for review.
    pub fn badge(&self) -> bool {
        !self.reviews.is_empty()
    }
}

fn unknown_meter() -> Meter {
    Meter {
        value: UNKNOWN.into(),
        detail: String::new(),
        fill: None,
        warn: false,
    }
}

/// The panel for a reading (`None` before the first one), the waiting items (newest first)
/// and the settings (pause, disk alert threshold).
pub fn panel_view(snap: Option<&Snapshot>, reviews: &[Review], settings: &Settings) -> PanelView {
    // The panel lists them under "Waiting for review": the menu's "Review: " is dropped.
    let reviews: Vec<String> = reviews
        .iter()
        .map(|r| {
            r.text
                .strip_prefix("Review: ")
                .unwrap_or(&r.text)
                .to_string()
        })
        .collect();
    let paused = settings.notifications_paused;
    let Some(s) = snap else {
        let r = Meter {
            value: READING.into(),
            detail: String::new(),
            fill: None,
            warn: false,
        };
        return PanelView {
            reading: true,
            cpu: r.clone(),
            memory: r.clone(),
            disk: r.clone(),
            battery: None,
            network: READING.into(),
            top: Vec::new(),
            reviews,
            paused,
        };
    };
    let cpu = match &s.cpu {
        Some(c) => Meter {
            value: monitor::pct(c.load),
            detail: c
                .cores
                .map(|n| crate::fsutil::count(n as usize, "core", "cores"))
                .unwrap_or_default(),
            fill: Some((c.load / 100.0).clamp(0.0, 1.0) as f32),
            warn: c.load >= 90.0,
        },
        None => unknown_meter(),
    };
    let memory = match &s.memory {
        Some(m) => {
            let pressed = matches!(
                m.pressure,
                Some(monitor::Pressure::Warning | monitor::Pressure::Critical)
            );
            Meter {
                value: match m.pressure_percent {
                    Some(p) => format!("{p}% pressure"),
                    None => format!("{} used", memory_size(m.used)),
                },
                detail: match (m.pressure_percent, m.pressure) {
                    (Some(_), _) => {
                        format!("{} of {} in use", memory_size(m.used), memory_size(m.total))
                    }
                    (None, Some(p)) => format!(
                        "of {}, pressure {}",
                        memory_size(m.total),
                        p.label().to_lowercase()
                    ),
                    (None, None) => format!("of {}", memory_size(m.total)),
                },
                fill: Some(m.gauge_fraction().clamp(0.0, 1.0) as f32),
                warn: pressed,
            }
        }
        None => unknown_meter(),
    };
    let low = schedule::disk_low(s, settings.disk_low_percent).unwrap_or(false);
    let disk = match (s.storage.free, s.storage.total) {
        (Some(free), Some(total)) if total > 0 => Meter {
            value: format!("{} free", human_size(free)),
            detail: format!("of {}", human_size(total)),
            fill: Some((1.0 - free as f64 / total as f64).clamp(0.0, 1.0) as f32),
            warn: low,
        },
        (Some(free), _) => Meter {
            value: format!("{} free", human_size(free)),
            detail: String::new(),
            fill: None,
            warn: low,
        },
        _ => unknown_meter(),
    };
    let battery = match &s.battery {
        Some(BatteryReading::Present(b)) => Some(Meter {
            value: format!("{}%", b.percent),
            detail: capitalize(&b.state),
            fill: Some(f32::from(b.percent.min(100)) / 100.0),
            warn: b.percent <= 15 && !b.charging() && !b.on_ac,
        }),
        Some(BatteryReading::None) => None,
        None => Some(unknown_meter()),
    };
    let network = match s.network {
        Some(n) => format!(
            "Down {}, up {}",
            monitor::rate_text(n.down),
            monitor::rate_text(n.up)
        ),
        // The first reading has nothing to compare with; a failed netstat says so.
        None if s
            .problems
            .iter()
            .any(|p| p.contains("netstat") || p.contains("network")) =>
        {
            UNKNOWN.into()
        }
        None => MEASURING.into(),
    };
    let top = proc_names::top_groups_by_cpu(&s.processes, TOP_N)
        .into_iter()
        .map(|g| TopRow {
            name: if g.processes > 1 {
                format!("{} ({} processes)", g.name, g.processes)
            } else {
                g.name.clone()
            },
            cpu: monitor::pct(g.cpu),
            what: g.what,
        })
        .collect();
    PanelView {
        reading: false,
        cpu,
        memory,
        disk,
        battery,
        network,
        top,
        reviews,
        paused,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// The status item image.

/// Side of the status item image in pixels: 22 pt at 2x. The 18 pt template (36 px) sits in
/// the middle with 2 pt around it, which leaves room for the badge dot in the top-right
/// corner. tray-icon shows an RGBA icon taller than 22 px at 22 pt, so this stays sharp on
/// Retina displays.
pub const ICON_PX: u32 = 44;
/// The template's side in pixels (MenuBarTemplate@2x.png).
pub const TEMPLATE_PX: u32 = 36;

/// The status item image from the @2x template (RGBA, `side` x `side`): centred on a
/// [`ICON_PX`] canvas, with a small dot in the top-right corner when `badge` is on. The glyph
/// is cleared in a thin ring around the dot so the dot reads on its own. Only alpha matters:
/// macOS draws a template in the menu bar's own colour.
pub fn status_icon(template: &[u8], side: u32, badge: bool) -> Vec<u8> {
    let n = ICON_PX as usize;
    let mut out = vec![0u8; n * n * 4];
    let side = side as usize;
    if template.len() >= side * side * 4 && side <= n {
        let off = (n - side) / 2;
        for y in 0..side {
            for x in 0..side {
                let s = (y * side + x) * 4;
                let d = ((y + off) * n + x + off) * 4;
                // Black with the template's alpha, whatever colour the file has.
                out[d + 3] = template[s + 3];
            }
        }
    }
    if badge {
        // Dot of radius 5 px (2.5 pt), centre 7 px in from the top-right corner; ring 2 px.
        let (cx, cy, r, ring) = (n as f32 - 7.0, 7.0f32, 5.0f32, 2.0f32);
        for y in 0..n {
            for x in 0..n {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let i = (y * n + x) * 4;
                if d <= r + ring {
                    // Coverage of the dot, anti-aliased over one pixel.
                    let a = (r + 0.5 - d).clamp(0.0, 1.0);
                    out[i..i + 4].copy_from_slice(&[0, 0, 0, (a * 255.0).round() as u8]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn retina() -> Screen {
        // A 1512 x 982 pt MacBook display at 2x.
        Screen {
            rect: PhysRect {
                x: 0.0,
                y: 0.0,
                w: 3024.0,
                h: 1964.0,
            },
            scale: 2.0,
        }
    }

    fn icon_at(x_pt: f64, scale: f64) -> PhysRect {
        PhysRect {
            x: x_pt * scale,
            y: 0.0,
            w: 30.0 * scale,
            h: 24.0 * scale,
        }
    }

    #[test]
    fn the_panel_opens_centred_under_the_item() {
        let (x, y) = panel_origin(icon_at(1000.0, 2.0), &[retina()], PANEL_W, PANEL_H);
        assert_eq!((x, y), (1015.0 - 170.0, 24.0 + GAP));
    }

    #[test]
    fn the_panel_stays_inside_the_right_and_left_edges() {
        let (x, _) = panel_origin(icon_at(1490.0, 2.0), &[retina()], PANEL_W, PANEL_H);
        assert_eq!(x, 1512.0 - PANEL_W - EDGE);
        let (x, _) = panel_origin(icon_at(2.0, 2.0), &[retina()], PANEL_W, PANEL_H);
        assert_eq!(x, EDGE);
    }

    #[test]
    fn a_second_display_with_another_scale_is_found() {
        // Main display 1920 x 1080 pt at 1x; a Retina display to its right (1512 pt wide at
        // 2x, so its physical origin is 1920 * 2).
        let main = Screen {
            rect: PhysRect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
            scale: 1.0,
        };
        let right = Screen {
            rect: PhysRect {
                x: 3840.0,
                y: 0.0,
                w: 3024.0,
                h: 1964.0,
            },
            scale: 2.0,
        };
        // An item at 1920 + 1400 pt on the right display.
        let icon = icon_at(3320.0, 2.0);
        let (i, scr) = locate(icon, &[main, right]).unwrap();
        assert_eq!(i.x, 3320.0);
        assert_eq!(scr.x, 1920.0);
        let (x, y) = panel_origin(icon, &[main, right], PANEL_W, PANEL_H);
        assert_eq!(y, 24.0 + GAP);
        assert_eq!(
            x,
            1920.0 + 1512.0 - PANEL_W - EDGE,
            "kept inside the right display"
        );
        // The same item read at 1x would fall on no display: the 2x display is chosen.
    }

    #[test]
    fn a_short_display_keeps_the_panel_above_its_bottom() {
        let small = Screen {
            rect: PhysRect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: PANEL_H - 60.0,
            },
            scale: 1.0,
        };
        let (_, y) = panel_origin(icon_at(400.0, 1.0), &[small], PANEL_W, PANEL_H);
        assert_eq!(y, 0.0, "never above the display's top");
        let taller = Screen {
            rect: PhysRect {
                h: PANEL_H + 20.0,
                ..small.rect
            },
            ..small
        };
        let (_, y) = panel_origin(icon_at(400.0, 1.0), &[taller], PANEL_W, PANEL_H);
        assert_eq!(y, 20.0 - EDGE);
    }

    #[test]
    fn no_display_list_still_gives_a_place() {
        let (x, y) = panel_origin(icon_at(100.0, 1.0), &[], PANEL_W, PANEL_H);
        assert_eq!((x, y), (100.0, 24.0 + GAP));
        assert_eq!(at_arg(845.4, 30.0), "845,30");
        assert_eq!(parse_at("845,30"), Some((845.0, 30.0)));
        assert_eq!(parse_at("-12, 30"), Some((-12.0, 30.0)));
        assert_eq!(parse_at("x,30"), None);
        assert_eq!(parse_at("12"), None);
    }

    #[test]
    fn clicks_open_and_close_and_the_closing_click_does_not_reopen() {
        let t0 = Instant::now();
        let mut p = PanelToggle::default();
        assert_eq!(p.click(t0), ClickDo::Open);
        assert!(p.is_open());
        // Second click while open: close.
        assert_eq!(p.click(t0 + Duration::from_secs(2)), ClickDo::Close);
        p.closed(t0 + Duration::from_secs(2));
        assert!(!p.is_open());
        // The panel closed itself on focus loss caused by this very click.
        assert_eq!(
            p.click(t0 + Duration::from_secs(2) + Duration::from_millis(100)),
            ClickDo::Nothing
        );
        assert_eq!(p.click(t0 + Duration::from_secs(3)), ClickDo::Open);
    }

    #[test]
    fn commands_round_trip_as_lines() {
        for c in [
            PanelCmd::Review(2),
            PanelCmd::Quick(3),
            PanelCmd::Open,
            PanelCmd::QuickSweep,
            PanelCmd::TogglePause,
            PanelCmd::QuitAgent,
        ] {
            assert_eq!(PanelCmd::parse(&c.line()), Some(c.clone()));
        }
        assert_eq!(PanelCmd::parse("review x"), None);
        assert_eq!(PanelCmd::parse("quick 4"), None, "only the listed actions");
        assert_eq!(
            PanelCmd::Quick(1).action(),
            MenuAction::Route("scan:trash_bins")
        );
        for (_, _, r) in QUICK_ACTIONS {
            assert!(crate::route::AppRoute::parse(r).is_some(), "{r}");
        }
        assert_eq!(PanelCmd::parse("rm -rf"), None);
        assert_eq!(PanelCmd::QuitAgent.action(), MenuAction::Quit);
        assert_eq!(PanelCmd::Review(1).action(), MenuAction::Review(1));
        assert!(!PanelCmd::TogglePause.closes_panel());
        assert!(PanelCmd::Open.closes_panel());
    }

    #[test]
    fn without_the_pressure_percent_the_panel_shows_memory_used() {
        let m = crate::testkit_monitor::sweep_mac();
        let fake = m.env.fake_sys().expect("fake");
        crate::testkit_monitor::answer_all(fake);
        fake.answer(
            "sysctl",
            &crate::testkit_monitor::SYSCTL.replace("kern.memorystatus_level: 89\n", ""),
        );
        let snap = monitor::Sampler::new().sample(&m.env, 1000.0);
        let mem = snap.memory.clone().expect("memory");
        assert_eq!(mem.pressure_percent, None);
        let v = panel_view(Some(&snap), &[], &Settings::default());
        assert!(v.memory.value.ends_with(" GB used"), "{}", v.memory.value);
        assert_eq!(v.memory.detail, "of 16.0 GB, pressure normal");
        assert_eq!(v.memory.fill, Some(mem.used_fraction() as f32));
    }

    #[test]
    fn the_view_reads_the_sample_mac() {
        let m = crate::testkit_monitor::sweep_mac();
        let fake = m.env.fake_sys().expect("fake");
        crate::testkit_monitor::answer_all(fake);
        let mut sm = monitor::Sampler::new();
        let first = sm.sample(&m.env, 1000.0);
        let s = Settings::default();
        let v = panel_view(Some(&first), &[], &s);
        assert!(!v.reading);
        assert_eq!(v.cpu.value, "17%");
        assert_eq!(v.cpu.detail, "8 cores");
        // kern.memorystatus_level 89: 11% pressure, and the bar follows the pressure.
        assert_eq!(v.memory.value, "11% pressure");
        assert!(
            v.memory.detail.ends_with(" GB of 16.0 GB in use"),
            "{}",
            v.memory.detail
        );
        assert_eq!(v.memory.fill, Some(0.11));
        assert_eq!(v.disk.value, "81.9 GB free");
        assert_eq!(v.disk.detail, "of 500 GB");
        let b = v.battery.clone().expect("a MacBook");
        assert_eq!(
            (b.value.as_str(), b.detail.as_str()),
            ("64%", "Discharging")
        );
        assert_eq!(v.network, MEASURING, "one reading has no speed yet");
        // Top 3 by CPU, with plain words for macOS processes.
        let names: Vec<&str> = v.top.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Google Chrome", "Safari", "WindowServer"]);
        assert!(
            v.top[2]
                .what
                .as_deref()
                .unwrap_or("")
                .starts_with("Part of macOS")
        );
        assert!(!v.badge());
        fake.answer("netstat", crate::testkit_monitor::NETSTAT_2);
        let second = sm.sample(&m.env, 1002.0);
        let v = panel_view(Some(&second), &[], &s);
        assert!(
            v.network.starts_with("Down 2.00 MB/s, up 100 KB/s"),
            "{}",
            v.network
        );
        // No battery (Mac mini): no tile. A broken netstat: Unknown, not Measuring.
        fake.answer("pmset", crate::testkit_monitor::PMSET_DESKTOP);
        crate::testkit_monitor::break_tool(fake, "netstat");
        let third = sm.sample(&m.env, 1003.0);
        let v = panel_view(Some(&third), &[], &s);
        assert!(v.battery.is_none());
        assert_eq!(v.network, UNKNOWN);
        let before = panel_view(None, &[], &s);
        assert!(before.reading);
        assert_eq!(before.cpu.value, READING);
        let paused = Settings {
            notifications_paused: true,
            ..Settings::default()
        };
        let r = Review {
            text: "Review: CleanCraft found 2 GB to clean".into(),
            route: schedule::SWEEP_ROUTE.into(),
        };
        let v = panel_view(None, std::slice::from_ref(&r), &paused);
        assert!(v.badge());
        assert_eq!(v.reviews, ["CleanCraft found 2 GB to clean"]);
        assert_eq!(v.pause_label(), schedule::RESUME);
    }

    #[test]
    fn the_badge_dot_sits_in_the_corner_and_the_glyph_in_the_middle() {
        let side = TEMPLATE_PX;
        let tpl = vec![255u8; (side * side * 4) as usize];
        let plain = status_icon(&tpl, side, false);
        let n = ICON_PX as usize;
        assert_eq!(plain.len(), n * n * 4);
        let alpha = |v: &[u8], x: usize, y: usize| v[(y * n + x) * 4 + 3];
        assert_eq!(alpha(&plain, 0, 0), 0, "padding is clear");
        assert_eq!(alpha(&plain, 22, 22), 255, "glyph in the middle");
        assert_eq!(alpha(&plain, n - 2, 2), 0, "no dot without the badge");
        assert_eq!(
            alpha(&status_icon(&vec![0; tpl.len()], side, false), n - 7, 6),
            0
        );
        let dot = status_icon(&tpl, side, true);
        assert_eq!(alpha(&dot, n - 7, 6), 255, "dot");
        // The ring around the dot is cleared out of the glyph.
        assert_eq!(alpha(&dot, n - 7, 7 + 6), 0);
        assert_eq!(alpha(&dot, 22, 22), 255);
        // A template of the wrong size gives an empty image, never a panic.
        assert!(status_icon(&[1, 2, 3], side, false).iter().all(|b| *b == 0));
    }
}
