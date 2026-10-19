//! System Junk: caches, logs and other files macOS and apps rebuild by themselves, plus
//! leftovers such as broken startup items, old device backups and developer caches.
//!
//! Each proposal is one entry (a cache folder of one app, one log folder, one backup), sized
//! with the shared formula, with `group` naming the app or tool it belongs to. Entries this
//! user cannot change without administrator rights, and entries CleanCraft must never change
//! (files inside signed apps, document versions, other accounts), are listed but locked, so the
//! numbers are honest about what exists and what can be freed. Locked-only places are not in
//! the allow-list, so the executor would refuse them even if a saved plan were edited.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{accessed_days_ago, children, modified_days_ago, tree_size, walk_files};
use crate::plan::{FeatureId, Plan, Risk};
use crate::policy;

pub struct SystemJunk;

/// Cache folders that belong to iCloud, sign-in and transfers in progress. Never proposed.
pub const KEEP_CACHES: &[&str] = &[
    "com.apple.bird",
    "CloudKit",
    "com.apple.cloudd",
    "com.apple.iCloudHelper",
    "com.apple.nsurlsessiond",
    "com.apple.akd",
    "com.apple.ap.adprivacyd",
    "FamilyCircle",
    "com.apple.HomeKit",
    "com.apple.containermanagerd",
    "com.apple.Safari.SafeBrowsing",
];

/// Package manager caches (J17): (tool, folder, preselected). Items are the folder's entries.
pub const DEV_CACHES: &[(&str, &str, bool)] = &[
    ("npm", "~/.npm/_cacache", true),
    ("Homebrew", "~/Library/Caches/Homebrew", true),
    ("pip", "~/Library/Caches/pip", true),
    ("CocoaPods", "~/Library/Caches/CocoaPods", true),
    (
        "Swift Package Manager",
        "~/Library/Caches/org.swift.swiftpm",
        true,
    ),
    ("Cargo", "~/.cargo/registry/cache", true),
    // Gradle downloads are slow to rebuild; listed, never preselected.
    ("Gradle", "~/.gradle/caches", false),
];

/// AI apps whose caches and logs are clutter (J18).
pub const AI_APPS: &[&str] = &["Claude", "Cursor"];
pub const AI_SUBDIRS: &[&str] = &["Cache", "Code Cache", "GPUCache", "logs"];

/// Local AI model stores (J19): (tool, folder).
pub const AI_MODELS: &[(&str, &str)] = &[
    ("Ollama", "~/.ollama/models"),
    ("LM Studio", "~/.lmstudio/models"),
    ("LM Studio", "~/.cache/lm-studio/models"),
];

/// Xcode folders (J13): (category, folder, label).
pub const XCODE: &[(&str, &str, &str)] = &[
    (
        "xcode_derived",
        "~/Library/Developer/Xcode/DerivedData",
        "Derived data",
    ),
    (
        "xcode_device_support",
        "~/Library/Developer/Xcode/iOS DeviceSupport",
        "iOS device support",
    ),
    (
        "xcode_device_support",
        "~/Library/Developer/Xcode/watchOS DeviceSupport",
        "watchOS device support",
    ),
    (
        "xcode_device_support",
        "~/Library/Developer/Xcode/tvOS DeviceSupport",
        "tvOS device support",
    ),
    (
        "xcode_simulator_caches",
        "~/Library/Developer/CoreSimulator/Caches",
        "Simulator caches",
    ),
];
pub const XCODE_ARCHIVES: &str = "~/Library/Developer/Xcode/Archives";
/// Kept: Xcode rebuilds it slowly and it is shared by every project.
pub const MODULE_CACHE: &str = "ModuleCache.noindex";

pub const SPARKLE: &str = "org.sparkle-project.Sparkle";

pub const LOCK_ADMIN: &str = "Needs administrator rights";
pub const LOCK_SIGNED: &str = "Inside a signed app: removing it would break the app's code signature, so CleanCraft only reports it";
pub const LOCK_LIPO: &str = "Stripping code from a signed app would break its code signature, so CleanCraft only reports it";
pub const LOCK_VERSIONS: &str = "Managed by macOS: remove old versions from the app that saved the document (File > Revert To > Browse All Versions)";
pub const LOCK_NEWEST: &str = "The newest backup of this device is always kept";
pub const LOCK_DELETED_USER: &str = "Belongs to another account: remove it in System Settings > Users & Groups with administrator rights";
pub const LOCK_DOWNLOADING: &str =
    "Still open in an app (download in progress or disk image mounted)";
pub const LOCK_UNCHECKED: &str = "CleanCraft could not double check it with plutil, so it is kept";

pub struct Cat {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub risk: Risk,
}

pub const CATEGORIES: &[Cat] = &[
    Cat {
        id: "user_caches",
        title: "User cache files",
        description: "Temporary data apps keep to work faster. Apps rebuild them when needed.",
        risk: Risk::Low,
    },
    Cat {
        id: "system_caches",
        title: "System cache files",
        description: "Caches shared by all users. Most need administrator rights.",
        risk: Risk::Low,
    },
    Cat {
        id: "user_logs",
        title: "User log files",
        description: "Activity records written by apps for troubleshooting.",
        risk: Risk::Low,
    },
    Cat {
        id: "system_logs",
        title: "System log files",
        description: "Activity records written by macOS services.",
        risk: Risk::Low,
    },
    Cat {
        id: "crash_reports",
        title: "Crash reports",
        description: "Reports written when an app quit unexpectedly.",
        risk: Risk::Low,
    },
    Cat {
        id: "language_files",
        title: "Language files",
        description: "Translations of apps into languages you do not use. Reported only: removing them breaks app signatures.",
        risk: Risk::High,
    },
    Cat {
        id: "universal_binaries",
        title: "Universal binaries",
        description: "Apps that carry code for both Intel and Apple silicon. Reported only.",
        risk: Risk::High,
    },
    Cat {
        id: "broken_agents",
        title: "Broken startup items",
        description: "Startup items whose program no longer exists.",
        risk: Risk::Medium,
    },
    Cat {
        id: "broken_prefs",
        title: "Broken preferences",
        description: "Preference files that cannot be read. The app makes a fresh one.",
        risk: Risk::Medium,
    },
    Cat {
        id: "document_versions",
        title: "Document versions",
        description: "Older versions of documents kept by macOS. Reported only.",
        risk: Risk::Medium,
    },
    Cat {
        id: "ios_backups",
        title: "iPhone and iPad backups",
        description: "Device backups. The newest backup of each device is kept.",
        risk: Risk::Medium,
    },
    Cat {
        id: "ios_updates",
        title: "iOS software updates",
        description: "Downloaded iPhone and iPad system updates, downloaded again when needed.",
        risk: Risk::Low,
    },
    Cat {
        id: "xcode_derived",
        title: "Xcode build data",
        description: "Intermediate build products. Xcode rebuilds them on the next build. Module caches are kept.",
        risk: Risk::Low,
    },
    Cat {
        id: "xcode_device_support",
        title: "Xcode device support",
        description: "Debug symbols copied from devices. Copied again when the device connects.",
        risk: Risk::Low,
    },
    Cat {
        id: "xcode_simulator_caches",
        title: "Simulator caches",
        description: "Caches of the iOS Simulator.",
        risk: Risk::Low,
    },
    Cat {
        id: "xcode_archives",
        title: "Xcode archives",
        description: "Builds you archived for distribution. Never selected for you.",
        risk: Risk::Medium,
    },
    Cat {
        id: "unfinished_downloads",
        title: "Unfinished downloads",
        description: "Browser downloads that never completed.",
        risk: Risk::Low,
    },
    Cat {
        id: "old_updates",
        title: "Old app updates",
        description: "Update packages apps downloaded and already installed.",
        risk: Risk::Low,
    },
    Cat {
        id: "disk_images",
        title: "Unused disk images",
        description: "Disk images in Downloads you have not opened for a while. Never selected for you.",
        risk: Risk::Medium,
    },
    Cat {
        id: "dev_caches",
        title: "Developer caches",
        description: "Package manager caches. Downloaded again when a build needs them.",
        risk: Risk::Low,
    },
    Cat {
        id: "ai_clutter",
        title: "AI tool clutter",
        description: "Caches and logs of AI apps.",
        risk: Risk::Low,
    },
    Cat {
        id: "ai_models",
        title: "Local AI models",
        description: "Models downloaded by local AI tools. Never selected for you.",
        risk: Risk::Medium,
    },
    Cat {
        id: "deleted_users",
        title: "Deleted users",
        description: "Archived home folders of removed accounts. Reported only.",
        risk: Risk::Medium,
    },
];

pub fn resolve(env: &Env, p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rel) => env.home_path(rel),
        None => env.sys_path(p),
    }
}

/// Can this folder be listed? (Without Full Disk Access macOS refuses to list protected
/// folders; an injected `PermissionDenied` fault means "cannot change", not "cannot list".)
pub fn can_read(_env: &Env, p: &Path) -> bool {
    std::fs::read_dir(p).is_ok()
}

/// A real folder (not a link to one).
fn real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn mtime(p: &Path) -> Option<i64> {
    let m = std::fs::symlink_metadata(p).ok()?.modified().ok()?;
    let d = m.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(d.as_secs() as i64)
}

/// `~/Library/Containers/<id>/Data/Library/Caches` folders, real folders only.
pub fn container_cache_dirs(env: &Env) -> Vec<(String, PathBuf)> {
    children(&env.home_path("Library/Containers"))
        .into_iter()
        .filter_map(|c| {
            let caches = c.join("Data/Library/Caches");
            (real_dir(&c) && real_dir(&c.join("Data")) && real_dir(&caches))
                .then(|| (name_of(&c), caches))
        })
        .collect()
}

/// `~/Library/iTunes/<device> Software Updates` folders.
pub fn ipsw_dirs(env: &Env) -> Vec<PathBuf> {
    children(&env.home_path("Library/iTunes"))
        .into_iter()
        .filter(|d| name_of(d).ends_with("Software Updates") && real_dir(d))
        .collect()
}

impl Feature for SystemJunk {
    fn id(&self) -> FeatureId {
        FeatureId::SystemJunk
    }

    /// System caches in `/Library/Caches` the user selects by hand move with the
    /// administrator password (only items the scan marked).
    fn allows_admin(&self) -> bool {
        true
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = [
            "~/Library/Caches",
            "/Library/Caches",
            "~/Library/Logs",
            "/Library/Logs",
            "/private/var/log",
            "~/Library/LaunchAgents",
            "~/Library/Preferences",
            "~/Library/Application Support/MobileSync/Backup",
            XCODE_ARCHIVES,
            "~/Downloads",
            "/Library/Updates",
            "~/.ollama",
        ]
        .iter()
        .map(|p| resolve(env, p))
        .collect();
        v.extend(XCODE.iter().map(|(_, p, _)| resolve(env, p)));
        v.extend(DEV_CACHES.iter().map(|(_, p, _)| resolve(env, p)));
        v.extend(
            AI_MODELS
                .iter()
                .filter(|(tool, _)| *tool != "Ollama")
                .map(|(_, p)| resolve(env, p)),
        );
        for app in AI_APPS {
            for sub in AI_SUBDIRS {
                v.push(env.home_path(&format!("Library/Application Support/{app}/{sub}")));
            }
        }
        v.extend(container_cache_dirs(env).into_iter().map(|(_, p)| p));
        v.extend(ipsw_dirs(env));
        v
    }

    /// The scan's rules again, for an item about to move (an edited plan may unlock or add
    /// anything inside the allow-list): never an iCloud or sign-in cache ([`KEEP_CACHES`]) or a
    /// hidden entry; the newest backup of each device stays; in Downloads only unfinished
    /// downloads and disk images; in Preferences only plists macOS's `plutil` finds damaged;
    /// in LaunchAgents only agents whose program is gone.
    fn validate(
        &self,
        env: &Env,
        _settings: &crate::settings::Settings,
        _plan: &Plan,
        item: &crate::plan::Item,
    ) -> Result<(), String> {
        validate_item(env, &self.allow_roots(env), &item.path)
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut s = Scanner {
            ctx,
            allow: self.allow_roots(ctx.env),
            plan: Plan::new(FeatureId::SystemJunk),
            seen: HashSet::new(),
            open: ctx.env.sys.open_files().unwrap_or_default(),
        };
        const STEPS: [&str; 18] = [
            "Developer caches",
            "Old app updates",
            "AI tool clutter",
            "User caches",
            "System caches",
            "Crash reports",
            "Logs",
            "Language files",
            "Universal binaries",
            "Startup items",
            "Preferences",
            "Document versions",
            "Device backups",
            "iOS updates",
            "Xcode",
            "Downloads",
            "AI models",
            "Deleted users",
        ];
        for (i, label) in STEPS.iter().enumerate() {
            if ctx.cancelled() {
                s.plan.notes.push("Scan stopped before the end.".into());
                break;
            }
            (ctx.progress)(label);
            match i {
                0 => s.dev_caches(),
                1 => s.old_updates(),
                2 => s.ai_clutter(),
                3 => s.user_caches(),
                4 => s.system_caches(),
                5 => s.crash_reports(),
                6 => s.logs(),
                7 => s.language_files(),
                8 => s.universal_binaries(),
                9 => s.broken_agents(),
                10 => s.broken_prefs(),
                11 => s.document_versions(),
                12 => s.ios_backups(),
                13 => s.ios_updates(),
                14 => s.xcode(),
                15 => s.downloads(),
                16 => s.ai_models(),
                _ => s.deleted_users(),
            }
        }
        Ok(s.plan)
    }
}

pub struct Scanner<'a, 'b> {
    ctx: &'a ScanCtx<'b>,
    allow: Vec<PathBuf>,
    pub plan: Plan,
    seen: HashSet<PathBuf>,
    open: HashSet<PathBuf>,
}

impl Scanner<'_, '_> {
    fn env(&self) -> &Env {
        self.ctx.env
    }

    fn ensure_cat(&mut self, id: &str) {
        if let Some(c) = CATEGORIES.iter().find(|c| c.id == id) {
            self.plan.add_category(c.id, c.title, c.description, c.risk);
        }
    }

    fn is_open(&self, p: &Path) -> bool {
        self.open.contains(p) || self.open.iter().any(|o| o.starts_with(p))
    }

    /// Propose an entry inside the allow-list. Skips ignored, fenced, already listed and empty
    /// entries. Returns the item id.
    fn propose(
        &mut self,
        p: &Path,
        cat: &str,
        reason: String,
        risk: Risk,
        selected: bool,
        group: Option<&str>,
    ) -> Option<usize> {
        if self.seen.contains(p) || !self.ctx.allowed(&self.allow, p) {
            return None;
        }
        let (size, _) = tree_size(p);
        if size == 0 {
            return None;
        }
        self.add(p, size, cat, reason, risk, selected, group)
    }

    /// List an entry CleanCraft only reports (outside the allow-list), locked with `why`.
    fn report(
        &mut self,
        p: &Path,
        size: u64,
        cat: &str,
        reason: String,
        risk: Risk,
        why: &str,
        group: Option<&str>,
    ) -> Option<usize> {
        let env = self.env();
        if self.seen.contains(p)
            || self.ctx.ignored(p)
            || !env.contains(p)
            || policy::protected_reason(env, p).is_some()
            || size == 0
        {
            return None;
        }
        let id = self.add(p, size, cat, reason, risk, false, group)?;
        self.plan.lock(id, why);
        Some(id)
    }

    fn add(
        &mut self,
        p: &Path,
        size: u64,
        cat: &str,
        reason: String,
        risk: Risk,
        selected: bool,
        group: Option<&str>,
    ) -> Option<usize> {
        self.ensure_cat(cat);
        let id = self
            .plan
            .push(p.to_path_buf(), size, cat, reason, risk, selected);
        let it = &mut self.plan.items[id];
        it.group = group.map(str::to_string);
        it.modified = mtime(p);
        self.seen.insert(p.to_path_buf());
        Some(id)
    }

    /// The folder exists and can be listed; a note otherwise.
    fn readable(&mut self, root: &Path) -> bool {
        if !root.exists() {
            return false;
        }
        if !can_read(self.env(), root) {
            let msg = format!(
                "{} could not be read. Full Disk Access may be needed.",
                self.env().display(root)
            );
            if !self.plan.notes.contains(&msg) {
                self.plan.notes.push(msg);
            }
            return false;
        }
        true
    }

    // J17
    fn dev_caches(&mut self) {
        for (tool, path, selected) in DEV_CACHES {
            let root = resolve(self.env(), path);
            if !real_dir(&root) || !self.readable(&root) {
                continue;
            }
            for e in children(&root) {
                self.propose(
                    &e,
                    "dev_caches",
                    format!("{tool} cache: {}", name_of(&e)),
                    Risk::Low,
                    *selected,
                    Some(tool),
                );
            }
            // Never offered again as a whole user cache folder.
            self.seen.insert(root);
        }
    }

    // J15
    fn old_updates(&mut self) {
        let caches = self.env().home_path("Library/Caches");
        for app in children(&caches) {
            let sp = app.join(SPARKLE);
            if real_dir(&app) && real_dir(&sp) {
                let n = name_of(&app);
                self.propose(
                    &sp,
                    "old_updates",
                    format!("Downloaded update of {n}"),
                    Risk::Low,
                    true,
                    Some(&n),
                );
            }
        }
        let updates = self.env().sys_path("/Library/Updates");
        if self.readable(&updates) {
            let writable = self.env().can_modify(&updates);
            for e in children(&updates) {
                if let Some(id) = self.propose(
                    &e,
                    "old_updates",
                    format!("macOS update download: {}", name_of(&e)),
                    Risk::Low,
                    true,
                    Some("macOS"),
                ) && !writable
                {
                    self.plan.lock(id, LOCK_ADMIN);
                }
            }
        }
    }

    // J18
    fn ai_clutter(&mut self) {
        for app in AI_APPS {
            for sub in AI_SUBDIRS {
                let dir = self
                    .env()
                    .home_path(&format!("Library/Application Support/{app}/{sub}"));
                if !real_dir(&dir) || !self.readable(&dir) {
                    continue;
                }
                for e in children(&dir) {
                    self.propose(
                        &e,
                        "ai_clutter",
                        format!("{app} {}: {}", sub.to_lowercase(), name_of(&e)),
                        Risk::Low,
                        true,
                        Some(app),
                    );
                }
            }
            let logs = self.env().home_path(&format!("Library/Logs/{app}"));
            if real_dir(&logs) {
                self.propose(
                    &logs,
                    "ai_clutter",
                    format!("{app} logs"),
                    Risk::Low,
                    true,
                    Some(app),
                );
            }
        }
    }

    // J1
    fn user_caches(&mut self) {
        let root = self.env().home_path("Library/Caches");
        if self.readable(&root) {
            for e in children(&root) {
                if keep(&e) || self.seen.contains(&e) {
                    continue;
                }
                let n = name_of(&e);
                if real_dir(&e.join(SPARKLE)) {
                    // The update download is offered under Old app updates; the rest here.
                    for c in children(&e) {
                        self.propose(
                            &c,
                            "user_caches",
                            format!("Cache of {n}: {}", name_of(&c)),
                            Risk::Low,
                            true,
                            Some(&n),
                        );
                    }
                } else {
                    self.propose(
                        &e,
                        "user_caches",
                        format!("Cache of {n}"),
                        Risk::Low,
                        true,
                        Some(&n),
                    );
                }
            }
        }
        for (id, caches) in container_cache_dirs(self.env()) {
            for e in children(&caches) {
                if keep(&e) {
                    continue;
                }
                self.propose(
                    &e,
                    "user_caches",
                    format!("Cache of {id} (sandboxed): {}", name_of(&e)),
                    Risk::Low,
                    true,
                    Some(&id),
                );
            }
        }
    }

    /// Entries of a folder, locked when this user cannot change the folder, or with `admin`
    /// marked for the administrator password and left for the user to select.
    fn area(&mut self, root: &Path, cat: &str, title: &str, skip: &[&str], admin: bool) {
        if !self.readable(root) {
            return;
        }
        let writable = self.env().can_modify(root);
        for e in children(root) {
            let n = name_of(&e);
            if keep(&e) || skip.contains(&n.as_str()) {
                continue;
            }
            if let Some(id) =
                self.propose(&e, cat, format!("{title}: {n}"), Risk::Low, true, Some(&n))
                && !writable
            {
                if admin {
                    // Never preselected: a default clean must not ask for the password.
                    self.plan.mark_admin(id);
                    self.plan.set_selected(id, false);
                } else {
                    self.plan.lock(id, LOCK_ADMIN);
                }
            }
        }
    }

    // J2
    fn system_caches(&mut self) {
        let root = self.env().sys_path("/Library/Caches");
        self.area(&root, "system_caches", "System cache", &[], true);
    }

    // J5
    fn crash_reports(&mut self) {
        let root = self.env().home_path("Library/Logs/DiagnosticReports");
        if !real_dir(&root) || !self.readable(&root) {
            return;
        }
        for e in children(&root) {
            let n = name_of(&e);
            let app = crash_app(&n);
            self.propose(
                &e,
                "crash_reports",
                format!("Crash report of {app}"),
                Risk::Low,
                true,
                Some(&app),
            );
        }
        self.seen.insert(root);
    }

    // J3, J4
    fn logs(&mut self) {
        let user = self.env().home_path("Library/Logs");
        let mut skip: Vec<&str> = vec!["DiagnosticReports"];
        skip.extend(AI_APPS.iter().copied());
        self.area(&user, "user_logs", "Log", &skip, false);
        for p in ["/Library/Logs", "/private/var/log"] {
            let root = self.env().sys_path(p);
            self.area(&root, "system_logs", "System log", &[], false);
        }
    }

    // J6
    fn language_files(&mut self) {
        let apps = self.env().sys_path("/Applications");
        if !apps.exists() {
            return;
        }
        let keep = self.keep_languages();
        for app in children(&apps) {
            if app.extension().is_none_or(|e| e != "app") || !real_dir(&app) {
                continue;
            }
            let app_name = app
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            for l in children(&app.join("Contents/Resources")) {
                let n = name_of(&l);
                let Some(lang) = n.strip_suffix(".lproj") else {
                    continue;
                };
                if language_kept(lang, &keep) {
                    continue;
                }
                let (size, _) = tree_size(&l);
                self.report(
                    &l,
                    size,
                    "language_files",
                    format!("{app_name}: {lang} translation"),
                    Risk::High,
                    LOCK_SIGNED,
                    Some(&app_name),
                );
            }
        }
    }

    /// Language codes to keep: the system languages and the settings' list.
    fn keep_languages(&mut self) -> Vec<String> {
        let mut keep: Vec<String> = self
            .ctx
            .settings
            .keep_languages
            .iter()
            .map(|l| lang_code(l))
            .collect();
        match self
            .env()
            .sys
            .run("/usr/bin/defaults", &["read", "-g", "AppleLanguages"])
        {
            Ok(out) if out.ok() => keep.extend(parse_apple_languages(&out.stdout)),
            _ => self.plan.notes.push(
                "Your system languages could not be read, so only English is treated as in use."
                    .into(),
            ),
        }
        keep.push("en".into());
        keep.sort();
        keep.dedup();
        keep
    }

    // J7
    fn universal_binaries(&mut self) {
        let apps = self.env().sys_path("/Applications");
        if !apps.exists() {
            return;
        }
        let native = match self.env().sys.run("/usr/bin/uname", &["-m"]) {
            Ok(o) if o.ok() && !o.stdout.trim().is_empty() => o.stdout.trim().to_string(),
            _ => {
                self.plan.notes.push(
                    "The processor type could not be read, so universal apps were not checked."
                        .into(),
                );
                return;
            }
        };
        let mut failed = 0;
        for app in children(&apps) {
            if app.extension().is_none_or(|e| e != "app") || !real_dir(&app) {
                continue;
            }
            let Some(exe) = main_executable(&app) else {
                continue;
            };
            let exe_s = exe.to_string_lossy().into_owned();
            let archs = match self.env().sys.run("/usr/bin/lipo", &["-archs", &exe_s]) {
                Ok(o) if o.ok() => parse_archs(&o.stdout),
                _ => {
                    failed += 1;
                    continue;
                }
            };
            let other: Vec<&String> = archs.iter().filter(|a| **a != native).collect();
            if archs.len() < 2 || other.is_empty() || !archs.contains(&native) {
                continue;
            }
            let size = self
                .env()
                .sys
                .run("/usr/bin/lipo", &["-detailed_info", &exe_s])
                .ok()
                .filter(|o| o.ok())
                .map(|o| {
                    parse_lipo_sizes(&o.stdout)
                        .into_iter()
                        .filter(|(a, _)| *a != native)
                        .map(|(_, s)| s)
                        .sum::<u64>()
                })
                .filter(|s| *s > 0)
                .unwrap_or_else(|| tree_size(&exe).0);
            let app_name = app
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let other = other
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.report(
                &exe,
                size,
                "universal_binaries",
                format!("{app_name} also carries {other} code this Mac does not use"),
                Risk::High,
                LOCK_LIPO,
                Some(&app_name),
            );
        }
        if failed > 0 {
            self.plan.notes.push(format!(
                "{failed} apps could not be checked for unused processor code."
            ));
        }
    }

    // J8
    fn broken_agents(&mut self) {
        let root = self.env().home_path("Library/LaunchAgents");
        if !real_dir(&root) || !self.readable(&root) {
            return;
        }
        for f in children(&root) {
            if f.extension().is_none_or(|e| e != "plist") {
                continue;
            }
            let Ok(v) = plist::Value::from_file(&f) else {
                continue;
            };
            let Some(prog) = agent_program(&v) else {
                continue;
            };
            if !program_gone(self.env(), &prog) {
                continue;
            }
            let label = v
                .as_dictionary()
                .and_then(|d| d.get("Label"))
                .and_then(|l| l.as_string())
                .map(str::to_string)
                .unwrap_or_else(|| name_of(&f));
            self.propose(
                &f,
                "broken_agents",
                format!("{label} starts {prog}, which no longer exists"),
                Risk::Medium,
                true,
                Some(&label),
            );
        }
    }

    // J9
    fn broken_prefs(&mut self) {
        let root = self.env().home_path("Library/Preferences");
        if !real_dir(&root) || !self.readable(&root) {
            return;
        }
        let mut files = children(&root);
        files.extend(children(&root.join("ByHost")));
        for f in files {
            if f.extension().is_none_or(|e| e != "plist") || !f.is_file() {
                continue;
            }
            if plist_readable(&f) {
                continue;
            }
            // The plist crate refused it. plutil is macOS's own judge; trust it when it runs.
            let fs = f.to_string_lossy().into_owned();
            let verdict = self.env().sys.run("/usr/bin/plutil", &["-lint", &fs]);
            let n = name_of(&f);
            match verdict {
                Ok(o) if o.ok() => continue,
                Ok(_) => {
                    self.propose(
                        &f,
                        "broken_prefs",
                        format!("Damaged preference file: {n}"),
                        Risk::Medium,
                        true,
                        None,
                    );
                }
                Err(_) => {
                    if let Some(id) = self.propose(
                        &f,
                        "broken_prefs",
                        format!("Possibly damaged preference file: {n}"),
                        Risk::Medium,
                        false,
                        None,
                    ) {
                        self.plan.lock(id, LOCK_UNCHECKED);
                    }
                }
            }
        }
    }

    // J10
    fn document_versions(&mut self) {
        let root = self.env().sys_path("/.DocumentRevisions-V100");
        if !root.exists() {
            return;
        }
        if !can_read(self.env(), &root) {
            self.plan.notes.push(
                "Document versions in /.DocumentRevisions-V100 are kept by macOS and can only be read with administrator rights.".into(),
            );
            return;
        }
        let (size, _) = tree_size(&root);
        self.report(
            &root,
            size,
            "document_versions",
            "Older versions of your documents".into(),
            Risk::Medium,
            LOCK_VERSIONS,
            None,
        );
    }

    // J11
    fn ios_backups(&mut self) {
        let root = self
            .env()
            .home_path("Library/Application Support/MobileSync/Backup");
        if !real_dir(&root) || !self.readable(&root) {
            return;
        }
        let mut by_device: BTreeMap<String, Vec<(i64, PathBuf, String)>> = BTreeMap::new();
        for b in children(&root) {
            if !real_dir(&b) {
                continue;
            }
            let info = backup_info(&b);
            by_device
                .entry(info.device_id)
                .or_default()
                .push((info.date, b, info.name));
        }
        for (_, mut list) in by_device {
            list.sort_by_key(|e| std::cmp::Reverse(e.0));
            for (i, (date, path, name)) in list.iter().enumerate() {
                let when = chrono::DateTime::from_timestamp(*date, 0)
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "an unknown date".into());
                let id = self.propose(
                    path,
                    "ios_backups",
                    format!("Backup of {name} from {when}"),
                    Risk::Medium,
                    false,
                    Some(name),
                );
                if i == 0
                    && let Some(id) = id
                {
                    self.plan.lock(id, LOCK_NEWEST);
                }
            }
        }
    }

    // J12
    fn ios_updates(&mut self) {
        for dir in ipsw_dirs(self.env()) {
            let dev = name_of(&dir).replace(" Software Updates", "");
            for f in children(&dir) {
                if f.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("ipsw"))
                {
                    self.propose(
                        &f,
                        "ios_updates",
                        format!("{dev} system update: {}", name_of(&f)),
                        Risk::Low,
                        true,
                        Some(&dev),
                    );
                }
            }
        }
    }

    // J13
    fn xcode(&mut self) {
        for (cat, path, label) in XCODE {
            let root = resolve(self.env(), path);
            if !real_dir(&root) || !self.readable(&root) {
                continue;
            }
            for e in children(&root) {
                if name_of(&e) == MODULE_CACHE {
                    continue;
                }
                self.propose(
                    &e,
                    cat,
                    format!("{label}: {}", name_of(&e)),
                    Risk::Low,
                    true,
                    Some(label),
                );
            }
        }
        let archives = resolve(self.env(), XCODE_ARCHIVES);
        if real_dir(&archives) && self.readable(&archives) {
            for day in children(&archives) {
                let list = if day.extension().is_some_and(|e| e == "xcarchive") {
                    vec![day.clone()]
                } else {
                    children(&day)
                };
                let group = name_of(&day);
                for a in list {
                    if a.extension().is_some_and(|e| e == "xcarchive") {
                        self.propose(
                            &a,
                            "xcode_archives",
                            format!("Archived build: {}", name_of(&a)),
                            Risk::Medium,
                            false,
                            Some(&group),
                        );
                    }
                }
            }
        }
    }

    // J14, J16
    fn downloads(&mut self) {
        let root = self.env().home_path("Downloads");
        if !real_dir(&root) || !self.readable(&root) {
            return;
        }
        let now = SystemTime::now();
        for e in children(&root) {
            let n = name_of(&e);
            let ext = e
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let browser = match ext.as_str() {
                "download" => "Safari",
                "crdownload" => "Chrome",
                "part" => "Firefox",
                _ => continue,
            };
            let open = self.is_open(&e);
            if let Some(id) = self.propose(
                &e,
                "unfinished_downloads",
                format!("Unfinished {browser} download: {n}"),
                Risk::Low,
                true,
                Some(browser),
            ) && open
            {
                self.plan.lock(id, LOCK_DOWNLOADING);
            }
        }
        let days = self.ctx.settings.downloads_days;
        let mut files = Vec::new();
        let skip = |p: &Path| {
            let n = name_of(p);
            p != root
                && (n.starts_with('.')
                    || n.ends_with(".app")
                    || n.ends_with(".download")
                    || n.ends_with(".photoslibrary"))
        };
        walk_files(&root, &skip, &mut files);
        for (f, md) in files {
            if f.extension().is_none_or(|e| !e.eq_ignore_ascii_case("dmg")) {
                continue;
            }
            let idle = match (accessed_days_ago(&md, now), modified_days_ago(&md, now)) {
                (Some(a), Some(m)) => a.min(m),
                (a, m) => a.or(m).unwrap_or(0),
            };
            if idle < days {
                continue;
            }
            let open = self.is_open(&f);
            if let Some(id) = self.propose(
                &f,
                "disk_images",
                format!("Disk image not opened for {idle} days"),
                Risk::Medium,
                false,
                None,
            ) && open
            {
                self.plan.lock(id, LOCK_DOWNLOADING);
            }
        }
    }

    // J19
    fn ai_models(&mut self) {
        for (tool, path) in AI_MODELS {
            let root = resolve(self.env(), path);
            if !real_dir(&root) || !self.readable(&root) {
                continue;
            }
            if *tool == "Ollama" {
                // Ollama shares blobs between models; only the whole store moves safely.
                let models = ollama_models(&root);
                let list = if models.is_empty() {
                    "downloaded models".to_string()
                } else {
                    models.join(", ")
                };
                self.propose(
                    &root,
                    "ai_models",
                    format!("Ollama models: {list}"),
                    Risk::Medium,
                    false,
                    Some(tool),
                );
                continue;
            }
            for publisher in children(&root) {
                if !real_dir(&publisher) {
                    continue;
                }
                let p = name_of(&publisher);
                for m in children(&publisher) {
                    self.propose(
                        &m,
                        "ai_models",
                        format!("{tool} model {p}/{}", name_of(&m)),
                        Risk::Medium,
                        false,
                        Some(tool),
                    );
                }
            }
        }
    }

    // J20
    fn deleted_users(&mut self) {
        let root = self.env().sys_path("/Users/Deleted Users");
        if !root.exists() {
            return;
        }
        if !can_read(self.env(), &root) {
            self.plan.notes.push(
                "/Users/Deleted Users could not be read; it needs administrator rights.".into(),
            );
            return;
        }
        for f in children(&root) {
            if f.extension().is_some_and(|e| e.eq_ignore_ascii_case("dmg")) {
                let (size, _) = tree_size(&f);
                let user = f
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.report(
                    &f,
                    size,
                    "deleted_users",
                    format!("Home folder of deleted account {user}"),
                    Risk::Medium,
                    LOCK_DELETED_USER,
                    None,
                );
            }
        }
    }
}

fn keep(p: &Path) -> bool {
    p.file_name().is_some_and(kept_name)
}

/// A name System Junk never proposes: an iCloud or sign-in cache, or a hidden entry. Letter case
/// does not matter (a default Mac volume ignores it).
fn kept_name(n: &std::ffi::OsStr) -> bool {
    n.to_string_lossy().starts_with('.')
        || KEEP_CACHES
            .iter()
            .any(|k| policy::same_name(n, std::ffi::OsStr::new(k)))
}

/// Is the program a launch agent starts really gone? An absolute path that does not exist,
/// on a disk that is connected: a program on `/Volumes/<disk>/...` while that disk is not
/// mounted is only unavailable, not gone.
pub fn program_gone(env: &Env, prog: &str) -> bool {
    if !prog.starts_with('/') {
        return false;
    }
    let mut parts = Path::new(prog).components().skip(1);
    if let (Some(first), Some(disk)) = (parts.next(), parts.next())
        && first.as_os_str() == "Volumes"
    {
        let mount = env.sys_path("/Volumes").join(disk.as_os_str());
        if std::fs::symlink_metadata(&mount).is_err() {
            return false;
        }
    }
    // Only "not found" means gone. A privacy refusal (a program in a protected folder) or any
    // other error means we cannot tell, so the agent is not called broken.
    matches!(
        std::fs::symlink_metadata(env.sys_path(prog)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    ) && env.fault_for(&env.sys_path(prog)).is_none()
}

/// See [`SystemJunk`]'s `validate`. `allow` is the feature's allow-list.
pub fn validate_item(env: &Env, allow: &[PathBuf], p: &Path) -> Result<(), String> {
    // The deepest allow-list folder holding the item (the fence refuses items outside it).
    let Some(root) = allow
        .iter()
        .filter(|r| p.starts_with(r) && p != r.as_path())
        .max_by_key(|r| r.components().count())
    else {
        return Ok(());
    };
    let rel = p.strip_prefix(root).unwrap_or(p);
    if rel.components().any(|c| kept_name(c.as_os_str())) {
        return Err("iCloud, sign-in and hidden data are never cleaned".into());
    }
    let is = |r: &str| root == &resolve(env, r);
    let one_level = rel.components().count() == 1;
    if is("~/Library/Application Support/MobileSync/Backup") {
        if !one_level {
            return Err("only whole backups are removed".into());
        }
        let mut newest: BTreeMap<String, (i64, PathBuf)> = BTreeMap::new();
        for b in children(root).into_iter().filter(|b| real_dir(b)) {
            let info = backup_info(&b);
            let e = newest
                .entry(info.device_id)
                .or_insert((info.date, b.clone()));
            if info.date > e.0 {
                *e = (info.date, b);
            }
        }
        if newest.values().any(|(_, b)| policy::same_path_ci(b, p)) {
            return Err(LOCK_NEWEST.into());
        }
    } else if is("~/Downloads") {
        let ext = |q: &Path| {
            q.extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        let unfinished = one_level && ["download", "crdownload", "part"].contains(&ext(p).as_str());
        let image = std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
            && ext(p) == "dmg"
            && rel.parent().is_none_or(|d| {
                d.components().all(|c| {
                    let n = c.as_os_str().to_string_lossy().to_lowercase();
                    !(n.ends_with(".app")
                        || n.ends_with(".download")
                        || n.ends_with(".photoslibrary"))
                })
            });
        if !unfinished && !image {
            return Err(
                "in Downloads only unfinished downloads and disk images are cleaned".into(),
            );
        }
    } else if is("~/Library/Preferences") {
        let in_place = one_level || rel.parent().is_some_and(|d| d == Path::new("ByHost"));
        let plist = p.extension().is_some_and(|e| e == "plist") && p.is_file();
        if !in_place || !plist || plist_readable(p) {
            return Err("only damaged preference files are removed".into());
        }
        let fs = p.to_string_lossy().into_owned();
        match env.sys.run("/usr/bin/plutil", &["-lint", &fs]) {
            Ok(o) if !o.ok() => {}
            _ => return Err(LOCK_UNCHECKED.into()),
        }
    } else if is("~/Library/LaunchAgents") {
        let broken = one_level
            && p.extension().is_some_and(|e| e == "plist")
            && plist::Value::from_file(p)
                .ok()
                .as_ref()
                .and_then(agent_program)
                .is_some_and(|prog| program_gone(env, &prog));
        if !broken {
            return Err("only startup agents whose program is gone are removed".into());
        }
    }
    Ok(())
}

/// "Example-2026-05-01-101010.ips" -> "Example"; "Example_2026...crash" -> "Example".
pub fn crash_app(file: &str) -> String {
    let stem = file.split('.').next().unwrap_or(file);
    let cut = stem
        .char_indices()
        .find(|(i, c)| {
            (*c == '-' || *c == '_') && stem[i + 1..].starts_with(|d: char| d.is_ascii_digit())
        })
        .map(|(i, _)| i)
        .unwrap_or(stem.len());
    let app = stem[..cut].trim();
    if app.is_empty() {
        stem.to_string()
    } else {
        app.to_string()
    }
}

/// `defaults read -g AppleLanguages` prints an old-style array:
/// `(\n    "en-US",\n    "it-IT"\n)`. Returns language codes ("en", "it").
pub fn parse_apple_languages(out: &str) -> Vec<String> {
    out.lines()
        .map(|l| l.trim().trim_end_matches(',').trim_matches('"').trim())
        .filter(|l| !l.is_empty() && *l != "(" && *l != ")")
        .map(lang_code)
        .collect()
}

/// "pt-BR" / "pt_BR" / "Portuguese" -> "pt".
pub fn lang_code(l: &str) -> String {
    let legacy = [
        ("english", "en"),
        ("french", "fr"),
        ("german", "de"),
        ("italian", "it"),
        ("japanese", "ja"),
        ("spanish", "es"),
        ("dutch", "nl"),
        ("portuguese", "pt"),
        ("swedish", "sv"),
        ("danish", "da"),
        ("finnish", "fi"),
        ("norwegian", "no"),
        ("korean", "ko"),
        ("russian", "ru"),
        ("polish", "pl"),
        ("chinese", "zh"),
    ];
    let l = l.trim().to_lowercase();
    if let Some((_, c)) = legacy.iter().find(|(n, _)| *n == l) {
        return (*c).to_string();
    }
    l.split(['-', '_']).next().unwrap_or("").to_string()
}

pub fn language_kept(lproj: &str, keep: &[String]) -> bool {
    if lproj.eq_ignore_ascii_case("base") {
        return true;
    }
    let code = lang_code(lproj);
    code.is_empty() || keep.contains(&code)
}

/// `Contents/MacOS/<CFBundleExecutable>` of an app bundle.
pub fn main_executable(app: &Path) -> Option<PathBuf> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let exe = v
        .as_dictionary()?
        .get("CFBundleExecutable")?
        .as_string()?
        .to_string();
    if exe.contains('/') || exe.is_empty() {
        return None;
    }
    let p = app.join("Contents/MacOS").join(exe);
    p.is_file().then_some(p)
}

/// `lipo -archs` prints "x86_64 arm64".
pub fn parse_archs(out: &str) -> Vec<String> {
    out.split_whitespace().map(str::to_string).collect()
}

/// `lipo -detailed_info` of a universal file: "architecture x86_64" blocks with "size N".
pub fn parse_lipo_sizes(out: &str) -> Vec<(String, u64)> {
    let mut v = Vec::new();
    let mut cur: Option<String> = None;
    for l in out.lines() {
        let l = l.trim();
        if let Some(a) = l.strip_prefix("architecture ") {
            cur = Some(a.trim().to_string());
        } else if let Some(s) = l.strip_prefix("size ")
            && let (Some(a), Ok(n)) = (cur.take(), s.trim().parse::<u64>())
        {
            v.push((a, n));
        }
    }
    v
}

/// The program a launch agent starts: `Program`, else `ProgramArguments[0]`.
pub fn agent_program(v: &plist::Value) -> Option<String> {
    let d = v.as_dictionary()?;
    if let Some(p) = d.get("Program").and_then(|p| p.as_string()) {
        return Some(p.to_string());
    }
    d.get("ProgramArguments")?
        .as_array()?
        .first()?
        .as_string()
        .map(str::to_string)
}

/// Parses as XML, binary or old-style text property list.
pub fn plist_readable(p: &Path) -> bool {
    if plist::Value::from_file(p).is_ok() {
        return true;
    }
    std::fs::File::open(p)
        .ok()
        .is_some_and(|f| plist::Value::from_reader_ascii(f).is_ok())
}

pub struct BackupInfo {
    pub name: String,
    pub device_id: String,
    /// Seconds since the epoch.
    pub date: i64,
}

/// Name, device and date of an iOS backup from its Info.plist (folder time as fallback).
pub fn backup_info(dir: &Path) -> BackupInfo {
    let v = plist::Value::from_file(dir.join("Info.plist")).ok();
    let d = v.as_ref().and_then(|v| v.as_dictionary());
    let s = |k: &str| {
        d.and_then(|d| d.get(k))
            .and_then(|x| x.as_string())
            .map(str::to_string)
    };
    let name = s("Device Name")
        .or_else(|| s("Display Name"))
        .unwrap_or_else(|| "an unknown device".into());
    let device_id = s("Target Identifier")
        .or_else(|| s("Unique Identifier"))
        .unwrap_or_else(|| name.clone());
    let date = d
        .and_then(|d| d.get("Last Backup Date"))
        .and_then(|x| x.as_date())
        .map(SystemTime::from)
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .or_else(|| mtime(dir))
        .unwrap_or(0);
    BackupInfo {
        name,
        device_id,
        date,
    }
}

/// Model names from Ollama manifests: `manifests/<registry>/<namespace>/<model>/<tag>`.
pub fn ollama_models(models: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let root = models.join("manifests");
    walk_files(&root, &|_| false, &mut files);
    let mut v: Vec<String> = files
        .iter()
        .filter_map(|(p, _)| {
            let rel = p.strip_prefix(&root).ok()?;
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if parts.len() < 4 {
                return None;
            }
            let (ns, model, tag) = (&parts[1], &parts[2], &parts[3]);
            Some(if ns == "library" {
                format!("{model}:{tag}")
            } else {
                format!("{ns}/{model}:{tag}")
            })
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_on_an_unplugged_disk_is_not_gone() {
        let d = tempfile::tempdir().unwrap();
        let env = Env::fake(d.path());
        assert!(!program_gone(&env, "/Volumes/Backup/tools/sync"));
        std::fs::create_dir_all(env.sys_path("/Volumes/Backup")).unwrap();
        assert!(program_gone(&env, "/Volumes/Backup/tools/sync"));
        assert!(program_gone(
            &env,
            "/Applications/Gone.app/Contents/MacOS/Gone"
        ));
        assert!(!program_gone(&env, "relative/tool"));
    }

    #[test]
    fn languages() {
        let out = "(\n    \"it-IT\",\n    \"en-US\",\n    \"zh-Hans-IT\"\n)\n";
        assert_eq!(parse_apple_languages(out), vec!["it", "en", "zh"]);
        let keep = vec!["en".to_string(), "it".to_string()];
        assert!(language_kept("Base", &keep));
        assert!(language_kept("en_GB", &keep));
        assert!(language_kept("Italian", &keep));
        assert!(!language_kept("de", &keep));
        assert!(!language_kept("pt-BR", &keep));
        assert!(!language_kept("French", &keep));
    }

    #[test]
    fn lipo_output() {
        assert_eq!(parse_archs("x86_64 arm64\n"), vec!["x86_64", "arm64"]);
        let info = "Fat header in: /Applications/X.app/Contents/MacOS/X\nfat_magic 0xcafebabe\nnfat_arch 2\narchitecture x86_64\n    cputype CPU_TYPE_X86_64\n    cpusubtype CPU_SUBTYPE_X86_64_ALL\n    capabilities 0x0\n    offset 16384\n    size 1234567\n    align 2^14 (16384)\narchitecture arm64\n    cputype CPU_TYPE_ARM64\n    cpusubtype CPU_SUBTYPE_ARM64_ALL\n    capabilities 0x0\n    offset 1261568\n    size 1111111\n    align 2^14 (16384)\n";
        assert_eq!(
            parse_lipo_sizes(info),
            vec![("x86_64".into(), 1_234_567), ("arm64".into(), 1_111_111)]
        );
    }

    #[test]
    fn crash_names() {
        assert_eq!(crash_app("Example-2026-05-01-101010.ips"), "Example");
        assert_eq!(crash_app("Google Chrome_2026-05-01.crash"), "Google Chrome");
        assert_eq!(crash_app("kernel-panic.panic"), "kernel-panic");
    }
}
