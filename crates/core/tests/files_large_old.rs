//! Large & Old Files and Downloads by source, on a fake home folder.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use cleancraft_core::features::large_old::{self, Location, ScanOptions, parse_folder};
use cleancraft_core::features::{self, ScanCtx};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{FeatureId, Plan, Settings};

fn settings() -> Settings {
    Settings {
        large_min_bytes: 2_000_000,
        old_days: 30,
        ..Default::default()
    }
}

fn scan(mac: &FakeMac, loc: &Location) -> Plan {
    let s = settings();
    let cancel = AtomicBool::new(false);
    let ctx = ScanCtx {
        env: &mac.env,
        settings: &s,
        cancel: &cancel,
        progress: &|_| {},
        meter: &cleancraft_core::progress::Progress::new(),
    };
    large_old::scan_location(&ctx, loc).unwrap()
}

fn paths(p: &Plan) -> BTreeSet<PathBuf> {
    p.items.iter().map(|i| i.path.clone()).collect()
}

#[test]
fn thresholds_kinds_and_exclusions() {
    let mac = FakeMac::new();
    let big_zip = mac.home_file("Downloads/big.zip", 2_500_000);
    let clip = mac.home_file("Movies/clip.mov", 2_100_000);
    let old_pic = mac.home_file("Pictures/old.jpg", 1_200_000);
    age(&old_pic, 40);
    let tiny_old = mac.home_file("Pictures/old_small.jpg", 500_000);
    age(&tiny_old, 40);
    mac.home_file("Projects/recent.bin", 1_500_000);
    let notes = mac.home_file("Projects/notes.txt", 1_100_000);
    age(&notes, 400);
    let pkg_data = mac.home_file("Projects/Report.pages/Data/big", 2_500_000);
    let pkg = pkg_data.parent().unwrap().parent().unwrap().to_path_buf();
    // Never looked at.
    for rel in [
        "Library/Caches/huge",
        "Documents/huge.pdf",
        "Desktop/huge.mov",
        ".hidden/big.zip",
        ".Trash/old.zip",
        "Applications/Tool.app/Contents/big",
        "Projects/Thing.app/Contents/big",
        "Pictures/Photos Library.photoslibrary/originals/a.jpg",
        "Music/Music/Media.localized/song.m4a",
    ] {
        mac.home_file(rel, 3_000_000);
    }
    let plan = scan(&mac, &Location::Home);
    let want: BTreeSet<PathBuf> = [
        big_zip.clone(),
        clip.clone(),
        old_pic.clone(),
        notes.clone(),
        pkg.clone(),
    ]
    .into_iter()
    .collect();
    assert_eq!(paths(&plan), want);
    assert_eq!(plan.selected_count(), 0, "nothing is preselected");
    let cat = |p: &PathBuf| {
        plan.items
            .iter()
            .find(|i| &i.path == p)
            .unwrap()
            .category
            .clone()
    };
    assert_eq!(cat(&big_zip), "archives");
    assert_eq!(cat(&clip), "movies");
    assert_eq!(cat(&old_pic), "pictures");
    assert_eq!(cat(&notes), "documents");
    assert_eq!(cat(&pkg), "documents");
    for it in &plan.items {
        assert_eq!(it.size, tree_size(&it.path).0, "{}", it.path.display());
        assert!(it.modified.is_some());
    }
    let reason = |p: &PathBuf| {
        plan.items
            .iter()
            .find(|i| &i.path == p)
            .unwrap()
            .reason
            .clone()
    };
    assert!(
        reason(&big_zip).starts_with("Large"),
        "{}",
        reason(&big_zip)
    );
    assert!(
        reason(&notes).starts_with("Not opened for 400 days"),
        "{}",
        reason(&notes)
    );
}

#[test]
fn locations_limit_the_walk() {
    let mac = FakeMac::new();
    let a = mac.home_file("Downloads/a.dmg", 2_200_000);
    mac.home_file("Movies/b.mov", 2_200_000);
    let plan = scan(&mac, &Location::Downloads);
    assert_eq!(paths(&plan), [a.clone()].into_iter().collect());
    // The remembered location is used by the registered feature.
    let mut o = ScanOptions::load(&mac.env);
    assert_eq!(o.large_old, Location::Home);
    o.set_location(FeatureId::LargeOldFiles, Location::Movies);
    o.save(&mac.env).unwrap();
    let plan = features::scan(&mac.env, &settings(), FeatureId::LargeOldFiles).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert!(plan.items[0].path.ends_with("Movies/b.mov"));
    // A typed folder.
    let p = mac.home_file("Work/Video/c.mp4", 2_200_000);
    let dir = parse_folder(&mac.env, "~/Work").unwrap();
    let plan = scan(&mac, &Location::Folder(dir));
    assert_eq!(paths(&plan), [p].into_iter().collect());
}

#[test]
fn typed_folders_are_checked() {
    let mac = FakeMac::new();
    fs::create_dir_all(mac.env.home_path("Work")).unwrap();
    mac.home_file("Work/file.txt", 10);
    let env = &mac.env;
    assert_eq!(parse_folder(env, "Work"), Ok(env.home_path("Work")));
    assert_eq!(parse_folder(env, "~/Work/"), Ok(env.home_path("Work")));
    assert_eq!(
        parse_folder(env, "/Users/tester/Work"),
        Ok(env.home_path("Work"))
    );
    assert_eq!(parse_folder(env, "~"), Ok(env.home().to_path_buf()));
    let err = |s: &str| parse_folder(env, s).unwrap_err();
    assert!(
        err("~/Documents").contains("protected"),
        "{}",
        err("~/Documents")
    );
    assert!(err("~/Desktop/x").contains("protected"));
    assert!(err("~/Library/Caches").contains("Library"));
    assert!(err("/Applications").contains("outside your home folder"));
    assert!(err("~/../other").contains(".."));
    assert!(err("~/Nope").contains("no folder"));
    assert!(err("~/Work/file.txt").contains("is a file"));
    assert!(err("  ").contains("Type the path"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(env.home_path("Work"), env.home_path("Link")).unwrap();
        assert!(err("~/Link").contains("link"));
    }
}

#[test]
fn spotlight_last_used_date_wins_over_access_time() {
    let mac = FakeMac::new();
    let f = mac.home_file("Downloads/disk.iso", 1_500_000);
    // Modified long ago, read today by something (backup, indexing).
    let old = SystemTime::now() - Duration::from_secs(200 * 86_400);
    let file = fs::File::options().write(true).open(&f).unwrap();
    file.set_times(
        fs::FileTimes::new()
            .set_modified(old)
            .set_accessed(SystemTime::now()),
    )
    .unwrap();
    drop(file);
    // Without Spotlight: the access time says it was opened today.
    assert!(scan(&mac, &Location::Downloads).items.is_empty());
    // Spotlight says the user last opened it 100 days ago.
    let when = chrono::Utc::now() - chrono::Duration::days(100);
    mac.env
        .fake_sys()
        .unwrap()
        .answer("mdls", &when.format("%Y-%m-%d %H:%M:%S +0000").to_string());
    let plan = scan(&mac, &Location::Downloads);
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].reason, "Not opened for 100 days");
    let calls = mac.env.fake_sys().unwrap().calls();
    assert!(
        calls.iter().any(
            |c| c.starts_with("/usr/bin/mdls -raw -name kMDItemLastUsedDate ")
                && c.ends_with("disk.iso")
        ),
        "{calls:?}"
    );
}

#[test]
fn downloads_grouped_by_source_app() {
    let mac = FakeMac::new();
    let a = mac.home_file("Downloads/a.dmg", 300_000);
    let b = mac.home_file("Downloads/b.zip", 200_000);
    let c = mac.home_file("Downloads/c.pdf", 100_000);
    let d = mac.home_file("Downloads/folder/inside.txt", 5_000);
    let dir = d.parent().unwrap().to_path_buf();
    mac.home_file("Downloads/.DS_Store", 6_000);
    let fake = mac.env.fake_sys().unwrap();
    let q = |p: &PathBuf| format!("/usr/bin/xattr -p com.apple.quarantine {}", p.display());
    fake.answer(&q(&a), "0083;65f1c2a0;Safari;4C1E0F2A\n");
    fake.answer(&q(&b), "0081;65f1c2a0;Google Chrome;77\n");
    fake.answer(&q(&c), "0083;65f1c2a0;Safari;99\n");
    let plan = scan(&mac, &Location::DownloadsBySource);
    assert_eq!(plan.feature, Some(FeatureId::LargeOldFiles));
    assert_eq!(
        paths(&plan),
        [a.clone(), b.clone(), c.clone(), dir.clone()]
            .into_iter()
            .collect()
    );
    assert_eq!(plan.selected_count(), 0);
    let titles: Vec<&str> = plan.categories.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Saved by Google Chrome",
            "Saved by Safari",
            "No source recorded"
        ]
    );
    assert_eq!(plan.category_count("src_safari"), 2);
    assert_eq!(plan.category_count("src_google_chrome"), 1);
    let item = plan.items.iter().find(|i| i.path == dir).unwrap();
    assert_eq!(item.category, "src_none");
    assert_eq!(item.size, tree_size(&dir).0);
}

#[test]
fn chosen_items_move_and_come_back() {
    let mac = FakeMac::new();
    let a = mac.home_file("Downloads/a.dmg", 2_200_000);
    let s = settings();
    let mut plan = scan(&mac, &Location::Home);
    plan.set_selected(0, true);
    let rep = features::run_plan(&mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 1);
    assert!(!a.exists());
    assert!(mac.env.user_trash().join("a.dmg").exists());
    cleancraft_core::executor::undo_plan(&mac.env, &plan.id);
    assert!(a.exists());
}
