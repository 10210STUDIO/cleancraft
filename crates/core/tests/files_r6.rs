//! Round 6 file features on a fake Mac: Duplicate Downloads for Quick Sweep, Large & Old size
//! and age groups, Disk Map locations (startup disk, external volumes from df/diskutil),
//! unreadable folders, the allow-list (locked outside it, also in an edited plan), external
//! volume removal with undo, hard links once, firmlinks not counted twice, snapshots.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use cleancraft_core::executor;
use cleancraft_core::features::duplicates::{self, DownloadDuplicates};
use cleancraft_core::features::large_old::{self, GroupBy, ScanOptions};
use cleancraft_core::features::space_lens::{self, SpaceLens};
use cleancraft_core::features::{self, Feature, ScanCtx};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::plan::{Item, Risk};
use cleancraft_core::space::{self, MapLocation};
use cleancraft_core::space_snapshots;
use cleancraft_core::sweep::{self, Sources};
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId, Plan, Settings};

fn same_file(m: &FakeMac, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = m.env.home_path(rel);
    m.write(&p, bytes);
    p
}

fn blob(seed: u8, n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

fn ctx_scan(m: &FakeMac, f: &dyn Feature) -> Plan {
    let s = Settings::default();
    let cancel = AtomicBool::new(false);
    let ctx = ScanCtx {
        env: &m.env,
        settings: &s,
        cancel: &cancel,
        progress: &|_| {},
    };
    features::scan_with(f, &ctx).unwrap()
}

/// Downloads with a recent duplicate set (3 copies), an old one (left out) and a copy outside
/// Downloads (left out).
pub fn downloads_mac() -> (FakeMac, Vec<PathBuf>, Vec<PathBuf>) {
    let m = FakeMac::new();
    let a = blob(1, 300_000);
    let recent = vec![
        same_file(&m, "Downloads/report.pdf", &a),
        same_file(&m, "Downloads/report (1).pdf", &a),
        same_file(&m, "Downloads/More/report copy.pdf", &a),
    ];
    let b = blob(2, 250_000);
    let old = vec![
        same_file(&m, "Downloads/old.zip", &b),
        same_file(&m, "Downloads/old (1).zip", &b),
    ];
    for p in &old {
        age(p, 200);
    }
    same_file(&m, "Movies/report.pdf", &a);
    // Downloads in progress are never candidates.
    same_file(&m, "Downloads/report.pdf.crdownload", &a);
    same_file(&m, "Downloads/report 2.pdf.download/report 2.pdf", &a);
    (m, recent, old)
}

#[test]
fn duplicate_downloads_recent_only_one_kept() {
    let (m, recent, old) = downloads_mac();
    let p = ctx_scan(&m, &DownloadDuplicates);
    assert_eq!(p.feature, Some(FeatureId::Duplicates));
    let paths: Vec<&PathBuf> = p.items.iter().map(|i| &i.path).collect();
    for r in &recent {
        assert!(paths.contains(&r), "{}", r.display());
    }
    for o in &old {
        assert!(!paths.contains(&o), "older than 3 months: {}", o.display());
    }
    assert!(
        !paths
            .iter()
            .any(|p| p.starts_with(m.env.home_path("Movies")))
    );
    assert!(!paths.iter().any(|p| duplicates::in_progress(p)));
    assert_eq!(p.items.len(), 3);
    // One copy of the set kept: the shortest path.
    assert_eq!(p.selected_count(), 2);
    let kept = p.items.iter().find(|i| !i.selected).unwrap();
    assert_eq!(kept.path, recent[0]);
    let sel: u64 = p.selected().map(|i| tree_size(&i.path).0).sum();
    assert_eq!(p.selected_size(), sel);
    assert!(p.notes.iter().any(|n| n.contains("last 3 months")));
}

#[test]
fn duplicate_downloads_tile_runs_keeps_one_and_undoes() {
    let (m, recent, _) = downloads_mac();
    let src = Sources {
        plans: vec![std::sync::Arc::new(DownloadDuplicates)],
        counts: vec![],
    };
    let s = Settings::default();
    let r = sweep::scan(&m.env, &s, &src, &AtomicBool::new(false), &|_| {});
    let tile = r.tile(FeatureId::Duplicates).unwrap();
    assert_eq!(sweep::tile_title(tile.id), "Duplicate Downloads");
    assert!(tile.runnable());
    let plan = tile.plan().unwrap().clone();
    let runs = sweep::run(&m.env, &s, &src, &[(plan.clone(), plan.digest())], &|_| {});
    let rep = runs[0].result.as_ref().unwrap();
    assert_eq!(rep.moved.len(), 2);
    assert!(recent[0].exists(), "the kept copy stays");
    assert!(!recent[1].exists() && !recent[2].exists());
    executor::undo_plan(&m.env, &rep.plan_id);
    assert!(recent.iter().all(|p| p.exists()));
    // An edited plan selecting every copy is refused: nothing moves.
    let p = ctx_scan(&m, &DownloadDuplicates);
    let mut all = p.clone();
    for it in all.items.iter_mut() {
        it.selected = true;
    }
    let d = all.digest();
    assert!(
        features::run_plan_with(&DownloadDuplicates, &m.env, &s, &all, &d, &mut |_, _| {}).is_err()
    );
    assert!(recent.iter().all(|p| p.exists()));
}

#[test]
fn registered_sweep_has_duplicate_downloads_and_late_modules() {
    let s = Sources::registered();
    assert!(s.feature(FeatureId::Duplicates).is_some());
    let ids = sweep::plan_module_ids();
    assert!(ids.contains(&FeatureId::Duplicates));
    // Developer Clutter is added by name when its module exists in the build.
    assert_eq!(
        ids.iter().any(|i| i.as_str() == "dev_clutter"),
        FeatureId::parse("dev_clutter").is_some()
    );
}

fn item(path: &str, size: u64, opened_days_ago: Option<i64>, now: i64) -> Item {
    Item {
        id: 0,
        path: PathBuf::from(path),
        size,
        category: "other".into(),
        reason: String::new(),
        risk: Risk::High,
        selected: false,
        locked: None,
        group: None,
        modified: opened_days_ago.map(|d| now - d * 86_400),
        needs_admin: false,
    }
}

#[test]
fn size_and_age_groups_math() {
    let now = 2_000_000_000;
    let mut p = Plan::new(FeatureId::LargeOldFiles);
    let specs = [
        ("/h/a.zip", 49_999_999, Some(10)),
        ("/h/b.zip", 50_000_000, Some(89)),
        ("/h/c.mov", 999_999_999, Some(90)),
        ("/h/d.mov", 1_000_000_000, Some(179)),
        ("/h/e.dmg", 4_999_999_999, Some(180)),
        ("/h/f.dmg", 5_000_000_000, Some(364)),
        ("/h/g.iso", 9_000_000_000, Some(365)),
        ("/h/h.bin", 2_000_000, None),
    ];
    for (n, (path, size, days)) in specs.iter().enumerate() {
        let mut it = item(path, *size, *days, now);
        it.id = n;
        p.items.push(it);
    }
    large_old::regroup(&mut p, GroupBy::Size, now);
    let cats: Vec<&str> = p.items.iter().map(|i| i.category.as_str()).collect();
    assert_eq!(
        cats,
        [
            "size_under_50mb",
            "size_50mb_1gb",
            "size_50mb_1gb",
            "size_1gb_5gb",
            "size_1gb_5gb",
            "size_over_5gb",
            "size_over_5gb",
            "size_under_50mb"
        ]
    );
    assert_eq!(large_old::grouped_by(&p), GroupBy::Size);
    assert_eq!(p.categories[0].title, "50 MB to 1 GB");
    // Each group selects all or none of its own items only.
    p.set_category_selected("size_1gb_5gb", true);
    assert_eq!(p.selected_count(), 2);
    assert_eq!(p.selected_size(), 1_000_000_000 + 4_999_999_999);
    p.set_category_selected("size_1gb_5gb", false);
    assert_eq!(p.selected_count(), 0);
    large_old::regroup(&mut p, GroupBy::Age, now);
    let cats: Vec<&str> = p.items.iter().map(|i| i.category.as_str()).collect();
    assert_eq!(
        cats,
        [
            "age_recent",
            "age_recent",
            "age_3_6_months",
            "age_3_6_months",
            "age_6_12_months",
            "age_6_12_months",
            "age_over_year",
            "age_unknown"
        ]
    );
    assert_eq!(large_old::grouped_by(&p), GroupBy::Age);
    // Regrouping keeps what is ticked.
    p.set_category_selected("age_over_year", true);
    large_old::regroup(&mut p, GroupBy::Kind, now);
    assert_eq!(p.selected_count(), 1);
    assert_eq!(p.items[6].category, "disk_images");
    assert_eq!(large_old::grouped_by(&p), GroupBy::Kind);
}

#[test]
fn large_old_scan_uses_saved_grouping_and_last_opened() {
    let m = FakeMac::new();
    let f = m.home_file("Downloads/big.bin", 2_500_000);
    age(&f, 200);
    let mut o = ScanOptions::load(&m.env);
    o.large_old_group = GroupBy::Age;
    o.save(&m.env).unwrap();
    let s = Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    };
    let p = features::scan(&m.env, &s, FeatureId::LargeOldFiles).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].category, "age_6_12_months");
    assert_eq!(p.selected_count(), 0, "nothing preselected");
}

const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1s1 971350180 10000000 500000000 3% /
devfs 200 200 0 100% /dev
/dev/disk3s5 971350180 400000000 500000000 45% /System/Volumes/Data
/dev/disk3s2 971350180 1000000 500000000 1% /System/Volumes/Preboot
/dev/disk5s1 1953514584 953514584 1000000000 49% /Volumes/My Backup
/dev/disk6s1 500000 100000 400000 20% /Volumes/Installer
/dev/disk7s1 300000 1000 299000 1% /Volumes/.timemachine/abc
/dev/disk8s1 400000 1000 399000 1% /Volumes/Recovery
map auto_home 0 0 0 100% /System/Volumes/Data/home
//me@nas/share 100 50 50 50% /Volumes/share
";

fn diskutil(disks: &[&str], mounts: &[(&str, &str)]) -> String {
    let all: String = disks
        .iter()
        .map(|d| format!("<string>{d}</string>"))
        .collect();
    let parts: String = mounts
        .iter()
        .map(|(m, n)| {
            format!(
                "<dict><key>DeviceIdentifier</key><string>disk5s1</string><key>MountPoint</key><string>{m}</string><key>VolumeName</key><string>{n}</string></dict>"
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>AllDisks</key><array>{all}</array><key>AllDisksAndPartitions</key><array><dict><key>DeviceIdentifier</key><string>disk4</string><key>APFSVolumes</key><array>{parts}</array></dict></array></dict></plist>"#
    )
}

pub fn volumes_mac() -> FakeMac {
    let m = FakeMac::new();
    for v in ["My Backup", "Installer", "Recovery", "share"] {
        std::fs::create_dir_all(m.env.sys_path(&format!("/Volumes/{v}"))).unwrap();
    }
    let f = m.env.fake_sys().unwrap();
    f.answer("/bin/df -Pk", DF);
    f.answer(
        "/usr/sbin/diskutil list -plist external",
        &diskutil(
            &["disk4", "disk5", "disk5s1"],
            &[("/Volumes/My Backup", "My Backup")],
        ),
    );
    m
}

#[test]
fn df_lines_with_spaces() {
    let v = space::parse_df_all(DF);
    let b = v.iter().find(|l| l.mount == "/Volumes/My Backup").unwrap();
    assert_eq!(b.device, "/dev/disk5s1");
    assert_eq!(b.total, 1_953_514_584 * 1024);
    assert_eq!(b.free, 1_000_000_000 * 1024);
}

#[test]
fn location_picker_lists_external_volumes_only() {
    let m = volumes_mac();
    let vols = space::external_volumes(&m.env);
    // diskutil says only disk5 is external: the disk image "Installer", Recovery, hidden
    // Time Machine and the network share are left out.
    assert_eq!(vols.len(), 1, "{vols:?}");
    assert_eq!(vols[0].name, "My Backup");
    assert_eq!(vols[0].mount, m.env.sys_path("/Volumes/My Backup"));
    let locs = space::locations(&m.env);
    assert_eq!(locs[0], MapLocation::Home);
    assert_eq!(locs[1], MapLocation::Startup);
    assert!(matches!(&locs[2], MapLocation::Volume(v) if v.name == "My Backup"));
    // Without diskutil: every /dev/disk mount under /Volumes that is not hidden or macOS's.
    m.env.fake_sys().unwrap().with(|s| {
        s.answers.remove("/usr/sbin/diskutil list -plist external");
    });
    let names: Vec<String> = space::external_volumes(&m.env)
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(names, ["Installer", "My Backup"]);
    // No df: no volumes (never a guess).
    let m = FakeMac::new();
    assert!(space::external_volumes(&m.env).is_empty());
}

fn startup_mac() -> FakeMac {
    let m = FakeMac::new();
    m.home_file("Movies/clip.mov", 400_000);
    m.sys_file("/Library/Caches/sys.cache", 100_000);
    m.sys_file("/Applications/Tool.app/Contents/MacOS/tool", 200_000);
    // The Data volume seen again through its mount point: must not count twice.
    m.sys_file("/System/Volumes/Data/Users/tester/Movies/clip.mov", 400_000);
    m.sys_file("/Volumes/USB/big.bin", 900_000);
    m
}

#[test]
fn startup_disk_counts_each_file_once() {
    let m = startup_mac();
    // A hard link in another top folder: counted once.
    let clip = m.env.home_path("Movies/clip.mov");
    std::fs::create_dir_all(m.env.sys_path("/opt")).unwrap();
    std::fs::hard_link(&clip, m.env.sys_path("/opt/clip-link.mov")).unwrap();
    let t = space::build_location(
        &m.env,
        &MapLocation::Startup,
        &AtomicBool::new(false),
        &|_, _| {},
    );
    let root = t.root.as_ref().unwrap();
    assert!(t.find(&m.env.sys_path("/System/Volumes/Data")).is_none());
    assert!(t.find(&m.env.sys_path("/Volumes")).is_none());
    assert!(t.find(&m.env.sys_path("/System/Volumes")).is_none());
    assert!(t.find(&m.env.sys_path("/System")).is_some());
    let files = |p: &str| tree_size(&m.env.sys_path(p)).0;
    let want = tree_size(m.env.root()).0 - files("/System/Volumes") - files("/Volumes");
    assert_eq!(root.size, want, "every file once, skipped places left out");
    let link = t.find(&m.env.sys_path("/opt/clip-link.mov")).unwrap();
    let orig = t.find(&clip).unwrap();
    assert_eq!(
        link.size + orig.size,
        tree_size(&clip).0,
        "hard link counted once"
    );
}

#[test]
fn unreadable_folders_are_reported_never_zero() {
    let m = startup_mac();
    let secret = m.env.home_path("Private");
    m.home_file("Private/a.bin", 300_000);
    m.env.inject_fault(&secret, Fault::PermissionDenied);
    let lib = m.env.sys_path("/Library/Caches");
    m.env.inject_fault(&lib, Fault::PermissionDenied);
    let t = space::build_location(
        &m.env,
        &MapLocation::Startup,
        &AtomicBool::new(false),
        &|_, _| {},
    );
    assert_eq!(t.unreadable, 2);
    assert!(t.unreadable_paths.contains(&secret));
    assert!(t.unreadable_paths.contains(&lib));
    assert!(t.find(&secret).unwrap().unreadable);
    // The whole disk: what was not measured is shown as a total.
    let used = t.root.as_ref().unwrap().size + 5_000_000;
    let d = space::DiskUse {
        total: Some(used * 2),
        used: Some(used),
        free: Some(used),
    };
    assert_eq!(space::not_measured(&t, &d), Some(5_000_000));
    // A subtree keeps its unreadable folders.
    let sub = t.subtree(m.env.home()).unwrap();
    assert_eq!(sub.unreadable_paths, vec![secret]);
}

#[test]
fn lock_reasons_outside_the_allow_list() {
    let m = volumes_mac();
    let env = &m.env;
    let r = |p: PathBuf| space::lock_reason(env, &p);
    assert_eq!(
        r(env.sys_path("/Library/Caches")).as_deref(),
        Some("part of macOS")
    );
    assert_eq!(r(env.sys_path("/System")).as_deref(), Some("part of macOS"));
    assert_eq!(
        r(env.sys_path("/private/var/log")).as_deref(),
        Some("part of macOS")
    );
    assert_eq!(
        r(env.sys_path("/Applications/Tool.app")).as_deref(),
        Some("outside your home folder")
    );
    assert_eq!(
        r(env.sys_path("/Users/other/file")).as_deref(),
        Some("outside your home folder")
    );
    let vol = env.sys_path("/Volumes/My Backup");
    assert_eq!(r(vol.clone()).as_deref(), Some("the disk itself"));
    for (name, _) in space::VOLUME_SYSTEM {
        assert!(
            r(vol.join(name)).is_some_and(|w| w.starts_with("system data")),
            "{name}"
        );
        assert!(r(vol.join(name).join("x")).is_some(), "{name}/x");
    }
    assert!(r(vol.join(".hidden")).is_some());
    assert!(r(vol.join("Old.app")).is_some());
    // Only what is inside the top user folders: the folders and top files stay.
    std::fs::create_dir_all(vol.join("Movies")).unwrap();
    m.sys_file("/Volumes/My Backup/top.bin", 1000);
    assert!(r(vol.join("Movies")).is_some_and(|w| w.contains("top folder")));
    assert!(r(vol.join("top.bin")).is_some());
    assert_eq!(r(vol.join("Movies/clip.mov")), None);
    assert_eq!(
        space::volume_user_folders(&vol),
        vec![vol.join("Movies")],
        "never the disk, its hidden or system folders"
    );
    assert!(
        !SpaceLens
            .allow_roots(env)
            .iter()
            .any(|a| *a == vol || a.ends_with("Volumes"))
    );
    assert_eq!(r(env.home_path("Movies")), None);
}

#[test]
fn edited_plan_cannot_remove_outside_the_allow_list() {
    let m = volumes_mac();
    let sys = m.sys_file("/Library/Caches/sys.cache", 100_000);
    let other = m.sys_file("/Users/other/file.bin", 100_000);
    let spot = m.sys_file("/Volumes/My Backup/.Spotlight-V100/store.db", 100_000);
    let tm = m.sys_file("/Volumes/My Backup/Backups.backupdb/x/y.bin", 100_000);
    let mine = m.home_file("Movies/keep.mov", 100_000);
    // A plan edited by hand: everything selected, nothing locked.
    let mut plan = Plan::new(FeatureId::SpaceLens);
    plan.add_category(space_lens::CATEGORY, "x", "x", Risk::High);
    for p in [&sys, &other, &spot, &tm] {
        plan.push(
            p.clone(),
            100_000,
            space_lens::CATEGORY,
            "x",
            Risk::High,
            true,
        );
    }
    let d = plan.digest();
    let s = Settings::default();
    let rep = features::run_plan_with(&SpaceLens, &m.env, &s, &plan, &d, &mut |_, _| {})
        .unwrap_or_default();
    assert!(rep.moved.is_empty());
    for p in [&sys, &other, &spot, &tm] {
        assert!(p.exists(), "{} must stay", p.display());
    }
    // plan_for locks them as the map does.
    let p = space_lens::plan_for(&m.env, &[(sys.clone(), 1), (mine.clone(), 1)]);
    assert!(
        p.items
            .iter()
            .find(|i| i.path == sys)
            .unwrap()
            .locked
            .is_some()
    );
    assert!(
        p.items
            .iter()
            .find(|i| i.path == mine)
            .unwrap()
            .locked
            .is_none()
    );
}

#[test]
fn external_volume_user_folder_moves_to_its_trash_and_back() {
    let m = volumes_mac();
    let movie = m.sys_file("/Volumes/My Backup/Movies/old.mov", 500_000);
    let vol = m.env.sys_path("/Volumes/My Backup");
    assert!(SpaceLens.allow_roots(&m.env).contains(&vol.join("Movies")));
    assert!(!SpaceLens.allow_roots(&m.env).contains(&vol));
    let p = space_lens::plan_for(&m.env, &[(movie.clone(), tree_size(&movie).0)]);
    assert_eq!(p.selected_count(), 1, "{:?}", p.items);
    let s = Settings::default();
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 1, "{rep:?}");
    assert!(!movie.exists());
    let trash = executor::trash_dir_for(&m.env, &movie);
    assert!(trash.starts_with(&vol), "the volume's own Trash");
    assert!(trash.join("old.mov").exists());
    let (n, errs) = executor::undo_plan(&m.env, &rep.plan_id);
    assert_eq!((n, errs.len()), (1, 0));
    assert!(movie.exists());
    // The volume unplugged: no longer in the allow-list.
    m.env.fake_sys().unwrap().answer(
        "/bin/df -Pk",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n",
    );
    assert!(!SpaceLens.allow_roots(&m.env).contains(&vol.join("Movies")));
    let p = space_lens::plan_for(&m.env, &[(movie.clone(), 1)]);
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap_or_default();
    assert!(rep.moved.is_empty() && movie.exists());
}

#[test]
fn snapshots_kept_two_per_location() {
    let m = startup_mac();
    let never = AtomicBool::new(false);
    let home = MapLocation::Home;
    let t1 = space::build_location(&m.env, &home, &never, &|_, _| {});
    assert!(space_snapshots::record_at(&m.env, &home, &t1, 100).unwrap());
    m.home_file("Movies/new.mov", 700_000);
    let t2 = space::build_location(&m.env, &home, &never, &|_, _| {});
    space_snapshots::record_at(&m.env, &home, &t2, 200).unwrap();
    m.home_file("Movies/newer.mov", 900_000);
    let t3 = space::build_location(&m.env, &home, &never, &|_, _| {});
    space_snapshots::record_at(&m.env, &home, &t3, 300).unwrap();
    let st = MapLocation::Startup;
    let t4 = space::build_location(&m.env, &st, &never, &|_, _| {});
    space_snapshots::record_at(&m.env, &st, &t4, 250).unwrap();
    // A stopped measure is not kept.
    let mut t5 = t4.clone();
    t5.stopped = true;
    assert!(!space_snapshots::record_at(&m.env, &st, &t5, 400).unwrap());
    let all = space_snapshots::saved(&m.env);
    assert_eq!(
        all.iter().map(|s| s.taken).collect::<Vec<_>>(),
        [300, 250, 200]
    );
    let (older, newer) = space_snapshots::latest_pair(&m.env, "home").unwrap();
    assert_eq!((older.taken, newer.taken), (200, 300));
    assert_eq!(newer.root, m.env.home());
    assert_eq!(newer.total, t3.root.as_ref().unwrap().size);
    assert!(newer.entries.len() <= space_snapshots::MAX_ENTRIES);
    let g = space_snapshots::growth(&older, &newer);
    let movies = m.env.home_path("Movies");
    let newer_mov = m.env.home_path("Movies/newer.mov");
    assert!(g.iter().any(|x| x.path == newer_mov && x.before == 0));
    assert!(g.windows(2).all(|w| w[0].grew() >= w[1].grew()));
    assert!(
        g.iter()
            .any(|x| x.path == movies && x.grew() == tree_size(&newer_mov).0)
    );
    assert!(space_snapshots::latest_pair(&m.env, "startup").is_none());
    // Depth 2 only.
    assert!(newer.entries.iter().all(|e| {
        e.path
            .strip_prefix(m.env.home())
            .unwrap()
            .components()
            .count()
            <= 2
    }));
}

#[test]
fn snapshot_entries_are_capped() {
    let m = FakeMac::new();
    for i in 0..(space_snapshots::MAX_ENTRIES + 20) {
        m.home_file(&format!("Many/f{i}.bin"), 1000 + i);
    }
    let t = space::build_location(
        &m.env,
        &MapLocation::Home,
        &AtomicBool::new(false),
        &|_, _| {},
    );
    let s = space_snapshots::snapshot_of(&m.env, &MapLocation::Home, &t, 1).unwrap();
    assert_eq!(s.entries.len(), space_snapshots::MAX_ENTRIES);
    assert!(s.entries.windows(2).all(|w| w[0].size >= w[1].size));
}
