//! The window: sidebar, the Full Disk Access banner, notices, and the current page.

use std::collections::BTreeMap;

use cleancraft_core::permissions::{self, Access};
use cleancraft_core::{Env, FeatureId, Settings};
use egui::{Color32, CornerRadius, RichText, Sense, Stroke, Ui, vec2};

use crate::icons::{self, Icon};
use crate::kit;
use crate::pages::{self, plan_page::PlanState};
use crate::theme::{self, Section, Tokens, Weight, font, icon as isz, motion, size, space};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Feature(FeatureId),
    History,
    Settings,
}

/// The sidebar's entries, in order: one per section, each opening one or more tools. Names
/// are CleanCraft's own. A section with several tools shows them as a switcher at the top of
/// its page.
pub const GROUPS: &[(&str, &[FeatureId])] = &[
    ("Quick Sweep", &[FeatureId::SmartCare]),
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
    ("Live Monitor", &[FeatureId::Monitor]),
];

/// The sidebar entry (index in [`GROUPS`]) a tool belongs to.
pub fn group_of(f: FeatureId) -> usize {
    GROUPS
        .iter()
        .position(|(_, fs)| fs.contains(&f))
        .unwrap_or(0)
}

/// The icon of a sidebar entry.
pub fn group_icon(i: usize) -> Icon {
    match i {
        0 => Icon::Sweep,
        1 => Icon::Junk,
        2 => Icon::Shield,
        3 => Icon::Tasks,
        4 => Icon::Remover,
        5 => Icon::Folder,
        _ => Icon::Monitor,
    }
}

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
    /// The tool each sidebar section showed last (by index in [`GROUPS`]), so a section
    /// opens where the person left it.
    pub section_tool: BTreeMap<usize, FeatureId>,
}

/// The window's top band where macOS draws the traffic lights (transparent title bar):
/// nothing clickable in the sidebar goes there.
const TITLE_BAND: f32 = 28.0;
/// Sidebar entry height.
const NAV_ROW: f32 = 44.0;

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
            section_tool: BTreeMap::new(),
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
        crate::sound::play(&self.env, crate::sound::Cue::Error);
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
        } else {
            crate::sound::apply(&self.env, &self.settings);
        }
    }

    /// Per-frame work that does not draw: finished jobs.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !self.fonts_installed {
            // macOS "Reduce motion" (Accessibility > Display), read once through env.sys.
            // Tests run with `sync` and keep motion on without asking.
            if !self.sync {
                crate::motion::detect_reduce_motion(self.env.sys.as_ref());
            }
            theme::install_fonts(ctx);
            theme::apply(ctx, &self.t);
            self.fonts_installed = true;
            self.fonts_pass = ctx.cumulative_pass_nr();
            ctx.request_repaint();
        }
        crate::images::ensure(ctx, &self.env, self.sync);
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
        if let Page::Feature(f) = self.page {
            self.section_tool.insert(group_of(f), f);
        }
        let mut content = full;
        content.min.x += space::SIDEBAR;
        kit::backdrop(ui.painter(), content, &t);
        egui::Panel::left("sidebar")
            .show_separator_line(false)
            .resizable(false)
            .exact_size(space::SIDEBAR)
            .frame(
                egui::Frame::new()
                    .fill(t.sidebar)
                    // No top margin: the sidebar's first 28 pt are the title band (traffic
                    // lights and the logo), see sidebar().
                    .inner_margin(egui::Margin {
                        left: space::SIDEBAR_X,
                        right: space::SIDEBAR_X,
                        top: 0,
                        bottom: 8,
                    }),
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
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(space::PAGE_X, space::PAGE_Y)),
            )
            .show(ui, |ui| {
                // A section with several tools: its switcher, the same in every phase (it
                // stands in for the compact page header).
                let multi =
                    matches!(self.page, Page::Feature(f) if GROUPS[group_of(f)].1.len() > 1);
                kit::set_switcher_shown(ui.ctx(), multi);
                if let Page::Feature(f) = self.page {
                    let feats = GROUPS[group_of(f)].1;
                    if feats.len() > 1 {
                        let items: Vec<(Icon, &str)> = feats
                            .iter()
                            .map(|x| (kit::feature_look(&t, *x).0, x.title()))
                            .collect();
                        let sel = feats.iter().position(|x| *x == f).unwrap_or(0);
                        if let Some(i) = kit::segmented(ui, &t, &items, sel) {
                            self.act();
                            if feats[i] != f {
                                self.page = Page::Feature(feats[i]);
                            }
                        }
                        ui.add_space(space::L);
                    }
                }
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
        let side = ui
            .max_rect()
            .expand2(vec2(space::SIDEBAR_X as f32, space::SIDEBAR_Y as f32));
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
        // The traffic lights' band, then the logo under it.
        ui.add_space(TITLE_BAND + space::XS);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(30.0), Sense::hover());
            let (a, b) = Section::Overview.colors();
            icons::gradient_tile(ui.painter(), r, 8.5, a, b);
            icons::paint(ui.painter(), r.shrink(5.0), Icon::Logo, theme::on_color(a));
            ui.add_space(6.0);
            ui.label(
                RichText::new("CleanCraft")
                    .font(font(size::TITLE2, Weight::Bold))
                    .color(t.text),
            );
        });
        ui.add_space(space::L);
        let current = match self.page {
            Page::Feature(f) => Some(group_of(f)),
            _ => None,
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::XS;
                for (i, (name, feats)) in GROUPS.iter().enumerate() {
                    let color = Section::of(feats[0]).colors().0;
                    if self.nav_row(ui, group_icon(i), color, name, current == Some(i)) {
                        self.act();
                        let f = self.section_tool.get(&i).copied().unwrap_or(feats[0]);
                        self.page = Page::Feature(f);
                    }
                }
                ui.add_space(space::S);
                let y = ui.cursor().min.y;
                ui.painter().hline(
                    ui.max_rect().x_range().shrink(6.0),
                    y,
                    Stroke::new(1.0, Color32::from_white_alpha(14)),
                );
                ui.add_space(space::S);
                let neutral = Section::Neutral.colors().0;
                if self.nav_row(
                    ui,
                    Icon::History,
                    neutral,
                    "History",
                    self.page == Page::History,
                ) {
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

    /// One sidebar entry: a 28 pt icon tile in the section's colour and a 15 pt label in a
    /// 44 pt row; the selected entry sits on a soft pill.
    fn nav_row(&self, ui: &mut Ui, icon: Icon, color: Color32, label: &str, active: bool) -> bool {
        let t = self.t;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), NAV_ROW), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label)
        });
        let hov = kit::anim(ui.ctx(), resp.id.with("hov"), resp.hovered(), motion::HOVER);
        let on = kit::anim(ui.ctx(), resp.id.with("on"), active, motion::PAGE);
        let p = ui.painter();
        let round = CornerRadius::same(theme::radius::M);
        if on > 0.0 {
            p.rect_filled(
                r,
                round,
                icons::lerp(
                    Color32::from_white_alpha(0),
                    icons::lerp(Color32::from_white_alpha(14), color, 0.14),
                    on,
                ),
            );
            p.rect_stroke(
                r,
                round,
                Stroke::new(1.0, Color32::from_white_alpha((16.0 * on) as u8)),
                egui::StrokeKind::Inside,
            );
        }
        if hov > 0.0 && !active {
            p.rect_filled(r, round, Color32::from_white_alpha((9.0 * hov) as u8));
        }
        let tile = egui::Rect::from_center_size(
            r.left_center() + vec2(8.0 + 14.0, 0.0),
            egui::Vec2::splat(28.0),
        );
        let mut q = p.clone();
        if !active {
            q.set_opacity(0.88 + 0.12 * hov);
        }
        kit::icon_tile(&q, tile, icon, color);
        p.text(
            egui::pos2(tile.right() + 12.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            // One weight in both states, so the label does not jump when selected.
            font(size::BODY, Weight::Medium),
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
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(isz::SIDEBAR), Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Warning, t.warning);
                        ui.label(
                            RichText::new(
                                "CleanCraft cannot see Mail, Safari and Trash data. Turn on Full Disk Access for CleanCraft to include them.",
                            )
                            .font(font(size::CALLOUT, Weight::Regular))
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
                .fill(icons::lerp(
                    Color32::from_rgb(0x1A, 0x1F, 0x29),
                    color,
                    0.14,
                ))
                .stroke(Stroke::new(1.0, color.gamma_multiply(0.4)))
                .corner_radius(CornerRadius::same(theme::radius::M))
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(isz::SIDEBAR), Sense::hover());
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
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&msg)
                                                .font(font(size::CALLOUT, Weight::Regular))
                                                .color(t.text),
                                        )
                                        .wrap(),
                                    );
                                },
                            );
                        });
                    });
                });
            ui.add_space(8.0);
        }
    }
}
