//! The menu bar panel: the small live window that opens under CleanCraft's menu bar icon.
//!
//! It shows CPU, memory, free space, battery and network, the three busiest processes with
//! plain words for what they are, what waits for review, Quick Sweep and Open CleanCraft, and
//! a More menu with Pause and Quit. The content comes from [`cleancraft_core::menubar`]; this
//! file only draws it in the app's own theme and reports what was clicked as
//! [`PanelCmd`]s. The window around it (borderless, on top, closing on focus loss) is the
//! `cleancraft panel` process in `apps/cleancraft/src/panel.rs`.
//!
//! Readings run on a thread about once a second (`top` itself takes a second), so the panel
//! never waits on a command. Tests run them inline with [`MenuPanel::with_sync`] on a fake Mac.
//! The panel never cleans or moves anything: every button opens the app or asks the agent.

use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use cleancraft_core::menubar::{self, Meter, PanelCmd, PanelView};
use cleancraft_core::monitor::{self, Sampler, Snapshot};
use cleancraft_core::schedule::{self, AgentState, Review};
use cleancraft_core::{Env, Settings};
use egui::{
    Align2, Color32, CornerRadius, Key, PopupCloseBehavior, Rect, Sense, Stroke, Ui, UiBuilder,
    WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::kit::{self, Kind};
use crate::theme::{self, Tokens, Weight, font, radius, size, space};

/// Accessible names of the panel's controls.
pub const QUICK_SWEEP: &str = "Quick Sweep";
pub const OPEN_APP: &str = schedule::OPEN_APP;
pub const MORE: &str = "More";
pub const WAITING: &str = "Waiting for review";
pub const NOTHING_WAITING: &str = "Nothing waits for review.";
pub const TOP_TITLE: &str = "Using the most CPU";
/// A meter tile's height.
const TILE_H: f32 = 72.0;

/// The panel's state: the latest reading and what the person clicked.
pub struct MenuPanel {
    pub env: Env,
    pub t: Tokens,
    sync: bool,
    fonts: bool,
    /// The pass that installed the fonts: they take effect from the next one.
    fonts_pass: Option<u64>,
    /// Inline sampler (tests), or the thread's channel (a real Mac).
    sampler: Sampler,
    rx: Option<Receiver<Snapshot>>,
    snap: Option<Snapshot>,
    reviews: Vec<Review>,
    settings: Settings,
    /// When the waiting items and settings were read last.
    read_at: Option<Instant>,
    out: Vec<PanelCmd>,
    close: bool,
}

impl MenuPanel {
    pub fn new(env: Env) -> MenuPanel {
        let settings = Settings::load(&env);
        let reviews = waiting(&env);
        MenuPanel {
            env,
            t: Tokens::dark(),
            sync: false,
            fonts: false,
            fonts_pass: None,
            sampler: Sampler::without_trash(),
            rx: None,
            snap: None,
            reviews,
            settings,
            read_at: None,
            out: Vec::new(),
            close: false,
        }
    }

    /// Readings run inline (tests): the first one at the first frame, the next ones by
    /// [`MenuPanel::refresh_at`].
    pub fn with_sync(mut self) -> MenuPanel {
        self.sync = true;
        self
    }

    /// A reading taken now, at `now` seconds (tests pick the time so speeds are exact), and
    /// the waiting items and settings read again.
    pub fn refresh_at(&mut self, now: f64) {
        self.snap = Some(self.sampler.sample(&self.env, now));
        self.reread();
    }

    fn reread(&mut self) {
        self.settings = Settings::load(&self.env);
        self.reviews = waiting(&self.env);
        self.read_at = Some(Instant::now());
    }

    /// What the panel shows now.
    pub fn view(&self) -> PanelView {
        menubar::panel_view(self.snap.as_ref(), &self.reviews, &self.settings)
    }

    /// Commands clicked since the last call.
    pub fn take_commands(&mut self) -> Vec<PanelCmd> {
        std::mem::take(&mut self.out)
    }

    /// Escape was pressed, or a command that leads elsewhere was clicked.
    pub fn wants_close(&self) -> bool {
        self.close
    }

    /// Fonts and theme once, new readings, Escape.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !self.fonts {
            theme::install_fonts(ctx);
            theme::apply(ctx, &self.t);
            self.fonts = true;
            self.fonts_pass = Some(ctx.cumulative_pass_nr());
            ctx.request_repaint();
        }
        if self.sync {
            if self.snap.is_none() {
                self.refresh_at(monitor::now_secs());
            }
        } else {
            self.poll(ctx);
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.close = true;
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let rx = self.rx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            let env = self.env.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let mut s = Sampler::without_trash();
                loop {
                    let started = Instant::now();
                    let snap = s.sample(&env, monitor::now_secs());
                    // The panel is gone: stop.
                    if tx.send(snap).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                    std::thread::sleep(menubar::REFRESH.saturating_sub(started.elapsed()));
                }
            });
            rx
        });
        let mut got = false;
        while let Ok(s) = rx.try_recv() {
            self.snap = Some(s);
            got = true;
        }
        if got || self.read_at.is_none_or(|t| t.elapsed() >= menubar::REFRESH) {
            self.reread();
        }
        ctx.request_repaint_after(menubar::REFRESH);
    }

    fn send(&mut self, c: PanelCmd) {
        if c.closes_panel() {
            self.close = true;
        }
        if c == PanelCmd::TogglePause {
            // Shown at once; the agent saves the setting.
            self.settings.notifications_paused = !self.settings.notifications_paused;
        }
        self.out.push(c);
    }

    /// The whole panel, filling `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        if self
            .fonts_pass
            .is_none_or(|p| p == ui.ctx().cumulative_pass_nr())
        {
            return;
        }
        let t = self.t;
        let v = self.view();
        let full = ui.max_rect();
        let p = ui.painter();
        // Translucent dark, like the app's window, with a hairline edge.
        p.rect(
            full.shrink(0.5),
            CornerRadius::same(radius::L),
            Color32::from_rgba_unmultiplied(t.bg.r(), t.bg.g(), t.bg.b(), 238),
            Stroke::new(1.0, t.glass_edge),
            egui::StrokeKind::Inside,
        );
        let inner = full.shrink(space::L);
        let mut cmds: Vec<PanelCmd> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing = vec2(space::S, space::S);
            header(ui, &t, &v, &mut cmds);
            ui.add_space(space::XS);
            tiles(ui, &t, &v);
            if v.battery.is_some() {
                line(ui, &t, "Network", &v.network);
            }
            ui.add_space(space::XS);
            let rest = (ui.available_height() - theme::hit::PRIMARY - space::M).max(60.0);
            egui::ScrollArea::vertical()
                .max_height(rest)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // What can be acted on first; the busiest apps under it.
                    ui.spacing_mut().item_spacing.y = space::XS;
                    review_list(ui, &t, &v, &mut cmds);
                    ui.add_space(space::S);
                    top_list(ui, &t, &v);
                });
            // The buttons, at the bottom.
            let bottom = Rect::from_min_max(
                pos2(inner.min.x, inner.max.y - theme::hit::PRIMARY),
                inner.max,
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(bottom)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    if kit::pill(ui, &t, QUICK_SWEEP, Kind::Primary, Some(Icon::Sweep)).clicked() {
                        cmds.push(PanelCmd::QuickSweep);
                    }
                    if kit::pill(ui, &t, OPEN_APP, Kind::Secondary, None).clicked() {
                        cmds.push(PanelCmd::Open);
                    }
                },
            );
        });
        for c in cmds {
            self.send(c);
        }
    }
}

/// The agent's waiting items, newest first (the order of its menu and of `Review(i)`).
fn waiting(env: &Env) -> Vec<Review> {
    AgentState::load(env).reviews.into_iter().rev().collect()
}

fn header(ui: &mut Ui, t: &Tokens, v: &PanelView, cmds: &mut Vec<PanelCmd>) {
    let (r, head) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let spoken = if v.paused {
        "CleanCraft, Paused"
    } else {
        "CleanCraft"
    };
    head.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, spoken));
    let p = ui.painter();
    let logo = Rect::from_center_size(pos2(r.min.x + 11.0, r.center().y), vec2(20.0, 20.0));
    icons::paint(p, logo, Icon::Logo, t.accent);
    let name = p.text(
        pos2(logo.max.x + space::S, r.center().y),
        Align2::LEFT_CENTER,
        "CleanCraft",
        font(size::BODY, Weight::SemiBold),
        t.text,
    );
    if v.paused {
        p.text(
            pos2(name.max.x + space::S, r.center().y),
            Align2::LEFT_CENTER,
            "Paused",
            font(size::FOOTNOTE, Weight::Medium),
            t.warning,
        );
    }
    // More: Pause and Quit.
    let more = Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(28.0, 28.0));
    let resp = ui.interact(more, ui.id().with("panel-more"), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, MORE));
    let f = kit::feel(ui, &resp);
    let p = ui.painter();
    if f.hov > 0.0 {
        p.circle_filled(more.center(), 14.0, t.glass_hover.gamma_multiply(f.hov));
    }
    for dx in [-6.0, 0.0, 6.0] {
        p.circle_filled(more.center() + vec2(dx, 0.0), 1.8, t.text_dim);
    }
    if f.focus {
        kit::focus_ring(p, t, more, 14.0);
    }
    let pause = v.pause_label();
    egui::Popup::menu(&resp)
        .id(egui::Id::new("panel-more-menu"))
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_min_width(220.0);
            if kit::menu_item(ui, pause).clicked() {
                cmds.push(PanelCmd::TogglePause);
            }
            ui.separator();
            if kit::menu_item(ui, schedule::QUIT).clicked() {
                cmds.push(PanelCmd::QuitAgent);
            }
        });
}

/// The 2 x 2 meters: CPU, Memory, Free space, and Battery (Network on a Mac without one).
fn tiles(ui: &mut Ui, t: &Tokens, v: &PanelView) {
    let w = (ui.available_width() - space::S) / 2.0;
    let net = Meter {
        value: v.network.clone(),
        detail: String::new(),
        fill: None,
        warn: false,
    };
    let fourth = match &v.battery {
        Some(b) => ("Battery", b),
        None => ("Network", &net),
    };
    ui.horizontal(|ui| {
        tile(ui, t, "CPU", &v.cpu, w);
        tile(ui, t, "Memory", &v.memory, w);
    });
    ui.horizontal(|ui| {
        tile(ui, t, "Free space", &v.disk, w);
        tile(ui, t, fourth.0, fourth.1, w);
    });
}

fn tile(ui: &mut Ui, t: &Tokens, label: &str, m: &Meter, w: f32) {
    let (r, resp) = ui.allocate_exact_size(vec2(w, TILE_H), Sense::hover());
    let spoken = if m.detail.is_empty() {
        format!("{label}: {}", m.value)
    } else {
        format!("{label}: {}, {}", m.value, m.detail)
    };
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
    let p = ui.painter();
    p.rect(
        r,
        CornerRadius::same(radius::M),
        t.glass,
        Stroke::new(1.0, t.glass_stroke),
        egui::StrokeKind::Inside,
    );
    // Label, value, bar, detail: one line each, so nothing overlaps at 340 pt.
    let x = r.min.x + space::M;
    let line_w = r.width() - space::M * 2.0;
    p.text(
        pos2(x, r.min.y + 8.0),
        Align2::LEFT_TOP,
        label.to_uppercase(),
        font(size::CAPTION, Weight::SemiBold),
        t.text_faint,
    );
    let vf = font(size::BODY, Weight::SemiBold);
    p.text(
        pos2(x, r.min.y + 22.0),
        Align2::LEFT_TOP,
        kit::truncate_end(ui.ctx(), &m.value, &vf, line_w),
        vf,
        if m.warn { t.warning } else { t.text },
    );
    if let Some(f) = m.fill {
        let bar = Rect::from_min_max(
            pos2(x, r.min.y + 45.0),
            pos2(r.max.x - space::M, r.min.y + 49.0),
        );
        let c = if m.warn { t.warning } else { t.accent };
        kit::paint_bar(p, bar, t, f, c);
    }
    if !m.detail.is_empty() {
        let df = font(size::CAPTION, Weight::Regular);
        p.text(
            pos2(x, r.min.y + 54.0),
            Align2::LEFT_TOP,
            kit::truncate_end(ui.ctx(), &m.detail, &df, line_w),
            df,
            t.text_dim,
        );
    }
}

/// "Network   Down 1.2 MB/s, up 30 KB/s".
fn line(ui: &mut Ui, t: &Tokens, label: &str, value: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("{label}: {value}")));
    let p = ui.painter();
    p.text(
        pos2(r.min.x + 2.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font(size::FOOTNOTE, Weight::Medium),
        t.text_faint,
    );
    p.text(
        pos2(r.max.x - 2.0, r.center().y),
        Align2::RIGHT_CENTER,
        value,
        font(size::FOOTNOTE, Weight::Medium),
        t.text_dim,
    );
}

fn top_list(ui: &mut Ui, t: &Tokens, v: &PanelView) {
    kit::eyebrow(ui, t, TOP_TITLE);
    if v.top.is_empty() {
        kit::dim(
            ui,
            t,
            if v.reading {
                "Reading the busiest processes."
            } else {
                "The process list could not be read."
            },
        );
        return;
    }
    for row in &v.top {
        let h = if row.what.is_some() { 36.0 } else { 22.0 };
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let spoken = match &row.what {
            Some(w) => format!("{}, {} CPU. {w}", row.name, row.cpu),
            None => format!("{}, {} CPU", row.name, row.cpu),
        };
        let resp = match &row.what {
            Some(w) => resp.on_hover_text(w.as_str()),
            None => resp,
        };
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
        let p = ui.painter();
        let cpu_f = font(size::CALLOUT, Weight::SemiBold);
        let cpu_w = kit::galley(ui, &row.cpu, cpu_f.clone(), t.text).size().x;
        let name_w = r.width() - cpu_w - space::M;
        let nf = font(size::CALLOUT, Weight::Medium);
        p.text(
            pos2(r.min.x, r.min.y + 2.0),
            Align2::LEFT_TOP,
            kit::truncate_end(ui.ctx(), &row.name, &nf, name_w),
            nf,
            t.text,
        );
        p.text(
            pos2(r.max.x, r.min.y + 2.0),
            Align2::RIGHT_TOP,
            &row.cpu,
            cpu_f,
            t.text,
        );
        if let Some(w) = &row.what {
            let wf = font(size::CAPTION, Weight::Regular);
            p.text(
                pos2(r.min.x, r.min.y + 19.0),
                Align2::LEFT_TOP,
                kit::truncate_end(ui.ctx(), w, &wf, r.width()),
                wf,
                t.text_faint,
            );
        }
    }
}

fn review_list(ui: &mut Ui, t: &Tokens, v: &PanelView, cmds: &mut Vec<PanelCmd>) {
    kit::eyebrow(ui, t, WAITING);
    if v.reviews.is_empty() {
        kit::dim(ui, t, NOTHING_WAITING);
        return;
    }
    for (i, text) in v.reviews.iter().enumerate() {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
        let f = kit::feel(ui, &resp);
        let p = ui.painter();
        p.rect_filled(
            r,
            CornerRadius::same(radius::S),
            icons::lerp(t.glass, t.glass_hover, f.hov),
        );
        p.circle_filled(pos2(r.min.x + 12.0, r.center().y), 3.0, t.accent);
        let tf = font(size::CALLOUT, Weight::Medium);
        p.text(
            pos2(r.min.x + 24.0, r.center().y),
            Align2::LEFT_CENTER,
            kit::truncate_end(ui.ctx(), text, &tf, r.width() - 48.0),
            tf,
            t.text,
        );
        let chev = Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(12.0, 12.0));
        icons::paint(p, chev, Icon::Chevron, t.text_faint);
        if f.focus {
            kit::focus_ring(p, t, r, f32::from(radius::S));
        }
        if resp.clicked() {
            cmds.push(PanelCmd::Review(i));
        }
    }
}
