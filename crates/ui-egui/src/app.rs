//! The window: sidebar, the Full Disk Access banner, notices, and the current page.

use std::collections::BTreeMap;

use cleancraft_core::permissions::{self, Access};
use cleancraft_core::{Env, FeatureId, Settings};
use egui::{Color32, CornerRadius, RichText, Sense, Stroke, Ui, vec2};

use crate::kit;
use crate::pages::{self, plan_page::PlanState};
use crate::theme::{self, Tokens, Weight, font};

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

    pub fn ui(&mut self, ui: &mut Ui) {
        let t = self.t;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.bg);
        // Inter is registered in logic() but only usable from the next pass.
        if !self.fonts_installed || ui.ctx().cumulative_pass_nr() <= self.fonts_pass {
            ui.ctx().request_repaint();
            return;
        }
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(232.0)
            .frame(
                egui::Frame::new()
                    .fill(t.sidebar)
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.bg)
                    .inner_margin(egui::Margin::symmetric(28, 22)),
            )
            .show(ui, |ui| {
                self.banners(ui);
                match self.page {
                    Page::Feature(f) => pages::feature_page(self, ui, f),
                    Page::History => pages::history::ui(self, ui),
                    Page::Settings => pages::settings::ui(self, ui),
                }
            });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t;
        ui.horizontal(|ui| {
            kit::badge(ui, t.accent, "C", 30.0);
            ui.label(
                RichText::new("CleanCraft")
                    .font(font(18.0, Weight::Bold))
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
                        ui.label(
                            RichText::new(group.to_uppercase())
                                .font(font(10.5, Weight::SemiBold))
                                .color(t.text_faint),
                        );
                    }
                    for f in *feats {
                        let (glyph, color) = kit::feature_look(&t, *f);
                        if self.nav_row(ui, glyph, color, f.title(), self.page == Page::Feature(*f))
                        {
                            self.act();
                            self.page = Page::Feature(*f);
                        }
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                if self.nav_row(ui, "☰", t.text_dim, "History", self.page == Page::History) {
                    self.act();
                    self.navigate("history");
                }
                if self.nav_row(ui, "⚙", t.text_dim, "Settings", self.page == Page::Settings) {
                    self.act();
                    self.navigate("settings");
                }
            });
    }

    fn nav_row(&self, ui: &mut Ui, glyph: &str, color: Color32, label: &str, active: bool) -> bool {
        let t = self.t;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label)
        });
        let p = ui.painter();
        if active {
            p.rect_filled(r, CornerRadius::same(8), t.accent_dim);
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(8), t.card);
        }
        p.text(
            r.left_center() + vec2(14.0, 0.0),
            egui::Align2::CENTER_CENTER,
            glyph,
            font(14.0, Weight::Bold),
            color,
        );
        p.text(
            r.left_center() + vec2(32.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            font(
                13.5,
                if active {
                    Weight::SemiBold
                } else {
                    Weight::Regular
                },
            ),
            if active { t.text } else { t.text_dim },
        );
        resp.clicked()
    }

    fn banners(&mut self, ui: &mut Ui) {
        let t = self.t;
        if self.fda == Access::Missing {
            egui::Frame::new()
                .fill(t.warning.gamma_multiply(0.15))
                .stroke(Stroke::new(1.0, t.warning))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(
                                "CleanCraft cannot see Mail, Safari and Trash data. Turn on Full Disk Access for CleanCraft to include them.",
                            )
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
            let color = if err { t.danger } else { t.accent };
            egui::Frame::new()
                .fill(color.gamma_multiply(0.15))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&msg).color(t.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Dismiss").clicked() {
                                self.act();
                                self.notice = None;
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }
    }
}
