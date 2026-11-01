//! Security Check (a tool of the Protect section): five everyday macOS security settings,
//! read only, each with one card and a button that opens the right System Settings pane. See
//! `cleancraft_core::security` for the facts and rules.

use std::time::{Duration, Instant};

use cleancraft_core::security::{self, Card, Facts, State as Verdict};
use egui::{Color32, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

/// Facts older than this are read again when the page opens.
pub const FRESH_FOR: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct State {
    pub facts: Option<Facts>,
    pub cards: Vec<Card>,
    job: Option<Job<Facts>>,
    pub checked: Option<Instant>,
    pub rules_open: bool,
}

impl State {
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Show these facts (tests, renders).
    pub fn set(&mut self, f: Facts) {
        self.cards = security::recommendations(&f);
        self.facts = Some(f);
        self.checked = Some(Instant::now());
    }
}

/// Read every fact again (in the background).
pub fn refresh(app: &mut CleanApp) {
    if app.security.job.is_some() {
        return;
    }
    let env = app.env.clone();
    app.security.job = Some(Job::start(app.sync, move |_| {
        let now = chrono::Utc::now().timestamp();
        security::gather(&env, now)
    }));
}

/// Read again when nothing is shown yet or what is shown is older than [`FRESH_FOR`].
pub fn refresh_if_old(app: &mut CleanApp) {
    if app
        .security
        .checked
        .is_none_or(|c| c.elapsed() >= FRESH_FOR)
    {
        refresh(app);
    }
}

pub fn poll(app: &mut CleanApp) -> bool {
    let Some(j) = app.security.job.as_mut() else {
        return false;
    };
    if let Some(f) = j.poll() {
        app.security.job = None;
        app.security.set(f);
        false
    } else if j.is_dead() {
        app.security.job = None;
        app.warn("Reading the Mac's security settings stopped unexpectedly.");
        false
    } else {
        true
    }
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if app.security.facts.is_none() {
        if app.security.job.is_none() {
            refresh(app);
        }
        let (status, meter) = app
            .security
            .job
            .as_ref()
            .map(|j| (j.status(), j.meter.clone()))
            .unwrap_or_default();
        kit::progress(
            ui,
            &t,
            cleancraft_core::FeatureId::Malware,
            "Reading security settings...",
            &status,
            &meter,
            false,
        );
        return;
    }
    let out = egui::ScrollArea::vertical()
        .id_salt("security")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            summary(app, ui);
            ui.add_space(space::M);
            recs(app, ui);
            ui.add_space(space::M);
            facts_card(app, ui);
            ui.add_space(space::XL);
        });
    kit::scroll_fade_page(ui, &out, &t);
}

fn summary(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let (verdict, why) = security::summary(&app.security.cards);
    let color = match verdict {
        Verdict::Good => t.success,
        Verdict::NeedsAttention => t.warning,
    };
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            kit::badge(
                ui,
                color,
                if verdict == Verdict::Good {
                    Icon::Check
                } else {
                    Icon::Warning
                },
                44.0,
            );
            ui.add_space(space::S);
            let text_w = (ui.available_width() - 170.0).max(200.0);
            ui.allocate_ui_with_layout(
                vec2(text_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(text_w);
                    ui.label(
                        crate::rich(verdict.label())
                            .font(font(size::TITLE, Weight::Bold))
                            .color(t.text),
                    );
                    if why.is_empty() {
                        kit::dim(ui, &t, "Every setting Security Check reads is in its safe state.");
                    } else {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        kit::dim(
                            ui,
                            &t,
                            &format!(
                                "{} need attention:",
                                cleancraft_core::fsutil::count(why.len(), "thing", "things")
                            ),
                        );
                        for w in &why {
                            ui.add(
                                egui::Label::new(
                                    crate::rich(format!("\u{2022}  {w}"))
                                        .font(font(size::CALLOUT, Weight::Regular))
                                        .color(t.text_dim),
                                )
                                .wrap(),
                            );
                        }
                    }
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.security.is_busy() {
                    kit::dim(ui, &t, "Checking...");
                } else if kit::secondary(ui, &t, "Check Again") {
                    app.act();
                    refresh(app);
                }
            });
        });
        ui.add_space(space::XS);
        let label = if app.security.rules_open {
            "Hide how this is decided"
        } else {
            "How this is decided"
        };
        if kit::ghost(ui, &t, label, Some(Icon::Chevron)).clicked() {
            app.act();
            app.security.rules_open = !app.security.rules_open;
        }
        if app.security.rules_open {
            ui.add_space(space::XS);
            kit::dim(
                ui,
                &t,
                "There is no score. The state is Needs attention when one of these applies; a setting that could not be read is listed under Facts as Unknown and never counted either way.",
            );
            ui.add_space(space::XS);
            for r in security::RULES {
                ui.add(
                    egui::Label::new(
                        crate::rich(format!("\u{2022}  {}", r.text))
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            }
        }
    });
}

fn recs(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let cards = app.security.cards.clone();
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        kit::card_head(ui, &t, Icon::Check, t.sec_a, "Recommendations");
        ui.add_space(space::S);
        if cards.is_empty() {
            kit::dim(
                ui,
                &t,
                "Nothing to do right now. Security Check checks again each time you open it.",
            );
            return;
        }
        for (i, c) in cards.iter().enumerate() {
            if i > 0 {
                let y = ui.cursor().min.y + space::XS;
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    egui::Stroke::new(1.0, Color32::from_white_alpha(12)),
                );
                ui.add_space(space::S + 1.0);
            }
            rec_row(app, ui, c, i);
        }
    });
}

fn rec_row(app: &mut CleanApp, ui: &mut Ui, c: &Card, i: usize) {
    let t = app.t;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(4.0, 36.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, t.warning);
        ui.add_space(space::S);
        let btn_w = kit::pill_size(ui, &c.action, kit::Kind::Secondary, false).x;
        let text_w = (ui.available_width() - btn_w - space::M).max(160.0);
        ui.allocate_ui_with_layout(
            vec2(text_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_max_width(text_w);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    crate::rich(&c.headline)
                        .font(font(size::BODY, Weight::SemiBold))
                        .color(t.text),
                );
                ui.add(
                    egui::Label::new(
                        crate::rich(&c.detail)
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.push_id(("sec_rec", i, c.rule), |ui| {
                if kit::secondary(ui, &t, &c.action) {
                    app.act();
                    // Opens System Settings; the switch itself is left to the person.
                    app.open_route(&c.route);
                }
            });
        });
    });
}

fn facts_card(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(f) = app.security.facts.clone() else {
        return;
    };
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        kit::card_head(ui, &t, Icon::Monitor, t.sec_a, "Facts");
        ui.add_space(space::S);
        let w = ui.available_width();
        for (k, v) in security::fact_rows(&f) {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                let lw = (w * 0.45).min(230.0);
                ui.allocate_ui_with_layout(
                    vec2(lw, 20.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(lw);
                        ui.label(
                            crate::rich(k)
                                .font(font(size::CALLOUT, Weight::Regular))
                                .color(t.text_dim),
                        );
                    },
                );
                ui.add(
                    egui::Label::new(
                        crate::rich(v)
                            .font(font(size::CALLOUT, Weight::Medium))
                            .color(t.text),
                    )
                    .wrap(),
                );
            });
        }
        if !f.problems.is_empty() {
            ui.add_space(space::XS);
            kit::dim(ui, &t, &format!("Not read: {}", f.problems.join("; ")));
        }
    });
}
