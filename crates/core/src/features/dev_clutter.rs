//! Developer Clutter: build folders, installed packages and environments that developer tools
//! make again by themselves, found next to the projects in the user's code folders, plus the
//! simulators Xcode can no longer start, and a report of what Docker and Homebrew could
//! clean themselves.
//!
//! Files: one item per project folder (`target/` next to a `Cargo.toml`, `node_modules` next
//! to a `package.json`, a Python virtual environment with its `pyvenv.cfg`, a Next.js cache, a
//! Gradle `build/`, a `dist` or `build` folder the project's `.gitignore` lists). An item is
//! selected only when no file of its project changed for [`Settings::dev_inactive_days`] days;
//! otherwise it is listed unselected as an active project. Links are never followed, nothing
//! inside another `node_modules` is offered, and a folder git tracks files in is locked. Code
//! folders inside protected places (Documents, Desktop) are listed locked with the reason.
//! Removing goes through the executor like every other feature (Trash or quarantine, undo).
//!
//! Simulators: the device folders (`~/Library/Developer/CoreSimulator/Devices/<UDID>`) of the
//! devices `xcrun simctl list devices unavailable -j` lists, never preselected, checked with
//! simctl again right before the move, moved by the executor like any item (Trash, undo).
//! CleanCraft never runs `simctl delete`.
//!
//! Tools: [`scan_tools`] asks `docker system df` and `brew cleanup -n` what they could free,
//! changing nothing. Their own cleanups delete for good, so CleanCraft never runs them: it
//! shows the numbers and the exact command for the user to run in Terminal.
//!
//! Not here on purpose: pip, npm, Cargo, Homebrew download caches and the Go build cache
//! (`~/Library/Caches/go-build`) are offered by System Junk already.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::fsutil::{children, human_size, tree_size};
use crate::plan::{FeatureId, Item, Plan, Risk};
use crate::policy;
use crate::settings::Settings;

pub struct DevClutter;

/// Code folders looked in by default, inside the home folder (when they exist).
pub const DEFAULT_ROOTS: &[&str] = &[
    "Developer",
    "Developers",
    "Projects",
    "Code",
    "src",
    "dev",
    "work",
    "GitHub",
];

/// How deep below a code folder projects are looked for.
pub const MAX_DEPTH: usize = 8;

/// The most files looked at to tell whether one project is in use. A project with more files
/// (besides its build folders) is called "not checked" and never selected for the user.
pub const ACTIVITY_FILE_CAP: usize = 50_000;

/// Names of Python virtual environment folders (each must hold a `pyvenv.cfg`).
pub const VENV_NAMES: &[&str] = &[".venv", "venv", "env"];

pub const CAT_RUST: &str = "rust_target";
pub const CAT_NODE: &str = "node_modules";
pub const CAT_PYTHON: &str = "python_venv";
pub const CAT_BUILD: &str = "build_cache";
pub const CAT_SIM: &str = "simulators";

/// (id, title, description).
pub const CATEGORIES: &[(&str, &str, &str)] = &[
    (
        CAT_RUST,
        "Rust build folders",
        "target folders Cargo fills when it builds. cargo build makes them again.",
    ),
    (
        CAT_NODE,
        "Node packages",
        "node_modules folders npm, yarn or pnpm installed. npm install brings them back.",
    ),
    (
        CAT_PYTHON,
        "Python environments",
        "Virtual environments of Python projects. Make them again with python -m venv and pip install.",
    ),
    (
        CAT_BUILD,
        "Other build output",
        "Next.js caches, Gradle build folders, and dist or build folders your .gitignore lists. Your build tool makes them again.",
    ),
    (
        CAT_SIM,
        "Unavailable simulators",
        "Simulator devices Xcode can no longer start because their system version is gone. Never selected for you.",
    ),
];

pub const LOCK_TRACKED: &str = "Git tracks files in this folder, so it is part of your project. To clean it here, stop tracking it in git, list it in .gitignore, then scan again";
pub const LOCK_UNCHECKED: &str = "CleanCraft could not ask git whether it tracks files here, and .gitignore does not list it, so it is kept. List it in the project's .gitignore (or install the command line tools with xcode-select --install), then scan again";
pub const LOCK_NOT_ALLOWED: &str = "Not inside one of your code folders. Add its folder under Developer Clutter in Settings, then scan again";
pub const LOCK_OWN_GIT: &str = "It holds its own git repository (.git), so it may be work of yours. Look inside it in Finder before you remove it yourself";
pub const LOCK_BOOTED: &str = "This simulator is running. Quit Simulator, then scan again";
/// The way forward for a project in Documents or Desktop.
pub const PROTECTED_STEPS: &str =
    "Move the project out of Documents and Desktop (for example to ~/Projects) to clean it here";

/// Words used for a project in use.
pub const ACTIVE: &str = "active project";

/// What kind of developer clutter a folder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    RustTarget,
    NodeModules,
    PythonVenv,
    NextCache,
    GradleBuild,
    /// `dist` or `build` listed by the project's `.gitignore`.
    IgnoredOutput,
}

impl Kind {
    pub fn category(self) -> &'static str {
        match self {
            Kind::RustTarget => CAT_RUST,
            Kind::NodeModules => CAT_NODE,
            Kind::PythonVenv => CAT_PYTHON,
            Kind::NextCache | Kind::GradleBuild | Kind::IgnoredOutput => CAT_BUILD,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Kind::RustTarget => "Rust build folder",
            Kind::NodeModules => "Node packages",
            Kind::PythonVenv => "Python environment",
            Kind::NextCache => "Next.js build cache",
            Kind::GradleBuild => "Gradle build folder",
            Kind::IgnoredOutput => "Build output",
        }
    }

    fn rebuild(self) -> &'static str {
        match self {
            Kind::RustTarget => "cargo build makes it again",
            Kind::NodeModules => "npm install brings them back",
            Kind::PythonVenv => "python -m venv and pip install make it again",
            Kind::NextCache => "next build makes it again",
            Kind::GradleBuild => "gradle build makes it again",
            Kind::IgnoredOutput => "your build makes it again",
        }
    }
}

/// Is `p` a regular file (links are not followed)?
fn is_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// Is `p` a real folder (a link to a folder is not)?
pub fn real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn has_gradle(dir: &Path) -> bool {
    [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|n| is_file(&dir.join(n)))
}

/// What `p` is and the project folder it belongs to, by what is on disk now. `None` when it is
/// not developer clutter CleanCraft knows, is not a real folder, or lies inside a
/// `node_modules` or `.git` folder (never offered on its own).
pub fn classify(p: &Path) -> Option<(Kind, PathBuf)> {
    if !real_dir(p) {
        return None;
    }
    let parent = p.parent()?;
    if parent.components().any(|c| {
        let n = c.as_os_str();
        policy::same_name(n, "node_modules".as_ref()) || policy::same_name(n, ".git".as_ref())
    }) {
        return None;
    }
    let name = name_of(p);
    let is = |n: &str| policy::same_name(name.as_ref(), n.as_ref());
    if is("target") && is_file(&parent.join("Cargo.toml")) {
        return Some((Kind::RustTarget, parent.to_path_buf()));
    }
    if is("node_modules") && is_file(&parent.join("package.json")) {
        return Some((Kind::NodeModules, parent.to_path_buf()));
    }
    if VENV_NAMES.iter().any(|v| is(v)) && is_file(&p.join("pyvenv.cfg")) {
        return Some((Kind::PythonVenv, parent.to_path_buf()));
    }
    if is("cache")
        && policy::same_name(parent.file_name()?, ".next".as_ref())
        && let Some(project) = parent.parent()
        && real_dir(parent)
        && is_file(&project.join("package.json"))
    {
        return Some((Kind::NextCache, project.to_path_buf()));
    }
    if is("build") && has_gradle(parent) {
        return Some((Kind::GradleBuild, parent.to_path_buf()));
    }
    if (is("dist") || is("build")) && gitignore_lists(&parent.join(".gitignore"), &name) {
        return Some((Kind::IgnoredOutput, parent.to_path_buf()));
    }
    None
}

/// A `*` and `?` match inside one name (enough for the patterns `.gitignore` files use for
/// build folders: `venv*`, `.venv*`).
fn glob(pat: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some('?') => !s.is_empty() && go(&p[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = s.chars().collect();
    go(&p, &s)
}

/// Does one `.gitignore` line ignore `rel` (a path relative to the `.gitignore`'s folder,
/// with `/`)? A folder is ignored when it or one of its parents is.
pub fn pattern_ignores(line: &str, rel: &str) -> bool {
    let p = line.trim();
    if p.is_empty() || p.starts_with('#') || p.starts_with('!') {
        return false;
    }
    let p = p.strip_prefix("**/").unwrap_or(p);
    let stripped = p.trim_end_matches("/**").trim_end_matches("/*");
    let stripped = stripped.trim_end_matches('/');
    let anchored = stripped.starts_with('/') || stripped.contains('/');
    let pat = stripped.trim_start_matches('/');
    if pat.is_empty() {
        return false;
    }
    let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    if anchored {
        let pat_parts: Vec<&str> = pat.split('/').collect();
        pat_parts.len() <= parts.len() && pat_parts.iter().zip(&parts).all(|(a, b)| glob(a, b))
    } else {
        parts.iter().any(|c| glob(pat, c))
    }
}

/// Does the `.gitignore` file at `file` ignore `rel`? A negation that matches `rel` makes the
/// answer no (CleanCraft stays careful rather than reproducing git's order rules).
pub fn gitignore_lists(file: &Path, rel: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    let mut hit = false;
    for line in text.lines() {
        if let Some(neg) = line.trim().strip_prefix('!') {
            if pattern_ignores(neg, rel) {
                return false;
            }
        } else if pattern_ignores(line, rel) {
            hit = true;
        }
    }
    hit
}

/// The code folders to look in: the [`DEFAULT_ROOTS`] that exist and the user's own
/// ([`Settings::dev_roots`]), without links and without one inside another; and notes about
/// folders that were left out.
pub fn code_roots(env: &Env, settings: &Settings) -> (Vec<PathBuf>, Vec<String>) {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut notes = Vec::new();
    let link_note = |p: &Path| {
        format!(
            "{} is a link to another place. CleanCraft does not follow links; add the real folder in Settings.",
            env.display(p)
        )
    };
    for d in DEFAULT_ROOTS {
        let p = env.home_path(d);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => roots.push(p),
            Ok(m) if m.file_type().is_symlink() => notes.push(link_note(&p)),
            _ => {}
        }
    }
    for p in &settings.dev_roots {
        if !p.is_absolute() || !env.contains(p) {
            notes.push(format!(
                "{} is not a folder CleanCraft can look in.",
                p.display()
            ));
            continue;
        }
        // The home folder (or a folder holding it) would mean Library, Documents and
        // everything else: only folders that hold projects.
        if policy::starts_with_ci(env.home(), p) {
            notes.push(format!(
                "{} holds your whole home folder. Add the folders that hold your projects instead.",
                env.display(p)
            ));
            continue;
        }
        match std::fs::symlink_metadata(p) {
            Ok(m) if m.is_dir() => roots.push(p.clone()),
            Ok(m) if m.file_type().is_symlink() => notes.push(link_note(p)),
            _ => notes.push(format!("{} was not found.", env.display(p))),
        }
    }
    // One folder inside another (or the same folder written in another letter case) is
    // looked in once.
    let mut out: Vec<PathBuf> = Vec::new();
    roots.sort_by_key(|p| p.components().count());
    for r in roots {
        if !out.iter().any(|o| policy::starts_with_ci(&r, o)) {
            out.push(r);
        }
    }
    (out, notes)
}

/// Whether a project is in use: the age in days of its newest file (build folders, `.git`
/// internals and installed packages left out; `.git/HEAD` and `.git/index` count, so a commit
/// or a checkout is a change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// A file changed this many days ago, fewer than the limit.
    Active(u64),
    /// Nothing changed for at least this many days (`None`: no file at all besides build
    /// folders).
    Inactive(Option<u64>),
    /// Too many files to look at all of them.
    Unknown,
}

impl Activity {
    pub fn text(self) -> String {
        match self {
            Activity::Active(0) => format!("{ACTIVE}, a file changed today"),
            Activity::Active(1) => format!("{ACTIVE}, a file changed yesterday"),
            Activity::Active(d) => format!("{ACTIVE}, a file changed {d} days ago"),
            Activity::Inactive(Some(d)) => format!("no file changed for {d} days"),
            Activity::Inactive(None) => "no project files besides build output".to_string(),
            Activity::Unknown => format!(
                "a large project: more than {} files, not all checked, so it counts as an {ACTIVE}",
                ACTIVITY_FILE_CAP
            ),
        }
    }
}

/// See [`Activity`]. `limit_days`: a change newer than this makes the project active.
pub fn activity(project: &Path, limit_days: u64, now: SystemTime) -> Activity {
    let age = |p: &Path| -> Option<u64> {
        let m = std::fs::symlink_metadata(p).ok()?.modified().ok()?;
        Some(now.duration_since(m).unwrap_or(Duration::ZERO).as_secs() / 86_400)
    };
    let mut newest: Option<u64> = None;
    let mut seen = 0usize;
    let mut note = |days: Option<u64>| -> Option<Activity> {
        let d = days?;
        if d < limit_days {
            return Some(Activity::Active(d));
        }
        newest = Some(newest.map_or(d, |n: u64| n.min(d)));
        None
    };
    for f in [".git/HEAD", ".git/index"] {
        let p = project.join(f);
        if is_file(&p)
            && let Some(a) = note(age(&p))
        {
            return a;
        }
    }
    let mut stack = vec![project.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for c in children(&dir) {
            let Ok(md) = std::fs::symlink_metadata(&c) else {
                continue;
            };
            if md.is_dir() {
                let n = name_of(&c);
                if n == ".git" || n == "node_modules" || n == ".next" || classify(&c).is_some() {
                    continue;
                }
                stack.push(c);
            } else if md.is_file() {
                seen += 1;
                if seen > ACTIVITY_FILE_CAP {
                    return Activity::Unknown;
                }
                if let Some(a) = note(age(&c)) {
                    return a;
                }
            }
        }
    }
    Activity::Inactive(newest)
}

/// The project's name as the user knows it: its path inside the home folder.
fn project_label(env: &Env, project: &Path) -> String {
    match project.strip_prefix(env.home()) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => env.display(project),
    }
}

/// The git CleanCraft may run without macOS offering to install developer tools: Homebrew's,
/// or Apple's once `xcode-select -p` says the command line tools are there.
pub fn git_program(env: &Env) -> Option<String> {
    for p in ["/opt/homebrew/bin/git", "/usr/local/bin/git"] {
        if std::fs::symlink_metadata(env.sys_path(p)).is_ok() {
            return Some(p.to_string());
        }
    }
    match env.sys.run("/usr/bin/xcode-select", &["-p"]) {
        Ok(o) if o.ok() => Some("/usr/bin/git".to_string()),
        _ => None,
    }
}

/// The git work tree holding `p`: the nearest folder above it with a `.git`.
pub fn repo_of(env: &Env, p: &Path) -> Option<PathBuf> {
    let mut cur = p.parent();
    while let Some(d) = cur {
        if !d.starts_with(env.root()) {
            return None;
        }
        if std::fs::symlink_metadata(d.join(".git")).is_ok() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

/// Of `folders` (all inside the work tree `repo`), the ones git tracks a file in. `Err` when
/// git could not be asked.
pub fn tracked_in(
    env: &Env,
    git: &str,
    repo: &Path,
    folders: &[PathBuf],
) -> Result<Vec<bool>, String> {
    let rels: Vec<String> = folders
        .iter()
        .map(|f| {
            f.strip_prefix(repo)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    if rels.iter().any(String::is_empty) {
        return Err("a folder outside the work tree".into());
    }
    let repo_s = repo.to_string_lossy().into_owned();
    // Names are taken literally: "app [old]" is not a pattern.
    let mut args: Vec<&str> = vec!["--literal-pathspecs", "-C", &repo_s, "ls-files", "-z", "--"];
    args.extend(rels.iter().map(String::as_str));
    let out = env.sys.run(git, &args).map_err(|e| e.to_string())?;
    if !out.ok() {
        return Err(out.stderr.trim().to_string());
    }
    let files: Vec<&str> = out
        .stdout
        .split(['\0', '\n'])
        .filter(|s| !s.is_empty())
        .collect();
    Ok(rels
        .iter()
        .map(|r| {
            files
                .iter()
                .any(|f| *f == r || f.starts_with(&format!("{r}/")))
        })
        .collect())
}

/// When git cannot be asked: is the folder listed by the `.gitignore` of its project or of its
/// work tree?
fn ignored_by_files(project: &Path, repo: Option<&Path>, p: &Path) -> bool {
    let mut dirs = vec![project.to_path_buf()];
    if let Some(r) = repo {
        dirs.push(r.to_path_buf());
    }
    dirs.iter().any(|d| {
        p.strip_prefix(d).is_ok_and(|rel| {
            gitignore_lists(
                &d.join(".gitignore"),
                &rel.to_string_lossy().replace('\\', "/"),
            )
        })
    })
}

/// Why git keeps `folders` (one answer each, `None` = may move): tracked by git, or git could
/// not be asked and no `.gitignore` lists it. Folders outside any work tree are fine.
pub fn git_locks(env: &Env, folders: &[(PathBuf, PathBuf)]) -> Vec<Option<&'static str>> {
    let mut out = vec![None; folders.len()];
    let mut by_repo: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (i, (p, _)) in folders.iter().enumerate() {
        if let Some(r) = repo_of(env, p) {
            by_repo.entry(r).or_default().push(i);
        }
    }
    if by_repo.is_empty() {
        return out;
    }
    let git = git_program(env);
    for (repo, ids) in by_repo {
        let paths: Vec<PathBuf> = ids.iter().map(|i| folders[*i].0.clone()).collect();
        let answer = match &git {
            Some(g) => tracked_in(env, g, &repo, &paths),
            None => Err("git is not available".into()),
        };
        match answer {
            Ok(tracked) => {
                for (i, t) in ids.iter().zip(tracked) {
                    if t {
                        out[*i] = Some(LOCK_TRACKED);
                    }
                }
            }
            Err(_) => {
                for i in ids {
                    let (p, project) = &folders[i];
                    if !ignored_by_files(project, Some(&repo), p) {
                        out[i] = Some(LOCK_UNCHECKED);
                    }
                }
            }
        }
    }
    out
}

/// The plan Quick Sweep and every front end use (same as [`crate::features::scan`]).
pub fn plan(env: &Env, settings: &Settings) -> anyhow::Result<Plan> {
    crate::features::scan(env, settings, FeatureId::DevClutter)
}

impl Feature for DevClutter {
    fn id(&self) -> FeatureId {
        FeatureId::DevClutter
    }

    /// The code folders (the user's own from the saved settings).
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        let mut v = code_roots(env, &Settings::load(env)).0;
        v.push(env.home_path(SIM_DEVICES));
        v
    }

    /// Again right before moving, from what is on disk now: still developer clutter of a known
    /// kind next to its project file, not inside another `node_modules`, inside a code folder,
    /// and git tracks nothing in it.
    fn validate(
        &self,
        env: &Env,
        settings: &Settings,
        _plan: &Plan,
        item: &Item,
    ) -> Result<(), String> {
        let devices = env.home_path(SIM_DEVICES);
        if item.path.starts_with(&devices) {
            return validate_simulator(env, &devices, &item.path);
        }
        let Some((_, project)) = classify(&item.path) else {
            return Err(
                "it is no longer a build folder next to its project file (or it is a link)".into(),
            );
        };
        let (roots, _) = code_roots(env, settings);
        if !roots
            .iter()
            .any(|r| item.path.starts_with(r) && item.path != *r)
        {
            return Err(LOCK_NOT_ALLOWED.to_lowercase());
        }
        if std::fs::symlink_metadata(item.path.join(".git")).is_ok() {
            return Err(LOCK_OWN_GIT.to_lowercase());
        }
        if let Some(why) = git_locks(env, &[(item.path.clone(), project)])[0] {
            return Err(why.to_lowercase());
        }
        Ok(())
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let (roots, notes) = code_roots(ctx.env, ctx.settings);
        let mut s = Scanner {
            ctx,
            allow: self.allow_roots(ctx.env),
            plan: Plan::new(FeatureId::DevClutter),
            now: SystemTime::now(),
            limit: ctx.settings.dev_inactive_days.max(1),
            activity: HashMap::new(),
            pending: Vec::new(),
        };
        // The allow-list comes from the saved settings, the roots of this scan from the ones
        // given: a root only in the latter is looked in, its items locked.
        s.plan.notes.extend(notes);
        if roots.is_empty() {
            s.plan.notes.push(format!(
                "No code folders found. CleanCraft looks in {} inside your home folder; add your own in Settings.",
                DEFAULT_ROOTS
                    .iter()
                    .map(|d| format!("~/{d}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for root in &roots {
            if ctx.cancelled() {
                s.plan.notes.push("Scan stopped before the end.".into());
                break;
            }
            (ctx.progress)(&ctx.env.display(root));
            // A code folder inside a protected place (Documents, Desktop): its projects are
            // listed, locked with the reason.
            let protected = root_lock(ctx.env, root);
            s.walk(root, 0, protected.as_deref());
        }
        s.git_pass();
        if !ctx.cancelled() {
            (ctx.progress)("Xcode simulators");
            s.simulators();
        }
        Ok(s.plan)
    }
}

/// Why nothing in the code folder `root` can be offered, in words with the next step: it lies
/// in a protected place (Documents, Desktop), or the way to it goes through a link (DV1: a
/// folder reached through a link into Documents says so, never "not inside one of your code
/// folders").
pub fn root_lock(env: &Env, root: &Path) -> Option<String> {
    if let Some(why) = policy::protected_reason(env, root) {
        return Some(format!(
            "Protected place ({why}): CleanCraft never moves anything from there. {PROTECTED_STEPS}"
        ));
    }
    let l = policy::link_at_or_above(env, root)?;
    let link = env.display(&l.link);
    let shown = env.display(root);
    Some(match (&l.protected, &l.real) {
        (Some(why), Some(real)) => format!(
            "Your code folder {shown} is reached through the link {link}, which leads into {why} ({}). CleanCraft never moves anything from there. {PROTECTED_STEPS}",
            env.display(real)
        ),
        (Some(why), None) => format!(
            "Your code folder {shown} is reached through the link {link}, which leads into {why}. CleanCraft never moves anything from there. {PROTECTED_STEPS}"
        ),
        (None, Some(real)) => format!(
            "Your code folder {shown} is reached through the link {link}, and CleanCraft never cleans through a link. Add the real folder {} under Developer Clutter in Settings instead, then scan again",
            env.display(real)
        ),
        (None, None) => format!(
            "Your code folder {shown} is reached through the link {link}, and CleanCraft never cleans through a link. Add the real folder under Developer Clutter in Settings instead, then scan again"
        ),
    })
}

struct Scanner<'a, 'b> {
    ctx: &'a ScanCtx<'b>,
    allow: Vec<PathBuf>,
    plan: Plan,
    now: SystemTime,
    limit: u64,
    activity: HashMap<PathBuf, Activity>,
    /// Items still to check with git: (item id, folder, project).
    pending: Vec<(usize, PathBuf, PathBuf)>,
}

impl Scanner<'_, '_> {
    fn walk(&mut self, dir: &Path, depth: usize, protected: Option<&str>) {
        if depth > MAX_DEPTH || self.ctx.cancelled() || self.ctx.ignored(dir) || !real_dir(dir) {
            return;
        }
        // A place that is protected inside an unprotected code folder (a Photos library).
        if protected.is_none() && depth > 0 && policy::protected_reason(self.ctx.env, dir).is_some()
        {
            return;
        }
        for c in children(dir) {
            let Ok(md) = std::fs::symlink_metadata(&c) else {
                continue;
            };
            // Links are never followed (a link to a folder is not a folder here).
            if !md.is_dir() {
                continue;
            }
            if let Some((kind, project)) = classify(&c) {
                self.candidate(&c, kind, &project, protected);
                continue;
            }
            let n = name_of(&c);
            // Inside these nothing is offered on its own.
            if n == ".git" || n == "node_modules" || n == ".next" {
                if n == ".next" && real_dir(&c.join("cache")) {
                    let cache = c.join("cache");
                    if let Some((kind, project)) = classify(&cache) {
                        self.candidate(&cache, kind, &project, protected);
                    }
                }
                continue;
            }
            // Other hidden folders and app bundles hold no projects worth a walk.
            if n.starts_with('.') || n.ends_with(".app") {
                continue;
            }
            self.walk(&c, depth + 1, protected);
        }
    }

    fn activity_of(&mut self, project: &Path) -> Activity {
        if let Some(a) = self.activity.get(project) {
            return *a;
        }
        let a = activity(project, self.limit, self.now);
        self.activity.insert(project.to_path_buf(), a);
        a
    }

    fn candidate(&mut self, p: &Path, kind: Kind, project: &Path, protected: Option<&str>) {
        if self.ctx.ignored(p) || self.plan.items.iter().any(|i| i.path == p) {
            return;
        }
        let act = self.activity_of(project);
        let inactive = matches!(act, Activity::Inactive(_));
        // A Python environment may be the only record of what was installed: never selected
        // for the user.
        let select = inactive && kind != Kind::PythonVenv;
        // dist and build folders count only in projects nobody works on.
        if kind == Kind::IgnoredOutput && !inactive {
            return;
        }
        let (size, _) = tree_size(p);
        if size == 0 {
            return;
        }
        let (cid, title, desc) = CATEGORIES
            .iter()
            .find(|c| c.0 == kind.category())
            .copied()
            .unwrap_or((CAT_BUILD, "Build output", ""));
        self.plan.add_category(cid, title, desc, Risk::Low);
        let label = project_label(self.ctx.env, project);
        let shown = name_of(project);
        let extra = match kind {
            Kind::IgnoredOutput => format!(" (.gitignore lists {})", name_of(p)),
            _ => String::new(),
        };
        let mut reason = format!(
            "{} of {shown}{extra}: {}. {}.",
            kind.what(),
            act.text(),
            sentence(kind.rebuild())
        );
        if kind == Kind::PythonVenv {
            reason.push_str(" Never selected for you: check the project lists its packages first.");
        }
        let id = self
            .plan
            .push(p.to_path_buf(), size, cid, reason, Risk::Low, select);
        let it = &mut self.plan.items[id];
        it.group = Some(label);
        it.modified = std::fs::symlink_metadata(p)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        if let Some(why) = protected {
            self.plan.lock(id, why);
        } else if std::fs::symlink_metadata(p.join(".git")).is_ok() {
            self.plan.lock(id, LOCK_OWN_GIT);
        } else if !self.ctx.allowed(&self.allow, p) {
            self.plan.lock(id, LOCK_NOT_ALLOWED);
        } else {
            self.pending
                .push((id, p.to_path_buf(), project.to_path_buf()));
        }
    }

    fn git_pass(&mut self) {
        let list: Vec<(PathBuf, PathBuf)> = self
            .pending
            .iter()
            .map(|(_, p, project)| (p.clone(), project.clone()))
            .collect();
        let locks = git_locks(self.ctx.env, &list);
        for ((id, _, _), why) in self.pending.iter().zip(locks) {
            if let Some(why) = why {
                self.plan.lock(*id, why);
            }
        }
        self.pending.clear();
    }
}

impl Scanner<'_, '_> {
    /// The device folders of the simulators simctl calls unavailable. Never selected.
    fn simulators(&mut self) {
        let env = self.ctx.env;
        let devices = env.home_path(SIM_DEVICES);
        if !real_dir(&devices) {
            return;
        }
        let list = match unavailable_simulators(env) {
            Ok(l) => l,
            Err(e) => {
                self.plan.notes.push(e);
                return;
            }
        };
        for d in list {
            let dir = devices.join(&d.udid);
            if !real_dir(&dir) || self.ctx.ignored(&dir) {
                continue;
            }
            let (size, _) = tree_size(&dir);
            if size == 0 {
                continue;
            }
            let (cid, title, desc) = CATEGORIES[4];
            self.plan.add_category(cid, title, desc, Risk::Low);
            let reason = format!(
                "Simulator {} ({}): {}. Xcode cannot start it any more.",
                d.name, d.runtime, d.why
            );
            let id = self
                .plan
                .push(dir.clone(), size, CAT_SIM, reason, Risk::Low, false);
            self.plan.items[id].group = Some("Xcode simulators".into());
            if d.booted {
                self.plan.lock(id, LOCK_BOOTED);
            } else if !self.ctx.allowed(&self.allow, &dir) {
                self.plan.lock(id, LOCK_NOT_ALLOWED);
            }
        }
    }
}

pub const XCRUN: &str = "/usr/bin/xcrun";
pub const SIM_DEVICES: &str = "Library/Developer/CoreSimulator/Devices";

/// One simulator device simctl lists as unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimDevice {
    pub udid: String,
    pub name: String,
    pub runtime: String,
    pub why: String,
    pub booted: bool,
}

/// `xcrun simctl list devices unavailable -j`, in words when it cannot be asked.
pub fn unavailable_simulators(env: &Env) -> Result<Vec<SimDevice>, String> {
    let out = env
        .sys
        .run(XCRUN, &["simctl", "list", "devices", "unavailable", "-j"])
        .map_err(|e| format!("The simulators could not be listed: {e}"))?;
    if !out.ok() {
        let err = format!("{}{}", out.stderr, out.stdout);
        return Err(if err.contains("unable to find utility") {
            "The simulators could not be listed: simctl comes with Xcode, which is not installed."
                .into()
        } else if err.contains("active developer path") {
            "The simulators could not be listed: the Xcode command line tools are missing.".into()
        } else {
            format!("The simulators could not be listed: {}", first_line(&err))
        });
    }
    parse_simctl(&out.stdout).ok_or_else(|| "simctl's list of simulators could not be read.".into())
}

/// A simulator folder about to move: directly in the Devices folder, and simctl still lists
/// that device as unavailable and not running.
fn validate_simulator(env: &Env, devices: &Path, p: &Path) -> Result<(), String> {
    if p.parent() != Some(devices) || !real_dir(p) {
        return Err("only whole simulator folders are moved".into());
    }
    let udid = name_of(p);
    let list = unavailable_simulators(env).map_err(|e| e.to_lowercase())?;
    match list.iter().find(|d| d.udid == udid) {
        None => Err("simctl no longer lists this simulator as unavailable".into()),
        Some(d) if d.booted => Err(LOCK_BOOTED.to_lowercase()),
        Some(_) => Ok(()),
    }
}

fn sentence(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------------------
// Docker and Homebrew: reported only. Their own cleanups delete for good, so CleanCraft
// never runs them; it shows what they could free and the exact command to run in Terminal.

/// A developer tool whose cleanup CleanCraft reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ToolId {
    DockerImages,
    DockerBuildCache,
    Homebrew,
}

impl ToolId {
    pub const ALL: [ToolId; 3] = [
        ToolId::DockerImages,
        ToolId::DockerBuildCache,
        ToolId::Homebrew,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ToolId::DockerImages => "Docker images",
            ToolId::DockerBuildCache => "Docker build cache",
            ToolId::Homebrew => "Homebrew old versions and downloads",
        }
    }

    /// The button that copies the command (one name per tool).
    pub fn copy_label(self) -> &'static str {
        match self {
            ToolId::DockerImages => "Copy Docker Images Command",
            ToolId::DockerBuildCache => "Copy Docker Build Cache Command",
            ToolId::Homebrew => "Copy Homebrew Command",
        }
    }

    /// Why CleanCraft does not do it itself, said plainly.
    pub fn why_not_here(self) -> &'static str {
        match self {
            ToolId::DockerImages | ToolId::DockerBuildCache => {
                "Docker keeps this data inside its own disk image, so it cannot go to the Trash. Its command deletes it for good, so CleanCraft leaves it to you: run the command in Terminal."
            }
            ToolId::Homebrew => {
                "Homebrew's command deletes old versions for good, so CleanCraft leaves it to you: run it in Terminal. Homebrew's download cache is also offered by System Junk, where it goes to the Trash."
            }
        }
    }
}

/// Where a tool stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolState {
    /// It reports something to clean; [`ToolReport::entries`] lists it.
    Ready,
    /// Installed, nothing to clean now.
    Clean(String),
    /// Not on this Mac.
    Absent(String),
    /// Installed but it could not be asked (Docker not running, the check failed).
    Unavailable(String),
}

/// One thing a tool says it would remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolEntry {
    pub label: String,
    pub detail: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolReport {
    pub id: ToolId,
    pub state: ToolState,
    pub entries: Vec<ToolEntry>,
    /// What it could free, as the tool reports it.
    pub size: u64,
    /// Where `size` comes from, in words.
    pub size_note: String,
    /// The exact command for Terminal.
    pub command: String,
}

impl ToolReport {
    pub fn ready(&self) -> bool {
        self.state == ToolState::Ready
    }

    fn new(id: ToolId, state: ToolState) -> ToolReport {
        let command = match id {
            ToolId::DockerImages => DOCKER_IMAGES_CMD,
            ToolId::DockerBuildCache => DOCKER_CACHE_CMD,
            ToolId::Homebrew => BREW_CMD,
        };
        ToolReport {
            id,
            state,
            entries: Vec::new(),
            size: 0,
            size_note: String::new(),
            command: command.to_string(),
        }
    }
}

/// The commands shown for Terminal (Docker asks there before it removes anything).
pub const DOCKER_IMAGES_CMD: &str = "docker image prune";
pub const DOCKER_CACHE_CMD: &str = "docker builder prune";
pub const BREW_CMD: &str = "brew cleanup";

pub const DOCKER_PATHS: &[&str] = &[
    "/usr/local/bin/docker",
    "/opt/homebrew/bin/docker",
    "/Applications/Docker.app/Contents/Resources/bin/docker",
];
pub const BREW_PATHS: &[&str] = &["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];

/// The first of `paths` present on this Mac (links count: Docker's is one).
fn find_program(env: &Env, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find(|p| std::fs::symlink_metadata(env.sys_path(p)).is_ok())
        .map(|p| p.to_string())
}

/// The first line of a tool's error output, for a message.
fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no details")
        .to_string()
}

/// Ask Docker and Homebrew what their cleanups would free. Only looks: `docker system df`
/// and `brew cleanup -n` change nothing.
pub fn scan_tools(env: &Env) -> Vec<ToolReport> {
    let mut v = docker(env);
    v.push(homebrew(env));
    v
}

/// "com.apple.CoreSimulator.SimRuntime.iOS-14-0" -> "iOS 14.0".
pub fn runtime_name(key: &str) -> String {
    let tail = key.rsplit('.').next().unwrap_or(key);
    match tail.split_once('-') {
        Some((os, ver)) => format!("{os} {}", ver.replace('-', ".")),
        None => tail.to_string(),
    }
}

/// `xcrun simctl list devices unavailable -j`: the unavailable devices.
pub fn parse_simctl(json: &str) -> Option<Vec<SimDevice>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let devices = v.get("devices")?.as_object()?;
    let mut out = Vec::new();
    for (runtime, list) in devices {
        for d in list.as_array()? {
            let available = d.get("isAvailable").and_then(|a| a.as_bool());
            let old_style = d
                .get("availability")
                .and_then(|a| a.as_str())
                .is_some_and(|a| a.contains("unavailable"));
            if available == Some(true) || (available.is_none() && !old_style) {
                continue;
            }
            let text = |k: &str| d.get(k).and_then(|u| u.as_str());
            let udid = text("udid")?.to_string();
            // A device id is a plain name, never a path.
            if udid.is_empty() || udid.contains('/') || udid.starts_with('.') {
                continue;
            }
            out.push(SimDevice {
                udid,
                name: text("name").unwrap_or("Simulator").to_string(),
                runtime: runtime_name(runtime),
                why: text("availabilityError")
                    .unwrap_or("unavailable")
                    .to_string(),
                booted: text("state").is_some_and(|s| s != "Shutdown"),
            });
        }
    }
    Some(out)
}

/// "16.43GB", "1.2kB", "0B", "245MB" (Docker, powers of 1000) or "3.4MB", "512KB" (Homebrew,
/// powers of 1024) to bytes.
pub fn parse_size(s: &str, base: f64) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let pow = match unit.trim().to_ascii_uppercase().as_str() {
        "B" => 0,
        "KB" | "K" => 1,
        "MB" | "M" => 2,
        "GB" | "G" => 3,
        "TB" | "T" => 4,
        _ => return None,
    };
    Some((n * base.powi(pow)).round() as u64)
}

/// One row of `docker system df`.
#[derive(Clone, Debug, PartialEq)]
pub struct DockerRow {
    pub kind: String,
    pub total: String,
    pub active: String,
    pub size: u64,
    pub reclaimable: u64,
}

/// `docker system df` table: TYPE TOTAL ACTIVE SIZE RECLAIMABLE.
pub fn parse_docker_df(out: &str) -> Option<Vec<DockerRow>> {
    let mut rows = Vec::new();
    for line in out.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // TYPE has spaces ("Local Volumes", "Build Cache"): the numbers from the right.
        let mut w: Vec<&str> = line.split_whitespace().collect();
        // RECLAIMABLE may end with " (68%)".
        if w.last().is_some_and(|x| x.starts_with('(')) {
            w.pop();
        }
        if w.len() < 5 {
            return None;
        }
        let n = w.len();
        rows.push(DockerRow {
            kind: w[..n - 4].join(" "),
            total: w[n - 4].to_string(),
            active: w[n - 3].to_string(),
            size: parse_size(w[n - 2], 1000.0)?,
            reclaimable: parse_size(w[n - 1], 1000.0)?,
        });
    }
    (!rows.is_empty()).then_some(rows)
}

fn docker(env: &Env) -> Vec<ToolReport> {
    let both = |state: ToolState| {
        vec![
            ToolReport::new(ToolId::DockerImages, state.clone()),
            ToolReport::new(ToolId::DockerBuildCache, state),
        ]
    };
    let Some(prog) = find_program(env, DOCKER_PATHS) else {
        return both(ToolState::Absent("Docker is not installed.".into()));
    };
    let out = match env.sys.run(&prog, &["system", "df"]) {
        Ok(o) => o,
        Err(e) => {
            return both(ToolState::Unavailable(format!(
                "Docker could not be asked: {e}"
            )));
        }
    };
    if !out.ok() {
        let err = format!("{}{}", out.stderr, out.stdout);
        let msg = if err.contains("Cannot connect to the Docker daemon")
            || err.contains("docker daemon running")
            || err.contains("error during connect")
        {
            "Docker is installed but not running. Open Docker, then check again.".to_string()
        } else {
            format!("Docker could not report its disk use: {}", first_line(&err))
        };
        return both(ToolState::Unavailable(msg));
    }
    let Some(rows) = parse_docker_df(&out.stdout) else {
        return both(ToolState::Unavailable(
            "Docker's answer could not be read.".into(),
        ));
    };
    let row = |k: &str| rows.iter().find(|r| r.kind.eq_ignore_ascii_case(k));
    let mut images = ToolReport::new(ToolId::DockerImages, ToolState::Ready);
    if let Some(r) = row("Images") {
        images.entries.push(ToolEntry {
            label: format!("Images: {} ({} in use)", r.total, r.active),
            detail: format!(
                "{} in all, {} not used by a container",
                human_size(r.size),
                human_size(r.reclaimable)
            ),
            size: r.reclaimable,
        });
        images.size = r.reclaimable;
    }
    images.size_note = "at most, as Docker reports it (the command removes only untagged images unless you add -a)".into();
    if images.size == 0 {
        images.state = ToolState::Clean("Docker reports no unused images.".into());
    }
    let mut cache = ToolReport::new(ToolId::DockerBuildCache, ToolState::Ready);
    if let Some(r) = row("Build Cache") {
        cache.entries.push(ToolEntry {
            label: format!("Build cache: {} entries", r.total),
            detail: format!("{} not in use", human_size(r.reclaimable)),
            size: r.reclaimable,
        });
        cache.size = r.reclaimable;
    }
    cache.size_note = "as Docker reports it".into();
    if cache.size == 0 {
        cache.state = ToolState::Clean("Docker reports no unused build cache.".into());
    }
    vec![images, cache]
}

/// `brew cleanup -n` lines: (path, size), and the total Homebrew estimates.
pub fn parse_brew(out: &str) -> (Vec<(String, u64)>, Option<u64>) {
    let mut items = Vec::new();
    let mut total = None;
    for line in out.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Would remove: ") {
            // "<path> (12 files, 3.4MB)" or "<path> (3.4MB)".
            let (path, size) = match rest.rfind(" (") {
                Some(i) if rest.ends_with(')') => {
                    let inner = &rest[i + 2..rest.len() - 1];
                    let s = inner.rsplit(", ").next().unwrap_or(inner);
                    (&rest[..i], parse_size(s, 1024.0).unwrap_or(0))
                }
                _ => (rest, 0),
            };
            items.push((path.to_string(), size));
        } else if let Some(i) = l.find("approximately ") {
            let s = l[i + "approximately ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or("");
            total = parse_size(s, 1024.0);
        }
    }
    (items, total)
}

fn homebrew(env: &Env) -> ToolReport {
    let Some(prog) = find_program(env, BREW_PATHS) else {
        return ToolReport::new(
            ToolId::Homebrew,
            ToolState::Absent("Homebrew is not installed.".into()),
        );
    };
    let mut r = ToolReport::new(ToolId::Homebrew, ToolState::Ready);
    // -n: a dry run, Homebrew only lists.
    let out = match env
        .sys
        .run_timeout(&prog, &["cleanup", "-n"], Duration::from_secs(120))
    {
        Ok(o) => o,
        Err(e) => {
            r.state = ToolState::Unavailable(format!("Homebrew could not be asked: {e}"));
            return r;
        }
    };
    if !out.ok() {
        r.state = ToolState::Unavailable(format!(
            "Homebrew could not list what it would clean: {}",
            first_line(&format!("{}{}", out.stderr, out.stdout))
        ));
        return r;
    }
    let (items, total) = parse_brew(&out.stdout);
    for (path, size) in &items {
        r.entries.push(ToolEntry {
            label: Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone()),
            detail: path.clone(),
            size: *size,
        });
    }
    r.size = total.unwrap_or_else(|| items.iter().map(|i| i.1).sum());
    r.size_note = "as Homebrew estimates it".into();
    if r.entries.is_empty() {
        r.state = ToolState::Clean("Homebrew has no old versions or downloads to remove.".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_patterns() {
        assert!(pattern_ignores("dist", "dist"));
        assert!(pattern_ignores("/dist/", "dist"));
        assert!(pattern_ignores("dist/", "dist"));
        assert!(pattern_ignores("**/node_modules", "node_modules"));
        assert!(pattern_ignores(".next", ".next/cache"));
        assert!(pattern_ignores("/.next/", ".next/cache"));
        assert!(pattern_ignores("venv*", "venv"));
        assert!(pattern_ignores("build/*", "build"));
        assert!(!pattern_ignores("# dist", "dist"));
        assert!(!pattern_ignores("dist.zip", "dist"));
        assert!(!pattern_ignores("src/dist", "dist"));
        assert!(!pattern_ignores("*.log", "build"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("16.43GB", 1000.0), Some(16_430_000_000));
        assert_eq!(parse_size("1.2kB", 1000.0), Some(1_200));
        assert_eq!(parse_size("0B", 1000.0), Some(0));
        assert_eq!(parse_size("3.4MB", 1024.0), Some(3_565_158));
        assert_eq!(parse_size("x", 1000.0), None);
    }

    #[test]
    fn docker_table() {
        let out = "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE\n\
                   Images          5         2         16.43GB   11.2GB (68%)\n\
                   Containers      2         0         1.2kB     1.2kB (100%)\n\
                   Local Volumes   3         1         245MB     200MB (81%)\n\
                   Build Cache     35        0         2.1GB     2.1GB\n";
        let rows = parse_docker_df(out).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].kind, "Local Volumes");
        assert_eq!(rows[0].reclaimable, 11_200_000_000);
        assert_eq!(rows[3].kind, "Build Cache");
        assert_eq!(rows[3].reclaimable, 2_100_000_000);
    }

    #[test]
    fn brew_lines() {
        let out = "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,000 files, 60.5MB)\n\
                   Would remove: /Users/me/Library/Caches/Homebrew/downloads/abc--wget.bottle.tar.gz (1.5MB)\n\
                   ==> This operation would free approximately 62MB of disk space.\n";
        let (items, total) = parse_brew(out);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "/opt/homebrew/Cellar/node/20.1.0");
        assert_eq!(items[0].1, (60.5 * 1024.0 * 1024.0_f64).round() as u64);
        assert_eq!(total, Some(62 * 1024 * 1024));
    }

    #[test]
    fn runtimes() {
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.iOS-14-0"),
            "iOS 14.0"
        );
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.watchOS-7-2"),
            "watchOS 7.2"
        );
    }
}
