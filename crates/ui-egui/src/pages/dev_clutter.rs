//! Developer Clutter (a tool in the Clean section).
//!
//! The project folders (Rust `target`, `node_modules`, Python environments, other build
//! output) and the folders of unavailable Xcode simulators are a plan like every other
//! cleanup: the shared Scan, Review, Confirm, Done flow of [`plan_page`], moved to the Trash
//! or the quarantine with undo.
//!
//! Docker and Homebrew clean their own data with commands that delete for good, so
//! CleanCraft never runs them: after a scan a banner says what they could free and opens the
//! "Developer Tools" dialog, which shows what each tool reports and the exact command, with a
//! button that copies it for Terminal.

use std::sync::{Arc, Mutex};

use cleancraft_core::FeatureId;
use cleancraft_core::features::dev_clutter::{self as dev, ToolReport, ToolState};
use cleancraft_core::fsutil::human_size;
use egui::{Id, RichText, Ui};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::kit;
use crate::pages::plan_page::{self, Phase};
use crate::theme::{Weight, font, mono, size, space};

pub const TOOLS_BUTTON: &str = "Developer Tools";
pub const CHECK_AGAIN: &str = "Check Again";
pub const CLOSE: &str = "Done";

type Shared<T> = Arc<Mutex<Option<T>>>;

#[derive(Clone, Default)]
struct State {
    /// The plan the tools were last asked for (they are asked again after each scan).
    asked_for: Option<String>,
    tools: Option<Vec<ToolReport>>,
    tools_job: Option<Shared<Vec<ToolReport>>>,
    /// The Developer Tools dialog is open.
    open: bool,
}

fn state_id() -> Id {
    Id::new("dev-clutter-state")
}

fn load(ui: &Ui) -> State {
    ui.ctx()
        .data_mut(|d| d.get_temp::<State>(state_id()))
        .unwrap_or_default()
}

fn store(ui: &Ui, s: State) {
    ui.ctx().data_mut(|d| d.insert_temp(state_id(), s));
}

/// Ask the tools on a thread (inline when the app runs work inline, in tests).
fn ask_tools(app: &CleanApp, st: &mut State) {
    let env = app.env.clone();
    let slot: Shared<Vec<ToolReport>> = Arc::default();
    if app.sync {
        st.tools = Some(dev::scan_tools(&env));
        st.tools_job = None;
        return;
    }
    let s2 = slot.clone();
    std::thread::spawn(move || {
        let v = dev::scan_tools(&env);
        if let Ok(mut g) = s2.lock() {
            *g = Some(v);
        }
    });
    st.tools_job = Some(slot);
}

/// The tools that are on this Mac (absent ones are named on one line only).
fn present(tools: &[ToolReport]) -> impl Iterator<Item = &ToolReport> {
    tools
        .iter()
        .filter(|r| !matches!(r.state, ToolState::Absent(_)))
}

/// The banner's sentence over the results, when Docker or Homebrew is on this Mac.
pub fn banner_text(tools: &[ToolReport]) -> Option<String> {
    present(tools).next()?;
    let ready: Vec<&ToolReport> = tools.iter().filter(|r| r.ready()).collect();
    if ready.is_empty() {
        return Some("Docker and Homebrew have nothing to clean right now.".into());
    }
    let size: u64 = ready.iter().map(|r| r.size).sum();
    let names: Vec<&str> = ready.iter().map(|r| r.id.title()).collect();
    Some(format!(
        "Docker and Homebrew can free about {} more with their own commands ({}). Those delete for good, so you run them in Terminal.",
        human_size(size),
        names.join(", ")
    ))
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: FeatureId) {
    let t = app.t;
    let mut st = load(ui);
    if let Some(job) = &st.tools_job {
        match job.lock().ok().and_then(|mut g| g.take()) {
            Some(v) => {
                st.tools = Some(v);
                st.tools_job = None;
            }
            None => ui
                .ctx()
                .request_repaint_after(std::time::Duration::from_millis(200)),
        }
    }
    // After each scan the tools are asked again.
    let (plan_id, shown) = {
        let ps = app.plan_state(f);
        (
            ps.plan.as_ref().map(|p| p.id.clone()),
            matches!(ps.phase, Phase::Results | Phase::Confirm | Phase::Done(_)),
        )
    };
    if let Some(id) = &plan_id
        && st.asked_for.as_ref() != Some(id)
        && st.tools_job.is_none()
    {
        st.asked_for = Some(id.clone());
        ask_tools(app, &mut st);
    }
    if shown && let Some(text) = st.tools.as_deref().and_then(banner_text) {
        if kit::banner(ui, &t, Icon::Tasks, t.sec_a, &text, TOOLS_BUTTON) {
            app.act();
            st.open = true;
        }
        ui.add_space(space::S);
    }
    plan_page::ui(app, ui, f);
    if st.open {
        tools_dialog(app, ui, &mut st);
    }
    store(ui, st);
}

fn tools_dialog(app: &mut CleanApp, ui: &mut Ui, st: &mut State) {
    let t = app.t;
    let tools = st.tools.clone().unwrap_or_default();
    let busy = st.tools_job.is_some();
    let mut close = false;
    let mut again = false;
    let mut copied: Option<ToolReport> = None;
    let w = kit::dialog_width(ui.ctx()).min(680.0);
    let dlg = kit::modal(Id::new("dev-tools"), &t).show(ui.ctx(), |ui| {
        ui.set_width(w);
        kit::dialog_head(
            ui,
            &t,
            Icon::Tasks,
            t.sec_a,
            "Docker and Homebrew",
            "These tools clean their own data with commands that delete for good, never to the Trash. CleanCraft only shows what they report; you run the command in Terminal if you want it.",
        );
        ui.add_space(space::M);
        let h = (ui.ctx().content_rect().height() - 330.0).clamp(160.0, 420.0);
        egui::ScrollArea::vertical()
            .max_height(h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for r in present(&tools) {
                    if tool_card(ui, &t, r) {
                        copied = Some(r.clone());
                    }
                    ui.add_space(space::S);
                }
                let absent: Vec<String> = tools
                    .iter()
                    .filter_map(|r| match &r.state {
                        ToolState::Absent(m) => Some(m.clone()),
                        _ => None,
                    })
                    .collect();
                if !absent.is_empty() {
                    let mut seen = Vec::new();
                    for m in absent {
                        if !seen.contains(&m) {
                            seen.push(m);
                        }
                    }
                    kit::dim(ui, &t, &seen.join(" "));
                }
            });
        ui.add_space(space::M);
        ui.horizontal(|ui| {
            if busy {
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                let time = ui.input(|i| i.time);
                kit::progress_ring(ui.painter(), r.center(), 14.0, &t, time);
                kit::dim(ui, &t, "Asking the tools...");
            } else if kit::secondary(ui, &t, CHECK_AGAIN) {
                again = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::secondary(ui, &t, CLOSE) {
                    close = true;
                }
            });
        });
    });
    if dlg.should_close() || close {
        app.act();
        st.open = false;
    } else if again {
        app.act();
        ask_tools(app, st);
    } else if let Some(r) = copied {
        app.act();
        ui.ctx().copy_text(r.command.clone());
        app.tell(format!(
            "Copied \"{}\". Paste it in Terminal to run it. It deletes for good; CleanCraft cannot undo it.",
            r.command
        ));
    }
}

/// One tool in the dialog. Returns true when its Copy button was clicked.
fn tool_card(ui: &mut Ui, t: &crate::theme::Tokens, r: &ToolReport) -> bool {
    let mut copy = false;
    kit::well(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(r.id.title())
                    .font(font(size::BODY, Weight::SemiBold))
                    .color(t.text),
            );
            if r.ready() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(human_size(r.size))
                            .font(font(size::BODY, Weight::SemiBold))
                            .color(t.text),
                    );
                });
            }
        });
        match &r.state {
            ToolState::Ready => {
                kit::dim(ui, t, &format!("Size {}.", r.size_note));
                ui.add_space(space::XS);
                for e in r.entries.iter().take(5) {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}: {}", e.label, e.detail))
                                    .font(font(size::FOOTNOTE, Weight::Regular))
                                    .color(t.text_dim),
                            )
                            .truncate(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(human_size(e.size))
                                    .font(font(size::FOOTNOTE, Weight::Medium))
                                    .color(t.text),
                            );
                        });
                    });
                }
                if r.entries.len() > 5 {
                    kit::dim(ui, t, &format!("and {} more", r.entries.len() - 5));
                }
                ui.add_space(space::XS);
                kit::dim(ui, t, r.id.why_not_here());
                ui.add_space(space::XS);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Command:")
                            .font(font(size::FOOTNOTE, Weight::Medium))
                            .color(t.text_dim),
                    );
                    ui.label(
                        RichText::new(&r.command)
                            .font(mono(size::CALLOUT))
                            .color(t.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        copy = kit::secondary(ui, t, r.id.copy_label());
                    });
                });
            }
            ToolState::Clean(m) => kit::dim(ui, t, m),
            ToolState::Unavailable(m) => kit::strip(ui, Icon::Warning, t.warning, m),
            ToolState::Absent(_) => {}
        }
    });
    copy
}
