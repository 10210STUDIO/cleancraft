//! Quitting an app before it is removed, with everything it keeps running in the background.
//!
//! Many apps leave helpers running from inside their bundle after the main window is closed: a
//! menu bar helper, a health monitor, a login item, a launch agent, a system service (launch
//! daemon or privileged helper). App Remover locks the whole plan while any process inside the
//! bundle runs ([`crate::features::uninstaller::app_lock`]), so this module lets the user see
//! what runs and stop it:
//!
//! - [`running_parts`] lists each process inside the bundle (or in a folder named after the
//!   app's id, such as `/Library/PrivilegedHelperTools/<id>.helper`), whether it is an app
//!   known to the window server (`lsappinfo list`), a login item, or a launchd job (label and
//!   domain from `launchctl print gui/<uid>` and `launchctl print system`), plus launchd jobs
//!   of the app that are loaded: their property list points into the bundle, or their label
//!   is the app's id or starts with it.
//! - [`quit_app`] asks politely: `launchctl bootout gui/<uid>/<label>` for the app's own
//!   agents and login items (so launchd does not start them again), `osascript` "quit" for the
//!   app and its helper apps, and for system services the administrator path (one password
//!   dialog), only when the caller says the user confirmed. It waits a few seconds, checking
//!   again, and reports what still runs and what started again by itself.
//! - [`force_quit`] is the explicit next step for what ignored the polite quit: `kill -TERM`,
//!   then `kill -KILL` after a grace period, only for processes inside the bundle.
//!
//! Every quit, bootout and kill is written to the journal. A stopped agent's line carries the
//! command that loads it again, so History can bring it back when the uninstall is cancelled.
//! Nothing here moves or changes a file. Every command goes through `env.sys`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::apps;
use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::features::uninstaller::matches_id;
use crate::plan::FeatureId;
use crate::startup::{self, LAUNCHCTL, OSASCRIPT, as_quote};
use crate::sys::{CmdOut, app_bundle_of};
use crate::tasks::{self, sh_quote};

pub const PS: &str = "/bin/ps";
/// `ps` with the owner of each process: "  612   501 /Applications/X.app/Contents/MacOS/X".
pub const PS_ARGS: &[&str] = &["-axo", "pid=,uid=,comm="];
pub const LSAPPINFO: &str = "/usr/bin/lsappinfo";
pub const KILL: &str = "/bin/kill";
/// How long an app may take to answer the polite "quit" (it may ask to save documents).
pub const QUIT_ANSWER_SECS: u32 = 10;

// ---------------------------------------------------------------------------------------------
// What runs

/// Which launchd domain a job is loaded in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobDomain {
    /// Your session (`gui/<uid>`): launch agents and login items. You can stop these.
    Gui,
    /// The whole Mac (`system`): launch daemons and privileged helpers. Stopping one needs the
    /// administrator password.
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LaunchJob {
    pub label: String,
    pub domain: JobDomain,
    /// The property list, as a macOS path, when CleanCraft found it.
    pub plist: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// The app itself (`X.app/Contents/MacOS/X`).
    App,
    /// An app inside the app (`X.app/Contents/Library/LoginItems/X Menu.app`).
    HelperApp,
    /// A plain program (`X.app/Contents/Library/LaunchServices/helper`, a privileged helper).
    Process,
    /// A launchd job of the app that is loaded but has no process right now.
    Job,
}

/// One thing of the app that runs (or is loaded and may start again).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunningPart {
    /// What to show: the app or helper name, else the program's file name, else the label.
    pub name: String,
    pub kind: PartKind,
    /// None for a loaded launchd job without a process.
    pub pid: Option<u32>,
    /// The process owner (0 is the system), when `ps` said.
    pub uid: Option<u32>,
    /// The program, as a macOS path.
    pub path: Option<String>,
    /// Bundle id of an app known to the window server (`lsappinfo`) or of the helper app.
    pub bundle_id: Option<String>,
    /// `lsappinfo` type: "Foreground" (a normal app), "UIElement" (menu bar helper),
    /// "BackgroundOnly".
    pub gui: Option<String>,
    pub job: Option<LaunchJob>,
    /// Started at login (an app in `Contents/Library/LoginItems`).
    pub login_item: bool,
    /// Inside the app's bundle: while it runs, nothing of the app may move.
    pub inside_bundle: bool,
}

impl RunningPart {
    pub fn running(&self) -> bool {
        self.pid.is_some()
    }

    /// Keeps the uninstall locked: a running process inside the bundle.
    pub fn locks(&self) -> bool {
        self.running() && self.inside_bundle
    }

    /// Stopping it needs the administrator password: a system job, or a process of another
    /// user (root) that no job of your session controls.
    pub fn needs_admin(&self, me: Option<u32>) -> bool {
        match &self.job {
            Some(j) if j.domain == JobDomain::System => true,
            Some(_) => false,
            None => match (self.uid, me) {
                (Some(u), Some(m)) => u != m,
                (Some(u), None) => u == 0,
                _ => false,
            },
        }
    }

    /// Short words for the row: "the app", "menu bar helper, opens at login",
    /// "background agent", "system service".
    pub fn role(&self) -> String {
        let mut words: Vec<&str> = Vec::new();
        match self.kind {
            PartKind::App => words.push("the app"),
            PartKind::HelperApp | PartKind::Process | PartKind::Job => {
                match (&self.job, self.gui.as_deref()) {
                    (Some(j), _) if j.domain == JobDomain::System => words.push("system service"),
                    (_, Some("UIElement")) => words.push("menu bar helper"),
                    (_, Some("Foreground")) => words.push("helper app"),
                    (Some(_), _) => words.push("background agent"),
                    _ => words.push("background helper"),
                }
            }
        }
        if self.login_item {
            words.push("opens at login");
        }
        if !self.running() {
            words.push("loaded, not running");
        }
        words.join(", ")
    }
}

/// Everything of one app that runs now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunningState {
    /// Finder's name of the app ("PhotoStudio_5").
    pub app_name: String,
    /// The bundle (env path).
    pub app_path: PathBuf,
    pub bundle_id: String,
    /// Your user id, when `id -u` answered.
    pub me: Option<u32>,
    /// The app first, then helpers by name.
    pub parts: Vec<RunningPart>,
    /// What could not be checked (`launchctl` or `ps` failed).
    pub notes: Vec<String>,
}

impl RunningState {
    /// Something inside the bundle runs: the uninstall stays locked.
    pub fn locked(&self) -> bool {
        self.parts.iter().any(RunningPart::locks)
    }

    /// Running parts (with a process).
    pub fn running(&self) -> impl Iterator<Item = &RunningPart> {
        self.parts.iter().filter(|p| p.running())
    }

    /// Nothing of the app runs.
    pub fn is_empty(&self) -> bool {
        self.running().next().is_none()
    }

    /// Running parts that need the administrator password to stop.
    pub fn admin_parts(&self) -> Vec<&RunningPart> {
        self.running().filter(|p| p.needs_admin(self.me)).collect()
    }

    /// Stopping everything asks for the administrator password: ask the user first.
    pub fn needs_admin(&self) -> bool {
        !self.admin_parts().is_empty()
    }

    /// "PhotoStudio_5 and 2 background helpers are running", "PhotoStudio_5 is running",
    /// "A background helper of PhotoStudio_5 is running", "" when nothing runs.
    pub fn headline(&self) -> String {
        let app = self.running().any(|p| p.kind == PartKind::App);
        let helpers = self.running().filter(|p| p.kind != PartKind::App).count();
        let name = &self.app_name;
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        match (app, helpers) {
            (false, 0) => String::new(),
            (true, 0) => format!("{name} is running"),
            (true, n) => format!("{name} and {n} background helper{} are running", plural(n)),
            (false, 1) => format!("A background helper of {name} is running"),
            (false, n) => format!("{n} background helpers of {name} are running"),
        }
    }

    /// The words for the administrator confirmation, when one is needed.
    pub fn admin_question(&self) -> Option<String> {
        let parts = self.admin_parts();
        if parts.is_empty() {
            return None;
        }
        let names = parts
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "{names} {} as the system. Stopping {} asks for your administrator password.",
            if parts.len() == 1 { "runs" } else { "run" },
            if parts.len() == 1 { "it" } else { "them" },
        ))
    }
}

/// The CFBundleIdentifier of a bundle, from its Info.plist.
fn bundle_id_at(bundle: &Path) -> Option<String> {
    let v = plist::Value::from_file(bundle.join("Contents/Info.plist")).ok()?;
    v.as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(str::to_string)
}

/// A macOS path as an env path (the fake root in tests).
fn env_path(env: &Env, mac: &str) -> PathBuf {
    if env.is_fake() {
        env.sys_path(mac)
    } else {
        PathBuf::from(mac)
    }
}

/// `ps -axo pid=,uid=,comm=`: pid -> (uid, program).
pub fn parse_ps_owners(stdout: &str) -> BTreeMap<u32, (u32, String)> {
    let mut m = BTreeMap::new();
    for l in stdout.lines() {
        let mut it = l.trim_start().splitn(2, char::is_whitespace);
        let (Some(pid), Some(rest)) = (it.next(), it.next()) else {
            continue;
        };
        let mut it = rest.trim_start().splitn(2, char::is_whitespace);
        let (Some(uid), Some(comm)) = (it.next(), it.next()) else {
            continue;
        };
        let (Ok(pid), Ok(uid)) = (pid.parse::<u32>(), uid.parse::<u32>()) else {
            continue;
        };
        let comm = comm.trim();
        if comm.starts_with('/') {
            m.insert(pid, (uid, comm.to_string()));
        }
    }
    m
}

/// One app as `lsappinfo list` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LsApp {
    pub name: String,
    pub bundle_id: Option<String>,
    pub kind: Option<String>,
}

/// `lsappinfo list`: pid -> app. Blocks start with ` 12) "Name" ASN:...:` and carry
/// `bundleID="..."` and `pid = 612 type="UIElement" ...`.
pub fn parse_lsappinfo(stdout: &str) -> BTreeMap<u32, LsApp> {
    let mut m = BTreeMap::new();
    let mut cur: Option<LsApp> = None;
    let quoted = |s: &str, key: &str| -> Option<String> {
        let i = s.find(key)?;
        let rest = &s[i + key.len()..];
        let rest = rest.strip_prefix('"')?;
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    };
    for l in stdout.lines() {
        let t = l.trim();
        if let Some((num, rest)) = t.split_once(") \"")
            && !num.is_empty()
            && num.chars().all(|c| c.is_ascii_digit())
        {
            let name = rest.split('"').next().unwrap_or_default().to_string();
            cur = Some(LsApp {
                name,
                ..Default::default()
            });
            continue;
        }
        let Some(app) = cur.as_mut() else {
            continue;
        };
        if let Some(id) = quoted(t, "bundleID=") {
            if !id.is_empty() && id != "[ NULL ]" {
                app.bundle_id = Some(id);
            }
        } else if let Some(rest) = t.strip_prefix("pid = ") {
            let pid = rest.split_whitespace().next().and_then(|p| p.parse().ok());
            app.kind = quoted(rest, "type=");
            if let Some(pid) = pid {
                m.insert(pid, app.clone());
            }
        }
    }
    m
}

/// The `services = { ... }` block of `launchctl print <domain>`: label -> pid (None when the
/// job is loaded but not running). Lines are "  612   0   com.example.label" (pid, last exit
/// status, label); pid "-" or 0 means not running.
pub fn parse_print_services(stdout: &str) -> BTreeMap<String, Option<u32>> {
    let mut m = BTreeMap::new();
    let mut depth_in: Option<usize> = None;
    let mut depth = 0usize;
    for l in stdout.lines() {
        let t = l.trim();
        if depth_in.is_none() && t == "services = {" {
            depth += 1;
            depth_in = Some(depth);
            continue;
        }
        if t.ends_with('{') {
            depth += 1;
            continue;
        }
        if t == "}" {
            if depth_in == Some(depth) {
                break;
            }
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth_in != Some(depth) {
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        if words.len() < 3 {
            continue;
        }
        let label = words[words.len() - 1].to_string();
        let pid = words[0].parse::<u32>().ok().filter(|p| *p > 0);
        m.insert(label, pid);
    }
    m
}

/// `launchctl print <domain>/<label>`: the job's property list (`path = ...`).
pub fn parse_print_job_path(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|l| {
        let t = l.trim();
        t.strip_prefix("path = ")
            .map(|p| p.trim().to_string())
            .filter(|p| p.starts_with('/'))
    })
}

/// Labels of launchd property lists that belong to the app: in the launch agent and daemon
/// folders and inside the bundle (`Contents/Library/LaunchAgents`, `LaunchDaemons`, used by
/// `SMAppService`). A list belongs when its program is inside the bundle, or its label is the
/// app's id or starts with it. label -> property list (macOS path).
fn app_plists(env: &Env, app_path: &Path, app_shown: &str, id: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut dirs: Vec<(PathBuf, bool)> = startup::Domain::ALL
        .iter()
        .map(|d| (d.path(env), false))
        .collect();
    dirs.push((app_path.join("Contents/Library/LaunchAgents"), true));
    dirs.push((app_path.join("Contents/Library/LaunchDaemons"), true));
    for (dir, in_bundle) in dirs {
        for p in startup::plists_in(&dir) {
            let Ok(lp) = startup::read_launch_plist(&p) else {
                continue;
            };
            let label = lp.label.clone().unwrap_or_else(|| {
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let points_in = lp
                .program
                .as_deref()
                .is_some_and(|pr| Path::new(pr).starts_with(app_shown));
            if in_bundle || points_in || (!id.is_empty() && matches_id(&label, id, &[])) {
                out.insert(label, env.display(&p));
            }
        }
    }
    out
}

/// Bundle ids of the apps inside the app (login items and helper apps).
fn helper_ids(app_path: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for sub in [
        "Contents/Library/LoginItems",
        "Contents/Helpers",
        "Contents/MacOS",
    ] {
        for p in crate::fsutil::children(&app_path.join(sub)) {
            if p.extension().is_some_and(|e| e == "app")
                && let Some(id) = bundle_id_at(&p)
            {
                ids.insert(id);
            }
        }
    }
    ids
}

/// Does a launchd label belong to the app? LaunchServices' own labels for apps opened from
/// Finder ("application.<id>.123.456") are not jobs to boot out: those apps quit politely.
fn label_is_app_job(label: &str, id: &str, helpers: &BTreeSet<String>) -> bool {
    !label.starts_with("application.")
        && ((!id.is_empty() && matches_id(label, id, &[]))
            || helpers.iter().any(|h| h.eq_ignore_ascii_case(label)))
}

/// Everything of the app at `app_path` (env path) that runs now. Never fails: what could not
/// be checked is in `notes`.
pub fn running_parts(env: &Env, app_path: &Path) -> RunningState {
    let app_name = app_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let app_shown = env.display(app_path);
    let id = bundle_id_at(app_path).unwrap_or_default();
    let mut notes = Vec::new();
    let me = tasks::current_uid(env).ok();

    let owners = match env.sys.run(PS, PS_ARGS) {
        Ok(out) if out.ok() => parse_ps_owners(&out.stdout),
        _ => BTreeMap::new(),
    };
    let lsapps = match env.sys.run(LSAPPINFO, &["list"]) {
        Ok(out) if out.ok() => parse_lsappinfo(&out.stdout),
        _ => BTreeMap::new(),
    };
    let mut services: Vec<(JobDomain, BTreeMap<String, Option<u32>>)> = Vec::new();
    let mut domains = Vec::new();
    match me {
        Some(uid) => domains.push((JobDomain::Gui, format!("gui/{uid}"))),
        None => notes.push(
            "Could not read your user id, so background agents of your session were not checked."
                .to_string(),
        ),
    }
    domains.push((JobDomain::System, "system".to_string()));
    for (d, name) in domains {
        match env.sys.run(LAUNCHCTL, &["print", &name]) {
            Ok(out) if out.ok() => services.push((d, parse_print_services(&out.stdout))),
            _ => notes.push(format!(
                "Could not ask launchd about {}.",
                if d == JobDomain::Gui {
                    "the agents of your session"
                } else {
                    "system services"
                }
            )),
        }
    }
    let helpers = helper_ids(app_path);
    let plists = app_plists(env, app_path, &app_shown, &id);
    let job_of_pid = |pid: u32| -> Option<LaunchJob> {
        services.iter().find_map(|(d, m)| {
            m.iter()
                .find(|(l, p)| **p == Some(pid) && !l.starts_with("application."))
                .map(|(l, _)| LaunchJob {
                    label: l.clone(),
                    domain: *d,
                    plist: plists.get(l).cloned(),
                })
        })
    };

    let procs = env.sys.running_apps();
    let alive: BTreeSet<u32> = procs.iter().map(|r| r.pid).collect();
    let mut parts: Vec<RunningPart> = Vec::new();
    let mut pids = BTreeSet::new();
    for r in procs {
        if r.pid == std::process::id() || !pids.insert(r.pid) {
            continue;
        }
        let owner = owners.get(&r.pid);
        let program = owner
            .map(|(_, c)| c.clone())
            .or_else(|| r.path.as_ref().map(|p| p.to_string_lossy().into_owned()));
        let Some(program) = program else {
            continue;
        };
        // The same mapping as the lock ([`crate::features::uninstaller::is_running`]).
        let lock_path = r
            .path
            .as_ref()
            .map(|p| env_path(env, &p.to_string_lossy()))
            .unwrap_or_else(|| env_path(env, &program));
        let inside =
            lock_path.starts_with(app_path) || env_path(env, &program).starts_with(app_path);
        let named_after = !id.is_empty()
            && !apps::macos_app(app_path)
            && Path::new(&program).components().any(|c| {
                let c = c.as_os_str().to_string_lossy();
                matches_id(&c, &id, &[]) || helpers.iter().any(|h| matches_id(&c, h, &[]))
            });
        if !inside && !named_after {
            continue;
        }
        let inner_app = app_bundle_of(Path::new(&program))
            .or_else(|| r.path.as_ref().and_then(|p| app_bundle_of(p)));
        let inner_env = inner_app
            .as_ref()
            .map(|a| env_path(env, &a.to_string_lossy()));
        // The app itself is its main program in Contents/MacOS; anything else is a helper.
        let main_program = env_path(env, &program)
            .parent()
            .is_some_and(|d| d == app_path.join("Contents/MacOS"));
        let kind = match &inner_env {
            Some(a) if a == app_path && main_program => PartKind::App,
            Some(a) if a == app_path => PartKind::Process,
            Some(a) if a.starts_with(app_path) => PartKind::HelperApp,
            _ => PartKind::Process,
        };
        let ls = lsapps.get(&r.pid);
        let bundle_id = ls
            .and_then(|l| l.bundle_id.clone())
            .or_else(|| r.bundle_id.clone())
            .or_else(|| inner_env.as_ref().and_then(|a| bundle_id_at(a)));
        let name = ls
            .map(|l| l.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                inner_app
                    .as_ref()
                    .filter(|_| kind != PartKind::Process)
                    .and_then(|a| a.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .or_else(|| {
                Path::new(&program)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| r.name.clone());
        parts.push(RunningPart {
            name,
            kind,
            pid: Some(r.pid),
            uid: owner.map(|(u, _)| *u),
            login_item: program.contains("/Contents/Library/LoginItems/"),
            path: Some(program),
            bundle_id,
            gui: ls.and_then(|l| l.kind.clone()),
            job: job_of_pid(r.pid),
            inside_bundle: inside,
        });
    }
    // Loaded jobs of the app with no process of the app (idle agents that may start again).
    for (d, m) in &services {
        for (label, pid) in m {
            let ours = label_is_app_job(label, &id, &helpers) || plists.contains_key(label);
            if !ours
                || parts
                    .iter()
                    .any(|p| p.job.as_ref().is_some_and(|j| &j.label == label))
            {
                continue;
            }
            // Its process runs from somewhere that is not named after the app: still the app's.
            let pid = pid.filter(|p| alive.contains(p));
            let owner = pid.and_then(|p| owners.get(&p));
            parts.push(RunningPart {
                name: label.clone(),
                kind: if pid.is_some() {
                    PartKind::Process
                } else {
                    PartKind::Job
                },
                pid,
                uid: owner.map(|(u, _)| *u),
                path: owner.map(|(_, c)| c.clone()),
                bundle_id: None,
                gui: None,
                job: Some(LaunchJob {
                    label: label.clone(),
                    domain: *d,
                    plist: plists.get(label).cloned(),
                }),
                login_item: false,
                inside_bundle: false,
            });
        }
    }
    parts.sort_by(|a, b| {
        (a.kind != PartKind::App, !a.running(), a.name.to_lowercase()).cmp(&(
            b.kind != PartKind::App,
            !b.running(),
            b.name.to_lowercase(),
        ))
    });
    RunningState {
        app_name,
        app_path: app_path.to_path_buf(),
        bundle_id: id,
        me,
        parts,
        notes,
    }
}

// ---------------------------------------------------------------------------------------------
// Quitting

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitOptions {
    /// The user confirmed the administrator password dialog for system services.
    pub allow_admin: bool,
    /// How long to wait for everything to stop, checking again every `poll`.
    pub wait: Duration,
    pub poll: Duration,
}

impl Default for QuitOptions {
    fn default() -> Self {
        QuitOptions {
            allow_admin: false,
            wait: Duration::from_secs(5),
            poll: Duration::from_millis(250),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceOptions {
    /// The user confirmed the administrator password dialog for processes of the system.
    pub allow_admin: bool,
    /// Time between `kill -TERM` and `kill -KILL`.
    pub grace: Duration,
    /// How long to wait after `kill -KILL`.
    pub wait: Duration,
    pub poll: Duration,
}

impl Default for ForceOptions {
    fn default() -> Self {
        ForceOptions {
            allow_admin: false,
            grace: Duration::from_secs(3),
            wait: Duration::from_secs(2),
            poll: Duration::from_millis(250),
        }
    }
}

/// What a quit or force quit did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuitReport {
    /// One line per stop asked for that worked ("Quit PhotoStudio_5", "Stopped agent ...").
    pub stopped: Vec<String>,
    /// One line per stop that failed, with the reason.
    pub failed: Vec<String>,
    /// Names of parts that started again by themselves right after they stopped.
    pub relaunched: Vec<String>,
    /// Running parts left alone because they need the administrator password and the caller
    /// did not confirm it.
    pub admin_needed: Vec<String>,
    /// The administrator dialog was cancelled.
    pub admin_cancelled: bool,
    /// What runs after the wait.
    pub after: RunningState,
}

impl QuitReport {
    /// Nothing inside the app runs any more: the uninstall plan unlocks on the next check.
    pub fn done(&self) -> bool {
        !self.after.locked()
    }

    /// Something inside the bundle still runs that Force Quit may stop.
    pub fn can_force(&self) -> bool {
        self.after.locked()
    }

    /// What to tell the user, in one or two sentences.
    pub fn message(&self) -> String {
        let name = &self.after.app_name;
        let mut out = Vec::new();
        if self.done() {
            out.push(format!("{name} has quit. You can uninstall it now."));
        } else {
            let still: Vec<&str> = self
                .after
                .parts
                .iter()
                .filter(|p| p.locks())
                .map(|p| p.name.as_str())
                .collect();
            out.push(format!(
                "Still running: {}. Use Force Quit to stop {}.",
                still.join(", "),
                if still.len() == 1 { "it" } else { "them" }
            ));
        }
        if !self.relaunched.is_empty() {
            out.push(format!(
                "{} started again by itself right after it stopped.",
                self.relaunched.join(", ")
            ));
        }
        if self.admin_cancelled {
            out.push(
                "The password dialog was cancelled, so the system services kept running.".into(),
            );
        } else if !self.admin_needed.is_empty() {
            out.push(format!(
                "{} {} as the system and needs your administrator password to stop.",
                self.admin_needed.join(", "),
                if self.admin_needed.len() == 1 {
                    "runs"
                } else {
                    "run"
                }
            ));
        }
        let outside: Vec<&str> = self
            .after
            .running()
            .filter(|p| !p.inside_bundle)
            .map(|p| p.name.as_str())
            .collect();
        if self.done()
            && !outside.is_empty()
            && self.admin_needed.is_empty()
            && !self.admin_cancelled
        {
            out.push(format!(
                "Still running outside the app: {}. Its files stay where they are until it stops.",
                outside.join(", ")
            ));
        }
        if !self.failed.is_empty() {
            out.push(self.failed.join(" "));
        }
        out.join(" ")
    }
}

fn record(kind: ActionKind, tag: &str) -> ActionRecord {
    let now = chrono::Utc::now();
    let plan_id = format!("quit-{tag}-{}", now.timestamp_nanos_opt().unwrap_or(0));
    ActionRecord {
        id: format!("{plan_id}#0"),
        plan_id,
        feature: Some(FeatureId::Uninstaller),
        time: now.timestamp(),
        size: 0,
        kind,
        undone: None,
    }
}

fn output_of(r: &std::io::Result<CmdOut>) -> (bool, String) {
    match r {
        Ok(out) => (
            out.ok(),
            tasks::first_lines(&format!("{}\n{}", out.stdout, out.stderr), 3),
        ),
        Err(e) => (false, e.to_string()),
    }
}

fn log_command(
    env: &Env,
    command: String,
    ok: bool,
    output: String,
    tag: &str,
) -> Result<(), String> {
    executor::append_journal(
        env,
        &record(
            ActionKind::Command {
                command,
                ok,
                output,
            },
            tag,
        ),
    )
    .map_err(|e| format!("The action log could not be written: {e}"))
}

/// Refuse apps CleanCraft never quits: part of macOS, or CleanCraft itself.
fn refuse(env: &Env, app_path: &Path) -> Result<(), String> {
    if apps::macos_app(app_path) || apps::in_system(env, app_path) {
        return Err("This app is part of macOS; CleanCraft does not quit it.".into());
    }
    let name = app_path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name == "cleancraft.app" {
        return Err("CleanCraft does not quit itself.".into());
    }
    tasks::journal_writable(env)
        .map_err(|e| format!("Could not write the action log, nothing was stopped: {e}"))
}

/// The command that brings a stopped agent back when the uninstall is cancelled: load its
/// property list again, else open the (helper) app, which registers its agents again.
fn reload_command(env: &Env, st: &RunningState, part: &RunningPart, job: &LaunchJob) -> String {
    let uid = st.me.unwrap_or_default();
    if let Some(pl) = &job.plist
        && !Path::new(pl).starts_with(env.display(&st.app_path))
    {
        let domain = match job.domain {
            JobDomain::Gui => format!("gui/{uid}"),
            JobDomain::System => "system".into(),
        };
        let cmd = format!("{LAUNCHCTL} bootstrap {domain} {}", sh_quote(pl));
        return match job.domain {
            JobDomain::Gui => cmd,
            JobDomain::System => startup::admin_wrap(&cmd),
        };
    }
    let target = part
        .path
        .as_deref()
        .and_then(|p| app_bundle_of(Path::new(p)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| env.display(&st.app_path));
    format!("/usr/bin/open {}", sh_quote(&target))
}

/// Find the property list of a loaded job through `launchctl print <domain>/<label>`.
fn job_plist(env: &Env, me: Option<u32>, job: &LaunchJob) -> Option<String> {
    if job.plist.is_some() {
        return job.plist.clone();
    }
    let target = match job.domain {
        JobDomain::Gui => format!("gui/{}/{}", me?, job.label),
        JobDomain::System => format!("system/{}", job.label),
    };
    let out = env.sys.run(LAUNCHCTL, &["print", &target]).ok()?;
    out.ok()
        .then(|| parse_print_job_path(&out.stdout))
        .flatten()
}

/// `launchctl bootout gui/<uid>/<label>` for each loaded agent of your session. Logged with
/// the command that loads it again.
fn bootout_user_jobs(
    env: &Env,
    st: &RunningState,
    rep: &mut QuitReport,
    acted: &mut BTreeSet<u32>,
) -> Result<(), String> {
    let Some(uid) = st.me else {
        return Ok(());
    };
    for p in &st.parts {
        let Some(job) = p.job.as_ref().filter(|j| j.domain == JobDomain::Gui) else {
            continue;
        };
        let job = LaunchJob {
            plist: job_plist(env, st.me, job),
            ..job.clone()
        };
        let target = format!("gui/{uid}/{}", job.label);
        let res = env.sys.run(LAUNCHCTL, &["bootout", &target]);
        let (ok, output) = output_of(&res);
        let undo = reload_command(env, st, p, &job);
        let what = format!(
            "Stopped {} of {} ({LAUNCHCTL} bootout {}) before uninstalling",
            if p.login_item {
                "login item"
            } else {
                "launch agent"
            },
            st.app_name,
            sh_quote(&target)
        );
        if ok {
            executor::append_journal(
                env,
                &record(
                    ActionKind::Toggled {
                        what: format!("{what}. Load it again: {undo}"),
                        undo_command: Some(undo),
                    },
                    "bootout",
                ),
            )
            .map_err(|e| format!("The action log could not be written: {e}"))?;
            rep.stopped
                .push(format!("Stopped {} ({})", p.name, job.label));
            acted.extend(p.pid);
        } else {
            log_command(
                env,
                format!("{LAUNCHCTL} bootout {}", sh_quote(&target)),
                false,
                output.clone(),
                "bootout",
            )?;
            if p.running() {
                rep.failed
                    .push(format!("{} did not stop: {output}.", p.name));
            }
        }
    }
    Ok(())
}

/// The `osascript` arguments that ask the app with `id` to quit, only if it runs (so the
/// question never opens it), waiting [`QUIT_ANSWER_SECS`] for its answer.
pub fn quit_args(id: &str) -> Vec<String> {
    let id = as_quote(id);
    [
        format!("with timeout of {QUIT_ANSWER_SECS} seconds"),
        format!("if application id {id} is running then tell application id {id} to quit"),
        "end timeout".to_string(),
    ]
    .into_iter()
    .flat_map(|l| ["-e".to_string(), l])
    .collect()
}

/// Ask the app and its helper apps to quit, helpers first.
fn quit_gui(
    env: &Env,
    st: &RunningState,
    rep: &mut QuitReport,
    acted: &mut BTreeSet<u32>,
) -> Result<(), String> {
    let mut asked = BTreeSet::new();
    let mut order: Vec<&RunningPart> = st
        .running()
        .filter(|p| p.inside_bundle && p.bundle_id.is_some() && !p.needs_admin(st.me))
        .filter(|p| p.pid.is_none_or(|x| !acted.contains(&x)))
        .collect();
    order.sort_by_key(|p| p.kind == PartKind::App);
    for p in order {
        let Some(id) = p.bundle_id.as_deref() else {
            continue;
        };
        if !asked.insert(id.to_lowercase()) {
            continue;
        }
        let args = quit_args(id);
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let res = env.sys.run_timeout(
            OSASCRIPT,
            &a,
            Duration::from_secs(u64::from(QUIT_ANSWER_SECS) + 10),
        );
        let (ok, output) = output_of(&res);
        let shown = std::iter::once(OSASCRIPT.to_string())
            .chain(args.iter().map(|x| tasks::sh_quote_if_needed(x)))
            .collect::<Vec<_>>()
            .join(" ");
        log_command(env, shown, ok, output.clone(), "quit")?;
        if ok {
            rep.stopped.push(format!("Asked {} to quit", p.name));
            acted.extend(p.pid);
        } else {
            let why = if output.contains("-128") {
                "it said no (it may be asking you something)".to_string()
            } else if output.contains("-1712") {
                "it did not answer in time (it may be asking you something)".to_string()
            } else {
                output
            };
            rep.failed.push(format!("{} did not quit: {why}.", p.name));
        }
    }
    Ok(())
}

/// One administrator command for system services and processes of the system inside the app.
fn admin_stop(
    env: &Env,
    st: &RunningState,
    kill_signal: Option<&str>,
    rep: &mut QuitReport,
    acted: &mut BTreeSet<u32>,
) -> Result<(), String> {
    let mut pids = Vec::new();
    let mut cmds = Vec::new();
    let mut undo_lines = Vec::new();
    let mut names = Vec::new();
    for p in &st.parts {
        let admin_job = p.job.as_ref().filter(|j| j.domain == JobDomain::System);
        if let Some(job) = admin_job {
            let job = LaunchJob {
                plist: job_plist(env, st.me, job),
                ..job.clone()
            };
            cmds.push(format!(
                "{LAUNCHCTL} bootout {}",
                sh_quote(&format!("system/{}", job.label))
            ));
            undo_lines.push((p.clone(), job));
            names.push(p.name.clone());
            pids.extend(p.pid);
        } else if p.running() && p.needs_admin(st.me) && p.inside_bundle {
            if let (Some(pid), Some(sig)) = (p.pid, kill_signal) {
                cmds.push(format!("{KILL} -{sig} {pid}"));
                names.push(p.name.clone());
                pids.push(pid);
            } else if kill_signal.is_none() && p.job.is_none() {
                // A process of the system with no job: only Force Quit stops it.
                rep.failed.push(format!(
                    "{} runs as the system without a launchd job; use Force Quit.",
                    p.name
                ));
            }
        }
    }
    if cmds.is_empty() {
        return Ok(());
    }
    let script = cmds.join("; ");
    let res = env.sys.run_admin(&script);
    if let Ok(out) = &res
        && tasks::is_admin_cancel(out)
    {
        rep.admin_cancelled = true;
        return Ok(());
    }
    let (ok, output) = output_of(&res);
    log_command(
        env,
        format!("(administrator) {script}"),
        ok,
        output,
        "admin",
    )?;
    acted.extend(pids);
    for (p, job) in undo_lines {
        let undo = job
            .plist
            .as_ref()
            .filter(|pl| !Path::new(pl.as_str()).starts_with(env.display(&st.app_path)))
            .map(|_| reload_command(env, st, &p, &job));
        let what = format!(
            "Stopped system service {} of {} before uninstalling. {}",
            job.label,
            st.app_name,
            match &undo {
                Some(u) => format!("Load it again: {u}"),
                None => "It starts again at the next restart while the app is installed.".into(),
            }
        );
        executor::append_journal(
            env,
            &record(
                ActionKind::Toggled {
                    what,
                    undo_command: undo,
                },
                "daemon",
            ),
        )
        .map_err(|e| format!("The action log could not be written: {e}"))?;
    }
    rep.stopped.push(format!(
        "Stopped with the administrator password: {}",
        names.join(", ")
    ));
    Ok(())
}

/// Wait until none of `pids` runs (or nothing locks), at most `wait`, checking every `poll`.
fn wait_for(
    env: &Env,
    app_path: &Path,
    pids: &BTreeSet<u32>,
    wait: Duration,
    poll: Duration,
) -> RunningState {
    let deadline = Instant::now() + wait;
    loop {
        let st = running_parts(env, app_path);
        let left = st
            .running()
            .any(|p| p.pid.is_some_and(|x| pids.contains(&x)));
        if !left || Instant::now() >= deadline {
            return st;
        }
        std::thread::sleep(poll.max(Duration::from_millis(10)));
    }
}

/// Parts that came back with a new pid after being stopped (same program).
fn relaunched(before: &RunningState, stopped: &BTreeSet<u32>, after: &RunningState) -> Vec<String> {
    let mut out = Vec::new();
    for p in before
        .running()
        .filter(|p| p.pid.is_some_and(|x| stopped.contains(&x)))
    {
        if after
            .running()
            .any(|q| q.path == p.path && q.pid != p.pid && q.path.is_some())
            && !out.contains(&p.name)
        {
            out.push(p.name.clone());
        }
    }
    out
}

fn empty_report(after: RunningState) -> QuitReport {
    QuitReport {
        stopped: Vec::new(),
        failed: Vec::new(),
        relaunched: Vec::new(),
        admin_needed: Vec::new(),
        admin_cancelled: false,
        after,
    }
}

/// Quit the app at `app_path` (env path) and everything it runs in the background, politely.
/// `Err` when CleanCraft refuses (part of macOS, CleanCraft itself, action log not writable).
pub fn quit_app(env: &Env, app_path: &Path, opts: QuitOptions) -> Result<QuitReport, String> {
    refuse(env, app_path)?;
    let before = running_parts(env, app_path);
    let mut rep = empty_report(before.clone());
    let mut acted = BTreeSet::new();
    bootout_user_jobs(env, &before, &mut rep, &mut acted)?;
    // What an agent started may be gone now: ask again what runs.
    let mid = running_parts(env, app_path);
    quit_gui(env, &mid, &mut rep, &mut acted)?;
    let system_jobs = before.parts.iter().any(|p| {
        p.job
            .as_ref()
            .is_some_and(|j| j.domain == JobDomain::System)
    });
    if before.needs_admin() || system_jobs {
        if opts.allow_admin {
            admin_stop(env, &before, None, &mut rep, &mut acted)?;
        } else {
            rep.admin_needed = before
                .admin_parts()
                .iter()
                .map(|p| p.name.clone())
                .collect();
        }
    }
    let after = wait_for(env, app_path, &acted, opts.wait, opts.poll);
    rep.relaunched = relaunched(&before, &acted, &after);
    rep.after = after;
    Ok(rep)
}

/// Stop what still runs inside the app's bundle: launch agents booted out again, then
/// `kill -TERM`, and `kill -KILL` after `grace` for what is left. Only processes inside the
/// bundle; processes of the system only when `allow_admin` (one password dialog, TERM only).
/// Always an explicit choice of the user.
pub fn force_quit(env: &Env, app_path: &Path, opts: ForceOptions) -> Result<QuitReport, String> {
    refuse(env, app_path)?;
    let before = running_parts(env, app_path);
    let mut rep = empty_report(before.clone());
    let mut acted = BTreeSet::new();
    bootout_user_jobs(env, &before, &mut rep, &mut acted)?;
    let targets: Vec<RunningPart> = before.parts.iter().filter(|p| p.locks()).cloned().collect();
    let (admin, mine): (Vec<_>, Vec<_>) = targets.iter().partition(|p| p.needs_admin(before.me));
    let send = |rep: &mut QuitReport, p: &RunningPart, sig: &str| -> Result<bool, String> {
        // The pid must still be the same program inside the app.
        let now = running_parts(env, app_path);
        let Some(pid) = p.pid else { return Ok(false) };
        if !now
            .parts
            .iter()
            .any(|q| q.pid == Some(pid) && q.path == p.path && q.inside_bundle)
        {
            return Ok(false);
        }
        let pid_s = pid.to_string();
        let flag = format!("-{sig}");
        let res = env.sys.run(KILL, &[&flag, &pid_s]);
        let (ok, output) = output_of(&res);
        log_command(
            env,
            format!("{KILL} {flag} {pid} ({})", p.name),
            ok,
            output.clone(),
            "force",
        )?;
        if ok {
            rep.stopped.push(format!("Sent {sig} to {}", p.name));
        } else {
            rep.failed
                .push(format!("{} could not be stopped: {output}.", p.name));
        }
        Ok(ok)
    };
    for p in &mine {
        send(&mut rep, p, "TERM")?;
    }
    if !admin.is_empty() {
        if opts.allow_admin {
            admin_stop(env, &before, Some("TERM"), &mut rep, &mut acted)?;
        } else {
            rep.admin_needed = admin.iter().map(|p| p.name.clone()).collect();
        }
    }
    let pids: BTreeSet<u32> = targets.iter().filter_map(|p| p.pid).collect();
    let mid = wait_for(env, app_path, &pids, opts.grace, opts.poll);
    for p in &mine {
        if mid.running().any(|q| q.pid == p.pid) {
            send(&mut rep, p, "KILL")?;
        }
    }
    let after = wait_for(env, app_path, &pids, opts.wait, opts.poll);
    rep.relaunched = relaunched(&before, &pids, &after);
    rep.after = after;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_owner_lines() {
        let m = parse_ps_owners(
            "  1     0 /sbin/launchd\n 612   501 /Applications/A B.app/Contents/MacOS/A B\n 7 x y\n",
        );
        assert_eq!(m[&1], (0, "/sbin/launchd".into()));
        assert_eq!(
            m[&612],
            (501, "/Applications/A B.app/Contents/MacOS/A B".into())
        );
        assert!(!m.contains_key(&7));
    }

    #[test]
    fn lsappinfo_blocks() {
        let out = r#" 1) "Finder" ASN:0x0-0x1001:
    bundleID="com.apple.finder"
    bundle path="/System/Library/CoreServices/Finder.app"
    pid = 512 type="Foreground" flavor=3 Version="14.0" fileType="FNDR" creator="MACS" Arch=ARM64
 2) "the reference app Menu" ASN:0x0-0x2002:
    bundleID="com.example.photostudio5.Menu"
    pid = 777 type="UIElement" flavor=3
"#;
        let m = parse_lsappinfo(out);
        assert_eq!(m[&512].bundle_id.as_deref(), Some("com.apple.finder"));
        assert_eq!(m[&777].name, "the reference app Menu");
        assert_eq!(m[&777].kind.as_deref(), Some("UIElement"));
    }

    #[test]
    fn services_block() {
        let out = "gui/501 = {\n\ttype = login\n\tendpoints = {\n\t\t\"x\" = {\n\t\t\tport = 0x1\n\t\t}\n\t}\n\tservices = {\n\t\t     612      -  com.example.photostudio5.Agent\n\t\t       0      -  com.example.photostudio5.Updater\n\t\t       -     78  com.example.other\n\t}\n\tother = {\n\t\t 99 - not.a.service\n\t}\n}\n";
        let m = parse_print_services(out);
        assert_eq!(m.get("com.example.photostudio5.Agent"), Some(&Some(612)));
        assert_eq!(m.get("com.example.photostudio5.Updater"), Some(&None));
        assert_eq!(m.get("com.example.other"), Some(&None));
        assert!(!m.contains_key("not.a.service"));
        assert!(!m.contains_key("port"));
    }

    #[test]
    fn job_path() {
        let out = "gui/501/com.x = {\n\tactive count = 1\n\tpath = /Users/a/Library/LaunchAgents/com.x.plist\n\tstate = running\n}\n";
        assert_eq!(
            parse_print_job_path(out).as_deref(),
            Some("/Users/a/Library/LaunchAgents/com.x.plist")
        );
    }

    #[test]
    fn quit_args_never_launch_the_app() {
        let a = quit_args("com.x.App");
        assert_eq!(
            a[3],
            "if application id \"com.x.App\" is running then tell application id \"com.x.App\" to quit"
        );
        assert_eq!(a.len(), 6);
    }
}
