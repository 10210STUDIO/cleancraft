//! CLI for apps: `cleancraft apps`, `cleancraft uninstall <app> [--reset] [--yes]`,
//! `cleancraft updates`. Same engine and safety as the app: the uninstall list is printed and
//! saved as a plan; nothing moves without `--yes` (or a later `cleancraft clean <plan> --yes`).

use std::io::Write;

use clap::Subcommand;
use cleancraft_core::apps;
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::plans;
use cleancraft_core::updater;
use cleancraft_core::{Env, Settings};

#[derive(Subcommand)]
pub enum AppsCmd {
    /// List installed apps with size, version, last opened and store.
    Apps {
        #[arg(long)]
        json: bool,
    },
    /// Show what removing an app would move (the app and its leftovers). --yes moves them.
    Uninstall {
        /// App name, bundle id or path.
        app: String,
        /// Remove the app's data but keep the app.
        #[arg(long)]
        reset: bool,
        /// Move the selected items now.
        #[arg(long)]
        yes: bool,
    },
    /// Check apps and macOS for newer versions (looks only).
    Updates {
        #[arg(long)]
        json: bool,
    },
}

pub fn run(env: &Env, cmd: AppsCmd, out: &mut dyn Write) -> anyhow::Result<()> {
    let settings = Settings::load(env);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match cmd {
        AppsCmd::Apps { json } => {
            let list = apps::inventory(env, true, &|_| {});
            if json {
                writeln!(out, "{}", serde_json::to_string_pretty(&list)?)?;
                return Ok(());
            }
            for a in &list {
                writeln!(
                    out,
                    "{:>10}  {:<28} {:<10} {:<10} opened {:<14}{}",
                    human_size(a.size),
                    a.name,
                    a.version,
                    a.store.label(),
                    apps::last_used_text(a.last_used, now),
                    if a.apple { "  (part of macOS)" } else { "" }
                )?;
            }
            writeln!(out, "{} apps", list.len())?;
        }
        AppsCmd::Uninstall { app, reset, yes } => {
            let list = apps::inventory(env, false, &|_| {});
            let a = apps::find(env, &list, &app)
                .ok_or_else(|| anyhow::anyhow!("no app named '{app}' (see `cleancraft apps`)"))?;
            let plan = uninstaller::uninstall_plan(env, &settings, &a.path, reset)?;
            for it in &plan.items {
                let mark = if it.locked.is_some() {
                    "-"
                } else if it.selected {
                    "x"
                } else {
                    " "
                };
                writeln!(
                    out,
                    "[{mark}] {:>10}  {}  ({}{})",
                    human_size(it.size),
                    env.display(&it.path),
                    it.reason,
                    it.locked
                        .as_ref()
                        .map(|l| format!("; {l}"))
                        .unwrap_or_default()
                )?;
            }
            for n in &plan.notes {
                writeln!(out, "note: {n}")?;
            }
            writeln!(out, "\n{}", plan.summary())?;
            if plan.selected_count() == 0 {
                anyhow::bail!("nothing can be moved for {}", a.name);
            }
            plans::save(env, &plan)?;
            if !yes {
                writeln!(
                    out,
                    "Plan {}. Nothing moved. To move the selected items to the Trash: cleancraft clean {} --yes",
                    plan.id, plan.id
                )?;
                return Ok(());
            }
            let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {})?;
            writeln!(
                out,
                "Moved {}, {} freed.",
                cleancraft_core::fsutil::items(rep.moved.len()),
                human_size(rep.freed)
            )?;
            for (p, why) in &rep.skipped {
                writeln!(out, "skipped {}: {why}", env.display(p))?;
            }
            writeln!(out, "Undo with: cleancraft undo {}", rep.plan_id)?;
        }
        AppsCmd::Updates { json } => {
            let list = apps::inventory(env, false, &|_| {});
            let r = updater::check_all(env, &list, &|_| {});
            if json {
                writeln!(out, "{}", serde_json::to_string_pretty(&r)?)?;
                return Ok(());
            }
            for u in &r.apps {
                let state = if let Some(e) = &u.error {
                    format!("could not check: {e}")
                } else if u.available {
                    format!(
                        "{} -> {} ({})",
                        u.installed,
                        u.latest.clone().unwrap_or_default(),
                        u.source.label()
                    )
                } else {
                    format!("{} is up to date", u.installed)
                };
                writeln!(out, "{:<28} {state}", u.name)?;
            }
            for m in &r.macos {
                writeln!(out, "macOS: {} {}", m.title, m.version)?;
            }
            if let Some(e) = &r.macos_error {
                writeln!(out, "macOS: {e}")?;
            }
            writeln!(
                out,
                "{} updates available. Install them from each app, the App Store or System Settings.",
                r.available().count()
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cleancraft_core::testkit::FakeMac;
    use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};

    fn text(env: &Env, cmd: AppsCmd) -> (anyhow::Result<()>, String) {
        let mut buf = Vec::new();
        let r = run(env, cmd, &mut buf);
        (r, String::from_utf8(buf).unwrap())
    }

    #[test]
    fn apps_uninstall_updates() {
        let mac = FakeMac::new();
        let editor = mac.app_spec(&AppSpec {
            build: "1",
            feed: Some("https://e.com/c.xml"),
            ..AppSpec::new("Editor", "com.example.Editor", "1.0")
        });
        mac.app("Safari", "com.apple.Safari", "18");
        let cache = mac.home_file("Library/Caches/com.example.Editor/c", 5_000);
        mac.web("https://e.com/c.xml", &appcast(&[("2", "1.1", "")]));

        let (r, s) = text(&mac.env, AppsCmd::Apps { json: false });
        r.unwrap();
        assert!(s.contains("Editor") && s.contains("(part of macOS)"));
        assert!(s.contains("2 apps"));

        let (r, s) = text(
            &mac.env,
            AppsCmd::Uninstall {
                app: "editor".into(),
                reset: false,
                yes: false,
            },
        );
        r.unwrap();
        assert!(s.contains("[x]") && s.contains("Nothing moved"));
        assert!(editor.exists());

        let (r, _) = text(
            &mac.env,
            AppsCmd::Uninstall {
                app: "Safari".into(),
                reset: false,
                yes: true,
            },
        );
        assert!(r.is_err(), "Apple app refused");

        let (r, s) = text(&mac.env, AppsCmd::Updates { json: false });
        r.unwrap();
        assert!(s.contains("1.0 -> 1.1"), "{s}");

        let (r, s) = text(
            &mac.env,
            AppsCmd::Uninstall {
                app: "com.example.Editor".into(),
                reset: false,
                yes: true,
            },
        );
        r.unwrap();
        assert!(s.contains("Moved 2 items"), "{s}");
        assert!(!editor.exists() && !cache.exists());
    }
}
