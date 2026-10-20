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
}

impl AppRoute {
    /// The string form: "system_junk", "scan:system_junk", "health", "history", "settings",
    /// "guide:ai_agents", "unused_apps", "uninstall:/Applications/X.app".
    pub fn to_arg(&self) -> String {
        match self {
            AppRoute::Feature(f) => f.as_str().to_string(),
            AppRoute::Scan(f) => format!("scan:{}", f.as_str()),
            AppRoute::Health => "health".into(),
            AppRoute::History => "history".into(),
            AppRoute::Settings => "settings".into(),
            AppRoute::AgentGuide => "guide:ai_agents".into(),
            AppRoute::UnusedApps => "unused_apps".into(),
            AppRoute::ConnectAi => "connect_ai".into(),
            AppRoute::UninstallApp(p) => format!("uninstall:{}", p.display()),
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
        if let Some(f) = s.strip_prefix("scan:") {
            return FeatureId::parse(f).map(AppRoute::Scan);
        }
        match s.to_lowercase().replace(['-', ' '], "_").as_str() {
            "health" | "mac_health" => Some(AppRoute::Health),
            "history" => Some(AppRoute::History),
            "settings" => Some(AppRoute::Settings),
            "guide:ai_agents" | "ai_agents_guide" | "guide" => Some(AppRoute::AgentGuide),
            "unused_apps" => Some(AppRoute::UnusedApps),
            "connect_ai" | "welcome" => Some(AppRoute::ConnectAi),
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
            AppRoute::History,
            AppRoute::Settings,
            AppRoute::AgentGuide,
            AppRoute::UnusedApps,
            AppRoute::ConnectAi,
            AppRoute::UninstallApp(PathBuf::from("/Applications/Editor.app")),
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
        assert_eq!(AppRoute::parse("uninstall:relative/X.app"), None);
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
