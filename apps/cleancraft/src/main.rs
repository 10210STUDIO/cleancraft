//! CleanCraft: one binary for the desktop app, the command line and the MCP server.
//!
//! No arguments: the desktop app. Any subcommand (`scan`, `clean`, `serve`...): the CLI,
//! where `serve` is the MCP server AI agents launch.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo)]

mod cli;
mod cli_apps;
mod cli_monitor;

use cleancraft_core::Env;
use cleancraft_ui_egui::CleanApp;

struct App(CleanApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
        #[cfg(target_os = "macos")]
        craft_quit::on_close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

/// macOS passes `-psn_...` to apps launched from Finder on older systems; that is not a command.
fn wants_cli(args: &[String]) -> bool {
    args.get(1).is_some_and(|a| !a.starts_with("-psn"))
}

fn icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/AppIcon.png")).ok()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if wants_cli(&args) {
        std::process::exit(cli::run(args));
    }
    let env = match Env::real() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("CleanCraft: {e}");
            std::process::exit(1);
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("CleanCraft")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([1060.0, 680.0]);
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "CleanCraft",
        options,
        Box::new(|cc| {
            let _ = &cc;
            #[cfg(target_os = "macos")]
            if let Err(e) = craft_quit::install_app_menu(&cc.egui_ctx, "CleanCraft") {
                eprintln!("CleanCraft: app menu not installed: {e}");
            }
            Ok(Box::new(App(CleanApp::new(env))))
        }),
    );
    if let Err(e) = result {
        eprintln!("CleanCraft could not open its window: {e}");
        std::process::exit(1);
    }
}
