//! "Use CleanCraft from your AI apps?": shown once at first launch, and later from Settings.
//! It finds Claude Desktop and Claude Code, connects the ones the person picks (one entry
//! each, with Undo), and keeps the separate choice "Let AI agents clean after a preview"
//! (off by default) apart. Not Now is remembered.

use cleancraft_core::executor;
use cleancraft_core::mcp_setup::{self, Answer, Client, ClientKind, ClientState, ConnectError};
use cleancraft_core::route::AppRoute;
use egui::{RichText, Ui};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font, size, space};

/// The switch's label (the same setting as Settings > AI assistants).
pub const CLEAN_SWITCH: &str = "Let AI agents clean after a preview";

#[derive(Default)]
pub struct State {
    pub clients: Option<Vec<Client>>,
    /// The binary agents would start; None when CleanCraft is not installed in Applications.
    pub binary: Option<String>,
    detect: Option<Job<(Vec<Client>, Option<String>)>>,
    connect: Option<(ClientKind, Job<Result<mcp_setup::Connected, ConnectError>>)>,
    /// The settings file CleanCraft refused to change, and why (offered to open).
    pub refused: Option<(ClientKind, String, std::path::PathBuf)>,
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
    let first = mcp_setup::welcome_due(&app.env);
    if app.welcome.clients.is_none() && app.welcome.detect.is_none() {
        refresh(app);
    }
    let out = egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let w = ui.available_width().min(760.0);
            ui.set_max_width(w);
            kit::title(ui, &t, "Use CleanCraft from your AI apps?");
            ui.add_space(space::XS);
            ui.add(
                egui::Label::new(
                    RichText::new(
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
                        RichText::new(CLEAN_SWITCH)
                            .font(font(size::BODY, Weight::Medium))
                            .color(t.text),
                    );
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(if app.settings.agents_may_clean {
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::primary(ui, &t, "Done") {
                        app.act();
                        finish(app, Answer::Done);
                    }
                    if first && kit::secondary(ui, &t, "Not Now") {
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
                    RichText::new(c.kind.title())
                        .font(font(size::BODY, Weight::SemiBold))
                        .color(t.text),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(status_line(c))
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
                // The exact change, before the person chooses Connect.
                if c.state == ClientState::Found
                    && let Some(change) = mcp_setup::planned_change(&app.env, c)
                {
                    ui.add(
                        egui::Label::new(
                            RichText::new(change)
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
                ClientState::Invalid(_) | ClientState::Conflict(_) => {
                    if kit::secondary(ui, &t, "Open the File") {
                        app.act();
                        let shown = app.env.display(&c.config);
                        if let Err(e) = app.env.sys.spawn("/usr/bin/open", &["-t", &shown]) {
                            app.warn(format!("Could not open {shown}: {e}"));
                        }
                    }
                }
                _ => {}
            },
        );
    });
}
