//! Duplicates on a fake home folder: exact sets, hard links, keep rules and the rule that one
//! copy of every set always stays.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use cleancraft_core::features::duplicates::{
    self, KeepRule, apply_keep_rule, keep_one_per_group, would_select_every_copy,
};
use cleancraft_core::features::large_old::Location;
use cleancraft_core::features::{self, ScanCtx};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Plan, Settings};

/// Varied bytes from a seed.
fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

fn put(mac: &FakeMac, rel: &str, data: &[u8]) -> PathBuf {
    let p = mac.env.home_path(rel);
    mac.write(&p, data);
    p
}

fn scan(mac: &FakeMac) -> Plan {
    let s = Settings::default();
    let cancel = AtomicBool::new(false);
    let ctx = ScanCtx {
        env: &mac.env,
        settings: &s,
        cancel: &cancel,
        progress: &|_| {},
        meter: &cleancraft_core::progress::Progress::new(),
    };
    duplicates::scan_location(&ctx, &Location::Home).unwrap()
}

fn sets(plan: &Plan) -> BTreeSet<BTreeSet<PathBuf>> {
    duplicates::groups(plan)
        .into_values()
        .map(|ids| ids.iter().map(|i| plan.items[*i].path.clone()).collect())
        .collect()
}

fn kept(plan: &Plan) -> BTreeSet<PathBuf> {
    plan.items
        .iter()
        .filter(|i| !i.selected)
        .map(|i| i.path.clone())
        .collect()
}

struct Fixture {
    mac: FakeMac,
    a: [PathBuf; 3],
    b: [PathBuf; 2],
    e: [PathBuf; 2],
    e_link: PathBuf,
}

fn fixture() -> Fixture {
    let mac = FakeMac::new();
    let da = bytes(1, 300_000);
    let a = [
        put(&mac, "Projects/a.bin", &da),
        put(&mac, "Downloads/a.bin", &da),
        put(&mac, "Music/old/a.bin", &da),
    ];
    // Also in Documents: protected, never proposed.
    put(&mac, "Documents/a.bin", &da);
    let db = bytes(2, 200_000);
    let b = [
        put(&mac, "Pictures/b1.jpg", &db),
        put(&mac, "Pictures/b2.jpg", &db),
    ];
    age(&b[0], 5);
    age(&b[1], 10);
    // Same length, same first 64 KiB, different end: not duplicates.
    let mut c1 = bytes(3, 150_000);
    put(&mac, "Projects/c1.dat", &c1);
    *c1.last_mut().unwrap() ^= 0xFF;
    put(&mac, "Projects/c2.dat", &c1);
    // Too small (100 KB or less).
    let dd = bytes(4, 100_000);
    put(&mac, "Projects/d1.bin", &dd);
    put(&mac, "Projects/d2.bin", &dd);
    // A hard link is the same file, not a copy.
    let de = bytes(5, 120_000);
    let e = [
        put(&mac, "Movies/e copy.bin", &de),
        put(&mac, "Projects/e.bin", &de),
    ];
    let e_link = mac.env.home_path("Projects/e_link.bin");
    fs::hard_link(&e[1], &e_link).unwrap();
    // Hidden and Library copies are never looked at.
    put(&mac, ".cache/a.bin", &da);
    put(&mac, "Library/Caches/a.bin", &da);
    Fixture {
        mac,
        a,
        b,
        e,
        e_link,
    }
}

#[test]
fn exact_sets() {
    let f = fixture();
    let plan = scan(&f.mac);
    let want: BTreeSet<BTreeSet<PathBuf>> = [
        f.a.iter().cloned().collect(),
        f.b.iter().cloned().collect(),
        f.e.iter().cloned().collect(),
    ]
    .into_iter()
    .collect();
    assert_eq!(sets(&plan), want);
    assert!(!plan.items.iter().any(|i| i.path == f.e_link));
    for it in &plan.items {
        assert_eq!(it.size, tree_size(&it.path).0);
        assert!(it.group.is_some());
    }
    // Sets never split across categories.
    for ids in duplicates::groups(&plan).values() {
        let cats: BTreeSet<_> = ids.iter().map(|i| &plan.items[*i].category).collect();
        assert_eq!(cats.len(), 1);
    }
    // Biggest waste first.
    assert_eq!(plan.items[0].group.as_deref(), Some("dup-0001"));
    assert!(f.a.contains(&plan.items[0].path));
}

#[test]
fn smart_keeps_one_per_set() {
    let f = fixture();
    let plan = scan(&f.mac);
    // a: shortest path outside Downloads. b: same length, the older one. e: shortest path.
    let want: BTreeSet<PathBuf> = [f.a[0].clone(), f.b[1].clone(), f.e[1].clone()]
        .into_iter()
        .collect();
    assert_eq!(kept(&plan), want);
    assert_eq!(plan.selected_count(), 2 + 1 + 1);
}

#[test]
fn keep_newest_and_oldest() {
    let f = fixture();
    let mut plan = scan(&f.mac);
    apply_keep_rule(&f.mac.env, &mut plan, KeepRule::Newest);
    assert!(kept(&plan).contains(&f.b[0]));
    assert!(!kept(&plan).contains(&f.b[1]));
    assert_eq!(kept(&plan).len(), 3);
    apply_keep_rule(&f.mac.env, &mut plan, KeepRule::Oldest);
    assert!(kept(&plan).contains(&f.b[1]));
    assert_eq!(kept(&plan).len(), 3);
}

#[test]
fn every_copy_selected_is_refused() {
    let f = fixture();
    let s = Settings::default();
    let mut plan = scan(&f.mac);
    let b_ids: Vec<usize> = plan
        .items
        .iter()
        .filter(|i| f.b.contains(&i.path))
        .map(|i| i.id)
        .collect();
    let keeper = *b_ids.iter().find(|i| !plan.items[**i].selected).unwrap();
    assert!(would_select_every_copy(&plan, keeper));
    plan.set_selected(keeper, true);
    let err = features::run_plan(&f.mac.env, &s, &plan, &plan.digest(), &mut |_, _| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("Keep at least one copy"), "{err}");
    for p in f.a.iter().chain(&f.b).chain(&f.e) {
        assert!(p.exists(), "{} moved", p.display());
    }
    // The fix-up deselects the Smart keeper again.
    assert_eq!(keep_one_per_group(&f.mac.env, &mut plan), 1);
    assert!(!plan.items[keeper].selected);
    // A keeper that vanished does not count as kept.
    let plan2 = scan(&f.mac);
    let k = plan2
        .items
        .iter()
        .find(|i| f.e.contains(&i.path) && !i.selected)
        .unwrap()
        .path
        .clone();
    fs::rename(&k, f.mac.env.home_path("moved-away.bin")).unwrap();
    plan2.digest();
    let err = features::run_plan(&f.mac.env, &s, &plan2, &plan2.digest(), &mut |_, _| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("Keep at least one copy"), "{err}");
}

#[test]
fn smart_selection_moves_only_extra_copies() {
    let f = fixture();
    let s = Settings::default();
    let plan = scan(&f.mac);
    let rep = features::run_plan(&f.mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 4);
    assert!(f.a[0].exists() && !f.a[1].exists() && !f.a[2].exists());
    assert!(f.b[1].exists() && !f.b[0].exists());
    assert!(f.e[1].exists() && f.e_link.exists() && !f.e[0].exists());
    let (n, errs) = cleancraft_core::executor::undo_plan(&f.mac.env, &plan.id);
    assert_eq!((n, errs.len()), (4, 0));
    assert!(f.a.iter().all(|p| p.exists()));
}

#[test]
fn moved_files_are_matched_by_content_only() {
    // Names differ, content equal: still one set.
    let mac = FakeMac::new();
    let d = bytes(9, 210_000);
    let x = put(&mac, "Projects/holiday.mov", &d);
    let y = put(&mac, "Movies/IMG_0001.MOV", &d);
    let plan = scan(&mac);
    assert_eq!(
        sets(&plan),
        [[x, y].into_iter().collect()].into_iter().collect()
    );
    assert!(Path::new(&plan.items[0].path).exists());
}
