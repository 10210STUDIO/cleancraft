//! Disk Map core: sizes against a recalculation, hard links once, links not followed,
//! refresh after a move, locked places, and the removal plan's own check.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use cleancraft_core::features::space_lens::{SpaceLens, plan_for};
use cleancraft_core::features::{self, Feature};
use cleancraft_core::fsutil::{entry_size, tree_size};
use cleancraft_core::space::{self, Tree, lock_reason};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Settings};

fn build(mac: &FakeMac) -> Tree {
    let never = AtomicBool::new(false);
    space::build(mac.env.home(), &never, &|_, _| {})
}

fn sample() -> FakeMac {
    let mac = FakeMac::sample();
    mac.home_file("Movies/film.mov", 3_000_000);
    mac.home_file("Projects/app/src/main.rs", 40_000);
    mac.home_file("Projects/app/target/big.bin", 1_200_000);
    mac.home_file("Downloads/setup.dmg", 800_000);
    mac
}

#[test]
fn sizes_match_a_recalculation() {
    let mac = sample();
    let t = build(&mac);
    let root = t.root.as_ref().unwrap();
    assert_eq!(root.size, tree_size(mac.env.home()).0);
    for c in &root.children {
        assert_eq!(c.size, tree_size(&c.path).0, "{}", c.name);
    }
    // Biggest first.
    assert!(root.children.windows(2).all(|w| w[0].size >= w[1].size));
    let app = t.find(&mac.env.home_path("Projects/app")).unwrap();
    assert_eq!(app.files, 2);
    assert_eq!(app.size, tree_size(&app.path).0);
}

#[test]
fn hard_links_once_and_links_not_followed() {
    let mac = FakeMac::new();
    let a = mac.home_file("Data/a.bin", 200_000);
    fs::hard_link(&a, mac.env.home_path("Data/b.bin")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(mac.env.home_path("Data"), mac.env.home_path("Shortcut")).unwrap();
    let t = build(&mac);
    let data = t.find(&mac.env.home_path("Data")).unwrap();
    // tree_size counts a hard-linked file once too (DAT-3): the same formula everywhere.
    let once = entry_size(&fs::symlink_metadata(mac.env.home_path("Data")).unwrap())
        + entry_size(&fs::metadata(&a).unwrap());
    assert_eq!(data.size, once);
    assert_eq!(data.size, tree_size(&mac.env.home_path("Data")).0);
    #[cfg(unix)]
    {
        let link = t.find(&mac.env.home_path("Shortcut")).unwrap();
        assert!(!link.is_dir && link.children.is_empty());
        assert!(link.size < 10_000);
    }
}

#[test]
fn refresh_after_move_and_back() {
    let mac = sample();
    let mut t = build(&mac);
    let film = mac.env.home_path("Movies/film.mov");
    let plan = plan_for(&mac.env, &[(film.clone(), t.find(&film).unwrap().size)]);
    let s = Settings::default();
    let rep = features::run_plan(&mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 1);
    t.refresh_after(&mac.env, &rep.moved);
    assert_eq!(
        flat(&t),
        flat(&build(&mac)),
        "patched tree differs from a new scan"
    );
    assert!(t.find(&film).is_none());
    cleancraft_core::executor::undo_plan(&mac.env, &plan.id);
    t.refresh_after(&mac.env, &rep.moved);
    assert_eq!(flat(&t), flat(&build(&mac)));
    assert!(t.find(&film).is_some());
}

fn flat(t: &Tree) -> Vec<(PathBuf, u64, u64)> {
    fn go(n: &space::Node, out: &mut Vec<(PathBuf, u64, u64)>) {
        out.push((n.path.clone(), n.size, n.files));
        for c in &n.children {
            go(c, out);
        }
    }
    let mut v = Vec::new();
    go(t.root.as_ref().unwrap(), &mut v);
    v
}

#[test]
fn locked_places() {
    let mac = sample();
    let env = &mac.env;
    for rel in [
        "Documents",
        "Documents/report.pdf",
        "Desktop",
        "Library",
        "Library/Caches/com.example.Browser",
        ".Trash",
        ".ssh",
        "Applications/Tool.app",
        "Applications/Tool.app/Contents",
        "Pictures/Photos Library.photoslibrary",
    ] {
        assert!(
            lock_reason(env, &env.home_path(rel)).is_some(),
            "{rel} not locked"
        );
    }
    assert!(lock_reason(env, env.home()).is_some());
    assert!(lock_reason(env, &env.sys_path("/Applications")).is_some());
    for rel in [
        "Movies",
        "Movies/film.mov",
        "Projects/app/target",
        "Downloads/setup.dmg",
    ] {
        assert_eq!(lock_reason(env, &env.home_path(rel)), None, "{rel}");
    }
}

#[test]
fn removal_plan_refuses_locked_items() {
    let mac = sample();
    let env = &mac.env;
    let docs = env.home_path("Documents/report.pdf");
    let target = env.home_path("Projects/app/target");
    let mut plan = plan_for(
        env,
        &[
            (docs.clone(), 1),
            (target.clone(), 2),
            (target.join("big.bin"), 3),
        ],
    );
    // Nested selection collapses to the folder; the protected file is locked.
    let ps: Vec<PathBuf> = plan.items.iter().map(|i| i.path.clone()).collect();
    assert_eq!(ps, vec![docs.clone(), target.clone()]);
    assert!(plan.items[0].locked.is_some());
    assert_eq!(plan.selected_count(), 1);
    // Forcing the lock off is caught before anything moves.
    plan.items[0].locked = None;
    plan.items[0].selected = true;
    let s = Settings::default();
    let err = features::run_plan(env, &s, &plan, &plan.digest(), &mut |_, _| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot be removed"), "{err}");
    assert!(docs.exists() && target.exists());
    // The registered scan lists the home folder's top level, nothing selected.
    let p = features::scan(env, &s, FeatureId::SpaceLens).unwrap();
    assert_eq!(p.selected_count(), 0);
    assert!(p.items.iter().any(|i| i.path == env.home_path("Movies")));
    assert!(
        p.items
            .iter()
            .find(|i| i.path == env.home_path("Library"))
            .unwrap()
            .locked
            .is_some()
    );
    assert_eq!(SpaceLens.allow_roots(env), vec![env.home().to_path_buf()]);
}

#[test]
fn disk_use_from_df() {
    let mac = FakeMac::new();
    let fake = mac.env.fake_sys().unwrap();
    // No df answer: only the free space is known.
    let u = space::disk_use(&mac.env, mac.env.home());
    assert_eq!((u.total, u.free), (None, Some(100_000_000_000)));
    fake.answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 1000000 600000 400000 60% /\n",
    );
    let u = space::disk_use(&mac.env, mac.env.home());
    assert_eq!(u.total, Some(1_024_000_000));
    assert_eq!(u.used, Some(614_400_000));
    assert_eq!(u.free, Some(409_600_000));
}
