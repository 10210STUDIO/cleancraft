//! User settings, shared by the app, the CLI and the MCP server:
//! `~/Library/Application Support/CleanCraft/settings.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::Env;
use crate::executor::Method;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// Where removed items go.
    pub method: Method,
    /// Paths never proposed by any scan (and everything inside them).
    pub ignore: Vec<PathBuf>,
    /// Large & Old Files: minimum size in bytes.
    pub large_min_bytes: u64,
    /// Large & Old Files: "old" means not opened for this many days.
    pub old_days: u64,
    /// Unused disk images and old downloads: days.
    pub downloads_days: u64,
    /// Let AI agents (MCP) carry out confirmed plans. Off: agents can scan and preview only.
    pub agents_may_clean: bool,
    /// Show the menu bar monitor.
    pub menu_bar: bool,
    /// Languages to keep when reporting language files (besides English and Base).
    pub keep_languages: Vec<String>,
    /// Quick Sweep: modules the user turned off (shown dimmed, not run).
    pub sweep_skip: Vec<crate::plan::FeatureId>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            method: Method::Trash,
            ignore: Vec::new(),
            large_min_bytes: 50_000_000,
            old_days: 365,
            downloads_days: 90,
            agents_may_clean: false,
            menu_bar: true,
            keep_languages: Vec::new(),
            sweep_skip: Vec::new(),
        }
    }
}

impl Settings {
    pub fn path(env: &Env) -> PathBuf {
        env.data_dir().join("settings.json")
    }

    /// Load settings; when the file exists but cannot be read as settings (cut short by a crash
    /// or a full disk), keep a copy of it next to it and return a warning to show, so the user
    /// knows their ignore list and choices were not applied.
    pub fn load_checked(env: &Env) -> (Settings, Option<String>) {
        let path = Self::path(env);
        let Ok(bytes) = std::fs::read(&path) else {
            return (Settings::default(), None);
        };
        match serde_json::from_slice(&bytes) {
            Ok(s) => (s, None),
            Err(_) => {
                let copy = path.with_extension(format!(
                    "damaged-{}.json",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                ));
                let kept = std::fs::write(&copy, &bytes).is_ok();
                let msg = if kept {
                    format!(
                        "Your settings file could not be read, so default settings are in use (ignore list empty). A copy of the damaged file is at {}.",
                        env.display(&copy)
                    )
                } else {
                    "Your settings file could not be read, so default settings are in use (ignore list empty).".to_string()
                };
                (Settings::default(), Some(msg))
            }
        }
    }

    pub fn load(env: &Env) -> Settings {
        std::fs::read(Self::path(env))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, env: &Env) -> std::io::Result<()> {
        std::fs::create_dir_all(env.data_dir())?;
        let tmp = Self::path(env).with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, Self::path(env))
    }

    /// Is `p` on the ignore list (or inside an entry)? Names compare like a default Mac volume
    /// (letter case and Unicode composition do not matter): `~/downloads/x` is `~/Downloads/x`.
    pub fn is_ignored(&self, p: &Path) -> bool {
        self.ignore
            .iter()
            .any(|i| p.starts_with(i) || crate::policy::starts_with_ci(p, i))
    }
}
