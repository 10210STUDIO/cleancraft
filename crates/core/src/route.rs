//! Where the app opens: a page, a page that starts its scan, or App Remover on one app.
//!
//! Shared by the window (`CleanApp::open_route`), the menu bar agent and the watchers, which
//! name a page without depending on the UI crate. A route only opens a page or starts a scan
//! (look only); it never runs a cleanup. The string form travels on the command line
//! (`cleancraft --open <route>`) and in notifications.

use std::path::PathBuf;

use crate::plan::FeatureId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppRoute {
    /// A module's page as the person left it.
    Feature(FeatureId),
    /// A module's page, starting its scan when nothing is shown yet (a scheduled scan's
    /// result, a notification). The scan only looks; the person reviews and confirms.
    Scan(FeatureId),
    /// Mac Health (a tool of the Live Monitor section).
    Health,
    /// Security Check (a tool of the Protect section).
    Security,
    History,
    Settings,
    /// The in-app guide "Connect an AI agent" (docs/guide-ai-agents.md).
    AgentGuide,
    /// The "Connect your AI apps" step (also shown once at first launch).
    ConnectAi,
    /// App Remover with its "Unused" list shown.
    UnusedApps,
    /// App Remover on one app (its files listed for review), e.g. after the app was dragged
    /// to the Trash.
    UninstallApp(PathBuf),
    /// Threat Check with one finding (a startup item's property list or an app) selected for
    /// review; the scan starts when nothing is shown yet. Selecting never removes anything.
    ThreatReview(PathBuf),
    /// Where to turn on Full Disk Access (Settings, its Full Disk Access group).
    FullDiskAccess,
    /// macOS System Settings > Battery (outside the app; the page stays).
    BatterySettings,
    /// macOS System Settings > Privacy & Security (outside the app; the page stays). FileVault
    /// and the firewall both live there.
    SecurityPrivacySettings,
    /// macOS System Settings > Users & Groups (outside the app; the page stays). Automatic
    /// login and the guest account both live there.
    UsersGroupsSettings,
    /// macOS System Settings > Lock Screen (outside the app; the page stays).
    LockScreenSettings,
    /// Tune-up Tasks with one task ticked and its confirmation shown (the exact command);
    /// nothing runs until the person confirms.
    Task(String),
}

/// The System Settings pane [`AppRoute::BatterySettings`] opens.
pub const BATTERY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Battery-Settings.extension";
/// The System Settings pane [`AppRoute::SecurityPrivacySettings`] opens.
pub const SECURITY_PRIVACY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension";
/// The System Settings pane [`AppRoute::UsersGroupsSettings`] opens.
pub const USERS_GROUPS_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Users-Groups-Settings.extension";
/// The System Settings pane [`AppRoute::LockScreenSettings`] opens.
pub const LOCK_SCREEN_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Lock-Screen-Settings.extension";

impl AppRoute {
    /// The string form: "system_junk", "scan:system_junk", "health", "history", "settings",
    /// "guide:ai_agents", "unused_apps", "uninstall:/Applications/X.app",
    /// "threat:/Library/LaunchAgents/x.plist", "full_disk_access", "task:free_ram".
    pub fn to_arg(&self) -> String {
        match self {
            AppRoute::Feature(f) => f.as_str().to_string(),
            AppRoute::Scan(f) => format!("scan:{}", f.as_str()),
            AppRoute::Health => "health".into(),
            AppRoute::Security => "security".into(),
            AppRoute::History => "history".into(),
            AppRoute::Settings => "settings".into(),
            AppRoute::AgentGuide => "guide:ai_agents".into(),
            AppRoute::UnusedApps => "unused_apps".into(),
            AppRoute::ConnectAi => "connect_ai".into(),
            AppRoute::UninstallApp(p) => format!("uninstall:{}", p.display()),
            AppRoute::ThreatReview(p) => format!("threat:{}", p.display()),
            AppRoute::FullDiskAccess => "full_disk_access".into(),
            AppRoute::BatterySettings => "battery_settings".into(),
            AppRoute::SecurityPrivacySettings => "security_privacy_settings".into(),
            AppRoute::UsersGroupsSettings => "users_groups_settings".into(),
            AppRoute::LockScreenSettings => "lock_screen_settings".into(),
            AppRoute::Task(id) => format!("task:{id}"),
        }
    }

    /// Read the string form. Unknown names give None (the caller stays where it is).
    pub fn parse(s: &str) -> Option<AppRoute> {
        let s = s.trim();
        if let Some(path) = s.strip_prefix("uninstall:") {
            let p = PathBuf::from(path.trim());
            // Only an absolute path to an app bundle.
            let ok = p.is_absolute()
                && p.extension().is_some_and(|e| e == "app")
                && !crate::env::has_parent_component(&p);
            return ok.then_some(AppRoute::UninstallApp(p));
        }
        if let Some(path) = s.strip_prefix("threat:") {
            let p = PathBuf::from(path.trim());
            let ok = p.is_absolute() && !crate::env::has_parent_component(&p);
            return ok.then_some(AppRoute::ThreatReview(p));
        }
        if let Some(id) = s.strip_prefix("task:") {
            return crate::tasks::info(id.trim()).map(|t| AppRoute::Task(t.id.to_string()));
        }
        if let Some(f) = s.strip_prefix("scan:") {
            return FeatureId::parse(f).map(AppRoute::Scan);
        }
        match s.to_lowercase().replace(['-', ' '], "_").as_str() {
            "health" | "mac_health" => Some(AppRoute::Health),
            "security" | "security_check" => Some(AppRoute::Security),
            "history" => Some(AppRoute::History),
            "settings" => Some(AppRoute::Settings),
            "guide:ai_agents" | "ai_agents_guide" | "guide" => Some(AppRoute::AgentGuide),
            "unused_apps" => Some(AppRoute::UnusedApps),
            "connect_ai" | "welcome" => Some(AppRoute::ConnectAi),
            "full_disk_access" | "fda" => Some(AppRoute::FullDiskAccess),
            "battery_settings" => Some(AppRoute::BatterySettings),
            "security_privacy_settings" => Some(AppRoute::SecurityPrivacySettings),
            "users_groups_settings" => Some(AppRoute::UsersGroupsSettings),
            "lock_screen_settings" => Some(AppRoute::LockScreenSettings),
            other => FeatureId::parse(other).map(AppRoute::Feature),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_round_trips() {
        let mut all = vec![
            AppRoute::Health,
            AppRoute::Security,
            AppRoute::History,
            AppRoute::Settings,
            AppRoute::AgentGuide,
            AppRoute::UnusedApps,
            AppRoute::ConnectAi,
            AppRoute::UninstallApp(PathBuf::from("/Applications/Editor.app")),
            AppRoute::ThreatReview(PathBuf::from("/Library/LaunchAgents/com.x.helper.plist")),
            AppRoute::FullDiskAccess,
            AppRoute::BatterySettings,
            AppRoute::SecurityPrivacySettings,
            AppRoute::UsersGroupsSettings,
            AppRoute::LockScreenSettings,
            AppRoute::Task("free_ram".into()),
        ];
        for f in FeatureId::ALL {
            all.push(AppRoute::Feature(f));
            all.push(AppRoute::Scan(f));
        }
        for r in all {
            assert_eq!(AppRoute::parse(&r.to_arg()), Some(r.clone()), "{r:?}");
        }
    }

    #[test]
    fn odd_input_is_refused() {
        assert_eq!(AppRoute::parse("nowhere"), None);
        assert_eq!(AppRoute::parse("scan:nowhere"), None);
        assert_eq!(AppRoute::parse("task:rm_rf"), None);
        assert_eq!(AppRoute::parse("uninstall:relative/X.app"), None);
        assert_eq!(AppRoute::parse("threat:relative/x.plist"), None);
        assert_eq!(AppRoute::parse("threat:/Library/../etc/x.plist"), None);
        assert_eq!(AppRoute::parse("uninstall:/Applications/notes.txt"), None);
        assert_eq!(
            AppRoute::parse("uninstall:/Applications/../etc/X.app"),
            None
        );
        assert_eq!(
            AppRoute::parse(" System Junk "),
            Some(AppRoute::Feature(FeatureId::SystemJunk))
        );
    }
}
