//! Extensions (A11): preference panes, internet plug-ins, Spotlight and Quick Look plug-ins and
//! screen savers in `~/Library` and `/Library`, which can be removed through a plan, and Safari
//! extensions, which live inside their app and are listed (`pluginkit -mAvvv`).

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::features::ScanCtx;
use crate::fsutil::{children, tree_size};
use crate::plan::{Plan, Risk};

pub const CAT_EXTENSIONS: &str = "extensions";

/// (folder name in Library, kind shown to the user)
pub const KINDS: &[(&str, &str)] = &[
    ("PreferencePanes", "Settings pane"),
    ("Internet Plug-Ins", "Internet plug-in"),
    ("Spotlight", "Spotlight plug-in"),
    ("QuickLook", "Quick Look plug-in"),
    ("Screen Savers", "Screen saver"),
];

/// Every extension folder with its kind, user ones first.
pub fn dirs(env: &Env) -> Vec<(PathBuf, &'static str)> {
    let mut v = Vec::new();
    for (d, kind) in KINDS {
        v.push((env.home_path(&format!("Library/{d}")), *kind));
    }
    for (d, kind) in KINDS {
        v.push((env.sys_path(&format!("/Library/{d}")), *kind));
    }
    v
}

/// One Safari extension from `pluginkit -mAvvv -p com.apple.Safari.web-extension`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SafariExt {
    pub id: String,
    pub version: String,
    pub name: String,
    pub path: String,
    pub parent: String,
    /// `+` (on) or `-` (off) as pluginkit shows it, when it does.
    pub enabled: Option<bool>,
}

/// Parse `pluginkit -mAvvv` output: a header line `[+|-]    com.x.Ext(1.0)` then
/// `Key = Value` lines.
pub fn parse_pluginkit(out: &str) -> Vec<SafariExt> {
    let mut v: Vec<SafariExt> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((k, val)) = line.split_once(" = ") {
            let Some(cur) = v.last_mut() else { continue };
            let val = val.trim().to_string();
            match k.trim() {
                "Path" => cur.path = val,
                "Parent Bundle" => cur.parent = val,
                "Display Name" => cur.name = val,
                _ => {}
            }
            continue;
        }
        let t = line.trim();
        let (mark, rest) = match t.chars().next() {
            Some(c @ ('+' | '-' | '!' | '=' | '?')) => (Some(c), t[1..].trim()),
            _ => (None, t),
        };
        let Some((id, ver)) = rest.split_once('(') else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || id.contains(' ') {
            continue;
        }
        v.push(SafariExt {
            id: id.to_string(),
            version: ver.trim_end_matches(')').to_string(),
            enabled: match mark {
                Some('+') => Some(true),
                Some('-') => Some(false),
                _ => None,
            },
            ..Default::default()
        });
    }
    for e in &mut v {
        if e.name.is_empty() {
            e.name = Path::new(&e.path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| e.id.clone());
        }
    }
    v
}

/// Add the extensions to `plan`: file-based ones selectable (never preselected), Safari ones
/// locked because they are part of their app.
pub fn scan(ctx: &ScanCtx<'_>, allow: &[PathBuf], plan: &mut Plan) {
    let env = ctx.env;
    plan.add_category(
        CAT_EXTENSIONS,
        "Extensions",
        "Settings panes, plug-ins and screen savers added by apps.",
        Risk::Medium,
    );
    for (dir, kind) in dirs(env) {
        if ctx.cancelled() {
            return;
        }
        (ctx.progress)(&env.display(&dir));
        let writable = env.can_modify(&dir);
        for entry in children(&dir) {
            if entry
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                || !ctx.allowed(allow, &entry)
            {
                continue;
            }
            let size = tree_size(&entry).0;
            let scope = if entry.starts_with(env.home()) {
                ""
            } else {
                " (all users)"
            };
            let id = plan.push(
                entry,
                size,
                CAT_EXTENSIONS,
                format!("{kind}{scope}"),
                Risk::Medium,
                false,
            );
            if !writable {
                // App Remover allows administrator moves: the confirmation asks for the password.
                plan.mark_admin(id);
            }
        }
    }
    match env.sys.run(
        "/usr/bin/pluginkit",
        &["-mAvvv", "-p", "com.apple.Safari.web-extension"],
    ) {
        Ok(o) if o.ok() => {
            for e in parse_pluginkit(&o.stdout) {
                if e.path.is_empty() {
                    continue;
                }
                let path = env.sys_path(&e.path);
                let size = tree_size(&path).0;
                let parent = Path::new(&e.parent)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "its app".into());
                let state = match e.enabled {
                    Some(true) => ", on",
                    Some(false) => ", off",
                    None => "",
                };
                let id = plan.push(
                    path,
                    size,
                    CAT_EXTENSIONS,
                    format!("Safari extension {} {}{state}", e.name, e.version),
                    Risk::Medium,
                    false,
                );
                plan.lock(
                    id,
                    format!("Part of {parent}; it goes away when {parent} is removed"),
                );
            }
        }
        _ => plan
            .notes
            .push("Safari extensions could not be listed.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = "+    com.example.Blocker.Extension(2.1)
\t            Path = /Applications/Blocker.app/Contents/PlugIns/Blocker Extension.appex
\t            UUID = 0B1C1B6E-6A43-4F3E-9D3B-111111111111
\t       Timestamp = 2025-03-01 10:00:00 +0000
\t             SDK = com.apple.Safari.web-extension
\t   Parent Bundle = /Applications/Blocker.app
\t    Display Name = Blocker Extension
\t      Short Name = Blocker
-    com.example.Notes.Safari(1.0)
\t            Path = /Applications/Notes Helper.app/Contents/PlugIns/Clipper.appex
\t   Parent Bundle = /Applications/Notes Helper.app
";

    #[test]
    fn pluginkit_lines() {
        let v = parse_pluginkit(SAMPLE);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "com.example.Blocker.Extension");
        assert_eq!(v[0].version, "2.1");
        assert_eq!(v[0].name, "Blocker Extension");
        assert_eq!(v[0].enabled, Some(true));
        assert_eq!(v[0].parent, "/Applications/Blocker.app");
        assert_eq!(v[1].name, "Clipper");
        assert_eq!(v[1].enabled, Some(false));
    }
}
