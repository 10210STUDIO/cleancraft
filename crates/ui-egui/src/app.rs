//! The window: sidebar, the Full Disk Access banner, notices, and the current page.

use std::collections::BTreeMap;

use cleancraft_core::permissions::{self, Access};
use cleancraft_core::{Env, FeatureId, Settings};
use egui::{Color32, CornerRadius, RichText, Sense, Stroke, Ui, vec2};

use crate::icons::{self, Icon};
use crate::kit;
use crate::pages::{self, plan_page::PlanState};
use crate::theme::{self, Section, Tokens, Weight, font, motion, space};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Feature(FeatureId),
    History,
    Settings,
}

/// Sidebar groups, in order. Names are CleanCraft's own.
pub const GROUPS: &[(&str, &[FeatureId])] = &[
    ("", &[FeatureId::SmartCare]),
    (
        "Clean",
        &[
            FeatureId::SystemJunk,
            FeatureId::MailAttachments,
            FeatureId::TrashBins,
        ],
    ),
    ("Protect", &[FeatureId::Malware, FeatureId::Privacy]),
    ("Speed", &[FeatureId::Maintenance, FeatureId::LoginItems]),
    ("Apps", &[FeatureId::Uninstaller, FeatureId::Updater]),
    (
        "Files",
        &[
            FeatureId::LargeOldFiles,
            FeatureId::Duplicates,
            FeatureId::SimilarPhotos,
            FeatureId::SpaceLens,
        ],
    ),
    ("System", &[FeatureId::Monitor]),
];

pub struct CleanApp {
    pub env: Env,
    pub settings: Settings,
    pub page: Page,
    pub t: Tokens,
    /// Run work on the UI thread (tests).
    pub sync: bool,
    pub plans: BTreeMap<FeatureId, PlanState>,
    pub history: pages::history::State,
    pub settings_page: pages::settings::State,
    pub sweep: pages::sweep::State,
    pub apps: pages::apps::State,
    pub startup: pages::startup::State,
    pub tasks: pages::tasks::State,
    pub disk_map: pages::disk_map::State,
    pub monitor: pages::monitor::State,
    pub fda: Access,
    /// A message at the top of the page: (text, is_error).
    pub notice: Option<(String, bool)>,
    actions: u64,
    fonts_installed: bool,
    fonts_pass: u64,
    /// The page the transition animation is running for, and when it started.
    shown_page: Page,
    page_since: f64,
}

impl CleanApp {
    pub fn new(env: Env) -> CleanApp {
        let (settings, settings_warning) = Settings::load_checked(&env);
        let fda = permissions::full_disk_access(&env);
        CleanApp {
            env,
            settings,
            page: Page::Feature(FeatureId::SmartCare),
            t: Tokens::dark(),
            sync: false,
            plans: BTreeMap::new(),
            history: Default::default(),
            settings_page: Default::default(),
            sweep: Default::default(),
            apps: Default::default(),
            startup: Default::default(),
            tasks: Default::default(),
            disk_map: Default::default(),
            monitor: Default::default(),
            fda,
            notice: settings_warning.map(|m| (m, true)),
            actions: 0,
            fonts_installed: false,
            fonts_pass: 0,
            shown_page: Page::Feature(FeatureId::SmartCare),
            page_since: f64::NEG_INFINITY,
        }
    }

    /// Work runs inline (for tests).
    pub fn with_sync(mut self) -> CleanApp {
        self.sync = true;
        self
    }

    /// Count a handled user action. The every-control test uses this to find dead clicks.
    pub fn act(&mut self) {
        self.actions += 1;
    }

    pub fn action_count(&self) -> u64 {
        self.actions
    }

    pub fn tell(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), false));
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), true));
    }

    /// Go to a page by name: a feature id ("system_junk"), "history" or "settings".
    pub fn navigate(&mut self, name: &str) {
        self.page = match name {
            "history" => Page::History,
            "settings" => Page::Settings,
            other => match FeatureId::parse(other) {
                Some(f) => Page::Feature(f),
                None => return,
            },
        };
        if self.page == Page::History {
            self.history.reload(&self.env);
        }
    }

    pub fn plan_state(&mut self, f: FeatureId) -> &mut PlanState {
        self.plans.entry(f).or_default()
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.env) {
            self.warn(format!("Settings could not be saved: {e}"));
        }
    }

    /// Per-frame work that does not draw: finished jobs.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !self.fonts_installed {
            theme::install_fonts(ctx);
            theme::apply(ctx, &self.t);
            self.fonts_installed = true;
            self.fonts_pass = ctx.cumulative_pass_nr();
            ctx.request_repaint();
        }
        let mut busy = false;
        let ids: Vec<FeatureId> = self.plans.keys().copied().collect();
        for f in ids {
            busy |= pages::plan_page::poll(self, f);
        }
        busy |= pages::sweep::poll(self);
        busy |= pages::apps::poll(self);
        busy |= pages::startup::poll(self);
        busy |= pages::tasks::poll(self);
        busy |= pages::disk_map::poll(self);
        busy |= pages::monitor::poll(self);
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The section of the page on screen.
    pub fn section(&self) -> Section {
        match self.page {
            Page::Feature(f) => Section::of(f),
            Page::History | Page::Settings => Section::Neutral,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.t = self.t.for_section(self.section());
        let t = self.t;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.bg);
        // Inter is registered in logic() but only usable from the next pass.
        if !self.fonts_installed || ui.ctx().cumulative_pass_nr() <= self.fonts_pass {
            ui.ctx().request_repaint();
            return;
        }
        let mut content = full;
        content.min.x += space::SIDEBAR;
        kit::backdrop(ui.painter(), content, &t);
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(space::SIDEBAR)
            .frame(
                egui::Frame::new()
                    .fill(t.sidebar)
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ui, |ui| self.sidebar(ui));
        // Page change: fade in with a short slide up.
        let now = ui.input(|i| i.time);
        if self.shown_page != self.page {
            self.shown_page = self.page;
            self.page_since = now;
        }
        let k = motion::ease_out(motion::progress(now, self.page_since, motion::PAGE));
        if k < 1.0 {
            ui.ctx().request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(
                space::PAGE_X,
                space::PAGE_Y,
            )))
            .show(ui, |ui| {
                self.banners(ui);
                ui.multiply_opacity(0.25 + 0.75 * k);
                let shift = egui::emath::TSTransform::from_translation(vec2(
                    0.0,
                    (1.0 - k) * motion::PAGE_SLIDE,
                ));
                ui.with_visual_transform(shift, |ui| match self.page {
                    Page::Feature(f) => pages::feature_page(self, ui, f),
                    Page::History => pages::history::ui(self, ui),
                    Page::Settings => pages::settings::ui(self, ui),
                });
            });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t;
        let side = ui.max_rect().expand2(vec2(12.0, 16.0));
        icons::vertical_gradient(
            ui.painter(),
            side,
            Color32::from_rgb(0x12, 0x16, 0x1F),
            t.sidebar,
        );
        ui.painter().vline(
            side.right() - 0.5,
            side.y_range(),
            Stroke::new(1.0, Color32::from_white_alpha(10)),
        );
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(30.0), Sense::hover());
            let (a, b) = Section::Overview.colors();
            icons::gradient_tile(ui.painter(), r, 9.0, a, b);
            icons::paint(ui.painter(), r.shrink(5.0), Icon::Logo, theme::on_color(a));
            ui.add_space(2.0);
            ui.label(
                RichText::new("CleanCraft")
                    .font(font(17.0, Weight::Bold))
                    .color(t.text),
            );
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Compact rows: every entry fits a 680 pt tall window without scrolling.
                ui.spacing_mut().item_spacing.y = 2.0;
                for (group, feats) in GROUPS {
                    if !group.is_empty() {
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(group.to_uppercase())
                                    .font(font(10.0, Weight::SemiBold))
                                    .color(t.text_faint),
                            );
                        });
                    }
                    for f in *feats {
                        let (icon, color) = kit::feature_look(&t, *f);
                        if self.nav_row(ui, icon, color, f.title(), self.page == Page::Feature(*f))
                        {
                            self.act();
                            self.page = Page::Feature(*f);
                        }
                    }
                }
                ui.add_space(8.0);
                let y = ui.cursor().min.y;
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    Stroke::new(1.0, Color32::from_white_alpha(14)),
                );
                ui.add_space(8.0);
                let neutral = Section::Neutral.colors().0;
                if self.nav_row(ui, Icon::History, neutral, "History", self.page == Page::History) {
                    self.act();
                    self.navigate("history");
                }
                if self.nav_row(
                    ui,
                    Icon::Settings,
                    neutral,
                    "Settings",
                    self.page == Page::Settings,
                ) {
                    self.act();
                    self.navigate("settings");
                }
            });
    }

    fn nav_row(&self, ui: &mut Ui, icon: Icon, color: Color32, label: &str, active: bool) -> bool {
        let t = self.t;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label)
        });
        let hov = kit::anim(ui.ctx(), resp.id.with("hov"), resp.hovered(), motion::HOVER);
        let on = kit::anim(ui.ctx(), resp.id.with("on"), active, motion::PAGE);
        let p = ui.painter();
        if on > 0.0 {
            p.rect_filled(
                r,
                CornerRadius::same(9),
                icons::lerp(Color32::TRANSPARENT, color.gamma_multiply(0.16), on),
            );
            let bar = egui::Rect::from_center_size(
                r.left_center() + vec2(1.5, 0.0),
                vec2(3.0, 14.0 * on),
            );
            p.rect_filled(bar, CornerRadius::same(2), color);
        }
        if hov > 0.0 && !active {
            p.rect_filled(
                r,
                CornerRadius::same(9),
                Color32::from_white_alpha((10.0 * hov) as u8),
            );
        }
        let ir = egui::Rect::from_center_size(
            r.left_center() + vec2(18.0, 0.0),
            egui::Vec2::splat(16.0),
        );
        let icon_color = if active {
            icons::lerp(color, Color32::WHITE, 0.2)
        } else {
            icons::lerp(color.gamma_multiply(0.85), color, hov)
        };
        icons::paint_with(p, ir, icon, icon_color, 1.45);
        p.text(
            r.left_center() + vec2(36.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            font(
                13.0,
                if active {
                    Weight::SemiBold
                } else {
                    Weight::Medium
                },
            ),
            if active {
                t.text
            } else {
                icons::lerp(t.text_dim, t.text, hov)
            },
        );
        resp.clicked()
    }

    fn banners(&mut self, ui: &mut Ui) {
        let t = self.t;
        if self.fda == Access::Missing {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(0x3A, 0x2C, 0x12, 200))
                .stroke(Stroke::new(1.0, t.warning.gamma_multiply(0.45)))
                .corner_radius(CornerRadius::same(theme::radius::M))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(18.0), Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Warning, t.warning);
                        ui.label(
                            RichText::new(
                                "CleanCraft cannot see Mail, Safari and Trash data. Turn on Full Disk Access for CleanCraft to include them.",
                            )
                            .font(font(12.5, Weight::Regular))
                            .color(t.text),
                        );
                        if kit::secondary(ui, &t, "Open Privacy Settings") {
                            self.act();
                            if let Err(e) = permissions::open_fda_settings(&self.env) {
                                self.warn(format!("Could not open System Settings: {e}"));
                            }
                        }
                        if kit::secondary(ui, &t, "Check Again") {
                            self.act();
                            self.fda = permissions::full_disk_access(&self.env);
                            if self.fda != Access::Missing {
                                self.tell("Full Disk Access is on.");
                            } else {
                                self.warn("Full Disk Access is still off.");
                            }
                        }
                    });
                });
            ui.add_space(8.0);
        }
        if let Some((msg, err)) = self.notice.clone() {
            let color = if err { t.danger } else { t.success };
            egui::Frame::new()
                .fill(icons::lerp(Color32::from_rgb(0x1A, 0x1F, 0x29), color, 0.14))
                .stroke(Stroke::new(1.0, color.gamma_multiply(0.4)))
                .corner_radius(CornerRadius::same(theme::radius::M))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(18.0), Sense::hover());
                        icons::paint(
                            ui.painter(),
                            r,
                            if err { Icon::Warning } else { Icon::Check },
                            color,
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::ghost(ui, &t, "Dismiss", None).clicked() {
                                self.act();
                                self.notice = None;
                            }
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&msg).font(font(12.5, Weight::Regular)).color(t.text),
                                    )
                                    .wrap(),
                                );
                            });
                        });
                    });
                });
            ui.add_space(8.0);
        }
    }
}
