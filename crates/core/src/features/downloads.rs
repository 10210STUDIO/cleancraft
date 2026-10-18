//! Downloads by source app: every item at the top of `~/Downloads`, grouped by the app that
//! saved it. Offered as a location of Large & Old Files ("Downloads by source app"), so it
//! shares that feature's allow-list (the home folder) and its plans.
//!
//! macOS marks downloaded files with the extended attribute `com.apple.quarantine`, whose value
//! is `flags;hex time;agent name;event id`, for example `0083;65f1c2a0;Safari;4C1E...`. The agent
//! name is the app. It is read with `xattr -p com.apple.quarantine <file>` (one call per item;
//! the command fails when the attribute is missing). Folders and files made on this Mac have no
//! such mark and are listed under "No source recorded". Nothing is selected.

use std::path::Path;

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Plan, Risk};

use super::large_old::{LargeOldFiles, skipped};

/// The app name in a `com.apple.quarantine` value.
pub fn parse_quarantine(value: &str) -> Option<String> {
    let agent = value.trim().split(';').nth(2)?.trim();
    if agent.is_empty() {
        None
    } else {
        Some(agent.to_string())
    }
}

/// The app that saved `p`, from its quarantine mark.
pub fn source_of(env: &Env, p: &Path) -> Option<String> {
    let path = p.to_string_lossy();
    let out = env
        .sys
        .run("/usr/bin/xattr", &["-p", "com.apple.quarantine", &path])
        .ok()?;
    if !out.ok() {
        return None;
    }
    parse_quarantine(&out.stdout)
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub const NO_SOURCE: &str = "src_none";

pub fn scan(ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
    let env = ctx.env;
    let allow = LargeOldFiles.allow_roots(env);
    let mut plan = Plan::new(FeatureId::LargeOldFiles);
    let root = env.home_path("Downloads");
    if !root.is_dir() {
        plan.notes
            .push(format!("{} does not exist.", env.display(&root)));
        return Ok(plan);
    }
    if std::fs::read_dir(&root).is_err() {
        plan.notes.push(format!(
            "{} could not be read. Allow CleanCraft to read Downloads in System Settings > Privacy & Security.",
            env.display(&root)
        ));
        return Ok(plan);
    }
    (ctx.progress)(&env.display(&root));
    let mut found: Vec<(String, String, std::path::PathBuf, u64, Option<i64>)> = Vec::new();
    for p in children(&root) {
        if ctx.cancelled() {
            plan.notes.push("Scan stopped before the end.".into());
            break;
        }
        if skipped(env, &p) || !ctx.allowed(&allow, &p) {
            continue;
        }
        let (size, _) = tree_size(&p);
        let modified = std::fs::symlink_metadata(&p)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        match source_of(env, &p) {
            Some(app) => found.push((format!("src_{}", slug(&app)), app, p, size, modified)),
            None => found.push((NO_SOURCE.into(), String::new(), p, size, modified)),
        }
    }
    // Categories: apps by name, "No source recorded" last.
    let mut apps: Vec<(String, String)> = found
        .iter()
        .filter(|f| f.0 != NO_SOURCE)
        .map(|f| (f.0.clone(), f.1.clone()))
        .collect();
    apps.sort_by_key(|a| a.1.to_lowercase());
    apps.dedup_by(|a, b| a.0 == b.0);
    for (id, app) in &apps {
        plan.add_category(
            id,
            &format!("Saved by {app}"),
            &format!("Items in Downloads that {app} saved."),
            Risk::High,
        );
    }
    if found.iter().any(|f| f.0 == NO_SOURCE) {
        plan.add_category(
            NO_SOURCE,
            "No source recorded",
            "Folders, and files macOS did not mark as downloaded.",
            Risk::High,
        );
    }
    for (cat, app, p, size, modified) in found {
        let reason = if app.is_empty() {
            "In Downloads, no record of the app that saved it".to_string()
        } else {
            format!("Saved to Downloads by {app}")
        };
        let id = plan.push(p, size, &cat, reason, Risk::High, false);
        plan.items[id].modified = modified;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_values() {
        assert_eq!(
            parse_quarantine("0083;65f1c2a0;Safari;4C1E0F2A-1111\n").as_deref(),
            Some("Safari")
        );
        assert_eq!(
            parse_quarantine("0081;5f1b2c3d;Google Chrome;").as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(parse_quarantine("0083;65f1c2a0;;x"), None);
        assert_eq!(parse_quarantine("garbage"), None);
    }
}
