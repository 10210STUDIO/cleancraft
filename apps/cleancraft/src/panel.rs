//! `cleancraft panel --at x,y`: the menu bar panel's window, a short-lived process the agent
//! starts when its icon is clicked.
//!
//! Why a process of its own and not a window inside the agent: the agent runs all day, and a
//! GPU renderer (wgpu device, surfaces, font atlas) costs tens of megabytes that are never fully
//! given back once created. Here the renderer exists only while the panel is open, and exiting
//! returns all of it. The window is eframe's, the same stack as the app (so it builds and its
//! UI is tested on Linux, see `crates/ui-egui/tests/menubar_panel_ui.rs`); only the
//! accessory activation policy below is macOS code. Opening costs a process start and a wgpu
//! setup, a few hundred milliseconds.
//!
//! The protocol with the agent is two pipes. Standard output: one [`menubar::PanelCmd`] line per click
//! (the agent carries it out, so its saved state stays the only writer). Standard input: never
//! written; when the agent closes it (second click on the icon, or the agent quits) the panel
//! closes. The panel also closes on Escape, when it loses focus, and after a command that leads
//! elsewhere. It never cleans or moves anything.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cleancraft_core::Env;
use cleancraft_core::menubar;
use cleancraft_ui_egui::menubar_panel::MenuPanel;

/// The panel's window: borderless, transparent (the panel paints its own rounded, translucent
/// background), above other windows, not resizable, at `at` (points), out of the Dock and the
/// window switcher.
pub fn viewport(at: (f32, f32)) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("CleanCraft")
        .with_inner_size([menubar::PANEL_W as f32, menubar::PANEL_H as f32])
        .with_position(at)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_taskbar(false)
        .with_has_shadow(true)
}

/// Close on focus loss, but only after the window had focus once: it may open unfocused for
/// a moment, and that must not close it.
#[derive(Default)]
pub struct FocusWatch {
    seen: bool,
}

impl FocusWatch {
    pub fn lost(&mut self, focused: Option<bool>) -> bool {
        match focused {
            Some(true) => {
                self.seen = true;
                false
            }
            Some(false) => self.seen,
            None => false,
        }
    }
}

struct PanelApp {
    panel: MenuPanel,
    /// The agent closed our standard input.
    agent_gone: Arc<AtomicBool>,
    focus: FocusWatch,
    closing: bool,
}

impl PanelApp {
    fn close(&mut self, ctx: &egui::Context) {
        if !self.closing {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for PanelApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.panel.logic(ctx);
        let mut out = std::io::stdout().lock();
        for c in self.panel.take_commands() {
            if writeln!(out, "{}", c.line())
                .and_then(|_| out.flush())
                .is_err()
            {
                // Nobody reads: the agent is gone.
                self.agent_gone.store(true, Ordering::SeqCst);
            }
        }
        drop(out);
        let focused = ctx.input(|i| i.viewport().focused);
        if self.focus.lost(focused)
            || self.panel.wants_close()
            || self.agent_gone.load(Ordering::SeqCst)
        {
            self.close(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.panel.ui(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}

/// Run the panel until it closes.
pub fn run(env: &Env, at: &str) -> anyhow::Result<()> {
    let at = menubar::parse_at(at)
        .ok_or_else(|| anyhow::anyhow!("--at must be \"x,y\" in points, not \"{at}\""))?;
    let agent_gone = Arc::new(AtomicBool::new(false));
    {
        // Standard input ends when the agent closes it (or quits).
        let flag = agent_gone.clone();
        std::thread::spawn(move || {
            let mut sink = [0u8; 64];
            let mut stdin = std::io::stdin().lock();
            while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
            flag.store(true, Ordering::SeqCst);
        });
    }
    let options = eframe::NativeOptions {
        viewport: viewport(at),
        persist_window: false,
        event_loop_builder: event_loop_hook(),
        ..Default::default()
    };
    let env = env.clone();
    cleancraft_core::i18n::apply(&env, &cleancraft_core::Settings::load(&env));
    eframe::run_native(
        "CleanCraft Panel",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(PanelApp {
                panel: MenuPanel::new(env),
                agent_gone,
                focus: FocusWatch::default(),
                closing: false,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("The menu bar panel could not open: {e}"))
}

/// macOS: an accessory app (no Dock icon, no menu bar of its own), activated so the panel
/// has focus at once and closes when you click elsewhere.
#[cfg(target_os = "macos")]
fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    Some(Box::new(
        |b: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
            b.with_activation_policy(ActivationPolicy::Accessory)
                .with_default_menu(false)
                .with_activate_ignoring_other_apps(true);
        },
    ))
}

#[cfg(not(target_os = "macos"))]
fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_a_borderless_panel_where_the_agent_says() {
        let v = viewport((845.0, 30.0));
        assert_eq!(v.position, Some(egui::pos2(845.0, 30.0)));
        assert_eq!(v.inner_size, Some(egui::vec2(340.0, 460.0)));
        assert_eq!(v.decorations, Some(false));
        assert_eq!(v.transparent, Some(true));
        assert_eq!(v.resizable, Some(false));
        assert_eq!(v.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(v.taskbar, Some(false));
    }

    #[test]
    fn focus_loss_closes_only_after_the_panel_had_focus() {
        let mut f = FocusWatch::default();
        assert!(!f.lost(None));
        assert!(!f.lost(Some(false)), "opened unfocused: stays");
        assert!(!f.lost(Some(true)));
        assert!(f.lost(Some(false)));
    }

    #[test]
    fn a_bad_place_is_refused_before_any_window() {
        let m = cleancraft_core::testkit::FakeMac::new();
        let e = run(&m.env, "middle").err().map(|e| e.to_string());
        assert_eq!(
            e.as_deref(),
            Some("--at must be \"x,y\" in points, not \"middle\"")
        );
    }
}
