//! Developer Clutter on a fake Mac: what is found in the code folders, the activity rule, git,
//! links, protected places, moving and undo, and the developer tools' own cleanups (commands
//! that run only with the exact text the user confirmed, logged, reported honestly when the
//! tool is missing or failing).

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, SkipReason};
use cleancraft_core::features::dev_clutter::{
    self as dev, ACTIVE, LOCK_TRACKED, LOCK_UNCHECKED, ToolId, ToolState,
};
use cleancraft_core::features::{self, run_plan};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId, Plan, Settings};

/// Every file under `dir` (and `dir` itself) changed `days` ago.
fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

fn file(m: &FakeMac, p: &Path, size: usize) {
    m.write(p, &vec![7u8; size]);
}

/// A Rust project with a 200 KB target folder; every file `days` old.
fn rust(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("Cargo.toml"), b"[package]\nname = \"x\"\n");
    m.write(&dir.join("src/main.rs"), b"fn main() {}\n");
    file(m, &dir.join("target/debug/app"), 200_000);
    age_tree(dir, days);
    dir.join("target")
}

fn node(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("package.json"), b"{}");
    m.write(&dir.join("index.js"), b"1");
    file(m, &dir.join("node_modules/left-pad/index.js"), 50_000);
    age_tree(dir, days);
    dir.join("node_modules")
}

fn scan(m: &FakeMac, s: &Settings) -> Plan {
    features::scan(&m.env, s, FeatureId::DevClutter).expect("scan")
}

fn item<'a>(plan: &'a Plan, p: &Path) -> &'a cleancraft_core::Item {
    plan.items
        .iter()
        .find(|i| i.path == p)
        .unwrap_or_else(|| panic!("{} not in the plan", p.display()))
}

fn paths(plan: &Plan) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = plan.items.iter().map(|i| i.path.clone()).collect();
    v.sort();
    v
}

fn calls(m: &FakeMac) -> Vec<String> {
    m.env.fake_sys().expect("fake").calls()
}

#[test]
fn discovery_per_category() {
    let m = FakeMac::new();
    let p = m.env.home_path("Projects");
    let t = rust(&m, &p.join("engine"), 60);
    let nm = node(&m, &p.join("web"), 60);
    // A package inside node_modules with its own package.json and node_modules: never on its
    // own.
    let inner = p.join("web/node_modules/left-pad");
    m.write(&inner.join("package.json"), b"{}");
    file(&m, &inner.join("node_modules/x/i.js"), 4_000);
    // Python: .venv, venv and env with pyvenv.cfg; an "env" folder without one is not a venv.
    let py = p.join("py");
    m.write(&py.join("main.py"), b"print(1)");
    for v in [".venv", "venv"] {
        m.write(&py.join(v).join("pyvenv.cfg"), b"home = /usr/bin");
        file(&m, &py.join(v).join("lib/site.py"), 30_000);
    }
    let py2 = p.join("tools/py2");
    m.write(&py2.join("setup.py"), b"x");
    m.write(&py2.join("env/pyvenv.cfg"), b"home = /usr/bin");
    file(&m, &py2.join("env/bin/python"), 20_000);
    let notvenv = p.join("tools/conf/env");
    file(&m, &notvenv.join("settings.txt"), 20_000);
    // Next.js cache, Gradle build, dist listed by .gitignore, build not listed.
    let next = p.join("site");
    m.write(&next.join("package.json"), b"{}");
    file(&m, &next.join(".next/cache/webpack/a.pack"), 40_000);
    file(&m, &next.join(".next/server/page.js"), 10_000);
    let gradle = p.join("android");
    m.write(&gradle.join("build.gradle"), b"x");
    file(&m, &gradle.join("build/outputs/app.apk"), 90_000);
    let web2 = p.join("dashboard");
    m.write(&web2.join(".gitignore"), b"# output\n/dist/\n*.log\n");
    m.write(&web2.join("main.ts"), b"x");
    file(&m, &web2.join("dist/bundle.js"), 25_000);
    file(&m, &web2.join("build/keep.js"), 25_000);
    age_tree(&p, 60);

    let plan = scan(&m, &Settings::default());
    let mut want = vec![
        t.clone(),
        nm.clone(),
        py.join(".venv"),
        py.join("venv"),
        py2.join("env"),
        next.join(".next/cache"),
        gradle.join("build"),
        web2.join("dist"),
    ];
    want.sort();
    assert_eq!(paths(&plan), want, "notes: {:?}", plan.notes);
    for it in &plan.items {
        assert_eq!(it.size, tree_size(&it.path).0, "{}", it.path.display());
        // Python environments are never selected for you (they may be the only record of
        // what was installed).
        let venv = it.category == dev::CAT_PYTHON;
        assert_eq!(it.selected, !venv, "{}: {}", it.path.display(), it.reason);
        assert!(it.locked.is_none());
        assert!(
            it.reason.contains("no file changed for 60 days"),
            "{}",
            it.reason
        );
    }
    assert_eq!(item(&plan, &t).category, dev::CAT_RUST);
    assert_eq!(item(&plan, &nm).category, dev::CAT_NODE);
    assert_eq!(item(&plan, &py2.join("env")).category, dev::CAT_PYTHON);
    assert_eq!(item(&plan, &gradle.join("build")).category, dev::CAT_BUILD);
    assert_eq!(item(&plan, &t).group.as_deref(), Some("Projects/engine"));
    assert!(
        item(&plan, &web2.join("dist"))
            .reason
            .contains(".gitignore lists dist")
    );
    // Nothing else of System Junk's: no pip or Go cache here.
    assert!(plan.categories.iter().all(|c| c.id != "dev_caches"));
}

#[test]
fn activity_rule_and_its_setting() {
    let m = FakeMac::new();
    let p = m.env.home_path("Code");
    let old = rust(&m, &p.join("old"), 45);
    let fresh = rust(&m, &p.join("fresh"), 45);
    // One source file changed two days ago: the whole project is in use.
    age(&p.join("fresh/src/main.rs"), 2);
    // A commit made today counts (.git/index), even when every file is old.
    let committed = rust(&m, &p.join("committed"), 45);
    m.write(&p.join("committed/.git/index"), b"x");
    m.write(&p.join("committed/.gitignore"), b"/target\n");
    age(&p.join("committed/.gitignore"), 45);
    // An active project's dist folder is not even listed.
    let busy = p.join("busy");
    m.write(&busy.join(".gitignore"), b"dist\n");
    file(&m, &busy.join("dist/a.js"), 9_000);
    // A build changing only target/ does not make a project active.
    age(&p.join("old/target/debug/app"), 0);

    let plan = scan(&m, &Settings::default());
    assert!(item(&plan, &old).selected, "{}", item(&plan, &old).reason);
    let f = item(&plan, &fresh);
    assert!(!f.selected && f.locked.is_none());
    assert!(
        f.reason
            .contains(&format!("{ACTIVE}, a file changed 2 days ago")),
        "{}",
        f.reason
    );
    let c = item(&plan, &committed);
    assert!(!c.selected && c.reason.contains(ACTIVE), "{}", c.reason);
    assert!(!paths(&plan).contains(&busy.join("dist")));

    // Inactive after 60 days instead of 30: the 45-day-old project is now active.
    let s = Settings {
        dev_inactive_days: 60,
        ..Settings::default()
    };
    let plan = scan(&m, &s);
    let o = item(&plan, &old);
    assert!(!o.selected && o.reason.contains(ACTIVE), "{}", o.reason);
}

#[test]
fn git_tracked_folder_is_never_offered() {
    let m = FakeMac::new();
    let repo = m.env.home_path("work/app");
    let t = rust(&m, &repo, 90);
    m.write(&repo.join(".git/HEAD"), b"ref: refs/heads/main");
    age(&repo.join(".git/HEAD"), 90);
    // Homebrew's git, so no xcode-select is needed; git says target/keep.txt is tracked.
    m.write(&m.env.sys_path("/opt/homebrew/bin/git"), b"#!");
    let fake = m.env.fake_sys().unwrap();
    fake.answer("git", "target/keep.txt\0");
    let plan = scan(&m, &Settings::default());
    let it = item(&plan, &t);
    assert_eq!(it.locked.as_deref(), Some(LOCK_TRACKED));
    assert!(!it.selected);
    let repo_s = repo.to_string_lossy().into_owned();
    assert!(
        calls(&m).contains(&format!(
            "/opt/homebrew/bin/git --literal-pathspecs -C {repo_s} ls-files -z -- target"
        )),
        "{:?}",
        calls(&m)
    );

    // An edited plan that unlocks and selects it is refused at the move: nothing moves.
    let mut forged = plan.clone();
    forged.items.iter_mut().for_each(|i| {
        i.locked = None;
        i.selected = true;
    });
    let d = forged.digest();
    let rep = run_plan(&m.env, &Settings::default(), &forged, &d, &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty());
    assert!(matches!(&rep.skipped[0].1, SkipReason::Refused(r) if r.contains("git tracks")));
    assert!(t.exists());

    // Git answers nothing tracked: offered.
    fake.answer("git", "");
    let plan = scan(&m, &Settings::default());
    assert!(item(&plan, &t).selected);

    // Git cannot be asked: offered only when .gitignore lists it.
    fake.answer_full(
        "git",
        CmdOut {
            status: 128,
            stdout: String::new(),
            stderr: "fatal: not a git repository".into(),
        },
    );
    let plan = scan(&m, &Settings::default());
    assert_eq!(item(&plan, &t).locked.as_deref(), Some(LOCK_UNCHECKED));
    m.write(&repo.join(".gitignore"), b"/target\n");
    age(&repo.join(".gitignore"), 90);
    let plan = scan(&m, &Settings::default());
    assert!(item(&plan, &t).selected && item(&plan, &t).locked.is_none());
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    use std::os::unix::fs::symlink;
    let m = FakeMac::new();
    let elsewhere = m.env.home_path("Elsewhere/proj");
    let t = rust(&m, &elsewhere, 90);
    let projects = m.env.home_path("Projects");
    std::fs::create_dir_all(&projects).unwrap();
    // A project reached through a link, a target folder that is a link, a code folder that
    // is a link.
    symlink(&elsewhere, projects.join("linked")).unwrap();
    let real = projects.join("real");
    m.write(&real.join("Cargo.toml"), b"x");
    symlink(elsewhere.join("target"), real.join("target")).unwrap();
    symlink(m.env.home_path("Elsewhere"), m.env.home_path("Code")).unwrap();
    let plan = scan(&m, &Settings::default());
    assert!(plan.items.is_empty(), "{:?}", paths(&plan));
    assert!(
        plan.notes.iter().any(|n| n.contains("/Code is a link")),
        "{:?}",
        plan.notes
    );
    // A saved plan naming the folder through the link is refused.
    let mut forged = Plan::new(FeatureId::DevClutter);
    forged.push(
        projects.join("linked/target"),
        1,
        dev::CAT_RUST,
        "x",
        cleancraft_core::Risk::Low,
        true,
    );
    let d = forged.digest();
    let rep = run_plan(&m.env, &Settings::default(), &forged, &d, &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty(), "{rep:?}");
    assert!(t.exists());
}

#[test]
fn protected_places_are_locked_and_never_walked() {
    let m = FakeMac::new();
    // Never looked at: a project in Documents without a code folder there.
    rust(&m, &m.env.home_path("Documents/stray"), 90);
    let plan = scan(&m, &Settings::default());
    assert!(plan.items.is_empty());
    // The user's own code folder inside Documents: listed, locked with the reason.
    let root = m.env.home_path("Documents/code");
    let t = rust(&m, &root.join("app"), 90);
    let s = Settings {
        dev_roots: vec![root.clone()],
        ..Settings::default()
    };
    s.save(&m.env).unwrap();
    let plan = scan(&m, &s);
    let it = item(&plan, &t);
    assert!(!it.selected);
    let why = it.locked.clone().unwrap();
    assert!(why.contains("the contents of Documents"), "{why}");
    // The home folder itself is never a code folder.
    let s = Settings {
        dev_roots: vec![m.env.home().to_path_buf()],
        ..Settings::default()
    };
    let plan = scan(&m, &s);
    assert!(plan.items.is_empty());
    assert!(plan.notes.iter().any(|n| n.contains("whole home folder")));
}

#[test]
fn own_code_folder_moves_to_the_trash_and_undo_restores() {
    let m = FakeMac::new();
    let root = m.env.home_path("Clients");
    let t = rust(&m, &root.join("acme/api"), 90);
    let nm = node(&m, &root.join("acme/web"), 90);
    let active = node(&m, &root.join("acme/new"), 0);
    // Not in the settings: not looked at.
    assert!(scan(&m, &Settings::default()).items.is_empty());
    let s = Settings {
        dev_roots: vec![root.clone()],
        ..Settings::default()
    };
    s.save(&m.env).unwrap();
    let plan = scan(&m, &s);
    assert_eq!(plan.selected_count(), 2);
    let want: u64 = [&t, &nm].iter().map(|p| tree_size(p).0).sum();
    assert_eq!(plan.selected_size(), want);
    let rep = run_plan(&m.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 2, "{:?}", rep.skipped);
    assert_eq!(rep.freed, want);
    assert!(!t.exists() && !nm.exists());
    assert!(active.exists());
    assert!(root.join("acme/api/Cargo.toml").exists());
    let trashed = executor::history(&m.env);
    assert!(
        trashed
            .iter()
            .all(|r| r.feature == Some(FeatureId::DevClutter))
    );
    let (n, errs) = executor::undo_plan(&m.env, &plan.id);
    assert_eq!((n, errs.len()), (2, 0));
    assert!(t.exists() && nm.exists());
}

#[test]
fn permission_denied_and_file_in_use_are_skipped() {
    let m = FakeMac::new();
    let root = m.env.home_path("Projects");
    let t = rust(&m, &root.join("a"), 90);
    let nm = node(&m, &root.join("b"), 90);
    let plan = scan(&m, &Settings::default());
    m.env.inject_fault(&t, Fault::PermissionDenied);
    m.env
        .fake_sys()
        .unwrap()
        .set_open(&nm.join("left-pad/index.js"));
    let rep = run_plan(
        &m.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(rep.moved.is_empty());
    assert!(t.exists() && nm.exists());
    let why: Vec<String> = rep.skipped.iter().map(|s| s.1.to_string()).collect();
    assert!(why.contains(&"permission denied".to_string()), "{why:?}");
    assert!(why.iter().any(|w| w.starts_with("open in")), "{why:?}");
}

// ---------------------------------------------------------------------------------------
// Simulators (plan items) and tools (reported only)

const SIMCTL_JSON: &str = r#"{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.iOS-14-0" : [
      { "udid" : "AAAA-1111", "isAvailable" : false, "name" : "iPhone 8",
        "availabilityError" : "runtime profile not found", "state" : "Shutdown" },
      { "udid" : "CCCC-3333", "isAvailable" : false, "name" : "iPad Air",
        "availabilityError" : "runtime profile not found", "state" : "Booted" },
      { "udid" : "../../Documents", "isAvailable" : false, "name" : "Evil", "state" : "Shutdown" }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-17-2" : [
      { "udid" : "BBBB-2222", "isAvailable" : true, "name" : "iPhone 15", "state" : "Shutdown" }
    ]
  }
}"#;

fn sim_mac() -> (FakeMac, PathBuf) {
    let m = FakeMac::new();
    let dev = m.env.home_path(dev::SIM_DEVICES);
    file(&m, &dev.join("AAAA-1111/data/big.bin"), 300_000);
    file(&m, &dev.join("BBBB-2222/data/keep.bin"), 100_000);
    file(&m, &dev.join("CCCC-3333/data/on.bin"), 100_000);
    m.env
        .fake_sys()
        .unwrap()
        .answer("xcrun simctl list devices unavailable -j", SIMCTL_JSON);
    (m, dev)
}

/// No command that deletes for good was ever asked for.
fn no_permanent_commands(m: &FakeMac) {
    for c in calls(m) {
        assert!(
            !(c.contains("simctl delete")
                || c.contains("simctl erase")
                || c.contains("prune")
                || c.ends_with("brew cleanup")
                || c.contains("autoremove")),
            "ran {c}"
        );
    }
}

#[test]
fn unavailable_simulators_move_to_the_trash_never_preselected() {
    let (m, dev) = sim_mac();
    let plan = scan(&m, &Settings::default());
    let old = dev.join("AAAA-1111");
    let booted = dev.join("CCCC-3333");
    assert_eq!(paths(&plan), vec![old.clone(), booted.clone()]);
    let it = item(&plan, &old);
    assert_eq!(it.category, dev::CAT_SIM);
    assert!(!it.selected, "simulators are never selected for you");
    assert_eq!(it.size, tree_size(&old).0);
    assert!(it.reason.contains("iPhone 8 (iOS 14.0)"), "{}", it.reason);
    assert_eq!(
        item(&plan, &booted).locked.as_deref(),
        Some(dev::LOCK_BOOTED)
    );
    let mut p = plan.clone();
    let id = item(&p, &old).id;
    p.set_selected(id, true);
    let rep = run_plan(
        &m.env,
        &Settings::default(),
        &p,
        &p.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 1, "{:?}", rep.skipped);
    assert!(!old.exists() && dev.join("BBBB-2222").exists());
    no_permanent_commands(&m);
    let (n, _) = executor::undo_plan(&m.env, &p.id);
    assert_eq!(n, 1);
    assert!(old.exists());
    // Rechecked at the move: simctl no longer lists it (Xcode got its runtime back).
    m.env.fake_sys().unwrap().answer(
        "xcrun simctl list devices unavailable -j",
        r#"{"devices":{}}"#,
    );
    let rep = run_plan(
        &m.env,
        &Settings::default(),
        &p,
        &p.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(rep.moved.is_empty() && old.exists());
    assert!(matches!(&rep.skipped[0].1, SkipReason::Refused(r) if r.contains("no longer lists")));
}

#[test]
fn simulators_without_xcode_are_a_note() {
    let m = FakeMac::new();
    assert!(scan(&m, &Settings::default()).items.is_empty());
    assert!(calls(&m).is_empty(), "no xcrun without a simulator folder");
    let (m, _) = sim_mac();
    m.env.fake_sys().unwrap().answer_full(
        "xcrun simctl list devices unavailable -j",
        CmdOut {
            status: 72,
            stdout: String::new(),
            stderr:
                "xcrun: error: unable to find utility \"simctl\", not a developer tool or in PATH"
                    .into(),
        },
    );
    let plan = scan(&m, &Settings::default());
    assert!(plan.items.is_empty());
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("Xcode, which is not installed")),
        "{:?}",
        plan.notes
    );
}

const DF: &str = "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE\n\
Images          5         2         16.43GB   11.2GB (68%)\n\
Containers      2         0         1.2kB     1.2kB (100%)\n\
Local Volumes   3         1         245MB     200MB (81%)\n\
Build Cache     35        0         2.1GB     2.1GB\n";

fn tool(m: &FakeMac, id: ToolId) -> dev::ToolReport {
    dev::scan_tools(&m.env)
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
}

#[test]
fn docker_absent_stopped_failing_and_running_is_reported_only() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().unwrap();
    assert_eq!(
        tool(&m, ToolId::DockerImages).state,
        ToolState::Absent("Docker is not installed.".into())
    );
    assert!(calls(&m).iter().all(|c| !c.contains("docker")));

    m.write(&m.env.sys_path("/usr/local/bin/docker"), b"#!");
    fake.answer_full(
        "docker system df",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Cannot connect to the Docker daemon at unix:///Users/me/.docker/run/docker.sock. Is the docker daemon running?".into(),
        },
    );
    let r = tool(&m, ToolId::DockerBuildCache);
    assert!(
        matches!(&r.state, ToolState::Unavailable(m) if m.contains("not running")),
        "{:?}",
        r.state
    );
    fake.answer_full(
        "docker system df",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "permission denied while trying to connect".into(),
        },
    );
    let r = tool(&m, ToolId::DockerImages);
    assert!(
        matches!(&r.state, ToolState::Unavailable(m) if m.contains("permission denied while trying")),
        "{:?}",
        r.state
    );

    fake.answer("docker system df", DF);
    let img = tool(&m, ToolId::DockerImages);
    let cache = tool(&m, ToolId::DockerBuildCache);
    assert!(img.ready() && cache.ready());
    assert_eq!(img.size, 11_200_000_000);
    assert_eq!(cache.size, 2_100_000_000);
    assert_eq!(img.command, "docker image prune");
    assert_eq!(cache.command, "docker builder prune");
    // Never in a plan, never run.
    assert!(scan(&m, &Settings::default()).items.is_empty());
    no_permanent_commands(&m);
}

#[test]
fn homebrew_absent_failing_and_dry_run_only() {
    let m = FakeMac::new();
    let fake = m.env.fake_sys().unwrap();
    assert_eq!(
        tool(&m, ToolId::Homebrew).state,
        ToolState::Absent("Homebrew is not installed.".into())
    );
    m.write(&m.env.sys_path("/opt/homebrew/bin/brew"), b"#!");
    fake.answer_full(
        "brew cleanup -n",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Error: Running Homebrew as root is extremely dangerous".into(),
        },
    );
    let r = tool(&m, ToolId::Homebrew);
    assert!(
        matches!(&r.state, ToolState::Unavailable(m) if m.contains("Running Homebrew as root")),
        "{:?}",
        r.state
    );
    fake.answer(
        "brew cleanup -n",
        "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,000 files, 60.5MB)\n\
         Would remove: /Users/me/Library/Caches/Homebrew/downloads/abc--wget.bottle.tar.gz (1.5MB)\n\
         ==> This operation would free approximately 62MB of disk space.\n",
    );
    let r = tool(&m, ToolId::Homebrew);
    assert!(r.ready());
    assert_eq!(r.entries.len(), 2);
    assert_eq!(r.entries[0].label, "20.1.0");
    assert_eq!(r.size, 62 * 1024 * 1024);
    assert_eq!(r.command, "brew cleanup");
    assert!(calls(&m).contains(&"/opt/homebrew/bin/brew cleanup -n".to_string()));
    no_permanent_commands(&m);
}

#[test]
fn a_build_folder_holding_its_own_repository_is_locked() {
    let m = FakeMac::new();
    let t = rust(&m, &m.env.home_path("Projects/[odd] name"), 90);
    m.write(&t.join(".git/HEAD"), b"ref");
    let plan = scan(&m, &Settings::default());
    let it = item(&plan, &t);
    assert_eq!(it.locked.as_deref(), Some(dev::LOCK_OWN_GIT));
    // An edited plan is refused at the move too.
    let mut forged = plan.clone();
    forged.items[0].locked = None;
    forged.items[0].selected = true;
    let d = forged.digest();
    let rep = run_plan(&m.env, &Settings::default(), &forged, &d, &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty() && t.exists());
}
