//! FIX-1: items in folders only an administrator can change. Features that allow it move the
//! items the plan marks `needs_admin` with ONE administrator command of `/bin/mv -n` lines
//! into the quarantine, after the same checks; what really moved is checked on disk and
//! journaled as `AdminQuarantine`; undo moves it back the same way; a cancelled password
//! dialog moves nothing.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, Method, UndoError};
use cleancraft_core::features::{self, uninstaller};
use cleancraft_core::sys::parse_admin_moves;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup as kit;
use cleancraft_core::testkit_perf as tp;
use cleancraft_core::{ActionKind, Fault, FeatureId, Plan, Settings, SkipReason};

const SHADY_PROG: &str = "/Library/Application Support/Shady/daemon";

/// The perf fake Mac (root-owned /Library/LaunchAgents and LaunchDaemons) plus an unsigned
/// daemon and a known adware agent in those folders.
fn threat_mac() -> (FakeMac, PathBuf, PathBuf) {
    let m = tp::startup_mac();
    let daemon = tp::add_launch_item(
        &m,
        "/Library/LaunchDaemons",
        "com.shady.daemon",
        SHADY_PROG,
        Some(tp::codesign_unsigned(SHADY_PROG)),
    );
    let agent = tp::add_launch_item(
        &m,
        "/Library/LaunchAgents",
        "com.genieo.global",
        "/Library/Application Support/Genieo Global/run",
        None,
    );
    (m, daemon, agent)
}

fn item<'a>(p: &'a Plan, path: &Path) -> &'a cleancraft_core::Item {
    p.items
        .iter()
        .find(|i| i.path == path)
        .unwrap_or_else(|| panic!("{} not in {:#?}", path.display(), p.items))
}

fn select_only(p: &mut Plan, paths: &[&Path]) {
    for i in 0..p.items.len() {
        p.set_selected(i, false);
    }
    for path in paths {
        let id = item(p, path).id;
        p.set_selected(id, true);
    }
}

fn run(m: &FakeMac, p: &Plan) -> cleancraft_core::Report {
    features::run_plan(&m.env, &Settings::default(), p, &p.digest(), &mut |_, _| {}).unwrap()
}

fn admin_calls(m: &FakeMac) -> Vec<String> {
    m.env.fake_sys().unwrap().admin_calls()
}

fn in_quarantine(m: &FakeMac, name: &str) -> Option<PathBuf> {
    fn walk(d: &Path, name: &str) -> Option<PathBuf> {
        for e in std::fs::read_dir(d).ok()?.flatten() {
            if e.file_name().to_string_lossy() == name {
                return Some(e.path());
            }
            if e.path().is_dir()
                && let Some(p) = walk(&e.path(), name)
            {
                return Some(p);
            }
        }
        None
    }
    walk(&m.env.quarantine_dir(), name)
}

#[test]
fn threats_in_root_folders_are_marked_not_locked_and_move_with_one_admin_command() {
    let (m, daemon, agent) = threat_mac();
    let user_agent = m
        .env
        .home_path("Library/LaunchAgents/com.genieo.engine.plist");
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    let d = item(&p, &daemon);
    assert!(d.needs_admin && d.locked.is_none(), "{d:#?}");
    let a = item(&p, &agent);
    assert!(a.needs_admin && a.locked.is_none() && a.selected, "{a:#?}");
    assert!(!item(&p, &user_agent).needs_admin);

    select_only(&mut p, &[&daemon, &agent, &user_agent]);
    assert_eq!(p.selected_admin_count(), 2);
    let rep = run(&m, &p);
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert_eq!(rep.moved.len(), 3);

    // Exactly one administrator command, made only of `mv -n` lines for the two items.
    let calls = admin_calls(&m);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let pairs = parse_admin_moves(&calls[0]).expect("only mv lines");
    let mut froms: Vec<PathBuf> = pairs.iter().map(|(a, _)| PathBuf::from(a)).collect();
    froms.sort();
    let mut want = vec![daemon.clone(), agent.clone()];
    want.sort();
    assert_eq!(froms, want);
    for (_, to) in &pairs {
        assert!(Path::new(to).starts_with(m.env.quarantine_dir()));
    }

    // Really moved, journaled with the administrator method.
    for p in [&daemon, &agent, &user_agent] {
        assert!(!p.exists(), "{} still there", p.display());
    }
    assert!(in_quarantine(&m, "com.shady.daemon.plist").is_some());
    let hist = executor::history(&m.env);
    let method_of = |orig: &Path| {
        hist.iter()
            .find_map(|r| match &r.kind {
                ActionKind::Moved {
                    original, method, ..
                } if original == orig => Some(*method),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(method_of(&daemon), Method::AdminQuarantine);
    assert_eq!(method_of(&agent), Method::AdminQuarantine);
    assert_eq!(method_of(&user_agent), Method::Quarantine);

    // Undo: the administrator items go back with one more administrator command.
    let (n, errs) = executor::undo_plan(&m.env, &p.id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, 3);
    for p in [&daemon, &agent, &user_agent] {
        assert!(p.exists(), "{} not back", p.display());
    }
    let calls = admin_calls(&m);
    assert_eq!(calls.len(), 2);
    let back = parse_admin_moves(&calls[1]).unwrap();
    assert_eq!(back.len(), 2);
    assert!(
        executor::history(&m.env)
            .iter()
            .filter(|r| r.plan_id == p.id && matches!(r.kind, ActionKind::Moved { .. }))
            .all(|r| r.undone.is_some())
    );
}

#[test]
fn cancelled_password_dialog_moves_nothing_and_says_so() {
    let (m, daemon, agent) = threat_mac();
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    select_only(&mut p, &[&daemon, &agent]);
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    let rep = run(&m, &p);
    assert!(rep.moved.is_empty());
    assert_eq!(rep.freed, 0);
    assert_eq!(rep.skipped.len(), 2);
    assert!(
        rep.skipped
            .iter()
            .all(|(_, r)| *r == SkipReason::AdminCancelled)
    );
    assert!(
        rep.skipped[0]
            .1
            .to_string()
            .contains("administrator password was not given")
    );
    assert!(daemon.exists() && agent.exists());
    assert!(
        !executor::history(&m.env)
            .iter()
            .any(|r| matches!(r.kind, ActionKind::Moved { .. }))
    );
}

#[test]
fn checks_run_before_the_admin_command_and_failures_are_found_on_disk() {
    let (m, daemon, agent) = threat_mac();
    let legacy = m.sys_file("/Library/StartupItems/OldThing/OldThing", 4_000);
    let legacy = legacy.parent().unwrap().to_path_buf();
    m.env.inject_fault(
        &m.env.sys_path("/Library/StartupItems"),
        Fault::PermissionDenied,
    );
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    assert!(item(&p, &legacy).needs_admin);
    select_only(&mut p, &[&daemon, &agent, &legacy]);
    let fs = m.env.fake_sys().unwrap();
    // The daemon is open in a process: never handed to the administrator command.
    fs.set_open(&daemon);
    // macOS refuses the legacy item even to the administrator.
    fs.fail_admin_move(&legacy);
    let rep = run(&m, &p);
    let calls = admin_calls(&m);
    assert_eq!(calls.len(), 1);
    let froms: Vec<String> = parse_admin_moves(&calls[0])
        .unwrap()
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    assert!(!froms.contains(&daemon.to_string_lossy().into_owned()));
    let reason = |p: &Path| {
        rep.skipped
            .iter()
            .find(|(s, _)| s == p)
            .map(|(_, r)| r.clone())
    };
    assert_eq!(reason(&daemon), Some(SkipReason::InUse));
    let r = reason(&legacy).expect("legacy reported").to_string();
    assert!(r.contains("Operation not permitted"), "{r}");
    assert!(legacy.exists() && daemon.exists());
    assert!(!agent.exists());
    assert_eq!(rep.moved.len(), 1);
    assert_eq!(
        rep.freed,
        rep.moved.iter().map(|r| r.size).sum::<u64>(),
        "freed counts only what moved"
    );
}

#[test]
fn awkward_names_survive_the_shell() {
    let m = tp::startup_mac();
    let name = "it's $HOME `x` \"q\"";
    let dir = m.env.sys_path("/Library/LaunchAgents");
    let plist = dir.join(format!("{name}.plist"));
    m.write(
        &plist,
        tp::launch_plist("com.genieo.odd", "/tmp/x", true).as_bytes(),
    );
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    assert!(item(&p, &plist).needs_admin);
    select_only(&mut p, &[&plist]);
    let rep = run(&m, &p);
    assert_eq!(rep.moved.len(), 1, "{:?}", rep.skipped);
    assert!(!plist.exists());
    assert!(in_quarantine(&m, &format!("{name}.plist")).is_some());
    let (n, errs) = executor::undo_plan(&m.env, &p.id);
    assert_eq!((n, errs.len()), (1, 0));
    assert!(plist.exists());
}

#[test]
fn without_the_feature_allowing_it_no_admin_command_runs() {
    let (m, daemon, _) = threat_mac();
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    select_only(&mut p, &[&daemon]);
    // The plain executor (what a feature without `allows_admin` gets) moves as the user.
    let rep = executor::execute(
        &m.env,
        &p,
        &features::allow_roots(&m.env, FeatureId::Malware),
        &p.digest(),
        Method::Quarantine,
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(
        rep.skipped,
        vec![(daemon.clone(), SkipReason::PermissionDenied)]
    );
    assert!(admin_calls(&m).is_empty());
    assert!(daemon.exists());
}

#[test]
fn the_fence_still_applies_to_admin_items() {
    let (m, daemon, _) = threat_mac();
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    // A plan edited to point an administrator item at a protected place is refused.
    let id = item(&p, &daemon).id;
    p.items[id].path = m
        .env
        .sys_path("/System/Library/LaunchDaemons/com.apple.x.plist");
    m.sys_file("/System/Library/LaunchDaemons/com.apple.x.plist", 10);
    select_only(&mut p, &[]);
    p.set_selected(id, true);
    let rep = run(&m, &p);
    assert!(matches!(rep.skipped[0].1, SkipReason::Refused(_)));
    assert!(admin_calls(&m).is_empty());
}

#[test]
fn undo_with_cancelled_dialog_keeps_the_item_in_quarantine() {
    let (m, daemon, _) = threat_mac();
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::Malware).unwrap();
    select_only(&mut p, &[&daemon]);
    let rep = run(&m, &p);
    assert_eq!(rep.moved.len(), 1);
    m.env.fake_sys().unwrap().with(|s| s.admin_cancelled = true);
    assert_eq!(
        executor::undo(&m.env, &rep.moved[0].id),
        Err(UndoError::AdminCancelled)
    );
    assert!(!daemon.exists());
    assert!(
        executor::history(&m.env)[0..]
            .iter()
            .all(|r| r.undone.is_none())
    );
    m.env
        .fake_sys()
        .unwrap()
        .with(|s| s.admin_cancelled = false);
    executor::undo(&m.env, &rep.moved[0].id).unwrap();
    assert!(daemon.exists());
}

#[test]
fn package_installed_app_and_system_leftovers_uninstall_with_the_password() {
    let m = FakeMac::new();
    let app = m.app("Pkg Tool", "com.example.pkgtool", "1.0");
    let sys_left = m.sys_file(
        "/Library/Application Support/com.example.pkgtool/data",
        20_000,
    );
    let sys_left = sys_left.parent().unwrap().to_path_buf();
    let user_left = m.home_file("Library/Caches/com.example.pkgtool/c", 5_000);
    let user_left = user_left.parent().unwrap().to_path_buf();
    // Root-owned bundle (installed by a .pkg) and a root-owned /Library folder.
    m.env.inject_fault(&app, Fault::PermissionDenied);
    m.env.inject_fault(
        &m.env.sys_path("/Library/Application Support"),
        Fault::PermissionDenied,
    );
    m.env.fake_sys().unwrap().answer("mdfind", "");
    let p = uninstaller::uninstall_plan(&m.env, &Settings::default(), &app, false).unwrap();
    assert!(item(&p, &app).needs_admin && item(&p, &app).locked.is_none());
    assert!(item(&p, &sys_left).needs_admin && item(&p, &sys_left).selected);
    assert!(!item(&p, &user_left).needs_admin);
    let rep = run(&m, &p);
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert!(!app.exists() && !sys_left.exists() && !user_left.exists());
    assert_eq!(admin_calls(&m).len(), 1);
    assert_eq!(parse_admin_moves(&admin_calls(&m)[0]).unwrap().len(), 2);
    let (n, errs) = executor::undo_plan(&m.env, &p.id);
    assert_eq!((n, errs.len()), (3, 0), "{errs:?}");
    assert!(app.exists() && sys_left.exists() && user_left.exists());
}

#[test]
fn system_caches_need_the_password_and_are_not_preselected() {
    let m = kit::junk_mac();
    let caches = m.env.sys_path("/Library/Caches");
    m.env.inject_fault(&caches, Fault::PermissionDenied);
    let p = features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
    let sys: Vec<usize> = p
        .items
        .iter()
        .filter(|i| i.category == "system_caches")
        .map(|i| i.id)
        .collect();
    assert!(!sys.is_empty());
    for &i in &sys {
        let it = &p.items[i];
        assert!(
            it.needs_admin && it.locked.is_none() && !it.selected,
            "{it:#?}"
        );
    }
    // The default clean never asks for the password.
    let rep = run(&m, &p);
    assert!(admin_calls(&m).is_empty());
    assert!(rep.moved.iter().all(|r| !matches!(
        r.kind,
        ActionKind::Moved {
            method: Method::AdminQuarantine,
            ..
        }
    )));
    // Selected by hand: one administrator command, into the quarantine whatever the setting.
    let mut p = features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
    for i in 0..p.items.len() {
        p.set_selected(i, p.items[i].category == "system_caches");
    }
    let n = p.selected_count();
    let rep = run(&m, &p);
    assert_eq!(rep.moved.len(), n, "{:?}", rep.skipped);
    assert_eq!(admin_calls(&m).len(), 1);
    assert!(rep.moved.iter().all(|r| matches!(
        r.kind,
        ActionKind::Moved {
            method: Method::AdminQuarantine,
            ..
        }
    )));
}
