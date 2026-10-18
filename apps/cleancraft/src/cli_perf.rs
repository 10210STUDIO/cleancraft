//! CLI for Tune-up Tasks and Startup Items: `cleancraft tasks`, `cleancraft run-task <id> --yes`,
//! `cleancraft startup`. Same engine and journal as the app; a task runs only with `--yes`,
//! after its exact commands are printed.

use std::io::Write;

use cleancraft_core::Env;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::startup::{self, Domain};
use cleancraft_core::tasks::{self, Outcome, TASKS};

pub fn tasks(env: &Env, out: &mut dyn Write) -> anyhow::Result<()> {
    let last = tasks::last_runs(env);
    for t in TASKS {
        let ready = tasks::prepare(env, t.id);
        writeln!(
            out,
            "{} {:<24} {}{}",
            if ready.is_ok() { "[ready]" } else { "[  -  ]" },
            t.id,
            t.title,
            if t.admin {
                "  (administrator password)"
            } else {
                ""
            }
        )?;
        let cmds = match &ready {
            Ok(p) => p.commands(),
            Err(_) => vec![t.command.to_string()],
        };
        for c in cmds {
            writeln!(out, "        $ {c}")?;
        }
        if let Err(why) = &ready {
            writeln!(out, "        cannot run now: {why}")?;
        }
        if let Some((ts, ok)) = last.get(t.id) {
            let when = tasks::local_time(*ts);
            writeln!(
                out,
                "        last run {when} ({})",
                if *ok { "done" } else { "failed" }
            )?;
        }
    }
    writeln!(out, "\nRun one with: cleancraft run-task <id> --yes")?;
    Ok(())
}

pub fn run_task(env: &Env, id: &str, yes: bool, out: &mut dyn Write) -> anyhow::Result<()> {
    let info = tasks::info(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task '{id}' (see `cleancraft tasks`)"))?;
    let p = tasks::prepare(env, id)
        .map_err(|e| anyhow::anyhow!("{} cannot run now: {e}", info.title))?;
    writeln!(out, "{}: {}", info.title, info.description)?;
    for c in p.commands() {
        writeln!(out, "  $ {c}")?;
    }
    if p.needs_admin() {
        writeln!(out, "macOS will ask for the administrator password.")?;
    }
    if !yes {
        anyhow::bail!("nothing ran: add --yes to run these commands");
    }
    let r = tasks::run(env, &p);
    writeln!(out, "{}", r.message)?;
    if let (Some(b), Some(a)) = (r.free_before, r.free_after) {
        writeln!(
            out,
            "Free space before {}, after {}",
            human_size(b),
            human_size(a)
        )?;
    }
    if let (Some(b), Some(a)) = (r.file_before, r.file_after) {
        writeln!(
            out,
            "Mail index before {}, after {}",
            human_size(b),
            human_size(a)
        )?;
    }
    if !r.output.is_empty() {
        writeln!(out, "{}", r.output)?;
    }
    match r.outcome {
        Outcome::Done | Outcome::Cancelled => Ok(()),
        Outcome::Failed => anyhow::bail!("{} failed", info.title),
    }
}

pub fn startup(env: &Env, out: &mut dyn Write) -> anyhow::Result<()> {
    let uid = tasks::current_uid(env).ok();
    writeln!(out, "Login items")?;
    match startup::list_login_items(env) {
        Ok(v) if v.is_empty() => writeln!(out, "  none")?,
        Ok(v) => {
            for l in v {
                writeln!(
                    out,
                    "  [on ] {}  {}",
                    l.name,
                    l.path.as_deref().unwrap_or("location unknown")
                )?;
            }
        }
        Err(e) => writeln!(out, "  {e}")?,
    }
    let (items, notes) = startup::list_launch_items(env, uid);
    for d in Domain::ALL {
        writeln!(out, "\n{} ({})", d.title(), d.folder())?;
        let mut any = false;
        for i in items.iter().filter(|i| i.domain == d) {
            any = true;
            writeln!(
                out,
                "  [{}] {}{}",
                if i.enabled { "on " } else { "off" },
                i.label,
                if i.apple { "  (part of macOS)" } else { "" }
            )?;
            writeln!(
                out,
                "        runs {}  signed by: {}{}",
                i.program.as_deref().unwrap_or("no program"),
                i.signer.label(),
                if i.program.is_some() && !i.program_exists {
                    "  (program missing)"
                } else {
                    ""
                }
            )?;
        }
        if !any {
            writeln!(out, "  none")?;
        }
    }
    for n in notes {
        writeln!(out, "note: {n}")?;
    }
    writeln!(out, "\nHeavy apps")?;
    match uid
        .ok_or_else(|| "could not read your user id".to_string())
        .and_then(|u| startup::list_processes(env, u))
    {
        Ok(v) => {
            let heavy = startup::heavy_apps(&v);
            if heavy.is_empty() {
                writeln!(out, "  none")?;
            }
            for p in heavy {
                writeln!(
                    out,
                    "  {:<24} pid {:<7} CPU {:>5.1}%  Memory {:>5.1}%",
                    p.name, p.pid, p.cpu, p.mem
                )?;
            }
        }
        Err(e) => writeln!(out, "  {e}")?,
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use cleancraft_core::testkit_perf as tp;

    fn text(f: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>) -> (String, bool) {
        let mut b = Vec::new();
        let ok = f(&mut b).is_ok();
        (String::from_utf8(b).unwrap_or_default(), ok)
    }

    #[test]
    fn lists_tasks_with_commands() {
        let m = tp::startup_mac();
        let (s, ok) = text(|o| tasks(&m.env, o));
        assert!(ok);
        assert!(s.contains("[ready] flush_dns"));
        assert!(s.contains("$ diskutil resetUserPermissions / 501"));
        assert!(s.contains("[  -  ] periodic_scripts"));
        assert!(s.contains("cannot run now: Only for macOS 13 and earlier"));
    }

    #[test]
    fn run_task_needs_yes() {
        let m = tp::startup_mac();
        let (s, ok) = text(|o| run_task(&m.env, "free_ram", false, o));
        assert!(!ok);
        assert!(s.contains("$ purge"));
        assert!(m.env.fake_sys().unwrap().admin_calls().is_empty());
        let (s, ok) = text(|o| run_task(&m.env, "free_ram", true, o));
        assert!(ok, "{s}");
        assert!(s.contains("Done"));
        assert_eq!(m.env.fake_sys().unwrap().admin_calls(), vec!["purge"]);
        m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
        let (s, ok) = text(|o| run_task(&m.env, "free_ram", true, o));
        assert!(ok);
        assert!(s.contains("Cancelled, nothing changed"));
        let (_, ok) = text(|o| run_task(&m.env, "nope", true, o));
        assert!(!ok);
    }

    #[test]
    fn lists_startup_items() {
        let m = tp::startup_mac();
        let (s, ok) = text(|o| startup(&m.env, o));
        assert!(ok);
        assert!(s.contains("[on ] Dropbox  /Applications/Dropbox.app"));
        assert!(s.contains("[off] com.example.global"));
        assert!(s.contains("com.apple.sampled  (part of macOS)"));
        assert!(s.contains("(program missing)"));
        assert!(s.contains("Example "));
        assert!(!s.contains("CleanCraft "));
    }
}
