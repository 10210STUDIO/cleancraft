//! CleanCraft's own updates: the "Updates" group of Settings and the "Update available"
//! banner with the release notes, Install and a four step progress bar (Download, Verify,
//! Install, Relaunch). The engine is [`cleancraft_core::self_update`].

use std::path::PathBuf;

use cleancraft_core::self_update::{self, Auto, AutoCtx, Found, Installed, Release, UpdateState};
use egui::{CornerRadius, RichText, Stroke, Ui};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::pages::settings::{heading, label_cell, note, switch_row};
use crate::theme::{self, Weight, font, size, space};

pub const CHECK_SWITCH: &str = "Check for updates automatically";
pub const INSTALL_SWITCH: &str = "Install updates automatically";
pub const CHECK_NOW: &str = "Check Now";
pub const INSTALL_BUTTON: &str = "Install Update";
pub const LATER: &str = "Later";
pub const CHECK_NOTE: &str = "CleanCraft looks at its GitHub Releases at most once a day, from the menu bar agent or when the app opens.";
pub const INSTALL_OFF_NOTE: &str =
    "Off: you get a notification and a banner with what is new, and you choose when to install.";
pub const INSTALL_ON_NOTE: &str = "A new version installs by itself when no cleanup is running. The previous version goes to the Trash, and History can put it back.";
pub const NOT_IN_BUNDLE: &str = "This copy of CleanCraft does not run from an app bundle (a build from source). Download the release from GitHub, or build the app again with scripts/install-app.sh.";

pub struct State {
    /// The version running (tests set their own).
    pub current: String,
    /// "Check Now" while it runs.
    check: Option<Job<Result<Found, String>>>,
    /// The launch check while it runs.
    auto: Option<Job<Auto>>,
    /// The install while it runs, with the version it installs.
    pub install: Option<(String, Job<Result<Installed, String>>)>,
    /// What the last check said in Settings: (text, is_error).
    pub line: Option<(String, bool)>,
    /// What the last check found (read from disk when first needed).
    state: Option<UpdateState>,
    /// The version whose banner was put away with Later (this session).
    pub dismissed: Option<String>,
    /// Installed; the app is quitting and the helper opens the new version.
    pub relaunching: Option<PathBuf>,
    /// The installed app (tests and builds outside a bundle set their own).
    pub app_path: Option<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        State {
            current: self_update::CURRENT.to_string(),
            check: None,
            auto: None,
            install: None,
            line: None,
            state: None,
            dismissed: None,
            relaunching: None,
            app_path: None,
        }
    }
}

impl State {
    pub fn is_installing(&self) -> bool {
        self.install.is_some()
    }

    fn st(&mut self, env: &cleancraft_core::Env) -> &UpdateState {
        self.state.get_or_insert_with(|| UpdateState::load(env))
    }

    /// Read the saved check again (after the agent or a check wrote it).
    pub fn reload(&mut self, env: &cleancraft_core::Env) {
        self.state = Some(UpdateState::load(env));
    }

    /// The release newer than the running version, from the last check.
    pub fn available(&mut self, env: &cleancraft_core::Env) -> Option<Release> {
        let cur = self.current.clone();
        self.st(env).available(&cur).cloned()
    }
}

/// Why an update must wait: a cleanup or another job that moves files is running.
pub fn busy_reason(app: &CleanApp) -> Option<String> {
    use crate::pages::{apps, disk_map, plan_page, sweep};
    for (f, st) in &app.plans {
        if matches!(st.phase, plan_page::Phase::Cleaning(_)) {
            return Some(format!("A cleanup is running in {}.", f.title()));
        }
    }
    if matches!(app.sweep.phase, sweep::Phase::Running(_)) {
        return Some("Quick Sweep is cleaning.".into());
    }
    if matches!(app.apps.phase, apps::Phase::Working(..)) {
        return Some("App Remover is working.".into());
    }
    if matches!(app.disk_map.phase, disk_map::Phase::Working(_)) {
        return Some("Disk Map is moving files.".into());
    }
    if app.tasks.is_busy() {
        return Some("Tune-up Tasks are running.".into());
    }
    None
}

fn app_bundle(app: &CleanApp) -> Option<PathBuf> {
    app.updates
        .app_path
        .clone()
        .or_else(|| self_update::installed_app(&app.env))
}

/// The launch check (at most once every 24 hours, shared with the menu bar agent). The app
/// shows its banner instead of a notification; with "Install automatically" on it installs
/// through the banner, with its progress bar.
pub fn launch_check(app: &mut CleanApp) {
    if !app.settings.update_check_auto || app.updates.auto.is_some() {
        return;
    }
    let env = app.env.clone();
    let settings = app.settings.clone();
    let current = app.updates.current.clone();
    app.updates.auto = Some(Job::start(app.sync, move |_| {
        let ctx = AutoCtx {
            now: chrono::Utc::now().timestamp(),
            notify: false,
            install: None,
            current: &current,
        };
        self_update::auto_check(&env, &settings, &ctx, &|e| {
            self_update::check_against(e, &current)
        })
    }));
    poll(app);
}

/// "Check Now": a check right away, whatever the time of the last one.
pub fn check_now(app: &mut CleanApp) {
    if app.updates.check.is_some() {
        return;
    }
    let env = app.env.clone();
    let current = app.updates.current.clone();
    app.updates.line = None;
    app.updates.check = Some(Job::start(app.sync, move |_| {
        let res = self_update::check_against(&env, &current);
        let mut st = UpdateState::load(&env);
        st.record(&res, chrono::Utc::now().timestamp(), &current);
        let _ = st.save(&env);
        res
    }));
    poll(app);
}

/// Start installing `rel`, unless a cleanup runs or the app is not in a bundle.
pub fn start_install(app: &mut CleanApp, rel: Release) {
    if app.updates.install.is_some() {
        return;
    }
    if let Some(b) = busy_reason(app) {
        app.warn(format!(
            "{b} Wait for it to finish, then install the update."
        ));
        return;
    }
    let Some(bundle) = app_bundle(app) else {
        app.warn(NOT_IN_BUNDLE);
        return;
    };
    let target = self_update::Target {
        app: bundle,
        current: app.updates.current.clone(),
        busy: None,
    };
    let env = app.env.clone();
    let version = rel.version.clone();
    let job = Job::start_metered(app.sync, move |m| {
        let r = self_update::install(&env, &rel, &target, m);
        if r.is_ok() {
            m.range(0.75, 1.0)
                .set_step(&format!("Restarting CleanCraft {}", rel.version));
        }
        r
    });
    app.updates.install = Some((version, job));
    poll(app);
}

/// Finished jobs. Returns true while one runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    if let Some(job) = app.updates.auto.as_mut() {
        match job.poll() {
            Some(a) => {
                app.updates.auto = None;
                app.updates.reload(&app.env);
                if let Auto::Told { release, .. } = a
                    && app.settings.update_install_auto
                    && busy_reason(app).is_none()
                {
                    start_install(app, release);
                }
            }
            None if job.is_dead() => app.updates.auto = None,
            None => busy = true,
        }
    }
    if let Some(job) = app.updates.check.as_mut() {
        match job.poll() {
            Some(r) => {
                app.updates.check = None;
                app.updates.reload(&app.env);
                let cur = app.updates.current.clone();
                app.updates.line = Some(match r {
                    Ok(f) => {
                        if matches!(f, Found::Available { .. }) {
                            app.updates.dismissed = None;
                        }
                        (f.message(&cur), false)
                    }
                    Err(e) => (self_update::check_failed_text(&e), true),
                });
            }
            None if job.is_dead() => app.updates.check = None,
            None => busy = true,
        }
    }
    if let Some((_, job)) = app.updates.install.as_mut() {
        match job.poll() {
            Some(Ok(done)) => {
                app.updates.install = None;
                finished(app, done);
            }
            Some(Err(e)) => {
                app.updates.install = None;
                app.warn(e);
            }
            None if job.is_dead() => {
                app.updates.install = None;
                app.warn("The update stopped unexpectedly. Nothing was changed.");
            }
            None => busy = true,
        }
    }
    busy
}

/// Installed: History has the line with Undo; the helper opens the new version once this
/// app has quit, then the app quits.
fn finished(app: &mut CleanApp, done: Installed) {
    app.updates.reload(&app.env);
    let mut msg = done.message();
    if let Some(n) = &done.note {
        msg.push(' ');
        msg.push_str(n);
    }
    match self_update::relaunch(&app.env, &done.app, std::process::id()) {
        Ok(()) => {
            app.tell(format!("{msg} Restarting CleanCraft."));
            app.updates.relaunching = Some(done.app.clone());
            if !app.sync
                && let Some(ctx) = &app.egui_ctx
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        Err(e) => app.warn(format!(
            "{msg} CleanCraft could not restart by itself ({e}): quit it and open it again."
        )),
    }
}

/// The Settings group. Returns true when a setting changed.
pub fn card(app: &mut CleanApp, ui: &mut Ui) -> bool {
    let t = app.t;
    let mut changed = false;
    kit::card(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, &t, Icon::Updates, "Updates");
        if switch_row(
            ui,
            &t,
            app.settings.update_check_auto,
            CHECK_SWITCH,
            "Check",
        ) {
            app.settings.update_check_auto = !app.settings.update_check_auto;
            changed = true;
        }
        note(ui, &t, CHECK_NOTE);
        if switch_row(
            ui,
            &t,
            app.settings.update_install_auto,
            INSTALL_SWITCH,
            "Install",
        ) {
            app.settings.update_install_auto = !app.settings.update_install_auto;
            changed = true;
        }
        note(
            ui,
            &t,
            if app.settings.update_install_auto {
                INSTALL_ON_NOTE
            } else {
                INSTALL_OFF_NOTE
            },
        );
        let current = app.updates.current.clone();
        let last = app.updates.st(&app.env).last_check;
        ui.horizontal(|ui| {
            label_cell(ui, &t, "Version");
            ui.label(
                RichText::new(format!("CleanCraft {current}"))
                    .font(font(size::CALLOUT, Weight::Medium))
                    .color(t.text),
            );
        });
        ui.horizontal(|ui| {
            label_cell(ui, &t, "Last checked");
            ui.label(
                RichText::new(self_update::last_check_text(last))
                    .font(font(size::CALLOUT, Weight::Medium))
                    .color(t.text),
            );
        });
        let available = app.updates.available(&app.env);
        ui.horizontal(|ui| {
            label_cell(ui, &t, "");
            let checking = app.updates.check.is_some();
            ui.add_enabled_ui(!checking && !app.updates.is_installing(), |ui| {
                if kit::pill(ui, &t, CHECK_NOW, kit::Kind::Secondary, Some(Icon::Search)).clicked()
                {
                    app.act();
                    check_now(app);
                }
            });
            if let Some(rel) = &available {
                let label = format!("Install CleanCraft {}", rel.version);
                let enabled = !app.updates.is_installing();
                if ui
                    .add_enabled_ui(enabled, |ui| {
                        kit::pill(ui, &t, &label, kit::Kind::Primary, Some(Icon::Updates))
                    })
                    .inner
                    .clicked()
                {
                    app.act();
                    start_install(app, rel.clone());
                }
            }
            if checking {
                kit::dim(ui, &t, "Checking...");
            }
        });
        if let Some((text, err)) = app.updates.line.clone() {
            ui.horizontal(|ui| {
                label_cell(ui, &t, "");
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .font(font(size::FOOTNOTE, Weight::Regular))
                            .color(if err { t.warning } else { t.text_dim }),
                    )
                    .wrap(),
                );
            });
        }
    });
    changed
}

/// The banner at the top of every page: "CleanCraft X is available" with the notes, Install
/// and Later; while installing, the four steps and the progress bar.
pub fn banner(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if let Some((version, job)) = &app.updates.install {
        let meter = job.meter.clone();
        let step = job.status();
        let version = version.clone();
        frame(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(
                    egui::Vec2::splat(theme::icon::SIDEBAR),
                    egui::Sense::hover(),
                );
                icons::paint(ui.painter(), r, Icon::Updates, t.accent);
                ui.label(
                    RichText::new(format!("Installing CleanCraft {version}"))
                        .font(font(size::CALLOUT, Weight::SemiBold))
                        .color(t.text),
                );
            });
            ui.add_space(space::XS);
            steps_row(ui, &t, meter.fraction().unwrap_or(0.0));
            ui.add_space(space::XS);
            kit::progress_block(ui, &t, &meter, &step);
        });
        ui.add_space(8.0);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
        return;
    }
    if app.updates.relaunching.is_some() {
        return;
    }
    let Some(rel) = app.updates.available(&app.env) else {
        return;
    };
    if app.updates.dismissed.as_deref() == Some(rel.version.as_str()) {
        return;
    }
    let current = app.updates.current.clone();
    frame(&t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let (r, _) = ui.allocate_exact_size(
                egui::Vec2::splat(theme::icon::SIDEBAR),
                egui::Sense::hover(),
            );
            icons::paint(ui.painter(), r, Icon::Updates, t.accent);
            ui.label(
                RichText::new(format!(
                    "Update available: CleanCraft {} (you have {current}).",
                    rel.version
                ))
                .font(font(size::CALLOUT, Weight::SemiBold))
                .color(t.text),
            );
            if kit::pill(ui, &t, INSTALL_BUTTON, kit::Kind::Primary, Some(Icon::Updates))
                .clicked()
            {
                app.act();
                start_install(app, rel.clone());
            }
            if kit::ghost(ui, &t, LATER, None).clicked() {
                app.act();
                app.updates.dismissed = Some(rel.version.clone());
            }
        });
        if !rel.notes.is_empty() {
            ui.add_space(space::XS);
            egui::ScrollArea::vertical()
                .id_salt("update-notes")
                .max_height(110.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&rel.notes)
                                .font(font(size::FOOTNOTE, Weight::Regular))
                                .color(t.text_dim),
                        )
                        .wrap(),
                    );
                });
        }
        ui.label(
            RichText::new(
                "The previous version goes to the Trash, and CleanCraft restarts when the update is in place.",
            )
            .font(font(size::FOOTNOTE, Weight::Regular))
            .color(t.text_faint),
        );
    });
    ui.add_space(8.0);
}

fn frame(t: &theme::Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(icons::lerp(t.card, t.accent, 0.12))
        .stroke(Stroke::new(1.0, t.accent.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(theme::radius::M))
        .inner_margin(egui::Margin::symmetric(14, 10))
}

/// Download, Verify, Install, Relaunch: done ones checked, the current one bright.
fn steps_row(ui: &mut Ui, t: &theme::Tokens, fraction: f32) {
    let n = self_update::STEPS.len();
    let at = ((fraction * n as f32).floor() as usize).min(n - 1);
    ui.horizontal(|ui| {
        for (i, s) in self_update::STEPS.iter().enumerate() {
            let (text, color) = if i < at {
                (format!("{s} done"), t.success)
            } else if i == at {
                (s.to_string(), t.text)
            } else {
                (s.to_string(), t.text_faint)
            };
            ui.label(
                RichText::new(text)
                    .font(font(size::FOOTNOTE, Weight::SemiBold))
                    .color(color),
            );
            if i + 1 < n {
                ui.label(RichText::new(">").color(t.text_faint));
            }
        }
    });
}
