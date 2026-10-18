//! Tune-up Tasks: maintenance commands macOS already ships (flush the DNS cache, reindex
//! Spotlight, thin local snapshots...). CleanCraft only runs them for the user, after showing
//! the exact command.
//!
//! Flow: [`prepare`] checks that a task can run now and resolves its exact commands (the user
//! id, the snapshot dates, Mail's index file). The confirmation shows those commands. [`run`]
//! runs exactly them, through `env.sys` (administrator ones through `run_admin`, which shows
//! the macOS password dialog), and logs the run in the journal as an
//! [`ActionKind::Command`]. Commands cannot be undone; the log says what ran.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::env::{Env, Fault};
use crate::executor::{self, ActionKind, ActionRecord};
use crate::plan::{FeatureId, Risk};
use crate::sys::CmdOut;

/// One task as shown in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskInfo {
    /// Stable id used by the CLI, MCP tools and the journal.
    pub id: &'static str,
    pub title: &'static str,
    /// What it does, in CleanCraft's own words.
    pub description: &'static str,
    /// The command, as a template shown before the exact one is known.
    pub command: &'static str,
    /// Needs the administrator password.
    pub admin: bool,
    pub risk: Risk,
}

pub const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister";

pub const TASKS: &[TaskInfo] = &[
    TaskInfo {
        id: "flush_dns",
        title: "Flush DNS cache",
        description: "Forgets the saved addresses of websites so they are looked up again. Helps when a site moved or will not load.",
        command: "dscacheutil -flushcache; killall -HUP mDNSResponder",
        admin: true,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "free_ram",
        title: "Free up RAM",
        description: "Asks macOS to write out and drop cached memory, so memory is free for the next app you open.",
        command: "purge",
        admin: true,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "purgeable_space",
        title: "Free up purgeable space",
        description: "Asks Time Machine to give back disk space held by local snapshots that macOS marked as removable. Shows free space before and after.",
        command: "tmutil thinlocalsnapshots / 999999999999 4",
        admin: false,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "thin_snapshots",
        title: "Thin Time Machine snapshots",
        description: "Removes the local Time Machine snapshots on the startup disk. Backups on your backup disk are not touched.",
        command: "tmutil deletelocalsnapshots <date> (for each local snapshot)",
        admin: true,
        risk: Risk::Medium,
    },
    TaskInfo {
        id: "reindex_spotlight",
        title: "Reindex Spotlight",
        description: "Erases and rebuilds the Spotlight index of the startup disk. Search results are incomplete until it finishes.",
        command: "mdutil -E /",
        admin: true,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "rebuild_launch_services",
        title: "Rebuild Launch Services",
        description: "Rebuilds the list that decides which app opens which file. Fixes duplicate entries in Open With menus.",
        command: "lsregister -kill -r -domain local -domain system -domain user",
        admin: false,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "repair_permissions",
        title: "Repair home folder permissions",
        description: "Resets the permissions of the files in your home folder to the macOS defaults.",
        command: "diskutil resetUserPermissions / <your user id>",
        admin: true,
        risk: Risk::Medium,
    },
    TaskInfo {
        id: "verify_disk",
        title: "Verify startup disk",
        description: "Checks the startup disk for file system errors. It only looks and reports.",
        command: "diskutil verifyVolume /",
        admin: false,
        risk: Risk::Low,
    },
    TaskInfo {
        id: "speed_up_mail",
        title: "Speed up Mail",
        description: "Compacts Mail's message index so searching and opening mailboxes is faster. Mail must be closed.",
        command: "sqlite3 \"<Mail Envelope Index>\" \"VACUUM;\"",
        admin: false,
        risk: Risk::Medium,
    },
    TaskInfo {
        id: "periodic_scripts",
        title: "Run periodic scripts",
        description: "Runs the daily, weekly and monthly housekeeping scripts of macOS 13 and earlier: rotates logs and clears old temporary files.",
        command: "periodic daily weekly monthly",
        admin: true,
        risk: Risk::Low,
    },
];

pub fn info(id: &str) -> Option<&'static TaskInfo> {
    TASKS.iter().find(|t| t.id == id)
}

/// One command of a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Step {
    /// Run as the user.
    Run { program: String, args: Vec<String> },
    /// Run as administrator (macOS asks for the password).
    Admin { shell: String },
}

impl Step {
    fn run(program: &str, args: &[&str]) -> Step {
        Step::Run {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// The command as a shell would read it.
    pub fn display(&self) -> String {
        match self {
            Step::Run { program, args } => std::iter::once(program.clone())
                .chain(args.iter().map(|a| sh_quote_if_needed(a)))
                .collect::<Vec<_>>()
                .join(" "),
            Step::Admin { shell } => shell.clone(),
        }
    }

    pub fn is_admin(&self) -> bool {
        matches!(self, Step::Admin { .. })
    }
}

/// `'it'\''s'`: a string the shell reads back unchanged.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Quote only when the shell would otherwise split or interpret the text.
pub fn sh_quote_if_needed(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-=:+,@%".contains(c))
    {
        s.to_string()
    } else {
        sh_quote(s)
    }
}

/// A task ready to run: the exact commands the confirmation shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Prepared {
    pub id: String,
    pub steps: Vec<Step>,
    /// Measure free space on the startup disk before and after.
    pub measure_free: bool,
    /// A file whose size is shown before and after (Mail's index).
    pub measure_file: Option<PathBuf>,
}

impl Prepared {
    pub fn commands(&self) -> Vec<String> {
        self.steps.iter().map(Step::display).collect()
    }

    pub fn needs_admin(&self) -> bool {
        self.steps.iter().any(Step::is_admin)
    }
}

/// The user id, from `id -u`.
pub fn current_uid(env: &Env) -> Result<u32, String> {
    let out = env
        .sys
        .run("/usr/bin/id", &["-u"])
        .map_err(|e| format!("could not read your user id: {e}"))?;
    out.stdout
        .trim()
        .parse()
        .map_err(|_| format!("could not read your user id from `{}`", out.stdout.trim()))
}

/// (major, minor) from `sw_vers -productVersion`, e.g. "13.6.1" -> (13, 6).
pub fn macos_version(env: &Env) -> Option<(u32, u32)> {
    let out = env.sys.run("/usr/bin/sw_vers", &["-productVersion"]).ok()?;
    parse_version(&out.stdout)
}

pub fn parse_version(s: &str) -> Option<(u32, u32)> {
    let mut it = s.trim().split('.');
    let major = it.next()?.trim().parse().ok()?;
    let minor = it.next().and_then(|m| m.trim().parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// Dates of the local snapshots in `tmutil listlocalsnapshots /` output. Lines look like
/// `com.apple.TimeMachine.2026-10-17-101530.local`.
pub fn parse_snapshots(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("com.apple.TimeMachine.")?
                .strip_suffix(".local")
                .map(str::to_string)
        })
        .filter(|d| d.chars().all(|c| c.is_ascii_digit() || c == '-') && !d.is_empty())
        .collect()
}

/// Mail's message index: `~/Library/Mail/V<n>/MailData/Envelope Index`, highest `n`.
pub fn mail_envelope_index(env: &Env) -> Result<PathBuf, String> {
    let mail = env.home_path("Library/Mail");
    if matches!(env.fault_for(&mail), Some(Fault::PermissionDenied)) {
        return Err(
            "CleanCraft cannot read Mail's folder. Turn on Full Disk Access for CleanCraft.".into(),
        );
    }
    let rd = match fs::read_dir(&mail) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(
                "CleanCraft cannot read Mail's folder. Turn on Full Disk Access for CleanCraft."
                    .into(),
            );
        }
        Err(_) => return Err("Mail has no data on this Mac.".into()),
    };
    let mut best: Option<(u32, PathBuf)> = None;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(n) = name.strip_prefix('V').and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let idx = e.path().join("MailData/Envelope Index");
        if idx.is_file() && best.as_ref().is_none_or(|(b, _)| n > *b) {
            best = Some((n, idx));
        }
    }
    best.map(|(_, p)| p)
        .ok_or_else(|| "Mail's message index was not found.".into())
}

/// Is Mail open?
pub fn mail_running(env: &Env) -> bool {
    env.sys.running_apps().iter().any(|a| {
        a.bundle_id.as_deref() == Some("com.apple.mail")
            || a.path
                .as_ref()
                .is_some_and(|p| p.file_name().is_some_and(|n| n == "Mail.app"))
    })
}

/// Check that the task can run now and resolve its exact commands. `Err` says why not.
pub fn prepare(env: &Env, id: &str) -> Result<Prepared, String> {
    let t = info(id).ok_or_else(|| format!("unknown task '{id}'"))?;
    if !env.sys.is_macos() {
        return Err("This task needs macOS.".into());
    }
    let mut p = Prepared {
        id: t.id.to_string(),
        steps: Vec::new(),
        measure_free: false,
        measure_file: None,
    };
    match t.id {
        "flush_dns" => p.steps.push(Step::Admin {
            shell: "dscacheutil -flushcache; killall -HUP mDNSResponder".into(),
        }),
        "free_ram" => p.steps.push(Step::Admin {
            shell: "purge".into(),
        }),
        "purgeable_space" => {
            p.steps.push(Step::run(
                "/usr/bin/tmutil",
                &["thinlocalsnapshots", "/", "999999999999", "4"],
            ));
            p.measure_free = true;
        }
        "thin_snapshots" => {
            let out = env
                .sys
                .run("/usr/bin/tmutil", &["listlocalsnapshots", "/"])
                .map_err(|e| format!("Could not list local snapshots: {e}"))?;
            if !out.ok() {
                return Err(format!(
                    "Could not list local snapshots: {}",
                    first_line(&out.stderr)
                ));
            }
            let dates = parse_snapshots(&out.stdout);
            if dates.is_empty() {
                return Err(
                    "There are no local Time Machine snapshots on the startup disk.".into(),
                );
            }
            let shell = dates
                .iter()
                .map(|d| format!("tmutil deletelocalsnapshots {d}"))
                .collect::<Vec<_>>()
                .join(" && ");
            p.steps.push(Step::Admin { shell });
            p.measure_free = true;
        }
        "reindex_spotlight" => p.steps.push(Step::Admin {
            shell: "mdutil -E /".into(),
        }),
        "rebuild_launch_services" => p.steps.push(Step::run(
            LSREGISTER,
            &[
                "-kill", "-r", "-domain", "local", "-domain", "system", "-domain", "user",
            ],
        )),
        "repair_permissions" => {
            let uid = current_uid(env)?;
            p.steps.push(Step::Admin {
                shell: format!("diskutil resetUserPermissions / {uid}"),
            });
        }
        "verify_disk" => p
            .steps
            .push(Step::run("/usr/sbin/diskutil", &["verifyVolume", "/"])),
        "speed_up_mail" => {
            if mail_running(env) {
                return Err("Mail is open. Quit Mail first.".into());
            }
            let idx = mail_envelope_index(env)?;
            let shown = env.display(&idx);
            p.steps
                .push(Step::run("/usr/bin/sqlite3", &[&shown, "VACUUM;"]));
            p.measure_file = Some(idx);
        }
        "periodic_scripts" => match macos_version(env) {
            Some((major, _)) if major <= 13 => p.steps.push(Step::Admin {
                shell: "periodic daily weekly monthly".into(),
            }),
            Some((major, minor)) => {
                return Err(format!(
                    "Only for macOS 13 and earlier. This Mac runs macOS {major}.{minor}, which runs this housekeeping by itself."
                ));
            }
            None => return Err("Could not read the macOS version.".into()),
        },
        _ => return Err(format!("unknown task '{id}'")),
    }
    Ok(p)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Failed,
    /// The administrator password dialog was cancelled; nothing ran.
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunResult {
    pub id: String,
    pub outcome: Outcome,
    /// What the user reads: "Done", the reason of a failure, "Cancelled, nothing changed".
    pub message: String,
    /// Output of the commands (first lines).
    pub output: String,
    pub commands: Vec<String>,
    pub free_before: Option<u64>,
    pub free_after: Option<u64>,
    pub file_before: Option<u64>,
    pub file_after: Option<u64>,
    /// The journal line written for this run.
    pub record_id: Option<String>,
}

pub const CANCELLED_TEXT: &str = "Cancelled, nothing changed";

/// Tune-up tasks may take long (Spotlight reindex, snapshots, scripts): their own time limit,
/// longer than the default one of [`crate::sys::Sys::run`]. Administrator steps have
/// [`crate::sys::ADMIN_TIMEOUT`].
pub const TASK_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The administrator dialog was cancelled: `osascript` reports error -128.
pub fn is_admin_cancel(out: &CmdOut) -> bool {
    !out.ok()
        && (out.stderr.contains("(-128)")
            || out.stderr.contains("User canceled")
            || out.stderr.contains("User cancelled"))
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// The first `n` non-empty lines.
pub fn first_lines(s: &str, n: usize) -> String {
    s.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The journal must be writable before a command runs, like before a file moves.
pub fn journal_writable(env: &Env) -> io::Result<()> {
    let path = executor::journal_path(env);
    if let Some(Fault::DiskFull) = env.fault_for(&path) {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
    }
    fs::create_dir_all(env.data_dir())?;
    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

fn startup_disk(env: &Env) -> PathBuf {
    env.sys_path("/")
}

/// Run a prepared task: exactly its steps, in order, stopping at the first failure.
pub fn run(env: &Env, p: &Prepared) -> RunResult {
    let mut r = RunResult {
        id: p.id.clone(),
        outcome: Outcome::Done,
        message: "Done".into(),
        output: String::new(),
        commands: p.commands(),
        free_before: None,
        free_after: None,
        file_before: None,
        file_after: None,
        record_id: None,
    };
    if let Err(e) = journal_writable(env) {
        r.outcome = Outcome::Failed;
        r.message = format!("Could not write the action log, nothing ran: {e}");
        return r;
    }
    // Mail could have been opened since the confirmation.
    if p.id == "speed_up_mail" && mail_running(env) {
        r.outcome = Outcome::Failed;
        r.message = "Mail is open. Quit Mail and run the task again.".into();
        return r;
    }
    if p.measure_free {
        r.free_before = env.sys.free_space(&startup_disk(env)).ok();
    }
    if let Some(f) = &p.measure_file {
        r.file_before = fs::metadata(f).ok().map(|m| m.len());
    }
    let mut output = String::new();
    for step in &p.steps {
        let res = match step {
            Step::Run { program, args } => {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                env.sys.run_timeout(program, &a, TASK_TIMEOUT)
            }
            Step::Admin { shell } => env.sys.run_admin(shell),
        };
        match res {
            Ok(out) => {
                if step.is_admin() && is_admin_cancel(&out) {
                    // Nothing ran: no journal line.
                    r.outcome = Outcome::Cancelled;
                    r.message = CANCELLED_TEXT.into();
                    r.output = output;
                    return r;
                }
                for s in [&out.stdout, &out.stderr] {
                    if !s.trim().is_empty() {
                        output.push_str(s.trim_end());
                        output.push('\n');
                    }
                }
                if !out.ok() {
                    r.outcome = Outcome::Failed;
                    let why = first_line(&out.stderr);
                    let why = if why.is_empty() {
                        first_line(&out.stdout)
                    } else {
                        why
                    };
                    r.message = if why.is_empty() {
                        format!("Failed (exit code {})", out.status)
                    } else {
                        format!("Failed: {why}")
                    };
                    break;
                }
            }
            Err(e) => {
                r.outcome = Outcome::Failed;
                r.message = if e.kind() == io::ErrorKind::TimedOut {
                    format!("Failed: {e}")
                } else {
                    format!("Could not start `{}`: {e}", step.display())
                };
                output.push_str(&e.to_string());
                break;
            }
        }
    }
    r.output = first_lines(&output, 20);
    if p.measure_free {
        r.free_after = env.sys.free_space(&startup_disk(env)).ok();
    }
    if let Some(f) = &p.measure_file {
        r.file_after = fs::metadata(f).ok().map(|m| m.len());
    }
    let now = chrono::Utc::now();
    let plan_id = format!("task-{}-{}", p.id, now.timestamp_millis());
    let rec = ActionRecord {
        id: format!("{plan_id}#0"),
        plan_id,
        feature: Some(FeatureId::Maintenance),
        time: now.timestamp(),
        size: match (r.free_before, r.free_after) {
            (Some(b), Some(a)) if a > b => a - b,
            _ => 0,
        },
        kind: ActionKind::Command {
            command: r.commands.join("\n"),
            ok: r.outcome == Outcome::Done,
            output: r.output.clone(),
        },
        undone: None,
    };
    match executor::append_journal(env, &rec) {
        Ok(()) => r.record_id = Some(rec.id),
        Err(e) => {
            r.message = format!("{} (the action log could not be written: {e})", r.message);
        }
    }
    r
}

/// Prepare and run one task by id (CLI and MCP).
pub fn run_by_id(env: &Env, id: &str) -> Result<RunResult, String> {
    let p = prepare(env, id)?;
    Ok(run(env, &p))
}

/// A journal time as local "YYYY-MM-DD HH:MM".
pub fn local_time(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Last run of each task from the journal: id -> (time, succeeded).
pub fn last_runs(env: &Env) -> BTreeMap<String, (i64, bool)> {
    let mut m = BTreeMap::new();
    for r in executor::history(env) {
        if r.feature != Some(FeatureId::Maintenance) {
            continue;
        }
        let ActionKind::Command { ok, .. } = r.kind else {
            continue;
        };
        if let Some(id) = task_of_plan(&r.plan_id) {
            m.insert(id, (r.time, ok));
        }
    }
    m
}

/// "task-flush_dns-1760000000000" -> "flush_dns".
pub fn task_of_plan(plan_id: &str) -> Option<String> {
    let rest = plan_id.strip_prefix("task-")?;
    let (id, ts) = rest.rsplit_once('-')?;
    ts.parse::<i64>().ok()?;
    info(id).map(|t| t.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
        assert_eq!(
            Step::run("/usr/bin/sqlite3", &["/Users/a/Envelope Index", "VACUUM;"]).display(),
            "/usr/bin/sqlite3 '/Users/a/Envelope Index' 'VACUUM;'"
        );
    }

    #[test]
    fn versions_and_snapshots() {
        assert_eq!(parse_version("13.6.1\n"), Some((13, 6)));
        assert_eq!(parse_version("15.0"), Some((15, 0)));
        assert_eq!(parse_version("26"), Some((26, 0)));
        assert_eq!(parse_version("x"), None);
        let s = "Snapshots for disk /:\ncom.apple.TimeMachine.2026-10-17-101530.local\ncom.apple.TimeMachine.2026-10-18-091200.local\n";
        assert_eq!(
            parse_snapshots(s),
            vec!["2026-10-17-101530", "2026-10-18-091200"]
        );
        assert!(parse_snapshots("Snapshots for disk /:\n").is_empty());
    }

    #[test]
    fn plan_ids() {
        assert_eq!(
            task_of_plan("task-flush_dns-1760000000000").as_deref(),
            Some("flush_dns")
        );
        assert_eq!(task_of_plan("task-nope-1"), None);
        assert_eq!(task_of_plan("system_junk-1"), None);
    }
}
