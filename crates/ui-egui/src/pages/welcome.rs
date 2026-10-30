//! The first launch (U7-5): a short welcome, then Full Disk Access (skipped when it is on),
//! then "Use CleanCraft from your AI apps?" when an AI app is found. The AI step is also
//! opened later from Settings.
//!
//! The AI step:
//! It finds Claude Desktop and Claude Code, connects the ones the person picks (one entry
//! each, with Undo), and keeps the separate choice "Let AI assistants clean after you confirm"
//! (off by default) apart. Not Now is remembered.

use cleancraft_core::executor;
use cleancraft_core::mcp_setup::{self, Answer, Client, ClientKind, ClientState, ConnectError};
use cleancraft_core::permissions::{self, Access};
use cleancraft_core::route::AppRoute;
use egui::Ui;

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

/// The switch's label (the same setting as Settings > AI assistants).
pub const CLEAN_SWITCH: &str = super::settings::AGENTS_SWITCH;

#[derive(Default)]
pub struct State {
    pub clients: Option<Vec<Client>>,
    /// The binary agents would start; None when CleanCraft is not installed in Applications.
    pub binary: Option<String>,
    detect: Option<Job<(Vec<Client>, Option<String>)>>,
    connect: Option<(ClientKind, Job<Result<mcp_setup::Connected, ConnectError>>)>,
    /// The settings file CleanCraft refused to change, and why (offered to open).
    pub refused: Option<(ClientKind, String, std::path::PathBuf)>,
    /// First launch: 0 the welcome, 1 Full Disk Access.
    pub step: u8,
    /// The AI step is shown as the last of the first launch's steps.
    pub in_setup: bool,
    /// The AI apps whose exact change is shown ("Show the exact change").
    pub details: Vec<ClientKind>,
    /// "How to turn it on" is open on the Full Disk Access step.
    pub fda_help: bool,
    /// The window had the focus last frame (Full Disk Access is read again when it comes
    /// back from System Settings).
    focused: bool,
}

/// The step titles of the first launch, in order.
pub const STEPS: [&str; 3] = ["Welcome", "Full Disk Access", "AI apps"];

/// Start the first launch's steps (from `launch_checks`). AI apps are looked for meanwhile.
pub fn start(app: &mut CleanApp) {
    app.welcome.step = 0;
    app.welcome.in_setup = true;
    app.page = crate::app::Page::Welcome;
    refresh(app);
}

/// The welcome is done: on to Full Disk Access when it is off, else to the AI step.
fn next_from_welcome(app: &mut CleanApp) {
    app.fda = permissions::full_disk_access(&app.env);
    if app.fda == Access::Missing {
        app.welcome.step = 1;
    } else {
        to_ai_step(app);
    }
}

/// The AI step when an AI app is found (or still being looked for); otherwise the first
/// launch ends here and the question is not asked (Settings has it).
fn to_ai_step(app: &mut CleanApp) {
    let found = app
        .welcome
        .clients
        .as_ref()
        .is_none_or(|c| c.iter().any(|c| !matches!(c.state, ClientState::NotFound)));
    if found {
        app.page = crate::app::Page::ConnectAi;
    } else {
        let _ = mcp_setup::remember(&app.env, Answer::NotNow);
        app.welcome.in_setup = false;
        app.open_route(&AppRoute::Feature(cleancraft_core::FeatureId::SmartCare));
    }
}

/// The row of steps at the top of each first-launch page: done, current, to come.
fn steps_row(app: &CleanApp, ui: &mut Ui, current: usize) {
    let t = app.t;
    ui.horizontal(|ui| {
        for (i, name) in STEPS.iter().enumerate() {
            let color = if i <= current { t.sec_a } else { t.text_faint };
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            if i < current {
                ui.painter().circle_filled(r.center(), 4.5, color);
            } else if i == current {
                ui.painter().circle_filled(r.center(), 5.0, color);
            } else {
                ui.painter()
                    .circle_stroke(r.center(), 4.0, egui::Stroke::new(1.5, color));
            }
            ui.label(
                crate::rich(format!("{} {name}", i + 1))
                    .font(font(size::FOOTNOTE, Weight::Medium))
                    .color(if i == current { t.text } else { t.text_dim }),
            );
            ui.add_space(space::M);
        }
    });
    ui.add_space(space::L);
}

fn para(app: &CleanApp, ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            crate::rich(text)
                .font(font(size::BODY_L, Weight::Regular))
                .color(app.t.text_dim),
        )
        .wrap(),
    );
}

/// The first launch's welcome and Full Disk Access steps.
pub fn intro_ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    // Back from System Settings: read Full Disk Access again.
    let focused = ui.input(|i| i.focused);
    if focused && !app.welcome.focused && app.welcome.step == 1 {
        app.fda = permissions::full_disk_access(&app.env);
    }
    app.welcome.focused = focused;
    let out = egui::ScrollArea::vertical()
        .id_salt("first-launch")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let w = ui.available_width().min(680.0);
            ui.set_max_width(w);
            ui.add_space(space::L);
            if app.welcome.step == 0 {
                steps_row(app, ui, 0);
                kit::title_announced(ui, &t, "Welcome to CleanCraft");
                ui.add_space(space::S);
                para(app, ui, "CleanCraft finds what takes space and what slows your Mac down.");
                ui.add_space(space::XS);
                para(
                    app,
                    ui,
                    "It looks first and moves nothing until you confirm the list. Everything it moves goes to the Trash, with Undo.",
                );
                ui.add_space(space::XL);
                ui.horizontal(|ui| {
                    if kit::primary(ui, &t, "Continue") {
                        app.act();
                        next_from_welcome(app);
                    }
                });
            } else {
                steps_row(app, ui, 1);
                kit::title_announced(ui, &t, "Let CleanCraft see all of your Mac");
                ui.add_space(space::S);
                para(
                    app,
                    ui,
                    "macOS keeps Mail, Safari and the Trash private. With Full Disk Access, CleanCraft can include them in its scans. It still moves nothing until you confirm.",
                );
                ui.add_space(space::M);
                let on = app.fda != Access::Missing;
                if on {
                    kit::strip(ui, Icon::Check, t.success, "Full Disk Access is on.");
                } else {
                    kit::strip(
                        ui,
                        Icon::Warning,
                        t.warning,
                        "Full Disk Access is off. Turn on CleanCraft in System Settings, then come back here.",
                    );
                }
                ui.add_space(space::S);
                let label = if app.welcome.fda_help {
                    "Hide how to turn it on"
                } else {
                    "How to turn it on"
                };
                if kit::ghost(ui, &t, label, Some(Icon::Chevron)).clicked() {
                    app.act();
                    app.welcome.fda_help = !app.welcome.fda_help;
                }
                if app.welcome.fda_help {
                    for line in [
                        "1. Open Privacy Settings opens System Settings > Privacy & Security > Full Disk Access.",
                        "2. Turn on CleanCraft in the list (use + to add it from Applications when it is missing).",
                        "3. Come back here: CleanCraft checks again by itself.",
                    ] {
                        kit::dim(ui, &t, line);
                    }
                }
                ui.add_space(space::XL);
                ui.horizontal(|ui| {
                    if on {
                        if kit::primary(ui, &t, "Continue") {
                            app.act();
                            to_ai_step(app);
                        }
                    } else {
                        if kit::primary(ui, &t, "Open Privacy Settings") {
                            app.act();
                            if let Err(e) = permissions::open_fda_settings(&app.env) {
                                app.warn(format!("Could not open System Settings: {e}"));
                            }
                        }
                        if kit::secondary(ui, &t, "Check Again") {
                            app.act();
                            app.fda = permissions::full_disk_access(&app.env);
                            if app.fda == Access::Missing {
                                app.warn("Full Disk Access is still off.");
                            }
                        }
                        if kit::ghost(ui, &t, "Skip", None).clicked() {
                            app.act();
                            to_ai_step(app);
                        }
                    }
                });
            }
            ui.add_space(space::XL);
        });
    kit::scroll_fade_page(ui, &out, &t);
}

/// Look again for AI apps (in the background).
pub fn refresh(app: &mut CleanApp) {
    let env = app.env.clone();
    app.welcome.detect = Some(Job::start(app.sync, move |_| {
        (mcp_setup::detect(&env), mcp_setup::server_binary(&env))
    }));
}

pub fn poll(app: &mut CleanApp) -> bool {
    let mut busy = false;
    if let Some(j) = app.welcome.detect.as_mut() {
        if let Some((c, b)) = j.poll() {
            app.welcome.clients = Some(c);
            app.welcome.binary = b;
            app.welcome.detect = None;
        } else if j.is_dead() {
            app.welcome.detect = None;
        } else {
            busy = true;
        }
    }
    if let Some((kind, j)) = app.welcome.connect.as_mut() {
        let kind = *kind;
        if let Some(res) = j.poll() {
            app.welcome.connect = None;
            match res {
                Ok(done) => {
                    app.welcome.refused = None;
                    let id = done.record.id.clone();
                    app.tell_undo(
                        done.message,
                        &format!("Connect {}", kind.title()),
                        move |app| {
                            match executor::undo(&app.env, &id) {
                                Ok(_) => app.tell(format!(
                                    "{} is no longer connected to CleanCraft.",
                                    kind.title()
                                )),
                                Err(e) => app.warn(format!("Could not undo: {e}")),
                            }
                            refresh(app);
                        },
                    );
                }
                Err(ConnectError::Refused { why, file }) => {
                    app.warn(format!(
                        "{}'s settings file was not changed: {why}.",
                        kind.title()
                    ));
                    app.welcome.refused = Some((kind, why, file));
                }
                Err(e) => app.warn(e.to_string()),
            }
            refresh(app);
        } else if j.is_dead() {
            app.welcome.connect = None;
            app.warn("Connecting stopped unexpectedly.");
        } else {
            busy = true;
        }
    }
    busy
}

fn status_line(c: &Client) -> String {
    match &c.state {
        ClientState::NotFound => "Not found on this Mac.".into(),
        ClientState::Found => match (c.kind, &c.cli) {
            (ClientKind::ClaudeCode, Some(_)) => {
                "Found. CleanCraft is added with its claude mcp add command.".into()
            }
            _ => "Found. One entry named cleancraft is added to its settings file.".into(),
        },
        ClientState::Connected => "Connected to CleanCraft.".into(),
        ClientState::Invalid(why) => format!("Its settings file was left alone: {why}."),
        ClientState::Conflict(why) => format!("Left alone: {why}."),
    }
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let first = app.welcome.in_setup;
    if app.welcome.clients.is_none() && app.welcome.detect.is_none() {
        refresh(app);
    }
    let out = egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let w = ui.available_width().min(760.0);
            ui.set_max_width(w);
            if first {
                ui.add_space(space::L);
                steps_row(app, ui, 2);
            }
            kit::title_announced(ui, &t, "Use CleanCraft from your AI apps?");
            ui.add_space(space::XS);
            ui.add(
                egui::Label::new(
                    crate::rich(
                        "AI apps such as Claude can ask CleanCraft to scan your Mac and show you what it would clean. Connected, they can look and preview. They cannot move anything unless you also allow it below.",
                    )
                    .font(font(size::BODY_L, Weight::Regular))
                    .color(t.text_dim),
                )
                .wrap(),
            );
            ui.add_space(space::L);
            if app.welcome.clients.is_some() && app.welcome.binary.is_none() {
                kit::strip(
                    ui,
                    Icon::Warning,
                    t.warning,
                    "CleanCraft is not installed in Applications, so AI apps have no fixed place to start it. Install it first (scripts/install-app.sh), then come back here from Settings.",
                );
                ui.add_space(space::M);
            }
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                kit::card_head(ui, &t, Icon::App, t.sec_a, "AI apps on this Mac");
                ui.add_space(space::S);
                let Some(clients) = app.welcome.clients.clone() else {
                    kit::dim(ui, &t, "Looking for AI apps...");
                    return;
                };
                for c in clients {
                    client_row(app, ui, &c);
                    ui.add_space(space::S);
                }
            });
            ui.add_space(space::M);
            kit::card(&t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                kit::card_head(ui, &t, Icon::Shield, t.sec_a, "What agents may do");
                ui.add_space(space::S);
                ui.horizontal(|ui| {
                    let on = app.settings.agents_may_clean;
                    if kit::switch(ui, &t, on, CLEAN_SWITCH).clicked() {
                        app.act();
                        app.settings.agents_may_clean = !on;
                        app.save_settings();
                    }
                    ui.add_space(space::S);
                    ui.label(
                        crate::rich(CLEAN_SWITCH)
                            .font(font(size::BODY, Weight::Medium))
                            .color(t.text),
                    );
                });
                ui.add(
                    egui::Label::new(
                        crate::rich(if app.settings.agents_may_clean {
                            "On: an agent can move the items of a list it showed you, checked by the list's digest. Everything goes to the Trash or quarantine, with Undo in History."
                        } else {
                            "Off (the default): agents can scan and preview only. Nothing moves unless you clean in CleanCraft."
                        })
                        .font(font(size::CALLOUT, Weight::Regular))
                        .color(t.text_dim),
                    )
                    .wrap(),
                );
            });
            ui.add_space(space::L);
            ui.horizontal(|ui| {
                if kit::ghost(ui, &t, "How It Works", Some(Icon::File)).clicked() {
                    app.act();
                    app.open_route(&AppRoute::AgentGuide);
                }
                // One way to close: Not Now while nothing is connected, Done once something
                // is (U7-9).
                let connected = app
                    .welcome
                    .clients
                    .as_ref()
                    .is_some_and(|c| c.iter().any(|c| c.state == ClientState::Connected));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if connected {
                        if kit::primary(ui, &t, "Done") {
                            app.act();
                            finish(app, Answer::Done);
                        }
                    } else if kit::secondary(ui, &t, "Not Now") {
                        app.act();
                        finish(app, Answer::NotNow);
                    }
                });
            });
            ui.add_space(space::XL);
        });
    kit::scroll_fade_page(ui, &out, &t);
}

fn finish(app: &mut CleanApp, answer: Answer) {
    app.welcome.in_setup = false;
    if let Err(e) = mcp_setup::remember(&app.env, answer) {
        app.warn(format!("Your answer could not be saved: {e}"));
        return;
    }
    app.open_route(&AppRoute::Feature(cleancraft_core::FeatureId::SmartCare));
    if answer == Answer::NotNow {
        app.tell("You can connect AI apps later in Settings, under AI assistants.");
    }
}

fn client_row(app: &mut CleanApp, ui: &mut Ui, c: &Client) {
    let t = app.t;
    let busy = app
        .welcome
        .connect
        .as_ref()
        .is_some_and(|(k, _)| *k == c.kind);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
        kit::initial_tile(
            ui.painter(),
            r,
            c.kind.title(),
            kit::name_color(c.kind.title()),
        );
        ui.add_space(space::S);
        let text_w = (ui.available_width() - 190.0).max(160.0);
        ui.allocate_ui_with_layout(
            egui::vec2(text_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_max_width(text_w);
                ui.label(
                    crate::rich(c.kind.title())
                        .font(font(size::BODY, Weight::SemiBold))
                        .color(t.text),
                );
                ui.add(
                    egui::Label::new(
                        crate::rich(status_line(c))
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
                // The exact change, before the person chooses Connect, behind a disclosure
                // (U7-5: technical details are there for whoever wants them).
                let open = app.welcome.details.contains(&c.kind);
                if c.state == ClientState::Found && mcp_setup::planned_change(&app.env, c).is_some()
                {
                    let label = if open {
                        "Hide the exact change"
                    } else {
                        "Show the exact change"
                    };
                    let r = ui.push_id(("change", c.kind.title()), |ui| {
                        kit::ghost(ui, &t, label, Some(Icon::Chevron))
                    });
                    let r = r.inner;
                    let name = format!("{label} for {}", c.kind.title());
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::tr(&name))
                    });
                    if r.clicked() {
                        app.act();
                        if open {
                            app.welcome.details.retain(|k| *k != c.kind);
                        } else {
                            app.welcome.details.push(c.kind);
                        }
                    }
                }
                if open
                    && c.state == ClientState::Found
                    && let Some(change) = mcp_setup::planned_change(&app.env, c)
                {
                    ui.add(
                        egui::Label::new(
                            crate::rich(change)
                                .font(crate::theme::mono(size::FOOTNOTE))
                                .color(t.text_faint),
                        )
                        .wrap(),
                    );
                }
            },
        );
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| match &c.state {
                ClientState::Found if app.welcome.binary.is_some() => {
                    if busy {
                        kit::dim(ui, &t, "Connecting...");
                    } else if kit::primary(ui, &t, &format!("Connect {}", c.kind.title())) {
                        app.act();
                        let env = app.env.clone();
                        let kind = c.kind;
                        app.welcome.connect = Some((
                            kind,
                            Job::start(app.sync, move |_| mcp_setup::connect(&env, kind)),
                        ));
                    }
                }
                ClientState::Connected => {
                    kit::tag(ui, "Connected", t.success);
                }
                ClientState::Invalid(_) | ClientState::Conflict(_)
                    if kit::secondary(ui, &t, "Open the File") =>
                {
                    app.act();
                    let shown = app.env.display(&c.config);
                    if let Err(e) = app.env.sys.spawn("/usr/bin/open", &["-t", &shown]) {
                        app.warn(format!("Could not open {shown}: {e}"));
                    }
                }
                _ => {}
            },
        );
    });
}
