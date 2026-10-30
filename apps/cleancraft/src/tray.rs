//! The menu bar status item: the only platform code of the agent.
//!
//! macOS: a `tray-icon` status item on winit's event loop, as an accessory app (no Dock icon).
//! It shows an icon only, a template image macOS tints for light and dark menu bars, with a
//! small dot while something waits for review. A left click opens the live panel under the
//! icon and a second click closes it; a right click or Control-click shows the compact menu
//! (muda, re-exported as `tray_icon::menu`) as before.
//!
//! The panel is a separate short-lived process, `cleancraft panel --at x,y` (see
//! `crate::panel` for why): this file only works out where it goes from the icon's rectangle
//! and the displays ([`cleancraft_core::menubar::panel_origin`]), starts it, forwards each
//! command line it prints to the worker as a [`MenuAction`], and closes it by closing its
//! standard input. Open and closed state, including "the click that made the panel lose focus
//! does not reopen it", is [`cleancraft_core::menubar::PanelToggle`]. Elsewhere: a stub that
//! says the agent needs macOS, so the workspace builds and tests on Linux.
//!
//! The macOS part cannot be compiled on the Linux build box: it is written against tray-icon
//! 0.26.1, muda 0.21.2, winit 0.30.13 and objc2-app-kit 0.3.2 as published, and CI checks it on
//! macos-14 for every pull request.

// The menu model helpers are used by the macOS glue and the tests only.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::mpsc::{Receiver, Sender};

use cleancraft_core::menubar;
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

/// The menu bar template at 2x (18 pt, 36 px). The brand's file replaces the placeholder at
/// this path; any square size is scaled to 36 px.
const TEMPLATE_2X: &[u8] = include_bytes!("../../../assets/MenuBarTemplate@2x.png");

/// The status item image, RGBA [`menubar::ICON_PX`] square: the template centred, with the
/// review dot when `badge` is on. `None` when the PNG cannot be read.
pub fn icon_rgba(badge: bool) -> Option<Vec<u8>> {
    let img = image::load_from_memory(TEMPLATE_2X).ok()?.to_rgba8();
    let side = menubar::TEMPLATE_PX;
    let img = if img.dimensions() == (side, side) {
        img
    } else {
        image::imageops::resize(&img, side, side, image::imageops::FilterType::Lanczos3)
    };
    Some(menubar::status_icon(img.as_raw(), side, badge))
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
    use std::io::Write;
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::{Duration, Instant};

    use cleancraft_core::menubar::{self, ClickDo, PanelCmd, PanelToggle, PhysRect, Screen};
    use cleancraft_core::schedule::{MenuAction, MenuModel};
    use cleancraft_core::sys::{RealSys, Sys};
    use objc2_app_kit::{NSEvent, NSEventModifierFlags};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };
    use winit::application::ApplicationHandler;
    use winit::event::{StartCause, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    use winit::window::WindowId;

    /// What wakes the loop besides its timer.
    #[derive(Debug)]
    enum Wake {
        Tray(TrayIconEvent),
        Menu(MenuEvent),
        /// The panel process ended.
        PanelClosed,
    }

    struct Tray {
        models: Receiver<MenuModel>,
        actions: Sender<MenuAction>,
        proxy: EventLoopProxy<Wake>,
        icon: Option<TrayIcon>,
        /// The badge the icon shows now.
        badge: bool,
        /// The rows the menu was built from, and its items (separators are `None`).
        rows: Vec<Option<(String, String, bool)>>,
        items: Vec<Option<MenuItem>>,
        toggle: PanelToggle,
        /// The open panel's standard input: dropping it closes the panel.
        panel_in: Option<Box<dyn Write + Send>>,
    }

    fn image(badge: bool) -> Option<Icon> {
        let rgba = super::icon_rgba(badge)?;
        Icon::from_rgba(rgba, menubar::ICON_PX, menubar::ICON_PX).ok()
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
                // Figures live in the panel; the tooltip keeps a short reading.
                let _ = icon.set_tooltip(Some(format!("CleanCraft: {}", m.title)));
                let badge = !m.reviews.is_empty();
                if badge != self.badge
                    && let Some(img) = image(badge)
                    && icon.set_icon_templated(Some(img)).is_ok()
                {
                    self.badge = badge;
                }
                if shape(&rows) == shape(&self.rows) {
                    for (row, item) in rows.iter().zip(&self.items) {
                        if let (Some((_, text, enabled)), Some(item)) = (row, item) {
                            item.set_text(cleancraft_core::i18n::tr(text));
                            item.set_enabled(*enabled);
                        }
                    }
                } else {
                    let menu = Menu::new();
                    let mut items = Vec::new();
                    for row in &rows {
                        match row {
                            Some((id, text, enabled)) => {
                                let item = MenuItem::with_id(
                                    id.as_str(),
                                    cleancraft_core::i18n::tr(text),
                                    *enabled,
                                    None,
                                );
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

        fn click(&mut self, event_loop: &ActiveEventLoop, rect: Rect) {
            // Control-click is the Mac's secondary click: the compact menu.
            if NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Control) {
                if let Some(icon) = &self.icon {
                    icon.show_menu();
                }
                return;
            }
            match self.toggle.click(Instant::now()) {
                ClickDo::Open => self.open_panel(event_loop, rect),
                // Closing its standard input closes the panel; PanelClosed follows.
                ClickDo::Close => self.panel_in = None,
                ClickDo::Nothing => {}
            }
        }

        fn open_panel(&mut self, event_loop: &ActiveEventLoop, rect: Rect) {
            let screens: Vec<Screen> = event_loop
                .available_monitors()
                .map(|m| {
                    let (p, s) = (m.position(), m.size());
                    Screen {
                        rect: PhysRect {
                            x: f64::from(p.x),
                            y: f64::from(p.y),
                            w: f64::from(s.width),
                            h: f64::from(s.height),
                        },
                        scale: m.scale_factor(),
                    }
                })
                .collect();
            let icon = PhysRect {
                x: rect.position.x,
                y: rect.position.y,
                w: f64::from(rect.size.width),
                h: f64::from(rect.size.height),
            };
            let (x, y) = menubar::panel_origin(icon, &screens, menubar::PANEL_W, menubar::PANEL_H);
            let exe = match std::env::current_exe() {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("CleanCraft agent: the panel could not open: {e}");
                    self.toggle.closed(Instant::now());
                    return;
                }
            };
            let exe = exe.to_string_lossy().into_owned();
            let at = menubar::at_arg(x, y);
            let child = RealSys.spawn_piped(&exe, &["panel", "--at", &at]);
            let child = match child {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("CleanCraft agent: the panel could not open: {e}");
                    self.toggle.closed(Instant::now());
                    return;
                }
            };
            self.panel_in = Some(child.stdin);
            let actions = self.actions.clone();
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                for line in child.lines {
                    if let Some(c) = PanelCmd::parse(&line) {
                        let _ = actions.send(c.action());
                    }
                }
                (child.wait)();
                let _ = proxy.send_event(Wake::PanelClosed);
            });
        }
    }

    impl ApplicationHandler<Wake> for Tray {
        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            // The status item can be made only once the loop runs (tray-icon's rule).
            if matches!(cause, StartCause::Init) && self.icon.is_none() {
                let mut b = TrayIconBuilder::new()
                    .with_tooltip("CleanCraft")
                    .with_menu_on_left_click(false)
                    .with_menu_on_right_click(true);
                if let Some(img) = image(false) {
                    b = b.with_icon_templated(img);
                } else {
                    // The image could not be read: a title keeps the item findable.
                    b = b.with_title("CleanCraft");
                }
                match b.build() {
                    Ok(icon) => self.icon = Some(icon),
                    Err(e) => {
                        eprintln!("CleanCraft agent: the menu bar item could not be made: {e}");
                        event_loop.exit();
                        return;
                    }
                }
            }
            let mut latest = None;
            loop {
                match self.models.try_recv() {
                    Ok(m) => latest = Some(m),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    // The worker ended (Quit, or the menu bar was turned off).
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.panel_in = None;
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

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wake) {
            match event {
                Wake::Tray(TrayIconEvent::Click {
                    rect,
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Down,
                    ..
                }) => self.click(event_loop, rect),
                Wake::Tray(_) => {}
                Wake::Menu(ev) => match super::action_for(ev.id.as_ref()) {
                    Some(MenuAction::Quit) => {
                        self.panel_in = None;
                        event_loop.exit();
                    }
                    Some(a) => {
                        let _ = self.actions.send(a);
                    }
                    None => {}
                },
                Wake::PanelClosed => {
                    self.panel_in = None;
                    self.toggle.closed(Instant::now());
                }
            }
        }

        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }

    pub fn run(models: Receiver<MenuModel>, actions: Sender<MenuAction>) -> anyhow::Result<()> {
        let event_loop = EventLoop::<Wake>::with_user_event()
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false)
            .build()?;
        // Clicks and menu choices wake the loop at once (no polling delay).
        let proxy = event_loop.create_proxy();
        let p = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            let _ = p.send_event(Wake::Tray(e));
        }));
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = p.send_event(Wake::Menu(e));
        }));
        let mut tray = Tray {
            models,
            actions,
            proxy,
            icon: None,
            badge: false,
            rows: Vec::new(),
            items: Vec::new(),
            toggle: PanelToggle::default(),
            panel_in: None,
        };
        event_loop.run_app(&mut tray)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
                "Pause Scans and Notifications",
                "---",
                "Quit Menu Bar Agent",
            ]
        );
        let with = texts(&model(1, true));
        assert_eq!(with[5], "Review 0");
        assert!(with.contains(&"Resume Scans and Notifications".to_string()));
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

    #[test]
    fn the_icon_is_the_template_with_an_optional_dot() {
        let n = menubar::ICON_PX as usize;
        let plain = icon_rgba(false).expect("the template reads");
        let dot = icon_rgba(true).expect("badge");
        assert_eq!(plain.len(), n * n * 4);
        let alpha = |v: &[u8], x: usize, y: usize| v[(y * n + x) * 4 + 3];
        // Template: only alpha and black, so macOS can tint it.
        assert!(plain.chunks(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
        assert!(plain.chunks(4).any(|p| p[3] > 200), "something is drawn");
        assert_eq!(alpha(&plain, n - 7, 6), 0);
        assert_eq!(alpha(&dot, n - 7, 6), 255);
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
