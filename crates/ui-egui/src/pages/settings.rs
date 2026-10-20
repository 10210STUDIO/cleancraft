//! Settings: where removed items go, the ignore list, thresholds, agents, menu bar.

use cleancraft_core::executor::Method;
use egui::{RichText, Sense, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::kit;
use crate::theme::{Weight, font, size, space};

/// A card's heading with its icon.
fn heading(ui: &mut Ui, t: &crate::theme::Tokens, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        kit::badge(ui, t.sec_a, icon, 28.0);
        ui.add_space(2.0);
        ui.label(
            RichText::new(text)
                .font(font(size::TITLE3, Weight::SemiBold))
                .color(t.text),
        );
    });
    ui.add_space(2.0);
}

/// A form row: the label in a leading column, the slider and its value with the unit after
/// it. The slider keeps `name` as its accessible name.
/// A form row: label in the right-aligned label column, a switch after it, and `text`
/// beside the switch. Returns true when clicked. The switch is announced as `text`.
fn switch_row(ui: &mut Ui, t: &crate::theme::Tokens, on: bool, text: &str, label: &str) -> bool {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
        ui.painter().text(
            r.right_center() - vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            font(size::BODY, Weight::Regular),
            t.text_dim,
        );
        let c = kit::switch(ui, t, on, text).clicked();
        // Painted: the switch already carries the words for VoiceOver.
        kit::heading(ui, text, font(size::CALLOUT, Weight::Medium), t.text);
        c
    })
    .inner
}

fn slider_row(
    ui: &mut Ui,
    t: &crate::theme::Tokens,
    name: &str,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    unit: &str,
) -> bool {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
        ui.painter().text(
            r.right_center() - vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            font(size::BODY, Weight::Regular),
            t.text_dim,
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 140.0).clamp(160.0, 360.0);
        let r = kit::slider(
            ui,
            egui::Slider::new(value, range)
                .suffix(unit)
                .trailing_fill(true),
        );
        let v = *value as f64;
        let name = name.to_string();
        r.widget_info(|| egui::WidgetInfo::slider(true, v, name.clone()));
        r.changed()
    })
    .inner
}

#[derive(Default)]
pub struct State {
    /// Text box for adding an ignore-list path.
    pub new_ignore: String,
    /// The macOS folder dialog, while it is open.
    pick: Option<crate::jobs::Job<Result<Option<std::path::PathBuf>, String>>>,
}

/// macOS's own folder dialog (through osascript and env.sys): the folder chosen, or None when
/// the person cancels.
pub fn choose_folder(
    env: &cleancraft_core::Env,
    prompt: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let script = format!(
        "POSIX path of (choose folder with prompt \"{}\")",
        prompt.replace(['\\', '"'], "")
    );
    let out = env
        .sys
        .run("/usr/bin/osascript", &["-e", &script])
        .map_err(|e| format!("The folder dialog could not open: {e}"))?;
    if out.status != 0 {
        // -128: the person chose Cancel.
        if out.stderr.contains("-128") {
            return Ok(None);
        }
        return Err(format!(
            "The folder dialog could not open: {}",
            out.stderr.trim()
        ));
    }
    let path = out.stdout.trim().trim_end_matches('/');
    if path.is_empty() {
        return Ok(None);
    }
    Ok(Some(env.sys_path(path)))
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    kit::header(
        ui,
        &t,
        Icon::Settings,
        "Settings",
        "Where removed items go, what scans leave alone, how big and old files are judged, and sounds. Changes are saved as you make them.",
    );
    let mut changed = false;
    // The folder dialog answered.
    if let Some(job) = app.settings_page.pick.as_mut() {
        if let Some(r) = job.poll() {
            app.settings_page.pick = None;
            match r {
                Ok(Some(p)) if !app.settings.ignore.contains(&p) => {
                    app.settings.ignore.push(p);
                    changed = true;
                }
                Ok(_) => {}
                Err(e) => app.warn(e),
            }
        } else if job.is_dead() {
            app.settings_page.pick = None;
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
    let out = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Trash, "Removed items go to");
                let mut m = app.settings.method;
                for (value, text) in [
                    (Method::Trash, "The Trash (empty it yourself when you are sure)"),
                    (
                        Method::Quarantine,
                        "CleanCraft quarantine (~/Library/Application Support/CleanCraft/Quarantine)",
                    ),
                ] {
                    if kit::radio(ui, &t, m == value, text).clicked() {
                        m = value;
                    }
                }
                if m != app.settings.method {
                    app.settings.method = m;
                    changed = true;
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Shield, "Ignore list");
                kit::dim(
                    ui,
                    &t,
                    "Scans never propose these paths or anything inside them.",
                );
                ui.add_space(space::XS);
                let list = app.settings.ignore.clone();
                if list.is_empty() {
                    ui.label(
                        RichText::new("Nothing is ignored yet. Add a folder or file below, or choose Ignore on an app in App Remover.")
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_faint),
                    );
                }
                for (i, p) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.set_min_height(crate::theme::hit::FIELD);
                        let shown = app.env.display(p);
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Folder, t.text_dim);
                        // The path, shortened in the middle so the Remove button stays in
                        // reach; the whole path in the tooltip and for VoiceOver.
                        let fid = font(size::CALLOUT, Weight::Regular);
                        let room = (ui.available_width() - 120.0).max(80.0);
                        let short = kit::truncate_middle(ui.ctx(), &shown, &fid, room);
                        let w = kit::galley(ui, &short, fid.clone(), t.text).size().x + 2.0;
                        let (pr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
                        kit::text_at(ui, pr, &short, &shown, fid, t.text, egui::Align::Min)
                            .on_hover_text(&shown);
                        let r = kit::ghost(ui, &t, "Remove", Some(Icon::Close));
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Remove {shown}"),
                            )
                        });
                        if r.clicked() {
                            app.settings.ignore.remove(i);
                            changed = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    let r = kit::text_field(
                        ui,
                        &t,
                        &mut app.settings_page.new_ignore,
                        "Path to ignore",
                        380.0,
                    );
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            true,
                            "Path to ignore",
                        )
                    });
                    if r.changed() {
                        app.act();
                    }
                    let enter = r.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !app.settings_page.new_ignore.trim().is_empty();
                    if kit::secondary(ui, &t, "Add to Ignore List") || enter {
                        let raw = app.settings_page.new_ignore.trim().to_string();
                        app.act();
                        if raw.is_empty() {
                            app.warn("Type a path first.");
                        } else {
                            let p = if raw == "~" {
                                app.env.home().to_path_buf()
                            } else if let Some(rest) = raw.strip_prefix("~/") {
                                app.env.home_path(rest)
                            } else {
                                app.env.sys_path(&raw)
                            };
                            if std::fs::symlink_metadata(&p).is_err() {
                                app.warn(format!(
                                    "Nothing is at {raw}. Check the path, or use Choose Folder."
                                ));
                            } else if app.settings.ignore.contains(&p) {
                                app.warn(format!("{raw} is on the list already."));
                            } else {
                                app.settings.ignore.push(p);
                                app.settings_page.new_ignore.clear();
                                changed = true;
                            }
                        }
                    }
                    let picking = app.settings_page.pick.is_some();
                    let ch = ui
                        .add_enabled_ui(!picking, |ui| {
                            kit::pill(ui, &t, "Choose Folder...", kit::Kind::Secondary, Some(Icon::Folder))
                        })
                        .inner;
                    if ch.on_hover_text("Pick a folder in the macOS file dialog").clicked() && !picking {
                        app.act();
                        let env = app.env.clone();
                        app.settings_page.pick = Some(crate::jobs::Job::start(app.sync, move |_| {
                            choose_folder(&env, "Choose a folder CleanCraft should leave alone")
                        }));
                    }
                });
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::LargeOld, "Thresholds");
                let mut mb = (app.settings.large_min_bytes / 1_000_000) as u32;
                if slider_row(ui, &t, "Large file minimum (MB)", "Large files start at", &mut mb, 10..=5000, " MB") {
                    app.settings.large_min_bytes = mb as u64 * 1_000_000;
                    changed = true;
                }
                let mut days = app.settings.old_days as u32;
                if slider_row(ui, &t, "Old means not opened for (days)", "Old means not opened for", &mut days, 30..=1825, " days") {
                    app.settings.old_days = days as u64;
                    changed = true;
                }
                let mut dl = app.settings.downloads_days as u32;
                if slider_row(ui, &t, "Unused disk images after (days)", "Disk images count as unused after", &mut dl, 7..=730, " days") {
                    app.settings.downloads_days = dl as u64;
                    changed = true;
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Sweep, "AI agents");
                if kit::check_text(
                    ui,
                    &t,
                    &mut app.settings.agents_may_clean,
                    "Let AI agents (MCP) carry out confirmed cleanups",
                )
                .changed()
                {
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(if app.settings.agents_may_clean {
                                "Agents can move the items of a plan you confirmed. Everything they move is in History, with Undo."
                            } else {
                                "Off: agents can scan and propose, but nothing moves unless you clean here."
                            })
                            .font(font(size::FOOTNOTE, Weight::Regular))
                            .color(t.text_faint),
                        )
                        .wrap(),
                    );
                });
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, &t, Icon::Monitor, "Sounds");
                if switch_row(ui, &t, app.settings.sounds, "Play sounds", "Sounds") {
                    app.settings.sounds = !app.settings.sounds;
                    changed = true;
                }
                ui.add_enabled_ui(app.settings.sounds, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(vec2(260.0, crate::theme::hit::FIELD), Sense::hover());
                        ui.painter().text(
                            r.right_center() - vec2(12.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            "Volume",
                            font(size::BODY, Weight::Regular),
                            t.text_dim,
                        );
                        ui.spacing_mut().slider_width =
                            (ui.available_width() - 160.0).clamp(160.0, 300.0);
                        let mut v = app.settings.sound_volume;
                        let r = kit::slider(
                            ui,
                            egui::Slider::new(&mut v, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        let shown = v as f64;
                        r.widget_info(|| egui::WidgetInfo::slider(true, shown, "Volume"));
                        if r.changed() {
                            app.settings.sound_volume = v;
                            changed = true;
                        }
                        let p = kit::pill(ui, &t, "Preview", kit::Kind::Secondary, None);
                        p.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Preview sound")
                        });
                        if p.clicked() {
                            app.act();
                            crate::sound::preview(&app.env, app.settings.sound_volume);
                        }
                    });
                    if switch_row(
                        ui,
                        &t,
                        app.settings.sound_ticks,
                        "Click sounds on switches",
                        "Clicks",
                    ) {
                        app.settings.sound_ticks = !app.settings.sound_ticks;
                        changed = true;
                    }
                });
            });
            ui.add_space(space::M);
        });
    kit::scroll_fade_page(ui, &out, &t);
    if changed {
        app.act();
        app.save_settings();
    }
}
