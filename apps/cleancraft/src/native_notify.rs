//! Notifications posted as CleanCraft itself (U7-2), so macOS shows CleanCraft's name and icon
//! and a click opens the app on the notification's page (`notify::request_open`). Installed by
//! the menu bar agent with [`install`]; `cleancraft_core::notify::send` falls back to
//! `osascript` when this fails. No unsafe code here: the `mac-notification-sys` crate wraps the
//! macOS API. On other systems [`install`] does nothing.
//!
//! MAC-CHECK: the click route is unverified on a real Mac (see docs/FEATURES.md), so the texts
//! also keep the menu bar hint.

use cleancraft_core::Env;

/// Install the native sender for the agent's notifications (macOS only).
#[cfg(target_os = "macos")]
pub fn install(env: &Env) {
    use std::sync::mpsc;
    use std::time::Duration;

    use cleancraft_core::notify::{self, Notification};
    use mac_notification_sys::NotificationResponse;

    /// How long a failure to deliver may take to come back before the notification counts as
    /// shown (a shown one blocks its thread until it is clicked or closed).
    const DELIVERY_WAIT: Duration = Duration::from_secs(2);

    static APP_SET: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();

    let env = env.clone();
    notify::set_native_sender(Box::new(move |n: &Notification| {
        APP_SET
            .get_or_init(|| {
                mac_notification_sys::set_application(notify::BUNDLE_ID).map_err(|e| e.to_string())
            })
            .clone()?;
        let title = notify::clean_text(&n.title);
        let subtitle = n.subtitle.as_deref().map(notify::clean_text);
        let body = notify::shown_body(n);
        let route = n.route.clone();
        let env = env.clone();
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let mut m = mac_notification_sys::Notification::new();
            m.title(&title)
                .maybe_subtitle(subtitle.as_deref())
                .message(&body)
                .wait_for_click(route.is_some());
            let res = m.send();
            match res {
                Ok(NotificationResponse::Click | NotificationResponse::ActionButton(_)) => {
                    let _ = tx.send(Ok(()));
                    if let Some(r) = route
                        && let Err(e) = notify::request_open(&env, &r)
                    {
                        eprintln!("CleanCraft agent: {e}");
                    }
                }
                Ok(_) => {
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        });
        match rx.recv_timeout(DELIVERY_WAIT) {
            Ok(r) => r,
            // Still waiting for a click: it was delivered.
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("the notification thread stopped".into())
            }
        }
    }));
}

/// Other systems: nothing to install (notifications use `osascript`, which is macOS only too).
#[cfg(not(target_os = "macos"))]
pub fn install(env: &Env) {
    let _ = env;
}
