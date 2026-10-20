//! The menu bar status item: the only platform code of the agent.
//!
//! macOS: a `tray-icon` status item (menu built with muda, re-exported as `tray_icon::menu`)
//! on winit's event loop, as an accessory app (no Dock icon, no window). It shows the models
//! the worker sends and sends back the menu choices. Elsewhere: a stub that says the agent
//! needs macOS, so the workspace builds and tests on Linux.
//!
//! This file cannot be compiled on the Linux build box: it is written against tray-icon
//! 0.26.1, muda 0.21.2 and winit 0.30.13 as published and must be built on a Mac.

use std::sync::mpsc::{Receiver, Sender};

use cleancraft_core::schedule::{MenuAction, MenuModel};

/// Menu item ids.
pub const ID_REVIEW: &str = "cleancraft.review.";
pub const ID_OPEN: &str = "cleancraft.open";
pub const ID_SWEEP: &str = "cleancraft.sweep";
pub const ID_PAUSE: &str = "cleancraft.pause";
pub const ID_QUIT: &str = "cleancraft.quit";

/// The action of a menu item id; `None` for the information lines.
pub fn action_for(id: &str) -> Option<MenuAction> {
    if let Some(n) = id.strip_prefix(ID_REVIEW) {
        return n.parse().ok().map(MenuAction::Review);
    }
    match id {
        ID_OPEN => Some(MenuAction::Open),
        ID_SWEEP => Some(MenuAction::QuickSweep),
        ID_PAUSE => Some(MenuAction::TogglePause),
        ID_QUIT => Some(MenuAction::Quit),
        _ => None,
    }
}

/// The menu, top to bottom, as (id, text, enabled); `None` is a separator. Built again only
/// when its shape changes (reviews come or go); otherwise only texts change.
pub fn menu_rows(m: &MenuModel) -> Vec<Option<(String, String, bool)>> {
    let mut v: Vec<Option<(String, String, bool)>> = Vec::new();
    for (i, line) in m.info.iter().enumerate() {
        v.push(Some((format!("cleancraft.info.{i}"), line.clone(), false)));
    }
    v.push(None);
    if !m.reviews.is_empty() {
        for (i, r) in m.reviews.iter().enumerate() {
            v.push(Some((format!("{ID_REVIEW}{i}"), r.text.clone(), true)));
        }
        v.push(None);
    }
    v.push(Some((
        ID_OPEN.into(),
        cleancraft_core::schedule::OPEN_APP.into(),
        true,
    )));
    v.push(Some((
        ID_SWEEP.into(),
        cleancraft_core::schedule::QUICK_SWEEP.into(),
        true,
    )));
    v.push(Some((ID_PAUSE.into(), m.pause_label().into(), true)));
    v.push(None);
    v.push(Some((
        ID_QUIT.into(),
        cleancraft_core::schedule::QUIT.into(),
        true,
    )));
    v
}

#[cfg(not(target_os = "macos"))]
pub fn run(models: Receiver<MenuModel>, actions: Sender<MenuAction>) -> anyhow::Result<()> {
    let _ = (models, actions);
    anyhow::bail!("The CleanCraft menu bar agent runs on macOS only.")
}

#[cfg(target_os = "macos")]
pub fn run(models: Receiver<MenuModel>, actions: Sender<MenuAction>) -> anyhow::Result<()> {
    mac::run(models, actions)
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::{Duration, Instant};

    use cleancraft_core::schedule::{MenuAction, MenuModel};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{TrayIcon, TrayIconBuilder};
    use winit::application::ApplicationHandler;
    use winit::event::{StartCause, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    use winit::window::WindowId;

    struct Tray {
        models: Receiver<MenuModel>,
        actions: Sender<MenuAction>,
        icon: Option<TrayIcon>,
        /// The rows the menu was built from, and its items (separators are `None`).
        rows: Vec<Option<(String, String, bool)>>,
        items: Vec<Option<MenuItem>>,
    }

    impl Tray {
        fn show(&mut self, m: &MenuModel) {
            let rows = super::menu_rows(m);
            let shape = |r: &[Option<(String, String, bool)>]| -> Vec<Option<String>> {
                r.iter()
                    .map(|x| x.as_ref().map(|(id, _, _)| id.clone()))
                    .collect()
            };
            if let Some(icon) = &self.icon {
                icon.set_title(Some(&m.title));
                if shape(&rows) == shape(&self.rows) {
                    for (row, item) in rows.iter().zip(&self.items) {
                        if let (Some((_, text, enabled)), Some(item)) = (row, item) {
                            item.set_text(text);
                            item.set_enabled(*enabled);
                        }
                    }
                } else {
                    let menu = Menu::new();
                    let mut items = Vec::new();
                    for row in &rows {
                        match row {
                            Some((id, text, enabled)) => {
                                let item = MenuItem::with_id(id.as_str(), text, *enabled, None);
                                let _ = menu.append(&item);
                                items.push(Some(item));
                            }
                            None => {
                                let _ = menu.append(&PredefinedMenuItem::separator());
                                items.push(None);
                            }
                        }
                    }
                    icon.set_menu(Some(Box::new(menu)));
                    self.items = items;
                }
            }
            self.rows = rows;
        }
    }

    impl ApplicationHandler for Tray {
        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            // The status item can be made only once the loop runs (tray-icon's rule).
            if matches!(cause, StartCause::Init) && self.icon.is_none() {
                match TrayIconBuilder::new()
                    .with_title("CleanCraft")
                    .with_tooltip("CleanCraft")
                    .build()
                {
                    Ok(icon) => self.icon = Some(icon),
                    Err(e) => {
                        eprintln!("CleanCraft agent: the menu bar item could not be made: {e}");
                        event_loop.exit();
                        return;
                    }
                }
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                match super::action_for(ev.id.as_ref()) {
                    Some(MenuAction::Quit) => {
                        event_loop.exit();
                        return;
                    }
                    Some(a) => {
                        let _ = self.actions.send(a);
                    }
                    None => {}
                }
            }
            let mut latest = None;
            loop {
                match self.models.try_recv() {
                    Ok(m) => latest = Some(m),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    // The worker ended (Quit, or the menu bar was turned off).
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        event_loop.exit();
                        return;
                    }
                }
            }
            if let Some(m) = latest {
                self.show(&m);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(250),
            ));
        }

        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }

    pub fn run(models: Receiver<MenuModel>, actions: Sender<MenuAction>) -> anyhow::Result<()> {
        let event_loop = EventLoop::builder()
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false)
            .build()?;
        let mut tray = Tray {
            models,
            actions,
            icon: None,
            rows: Vec::new(),
            items: Vec::new(),
        };
        event_loop.run_app(&mut tray)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::schedule::{Review, SWEEP_ROUTE};

    fn model(reviews: usize, paused: bool) -> MenuModel {
        MenuModel {
            title: "CPU 12% RAM 61%".into(),
            disk_low: false,
            info: [
                "CPU: 12%".into(),
                "Memory: 9.8 GB of 16 GB used".into(),
                "Free space: 81.9 GB".into(),
                "Trash: 4 MB".into(),
            ],
            reviews: (0..reviews)
                .map(|i| Review {
                    text: format!("Review {i}"),
                    route: SWEEP_ROUTE.into(),
                })
                .collect(),
            paused,
        }
    }

    fn texts(m: &MenuModel) -> Vec<String> {
        menu_rows(m)
            .into_iter()
            .map(|r| r.map(|(_, t, _)| t).unwrap_or_else(|| "---".into()))
            .collect()
    }

    #[test]
    fn the_menu_lists_readings_then_actions() {
        assert_eq!(
            texts(&model(0, false)),
            [
                "CPU: 12%",
                "Memory: 9.8 GB of 16 GB used",
                "Free space: 81.9 GB",
                "Trash: 4 MB",
                "---",
                "Open CleanCraft",
                "Quick Sweep...",
                "Pause Notifications",
                "---",
                "Quit Menu Bar Agent",
            ]
        );
        let with = texts(&model(1, true));
        assert_eq!(with[5], "Review 0");
        assert!(with.contains(&"Resume Notifications".to_string()));
        // Reading lines are not clickable; actions are.
        for (id, _, enabled) in menu_rows(&model(1, false)).into_iter().flatten() {
            assert_eq!(enabled, action_for(&id).is_some(), "{id}");
        }
    }

    #[test]
    fn ids_map_to_actions() {
        assert_eq!(action_for(ID_OPEN), Some(MenuAction::Open));
        assert_eq!(action_for(ID_SWEEP), Some(MenuAction::QuickSweep));
        assert_eq!(action_for(ID_PAUSE), Some(MenuAction::TogglePause));
        assert_eq!(action_for(ID_QUIT), Some(MenuAction::Quit));
        assert_eq!(
            action_for("cleancraft.review.2"),
            Some(MenuAction::Review(2))
        );
        assert_eq!(action_for("cleancraft.info.0"), None);
        assert_eq!(action_for("cleancraft.review.x"), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn elsewhere_the_agent_says_it_needs_macos() {
        let (_tx, models) = std::sync::mpsc::channel();
        let (actions, _rx) = std::sync::mpsc::channel();
        let e = run(models, actions).err().map(|e| e.to_string());
        assert_eq!(
            e.as_deref(),
            Some("The CleanCraft menu bar agent runs on macOS only.")
        );
    }
}
