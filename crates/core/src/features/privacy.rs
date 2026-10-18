//! Privacy Traces.
//!
//! - Browsers (P3): per browser and profile, the files holding history, cookies, cache, the
//!   last session, the download list and form values. Saved passwords are listed and never
//!   preselected. A browser that is running has every item locked ("Quit <browser> first"), and
//!   the cleanup refuses to start while a browser with selected items runs.
//! - Recent items (P4): the recent documents, apps and servers lists (`.sfl2`, `.sfl3`).
//! - Wi-Fi networks (P5): listed from `networksetup -listpreferredwirelessnetworks <device>`;
//!   macOS removes them in System Settings, so they are locked and the page offers a button to
//!   open the Wi-Fi pane.
//! - App permissions (P6): read from the TCC databases with `sqlite3 -readonly` (needs Full
//!   Disk Access); taken back with `tccutil reset <Service> <bundle id>`, logged as a command.
//!   Wi-Fi and permission items are not files; their paths are labels under the database they
//!   come from, they are always locked and nothing of them is in the allow-list.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Plan, Risk};

pub struct Privacy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Safari,
    Chromium {
        data: &'static str,
        cache: &'static str,
    },
    Firefox,
}

pub struct Browser {
    /// Category id.
    pub id: &'static str,
    pub name: &'static str,
    /// Process names as `ps` shows them (app bundle name).
    pub process: &'static [&'static str],
    pub engine: Engine,
}

pub const BROWSERS: &[Browser] = &[
    Browser {
        id: "safari",
        name: "Safari",
        process: &["Safari"],
        engine: Engine::Safari,
    },
    Browser {
        id: "chrome",
        name: "Google Chrome",
        process: &["Google Chrome"],
        engine: Engine::Chromium {
            data: "Library/Application Support/Google/Chrome",
            cache: "Library/Caches/Google/Chrome",
        },
    },
    Browser {
        id: "brave",
        name: "Brave",
        process: &["Brave Browser"],
        engine: Engine::Chromium {
            data: "Library/Application Support/BraveSoftware/Brave-Browser",
            cache: "Library/Caches/BraveSoftware/Brave-Browser",
        },
    },
    Browser {
        id: "edge",
        name: "Microsoft Edge",
        process: &["Microsoft Edge"],
        engine: Engine::Chromium {
            data: "Library/Application Support/Microsoft Edge",
            cache: "Library/Caches/Microsoft Edge",
        },
    },
    Browser {
        id: "vivaldi",
        name: "Vivaldi",
        process: &["Vivaldi"],
        engine: Engine::Chromium {
            data: "Library/Application Support/Vivaldi",
            cache: "Library/Caches/Vivaldi",
        },
    },
    Browser {
        id: "arc",
        name: "Arc",
        process: &["Arc"],
        engine: Engine::Chromium {
            data: "Library/Application Support/Arc/User Data",
            cache: "Library/Caches/Arc/User Data",
        },
    },
    Browser {
        id: "firefox",
        name: "Firefox",
        process: &["Firefox", "firefox"],
        engine: Engine::Firefox,
    },
];

/// What a browser file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub label: &'static str,
    pub what: &'static str,
    pub risk: Risk,
    pub selected: bool,
}

pub const HISTORY: Kind = Kind {
    label: "History",
    what: "browsing history and download list",
    risk: Risk::Medium,
    selected: true,
};
pub const COOKIES: Kind = Kind {
    label: "Cookies",
    what: "cookies (you will be signed out of websites)",
    risk: Risk::Medium,
    selected: true,
};
pub const CACHE: Kind = Kind {
    label: "Cache",
    what: "cached web pages and images",
    risk: Risk::Low,
    selected: true,
};
pub const SESSIONS: Kind = Kind {
    label: "Last session",
    what: "tabs of the last session",
    risk: Risk::Medium,
    selected: true,
};
pub const DOWNLOADS: Kind = Kind {
    label: "Download list",
    what: "list of downloads (the files stay)",
    risk: Risk::Low,
    selected: true,
};
pub const FORMS: Kind = Kind {
    label: "Form values",
    what: "values typed in web forms",
    risk: Risk::Medium,
    selected: true,
};
/// Chromium keeps saved addresses, cards and search engines in the same file as form values.
pub const FORMS_CHROMIUM: Kind = Kind {
    label: "Form values",
    what: "autofill entries, saved addresses and cards and search engines",
    risk: Risk::Medium,
    selected: false,
};
/// Firefox keeps bookmarks in the same file as history.
pub const HISTORY_FIREFOX: Kind = Kind {
    label: "History",
    what: "browsing history, download list and bookmarks (one file in Firefox)",
    risk: Risk::High,
    selected: false,
};
pub const PASSWORDS: Kind = Kind {
    label: "Saved passwords",
    what: "saved passwords",
    risk: Risk::High,
    selected: false,
};

pub const CHROMIUM_FILES: &[(Kind, &[&str])] = &[
    (
        HISTORY,
        &[
            "History",
            "History-journal",
            "Visited Links",
            "Top Sites",
            "Top Sites-journal",
            "Shortcuts",
            "Shortcuts-journal",
        ],
    ),
    (
        COOKIES,
        &[
            "Cookies",
            "Cookies-journal",
            "Network/Cookies",
            "Network/Cookies-journal",
        ],
    ),
    (
        SESSIONS,
        &[
            "Current Session",
            "Current Tabs",
            "Last Session",
            "Last Tabs",
            "Sessions",
        ],
    ),
    (FORMS_CHROMIUM, &["Web Data", "Web Data-journal"]),
    (
        PASSWORDS,
        &[
            "Login Data",
            "Login Data-journal",
            "Login Data For Account",
            "Login Data For Account-journal",
        ],
    ),
];

pub const FIREFOX_FILES: &[(Kind, &[&str])] = &[
    (HISTORY_FIREFOX, &["places.sqlite", "places.sqlite-wal"]),
    (COOKIES, &["cookies.sqlite", "cookies.sqlite-wal"]),
    (FORMS, &["formhistory.sqlite"]),
    (SESSIONS, &["sessionstore.jsonlz4", "sessionstore-backups"]),
    (PASSWORDS, &["logins.json", "logins-backup.json"]),
];

pub const SAFARI_DIR: &str = "Library/Safari";
pub const SAFARI_CONTAINER_COOKIES: &str =
    "Library/Containers/com.apple.Safari/Data/Library/Cookies";
pub const SAFARI_CONTAINER_CACHES: &str = "Library/Containers/com.apple.Safari/Data/Library/Caches";
pub const OLD_COOKIES: &str = "Library/Cookies";
pub const FIREFOX_PROFILES: &str = "Library/Application Support/Firefox/Profiles";
pub const FIREFOX_CACHE: &str = "Library/Caches/Firefox/Profiles";
pub const SHARED_FILE_LIST: &str = "Library/Application Support/com.apple.sharedfilelist";
pub const RECENT_DOCS_DIR: &str = "com.apple.LSSharedFileList.ApplicationRecentDocuments";

pub const KNOWN_NETWORKS: &str = "/Library/Preferences/com.apple.wifi.known-networks.plist";
pub const WIFI_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.wifi-settings-extension";
pub const NETWORKSETUP: &str = "/usr/sbin/networksetup";

pub const TCC_USER: &str = "Library/Application Support/com.apple.TCC/TCC.db";
pub const TCC_SYSTEM: &str = "/Library/Application Support/com.apple.TCC/TCC.db";
pub const TCC_QUERY: &str = "SELECT service, client, auth_value FROM access WHERE client_type = 0;";
pub const SQLITE3: &str = "/usr/bin/sqlite3";
pub const TCCUTIL: &str = "/usr/bin/tccutil";

pub const LOCK_WIFI: &str =
    "macOS removes known networks in System Settings > Wi-Fi: use Open Wi-Fi Settings";
pub const LOCK_PERMISSION: &str =
    "Not a file: use Reset Permissions to take this access back (the app asks again next time)";
pub const NOTE_TCC: &str =
    "App permissions could not be read. Turn on Full Disk Access for CleanCraft to list them.";

pub fn lock_running(browser: &str) -> String {
    format!("Quit {browser} first")
}

/// Which browsers are running now.
pub fn running_browsers(env: &Env) -> Vec<&'static Browser> {
    let apps = env.sys.running_apps();
    BROWSERS
        .iter()
        .filter(|b| apps.iter().any(|a| b.process.iter().any(|p| a.name == *p)))
        .collect()
}

/// Chromium profile folders: those with a Preferences file, except system and guest ones.
pub fn chromium_profiles(data: &Path) -> Vec<PathBuf> {
    children(data)
        .into_iter()
        .filter(|p| {
            let n = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            n != "System Profile"
                && n != "Guest Profile"
                && std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
                && p.join("Preferences").is_file()
        })
        .collect()
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Parse `networksetup -listallhardwareports`: the device of the Wi-Fi port.
pub fn parse_wifi_device(out: &str) -> Option<String> {
    let mut wifi = false;
    for l in out.lines() {
        let l = l.trim();
        if let Some(port) = l.strip_prefix("Hardware Port:") {
            let port = port.trim();
            wifi = port == "Wi-Fi" || port == "AirPort";
        } else if wifi && let Some(dev) = l.strip_prefix("Device:") {
            return Some(dev.trim().to_string());
        }
    }
    None
}

/// Parse `networksetup -listpreferredwirelessnetworks <dev>`: "Preferred networks on en0:" and
/// one tab-indented name per line. None when the output is an error.
pub fn parse_preferred_networks(out: &str) -> Option<Vec<String>> {
    let mut lines = out.lines();
    let first = lines.next()?.trim();
    if !first.starts_with("Preferred networks on") {
        return None;
    }
    Some(
        lines
            .map(|l| l.trim_start_matches('\t').trim_end().to_string())
            .filter(|l| !l.trim().is_empty())
            .collect(),
    )
}

/// Parse `sqlite3 -separator '|'` rows of [`TCC_QUERY`]: (service, client) with access granted
/// (auth_value 2) or limited (3).
pub fn parse_tcc_rows(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.trim().split('|');
            let service = it.next()?.trim().to_string();
            let client = it.next()?.trim().to_string();
            let auth: i64 = it.next()?.trim().parse().ok()?;
            (auth == 2 || auth == 3).then_some((service, client))
        })
        .filter(|(s, c)| !s.is_empty() && !c.is_empty())
        .collect()
}

/// "kTCCServiceCamera" -> "Camera" (the name `tccutil` takes).
pub fn tccutil_service(service: &str) -> &str {
    service.strip_prefix("kTCCService").unwrap_or(service)
}

/// What a TCC service means, in plain words.
pub fn service_title(service: &str) -> String {
    let s = tccutil_service(service);
    match s {
        "Camera" => "Camera".into(),
        "Microphone" => "Microphone".into(),
        "ScreenCapture" => "Screen recording".into(),
        "SystemPolicyAllFiles" => "Full Disk Access".into(),
        "SystemPolicyDocumentsFolder" => "Documents folder".into(),
        "SystemPolicyDesktopFolder" => "Desktop folder".into(),
        "SystemPolicyDownloadsFolder" => "Downloads folder".into(),
        "SystemPolicyRemovableVolumes" => "Removable disks".into(),
        "SystemPolicyNetworkVolumes" => "Network disks".into(),
        "AddressBook" => "Contacts".into(),
        "Calendar" => "Calendars".into(),
        "Reminders" => "Reminders".into(),
        "Photos" => "Photos".into(),
        "Accessibility" => "Accessibility".into(),
        "ListenEvent" => "Input monitoring".into(),
        "PostEvent" => "Keyboard and mouse control".into(),
        "AppleEvents" => "Automation".into(),
        "BluetoothAlways" => "Bluetooth".into(),
        "MediaLibrary" => "Media library".into(),
        "SpeechRecognition" => "Speech recognition".into(),
        other => other.to_string(),
    }
}

fn safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// The TCC databases: (path, label).
pub fn tcc_dbs(env: &Env) -> Vec<PathBuf> {
    vec![env.home_path(TCC_USER), env.sys_path(TCC_SYSTEM)]
}

/// The (service, client) a permission item stands for.
pub fn permission_of(env: &Env, item: &Path) -> Option<(String, String)> {
    let client = item.file_name()?.to_string_lossy().into_owned();
    let svc_dir = item.parent()?;
    let service = svc_dir.file_name()?.to_string_lossy().into_owned();
    let db = svc_dir.parent()?;
    tcc_dbs(env)
        .iter()
        .any(|d| d == db)
        .then_some((service, client))
}

/// Take an app's access back with `tccutil reset`. Logged as a command.
pub fn reset_permission(env: &Env, service: &str, client: &str) -> Result<String, String> {
    let svc = tccutil_service(service);
    if !safe_token(svc) || !safe_token(client) {
        return Err(format!("{service} for {client} cannot be reset by name"));
    }
    // The log must be writable before anything changes.
    crate::features::trash::journal_ready(env)
        .map_err(|e| format!("the action log could not be written ({e}); nothing was reset"))?;
    let res = env.sys.run(TCCUTIL, &["reset", svc, client]);
    let (ok, output) = match &res {
        Ok(o) => (
            o.ok(),
            format!("{}{}", o.stdout, o.stderr).trim().to_string(),
        ),
        Err(e) => (false, e.to_string()),
    };
    let now = chrono::Utc::now();
    let rec = ActionRecord {
        id: format!("tcc-{svc}-{client}-{}", now.format("%Y%m%d%H%M%S%3f")),
        plan_id: format!("privacy-permissions-{}", now.format("%Y%m%d%H%M%S%3f")),
        feature: Some(FeatureId::Privacy),
        time: now.timestamp(),
        size: 0,
        kind: ActionKind::Command {
            command: format!("tccutil reset {svc} {client}"),
            ok,
            output: output.clone(),
        },
        undone: None,
    };
    if let Err(e) = executor::append_journal(env, &rec) {
        return Err(format!(
            "tccutil ran, but the action log could not be written: {e}"
        ));
    }
    if ok {
        Ok(format!(
            "{} access of {client} was reset; it asks again next time.",
            service_title(service)
        ))
    } else {
        Err(format!(
            "tccutil could not reset {} for {client}: {output}",
            service_title(service)
        ))
    }
}

impl Feature for Privacy {
    fn id(&self) -> FeatureId {
        FeatureId::Privacy
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = [
            SAFARI_DIR,
            SAFARI_CONTAINER_COOKIES,
            SAFARI_CONTAINER_CACHES,
            OLD_COOKIES,
            "Library/Caches",
            FIREFOX_PROFILES,
            SHARED_FILE_LIST,
        ]
        .iter()
        .map(|p| env.home_path(p))
        .collect();
        for b in BROWSERS {
            if let Engine::Chromium { data, .. } = b.engine {
                v.push(env.home_path(data));
            }
        }
        v
    }

    fn prepare(&self, env: &Env, plan: &Plan) -> anyhow::Result<()> {
        let running = running_browsers(env);
        let blocked: Vec<&str> = running
            .iter()
            .filter(|b| plan.selected().any(|i| i.category == b.id))
            .map(|b| b.name)
            .collect();
        if !blocked.is_empty() {
            anyhow::bail!(
                "{} is open. Quit it, then scan again.",
                blocked.join(" and ")
            );
        }
        Ok(())
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut s = Scan {
            ctx,
            allow: self.allow_roots(ctx.env),
            plan: Plan::new(FeatureId::Privacy),
            seen: BTreeSet::new(),
        };
        let running = running_browsers(ctx.env);
        for b in BROWSERS {
            if ctx.cancelled() {
                s.plan.notes.push("Scan stopped before the end.".into());
                return Ok(s.plan);
            }
            (ctx.progress)(b.name);
            let first = s.plan.items.len();
            match b.engine {
                Engine::Safari => s.safari(b),
                Engine::Chromium { data, cache } => s.chromium(b, data, cache),
                Engine::Firefox => s.firefox(b),
            }
            if running.iter().any(|r| r.id == b.id) && s.plan.items.len() > first {
                for i in first..s.plan.items.len() {
                    s.plan.lock(i, lock_running(b.name));
                }
                s.plan.notes.push(format!(
                    "{} is open. Quit it and scan again to clean its traces.",
                    b.name
                ));
            }
        }
        (ctx.progress)("Recent items");
        s.recent_items();
        (ctx.progress)("Wi-Fi networks");
        s.wifi();
        (ctx.progress)("App permissions");
        s.permissions();
        Ok(s.plan)
    }
}

struct Scan<'a, 'b> {
    ctx: &'a ScanCtx<'b>,
    allow: Vec<PathBuf>,
    plan: Plan,
    seen: BTreeSet<PathBuf>,
}

impl Scan<'_, '_> {
    fn env(&self) -> &Env {
        self.ctx.env
    }

    fn browser_cat(&mut self, b: &Browser) {
        self.plan.add_category(
            b.id,
            b.name,
            &format!("History, cookies, cache and other traces {} keeps.", b.name),
            Risk::Medium,
        );
    }

    fn file(&mut self, b: &Browser, p: &Path, kind: Kind, profile: Option<&str>) {
        if self.seen.contains(p) || !self.ctx.allowed(&self.allow, p) {
            return;
        }
        let (size, _) = tree_size(p);
        if size == 0 {
            return;
        }
        self.browser_cat(b);
        let whose = match profile {
            Some(pr) => format!("{} ({pr} profile)", b.name),
            None => b.name.to_string(),
        };
        let name = dir_name(p);
        let id = self.plan.push(
            p.to_path_buf(),
            size,
            b.id,
            format!("{whose} {}: {name}", kind.what),
            kind.risk,
            kind.selected,
        );
        self.plan.items[id].group = Some(kind.label.to_string());
        self.seen.insert(p.to_path_buf());
    }

    /// False (with a note) when the folder exists but cannot be read.
    fn note_unreadable(&mut self, dir: &Path) -> bool {
        if dir.exists() && !crate::features::system_junk::can_read(self.env(), dir) {
            let msg = format!(
                "{} could not be read. Turn on Full Disk Access to include it.",
                self.env().display(dir)
            );
            if !self.plan.notes.contains(&msg) {
                self.plan.notes.push(msg);
            }
            return false;
        }
        true
    }

    fn safari(&mut self, b: &Browser) {
        let dir = self.env().home_path(SAFARI_DIR);
        if !self.note_unreadable(&dir) {
            return;
        }
        for (kind, name) in [
            (HISTORY, "History.db"),
            (HISTORY, "History.db-wal"),
            (HISTORY, "History.db-shm"),
            (HISTORY, "History.db-lock"),
            (SESSIONS, "LastSession.plist"),
            (DOWNLOADS, "Downloads.plist"),
            (FORMS, "Form Values"),
        ] {
            let p = dir.join(name);
            self.file(b, &p, kind, None);
        }
        for d in [SAFARI_CONTAINER_COOKIES, OLD_COOKIES] {
            let p = self.env().home_path(d).join("Cookies.binarycookies");
            self.file(b, &p, COOKIES, None);
        }
        for p in [
            self.env().home_path("Library/Caches/com.apple.Safari"),
            self.env()
                .home_path(SAFARI_CONTAINER_CACHES)
                .join("com.apple.Safari"),
        ] {
            self.file(b, &p, CACHE, None);
        }
        if self.plan.items.iter().any(|i| i.category == b.id) {
            self.plan.notes.push(
                "Safari passwords are kept in the Passwords app; CleanCraft does not touch them."
                    .into(),
            );
        }
    }

    fn chromium(&mut self, b: &Browser, data: &str, cache: &str) {
        let data = self.env().home_path(data);
        if !self.note_unreadable(&data) {
            return;
        }
        let cache = self.env().home_path(cache);
        for prof in chromium_profiles(&data) {
            let pname = dir_name(&prof);
            for (kind, names) in CHROMIUM_FILES {
                for n in *names {
                    self.file(b, &prof.join(n), *kind, Some(&pname));
                }
            }
            self.file(b, &cache.join(&pname), CACHE, Some(&pname));
        }
    }

    fn firefox(&mut self, b: &Browser) {
        let profiles = self.env().home_path(FIREFOX_PROFILES);
        if !self.note_unreadable(&profiles) {
            return;
        }
        let cache = self.env().home_path(FIREFOX_CACHE);
        for prof in children(&profiles) {
            if !std::fs::symlink_metadata(&prof).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let pname = dir_name(&prof);
            for (kind, names) in FIREFOX_FILES {
                for n in *names {
                    self.file(b, &prof.join(n), *kind, Some(&pname));
                }
            }
            self.file(b, &cache.join(&pname), CACHE, Some(&pname));
        }
    }

    fn recent_items(&mut self) {
        let dir = self.env().home_path(SHARED_FILE_LIST);
        if !self.note_unreadable(&dir) {
            return;
        }
        let mut files: Vec<PathBuf> = children(&dir)
            .into_iter()
            .filter(|p| dir_name(p).contains(".Recent"))
            .collect();
        files.extend(children(&dir.join(RECENT_DOCS_DIR)));
        for f in files {
            let n = dir_name(&f);
            if !(n.ends_with(".sfl2") || n.ends_with(".sfl3")) || !f.is_file() {
                continue;
            }
            if !self.ctx.allowed(&self.allow, &f) {
                continue;
            }
            let (size, _) = tree_size(&f);
            if size == 0 {
                continue;
            }
            let what = if n.contains("RecentApplications") {
                "Recently opened apps".to_string()
            } else if n.contains("RecentDocuments") {
                "Recently opened documents".to_string()
            } else if n.contains("RecentServers") || n.contains("RecentHosts") {
                "Recently used servers".to_string()
            } else {
                let app = n
                    .trim_end_matches(".sfl3")
                    .trim_end_matches(".sfl2")
                    .to_string();
                format!("Recent documents of {app}")
            };
            self.plan.add_category(
                "recent_items",
                "Recent items",
                "Lists of recently opened documents, apps and servers.",
                Risk::Low,
            );
            let id = self
                .plan
                .push(f.clone(), size, "recent_items", what, Risk::Low, true);
            self.plan.items[id].group = Some("Recent items".into());
        }
    }

    fn wifi(&mut self) {
        let env = self.env().clone();
        let dev = env
            .sys
            .run(NETWORKSETUP, &["-listallhardwareports"])
            .ok()
            .filter(|o| o.ok())
            .and_then(|o| parse_wifi_device(&o.stdout));
        let Some(dev) = dev else {
            // No Wi-Fi hardware, or the tool failed: nothing to list.
            return;
        };
        let out = env
            .sys
            .run(NETWORKSETUP, &["-listpreferredwirelessnetworks", &dev]);
        let nets = match out {
            Ok(o) if o.ok() => parse_preferred_networks(&o.stdout),
            _ => None,
        };
        let Some(nets) = nets else {
            self.plan
                .notes
                .push("Known Wi-Fi networks could not be listed.".into());
            return;
        };
        let base = env.sys_path(KNOWN_NETWORKS);
        for n in nets {
            let label = n.replace('/', ":");
            let p = base.join(&label);
            if self.ctx.ignored(&p) {
                continue;
            }
            self.plan.add_category(
                "wifi",
                "Wi-Fi networks",
                "Networks this Mac joined before and joins again automatically.",
                Risk::Medium,
            );
            let id = self.plan.push(
                p,
                0,
                "wifi",
                format!("Known Wi-Fi network {n}"),
                Risk::Medium,
                false,
            );
            self.plan.items[id].group = Some("Wi-Fi".into());
            self.plan.lock(id, LOCK_WIFI);
        }
    }

    fn permissions(&mut self) {
        let env = self.env().clone();
        let mut seen = BTreeSet::new();
        let mut failed = false;
        for db in tcc_dbs(&env) {
            if !db.exists() {
                continue;
            }
            let dbs = db.to_string_lossy().into_owned();
            let rows = match env
                .sys
                .run(SQLITE3, &["-readonly", "-separator", "|", &dbs, TCC_QUERY])
            {
                Ok(o) if o.ok() => parse_tcc_rows(&o.stdout),
                _ => {
                    failed = true;
                    continue;
                }
            };
            for (service, client) in rows {
                if !seen.insert((service.clone(), client.clone())) {
                    continue;
                }
                let p = db.join(&service).join(&client);
                if self.ctx.ignored(&p) {
                    continue;
                }
                self.plan.add_category(
                    "permissions",
                    "App permissions",
                    "Apps you allowed to use the camera, microphone, screen, files and more.",
                    Risk::Low,
                );
                let title = service_title(&service);
                let id = self.plan.push(
                    p,
                    0,
                    "permissions",
                    format!("{title} access granted to {client}"),
                    Risk::Low,
                    false,
                );
                self.plan.items[id].group = Some(title);
                self.plan.lock(id, LOCK_PERMISSION);
            }
        }
        if failed {
            self.plan.notes.push(NOTE_TCC.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn networksetup_output() {
        let ports = "\nHardware Port: Ethernet\nDevice: en1\nEthernet Address: aa\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: bb\n";
        assert_eq!(parse_wifi_device(ports).as_deref(), Some("en0"));
        assert_eq!(
            parse_wifi_device("Hardware Port: Ethernet\nDevice: en1\n"),
            None
        );
        let list = "Preferred networks on en0:\n\tHome Net\n\tCafe/Guest\n";
        assert_eq!(
            parse_preferred_networks(list),
            Some(vec!["Home Net".to_string(), "Cafe/Guest".to_string()])
        );
        assert_eq!(
            parse_preferred_networks(
                "en1 is not a Wi-Fi interface.\n** Error: Error obtaining wireless information.\n"
            ),
            None
        );
    }

    #[test]
    fn tcc_rows() {
        let out = "kTCCServiceCamera|us.zoom.xos|2\nkTCCServiceMicrophone|com.x|0\nkTCCServicePhotos|com.y|3\nbad\n";
        assert_eq!(
            parse_tcc_rows(out),
            vec![
                ("kTCCServiceCamera".into(), "us.zoom.xos".into()),
                ("kTCCServicePhotos".into(), "com.y".into())
            ]
        );
        assert_eq!(tccutil_service("kTCCServiceScreenCapture"), "ScreenCapture");
        assert_eq!(
            service_title("kTCCServiceScreenCapture"),
            "Screen recording"
        );
        assert!(!safe_token("com.x; rm"));
    }
}
