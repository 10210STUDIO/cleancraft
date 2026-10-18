//! System Junk: caches, logs and other files macOS and apps rebuild by themselves.
//!
//! Each proposal is one entry (a cache folder of one app, one log folder), sized with the
//! shared formula. Entries in folders this user cannot change without administrator rights
//! are listed but locked, so the numbers are honest about what can be freed.

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Plan, Risk};

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

pub struct Area {
    pub category: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub risk: Risk,
    pub selected: bool,
    /// Absolute macOS path; "~/" means the home folder.
    pub path: &'static str,
}

pub const AREAS: &[Area] = &[
    Area {
        category: "user_caches",
        title: "User cache files",
        description: "Temporary data apps keep to work faster. Apps rebuild them when needed.",
        risk: Risk::Low,
        selected: true,
        path: "~/Library/Caches",
    },
    Area {
        category: "system_caches",
        title: "System cache files",
        description: "Caches shared by all users. Most need administrator rights.",
        risk: Risk::Low,
        selected: true,
        path: "/Library/Caches",
    },
    Area {
        category: "user_logs",
        title: "User log files",
        description: "Activity records written by apps for troubleshooting.",
        risk: Risk::Low,
        selected: true,
        path: "~/Library/Logs",
    },
    Area {
        category: "system_logs",
        title: "System log files",
        description: "Activity records written by macOS services.",
        risk: Risk::Low,
        selected: true,
        path: "/Library/Logs",
    },
    Area {
        category: "xcode_derived",
        title: "Xcode build data",
        description: "Intermediate build products. Xcode rebuilds them on the next build.",
        risk: Risk::Low,
        selected: true,
        path: "~/Library/Developer/Xcode/DerivedData",
    },
];

fn resolve(env: &Env, p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rel) => env.home_path(rel),
        None => env.sys_path(p),
    }
}

impl Feature for SystemJunk {
    fn id(&self) -> FeatureId {
        FeatureId::SystemJunk
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        AREAS.iter().map(|a| resolve(env, a.path)).collect()
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let env = ctx.env;
        let allow = self.allow_roots(env);
        let mut plan = Plan::new(FeatureId::SystemJunk);
        for area in AREAS {
            if ctx.cancelled() {
                plan.notes.push("Scan stopped before the end.".into());
                break;
            }
            let root = resolve(env, area.path);
            (ctx.progress)(&env.display(&root));
            if !root.exists() {
                continue;
            }
            plan.add_category(area.category, area.title, area.description, area.risk);
            if std::fs::read_dir(&root).is_err() {
                plan.notes.push(format!(
                    "{} could not be read. Full Disk Access may be needed.",
                    env.display(&root)
                ));
                continue;
            }
            let writable = env.can_modify(&root);
            for entry in children(&root) {
                if !keep(&entry) && ctx.allowed(&allow, &entry) {
                    let (size, _) = tree_size(&entry);
                    if size == 0 {
                        continue;
                    }
                    let name = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let id = plan.push(entry.clone(), size, area.category, format!("{}: {name}", area.title), area.risk, area.selected);
                    if !writable {
                        plan.lock(id, "Needs administrator rights");
                    }
                }
            }
        }
        Ok(plan)
    }
}

fn keep(p: &Path) -> bool {
    p.file_name()
        .map(|n| n.to_string_lossy())
        .is_some_and(|n| KEEP_CACHES.iter().any(|k| *k == n) || n.starts_with('.'))
}
