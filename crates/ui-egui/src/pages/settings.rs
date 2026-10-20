//! Settings: where removed items go, the ignore list, thresholds, agents, menu bar.

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::human_size;
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
        // Wide ranges (10 MB to 5 GB) move in even steps of scale, not of megabytes.
        let wide = *range.end() >= range.start().saturating_mul(50).max(1);
        let r = kit::slider(
            ui,
            egui::Slider::new(value, range)
                .show_value(false)
                .logarithmic(wide)
                .trailing_fill(true),
        );
        // A slider senses drags only, so kit::slider's click check misses it.
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let v = *value as f64;
        let name = name.to_string();
        r.widget_info(|| egui::WidgetInfo::slider(true, v, name.clone()));
        // The value, beside the slider: shown "50 MB", announced with what it sets.
        let shown = format!("{value}{unit}");
        let full = format!("{label}: {value}{unit}");
        let fid = font(size::CALLOUT, Weight::SemiBold);
        let w = kit::galley(ui, &shown, fid.clone(), t.text).size().x + 4.0;
        let (vr, _) = ui.allocate_exact_size(vec2(w.max(64.0), 20.0), Sense::hover());
        kit::text_at(ui, vr, &shown, &full, fid, t.text, egui::Align::Min);
        r.changed()
    })
    .inner
}

/// What the quarantine holds now, a way to see it in Finder, and "Move Quarantine to
/// Trash..." (REVIEW-U U5): every item moves to the Trash, journaled, with Undo. Nothing is
/// deleted here; Finder's Empty Trash is what frees the space.
fn quarantine_line(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let changed = app.settings_page.watch.changes(&app.env).is_some();
    if app.settings_page.quarantine.is_none() || changed {
        app.settings_page.watch.prime(&app.env);
        app.settings_page.quarantine = Some(executor::quarantine_size(&app.env));
    }
    let (bytes, items) = app.settings_page.quarantine.unwrap_or((0, 0));
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.add_space(28.0);
        let text = if items == 0 {
            "The quarantine is empty.".to_string()
        } else {
            format!(
                "The quarantine holds {} ({}). Items stay there until you put them back from History or move them to the Trash.",
                human_size(bytes),
                cleancraft_core::fsutil::items(items)
            )
        };
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(font(size::FOOTNOTE, Weight::Regular))
                    .color(t.text_faint),
            )
            .wrap(),
        );
    });
    if items == 0 {
        return;
    }
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.add_space(28.0);
        if kit::pill(
            ui,
            &t,
            "Show Quarantine in Finder",
            kit::Kind::Secondary,
            Some(Icon::Reveal),
        )
        .clicked()
        {
            app.act();
            let dir = app.env.quarantine_dir().to_string_lossy().into_owned();
            match app.env.sys.run("/usr/bin/open", &[&dir]) {
                Ok(out) if out.ok() => {}
                Ok(out) => app.warn(format!("Could not open Finder: {}", out.stderr.trim())),
                Err(e) => app.warn(format!("Could not open Finder: {e}")),
            }
        }
        if kit::pill(ui, &t, MOVE_QUARANTINE, kit::Kind::Secondary, Some(Icon::Trash))
            .on_hover_text("Move everything in the quarantine to the Trash. You can put it back from History.")
            .clicked()
        {
            app.act();
            app.settings_page.confirm_quarantine = true;
        }
    });
    if app.settings_page.confirm_quarantine {
        confirm_quarantine(app, ui, bytes, items);
    }
}

pub const MOVE_QUARANTINE: &str = "Move Quarantine to Trash...";

/// The confirmation, then the move, with Undo in the notice and in History.
fn confirm_quarantine(app: &mut CleanApp, ui: &mut Ui, bytes: u64, items: usize) {
    let t = app.t;
    let mut go = false;
    let mut cancel = false;
    let dlg = kit::modal(egui::Id::new("confirm-quarantine-trash"), &t).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        kit::dialog_head(
            ui,
            &t,
            Icon::Trash,
            t.warning,
            &format!(
                "Move the quarantine to the Trash ({}, {})?",
                human_size(bytes),
                cleancraft_core::fsutil::items(items)
            ),
            "Every item in the CleanCraft quarantine moves to the Trash. Nothing is deleted: you can put them back from History until the Trash is emptied.",
        );
        ui.add_space(16.0);
        let (ok, no) = kit::dialog_actions(ui, &t, "Move to Trash", kit::Kind::Danger);
        go |= ok;
        cancel |= no;
    });
    if dlg.should_close() {
        cancel = true;
    }
    if cancel {
        app.act();
        app.settings_page.confirm_quarantine = false;
    } else if go {
        app.act();
        app.settings_page.confirm_quarantine = false;
        match executor::move_quarantine_to_trash(&app.env, &mut |_, _| {}) {
            Ok(rep) => {
                app.settings_page.quarantine = None;
                let moved: Vec<std::path::PathBuf> = CleanApp::moved_paths(&rep);
                app.files_moved(&moved);
                if rep.moved.is_empty() {
                    app.warn(format!(
                        "Nothing moved: {}",
                        rep.skipped
                            .first()
                            .map(|(_, w)| w.to_string())
                            .unwrap_or_else(|| "the quarantine is empty".into())
                    ));
                } else {
                    let plan_id = rep.plan_id.clone();
                    let mut msg = format!(
                        "{} ({}) moved from the quarantine to the Trash.",
                        cleancraft_core::fsutil::items(rep.moved.len()),
                        human_size(rep.freed)
                    );
                    if !rep.skipped.is_empty() {
                        msg.push_str(&format!(
                            " {} stayed: {}.",
                            cleancraft_core::fsutil::items(rep.skipped.len()),
                            rep.skipped[0].1
                        ));
                    }
                    app.tell_undo(msg, "Move to Trash", move |app| {
                        let (n, errs) = executor::undo_plan(&app.env, &plan_id);
                        app.files_back(&[]);
                        app.settings_page.quarantine = None;
                        if errs.is_empty() {
                            app.tell(format!(
                                "{} back in the quarantine.",
                                cleancraft_core::fsutil::items(n)
                            ));
                        } else {
                            app.warn(format!(
                                "{} back in the quarantine, {} could not be: {}",
                                cleancraft_core::fsutil::items(n),
                                errs.len(),
                                errs[0].1
                            ));
                        }
                    });
                }
            }
            Err(e) => app.warn(format!("Nothing moved: {e}")),
        }
    }
}

#[derive(Default)]
pub struct State {
    /// Text box for adding an ignore-list path.
    pub new_ignore: String,
    /// What the quarantine holds (bytes, items), read again after each move or undo.
    quarantine: Option<(u64, usize)>,
    /// "Move Quarantine to Trash..." asks first.
    confirm_quarantine: bool,
    watch: super::history::JournalWatch,
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
                quarantine_line(app, ui);
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
                        let seen = super::history::short_path(&app.env, p);
                        let short = kit::truncate_middle(ui.ctx(), &seen, &fid, room);
                        let w = kit::galley(ui, &short, fid.clone(), t.text).size().x + 2.0;
                        let (pr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
                        kit::text_at(ui, pr, &short, &shown, fid, t.text, egui::Align::Min)
                            .on_hover_text(&seen);
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
