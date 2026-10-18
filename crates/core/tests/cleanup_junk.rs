//! System Junk (J1 to J20) on a rich fake Mac: what is proposed, what is locked and why, the
//! default selection, sizes against a recalculation, the fence, cleaning and the error paths.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, SkipReason};
use cleancraft_core::features::{self, Feature, system_junk as sj};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, junk_mac};
use cleancraft_core::{Fault, FeatureId, Item, Plan, Settings, policy};

fn scan(m: &FakeMac, s: &Settings) -> Plan {
    features::scan(&m.env, s, FeatureId::SystemJunk).unwrap()
}

fn item<'a>(p: &'a Plan, path: &Path) -> &'a Item {
    p.items
        .iter()
        .find(|i| i.path == path)
        .unwrap_or_else(|| panic!("{} not proposed; have {:#?}", path.display(), paths(p)))
}

fn has(p: &Plan, path: &Path) -> bool {
    p.items.iter().any(|i| i.path == path)
}

fn paths(p: &Plan) -> Vec<String> {
    p.items
        .iter()
        .map(|i| format!("{} [{}]", i.path.display(), i.category))
        .collect()
}

fn h(m: &FakeMac, rel: &str) -> PathBuf {
    m.env.home_path(rel)
}

#[test]
fn every_category_is_found_with_its_default() {
    let m = junk_mac();
    let p = scan(&m, &Settings::default());
    let cats: Vec<&str> = p.categories.iter().map(|c| c.id.as_str()).collect();
    for c in [
        "user_caches",
        "system_caches",
        "user_logs",
        "system_logs",
        "crash_reports",
        "language_files",
        "universal_binaries",
        "broken_agents",
        "broken_prefs",
        "document_versions",
        "ios_backups",
        "ios_updates",
        "xcode_derived",
        "xcode_device_support",
        "xcode_simulator_caches",
        "xcode_archives",
        "unfinished_downloads",
        "old_updates",
        "disk_images",
        "dev_caches",
        "ai_clutter",
        "ai_models",
        "deleted_users",
    ] {
        assert!(cats.contains(&c), "category {c} missing: {cats:?}");
    }
    let sel = |path: PathBuf, cat: &str, on: bool| {
        let it = item(&p, &path);
        assert_eq!(it.category, cat, "{}", path.display());
        assert_eq!(it.selected, on, "{} selection", path.display());
        assert!(it.locked.is_none(), "{} locked", path.display());
    };
    let locked = |path: PathBuf, cat: &str, why: &str| {
        let it = item(&p, &path);
        assert_eq!(it.category, cat, "{}", path.display());
        assert!(!it.selected);
        assert_eq!(it.locked.as_deref(), Some(why), "{}", path.display());
    };
    // J1: one entry per app cache, sandboxed caches too; iCloud caches never.
    sel(h(&m, "Library/Caches/com.example.Browser"), "user_caches", true);
    sel(h(&m, "Library/Caches/com.example.Editor"), "user_caches", true);
    sel(
        h(&m, "Library/Containers/com.example.Notes/Data/Library/Caches/com.example.Notes"),
        "user_caches",
        true,
    );
    assert!(!has(&p, &h(&m, "Library/Caches/com.apple.bird")));
    assert!(!has(
        &p,
        &h(&m, "Library/Containers/com.example.Notes/Data/Library/Caches/CloudKit")
    ));
    assert_eq!(
        item(&p, &h(&m, "Library/Caches/com.example.Browser"))
            .group
            .as_deref(),
        Some("com.example.Browser")
    );
    // J15: the Sparkle download is its own entry; the rest of that app's cache stays in J1.
    sel(
        h(&m, "Library/Caches/com.example.Updater/org.sparkle-project.Sparkle"),
        "old_updates",
        true,
    );
    sel(
        h(&m, "Library/Caches/com.example.Updater/fsCachedData"),
        "user_caches",
        true,
    );
    assert!(!has(&p, &h(&m, "Library/Caches/com.example.Updater")));
    sel(
        m.env.sys_path("/Library/Updates/012-34567"),
        "old_updates",
        true,
    );
    // J2 to J5.
    sel(
        m.env.sys_path("/Library/Caches/com.example.updater"),
        "system_caches",
        true,
    );
    sel(h(&m, "Library/Logs/Example"), "user_logs", true);
    sel(m.env.sys_path("/Library/Logs/Install.log"), "system_logs", true);
    sel(
        m.env.sys_path("/private/var/log/system.log"),
        "system_logs",
        true,
    );
    let crash =
        h(&m, "Library/Logs/DiagnosticReports/Example-2026-05-01-101010.ips");
    sel(crash.clone(), "crash_reports", true);
    assert_eq!(item(&p, &crash).group.as_deref(), Some("Example"));
    assert!(!has(&p, &h(&m, "Library/Logs/DiagnosticReports")));
    // J6: languages not in use, locked; Base, English and the system's Italian kept.
    let res = m.env.sys_path("/Applications/Atlas.app/Contents/Resources");
    locked(res.join("de.lproj"), "language_files", sj::LOCK_SIGNED);
    locked(res.join("fr.lproj"), "language_files", sj::LOCK_SIGNED);
    for kept in ["en.lproj", "Base.lproj", "it.lproj"] {
        assert!(!has(&p, &res.join(kept)), "{kept} must be kept");
    }
    // J7: the unused slice size, from lipo.
    let atlas = kit::exe_of(&m, "Atlas");
    locked(atlas.clone(), "universal_binaries", sj::LOCK_LIPO);
    assert_eq!(item(&p, &atlas).size, 600_000);
    assert!(!has(&p, &kit::exe_of(&m, "Solo")));
    // J8, J9.
    sel(
        h(&m, "Library/LaunchAgents/com.example.gone.plist"),
        "broken_agents",
        true,
    );
    assert!(!has(&p, &h(&m, "Library/LaunchAgents/com.example.atlas.plist")));
    sel(
        h(&m, "Library/Preferences/com.example.broken.plist"),
        "broken_prefs",
        true,
    );
    locked(
        h(&m, "Library/Preferences/com.example.unchecked.plist"),
        "broken_prefs",
        sj::LOCK_UNCHECKED,
    );
    assert!(!has(&p, &h(&m, "Library/Preferences/com.example.good.plist")));
    // J10.
    locked(
        m.env.sys_path("/.DocumentRevisions-V100"),
        "document_versions",
        sj::LOCK_VERSIONS,
    );
    // J11: newest per device kept, older ones offered but not selected.
    let bk = |d: &str| h(&m, &format!("Library/Application Support/MobileSync/Backup/{d}"));
    sel(bk("AAA"), "ios_backups", false);
    locked(bk("BBB"), "ios_backups", sj::LOCK_NEWEST);
    locked(bk("CCC"), "ios_backups", sj::LOCK_NEWEST);
    assert!(item(&p, &bk("AAA")).reason.contains("Tester's iPhone"));
    // J12, J13.
    sel(
        h(&m, "Library/iTunes/iPhone Software Updates/iPhone_17.0_Restore.ipsw"),
        "ios_updates",
        true,
    );
    sel(
        h(&m, "Library/Developer/Xcode/DerivedData/Demo-abc"),
        "xcode_derived",
        true,
    );
    assert!(!has(
        &p,
        &h(&m, "Library/Developer/Xcode/DerivedData/ModuleCache.noindex")
    ));
    sel(
        h(&m, "Library/Developer/Xcode/Archives/2026-05-01/Demo 5-1-26.xcarchive"),
        "xcode_archives",
        false,
    );
    sel(
        h(&m, "Library/Developer/Xcode/iOS DeviceSupport/17.0 (21A329)"),
        "xcode_device_support",
        true,
    );
    sel(
        h(&m, "Library/Developer/CoreSimulator/Caches/dyld"),
        "xcode_simulator_caches",
        true,
    );
    // J14: an open download is locked.
    sel(
        h(&m, "Downloads/movie.mp4.crdownload"),
        "unfinished_downloads",
        true,
    );
    sel(
        h(&m, "Downloads/big.zip.download"),
        "unfinished_downloads",
        true,
    );
    locked(
        h(&m, "Downloads/active.part"),
        "unfinished_downloads",
        sj::LOCK_DOWNLOADING,
    );
    // J16: older than settings.downloads_days (90), never preselected.
    sel(h(&m, "Downloads/Old Installer.dmg"), "disk_images", false);
    sel(h(&m, "Downloads/sub/Nested.dmg"), "disk_images", false);
    assert!(!has(&p, &h(&m, "Downloads/New.dmg")));
    // J17: Gradle never preselected; grouped per tool.
    sel(h(&m, "Library/Caches/Homebrew/downloads"), "dev_caches", true);
    sel(h(&m, ".npm/_cacache/content-v2"), "dev_caches", true);
    sel(h(&m, ".cargo/registry/cache/index.crates.io"), "dev_caches", true);
    sel(h(&m, ".gradle/caches/modules-2"), "dev_caches", false);
    assert!(!has(&p, &h(&m, "Library/Caches/Homebrew")));
    assert!(!has(&p, &h(&m, ".npm/_logs")));
    assert_eq!(
        item(&p, &h(&m, ".npm/_cacache/content-v2")).group.as_deref(),
        Some("npm")
    );
    // J18.
    sel(
        h(&m, "Library/Application Support/Claude/Cache/Cache_Data"),
        "ai_clutter",
        true,
    );
    sel(
        h(&m, "Library/Application Support/Cursor/GPUCache/data"),
        "ai_clutter",
        true,
    );
    sel(h(&m, "Library/Logs/Claude"), "ai_clutter", true);
    assert!(!has(&p, &h(&m, "Library/Application Support/Claude/config.json")));
    // J19.
    sel(h(&m, ".ollama/models"), "ai_models", false);
    assert!(item(&p, &h(&m, ".ollama/models")).reason.contains("llama3:8b"));
    assert!(!has(&p, &h(&m, ".ollama/id_ed25519")));
    sel(
        h(&m, ".lmstudio/models/lmstudio-community/Qwen-7B-GGUF"),
        "ai_models",
        false,
    );
    // J20.
    locked(
        m.env.sys_path("/Users/Deleted Users/olduser.dmg"),
        "deleted_users",
        sj::LOCK_DELETED_USER,
    );
    // Personal files never.
    assert!(
        !p.items
            .iter()
            .any(|i| i.path.starts_with(h(&m, "Documents")) || i.path.starts_with(h(&m, "Desktop")))
    );
}

#[test]
fn sizes_match_disk_and_the_fence_matches_the_locks() {
    let m = junk_mac();
    let p = scan(&m, &Settings::default());
    let allow = sj::SystemJunk.allow_roots(&m.env);
    let report_only = [
        "language_files",
        "universal_binaries",
        "document_versions",
        "deleted_users",
    ];
    for it in &p.items {
        if it.category != "universal_binaries" {
            assert_eq!(it.size, tree_size(&it.path).0, "{}", it.path.display());
        }
        let fenced = policy::check(&m.env, &allow, &it.path);
        if report_only.contains(&it.category.as_str()) {
            assert!(it.locked.is_some());
            assert!(
                fenced.is_err(),
                "{} is report-only but inside the allow-list",
                it.path.display()
            );
        } else {
            assert_eq!(fenced, Ok(()), "{}", it.path.display());
        }
    }
    assert_eq!(p.total_size(), p.items.iter().map(|i| i.size).sum::<u64>());
    // No path proposed twice, no entry inside another.
    for a in &p.items {
        for b in &p.items {
            if a.id != b.id {
                assert!(
                    !b.path.starts_with(&a.path),
                    "{} inside {}",
                    b.path.display(),
                    a.path.display()
                );
            }
        }
    }
}

#[test]
fn default_clean_moves_selected_only_and_undo_restores() {
    let m = junk_mac();
    let s = Settings::default();
    let p = scan(&m, &s);
    let selected: Vec<(PathBuf, u64)> = p.selected().map(|i| (i.path.clone(), i.size)).collect();
    let kept: Vec<PathBuf> = p
        .items
        .iter()
        .filter(|i| !i.selected)
        .map(|i| i.path.clone())
        .collect();
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    assert_eq!(rep.moved.len(), selected.len());
    assert_eq!(rep.freed, selected.iter().map(|(_, s)| s).sum::<u64>());
    for (path, _) in &selected {
        assert!(!path.exists(), "{} not moved", path.display());
    }
    for path in &kept {
        assert!(path.exists(), "{} moved but was not selected", path.display());
    }
    assert!(m.env.user_trash().join("com.example.Browser").exists());
    assert_eq!(executor::history(&m.env).len(), selected.len());
    let (n, errs) = executor::undo_plan(&m.env, &p.id);
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(n, selected.len());
    for (path, _) in &selected {
        assert!(path.exists(), "{} not restored", path.display());
    }
}

#[test]
fn errors_per_item_and_unwritable_log() {
    let m = junk_mac();
    let s = Settings::default();
    let p = scan(&m, &s);
    let denied = h(&m, "Library/Caches/com.example.Browser");
    let busy = h(&m, "Library/Caches/com.example.Editor");
    let full = h(&m, "Library/Logs/Example");
    m.env.inject_fault(&denied, Fault::PermissionDenied);
    m.env.fake_sys().unwrap().set_open(&busy.join("index"));
    m.env.inject_fault(&full, Fault::DiskFull);
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    let why = |path: &Path| {
        rep.skipped
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, r)| r.clone())
    };
    assert_eq!(why(&denied), Some(SkipReason::PermissionDenied));
    assert_eq!(why(&busy), Some(SkipReason::InUse));
    assert_eq!(why(&full), Some(SkipReason::DiskFull));
    assert!(denied.exists() && busy.exists() && full.exists());
    assert_eq!(rep.moved.len(), p.selected_count() - 3);

    // The action log cannot be written: nothing moves at all.
    let m = junk_mac();
    let p = scan(&m, &s);
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    let err = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap_err();
    assert!(err.to_string().contains("could not write the action log"));
    assert!(p.selected().all(|i| i.path.exists()));
}

#[test]
fn admin_folders_are_locked_and_unreadable_folders_noted() {
    let m = junk_mac();
    m.env
        .inject_fault(&m.env.sys_path("/Library/Caches"), Fault::PermissionDenied);
    let p = scan(&m, &Settings::default());
    // A folder that cannot be listed gives a note instead of items.
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("/Library/Caches could not be read"))
    );
    assert!(!p.items.iter().any(|i| i.category == "system_caches"));

    let m = junk_mac();
    // Readable but not writable: listed, locked.
    let caches = m.env.sys_path("/Library/Caches");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // No write bit for the owner: Env::can_modify answers no (it reads the mode, so this
        // also holds when tests run as root).
        let _ = std::fs::set_permissions(&caches, std::fs::Permissions::from_mode(0o555));
    }
    let p = scan(&m, &Settings::default());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&caches, std::fs::Permissions::from_mode(0o755));
    }
    let it = item(&p, &caches.join("com.example.updater"));
    assert_eq!(it.locked.as_deref(), Some(sj::LOCK_ADMIN));
}

#[test]
fn settings_drive_the_scan() {
    let m = junk_mac();
    let mut s = Settings::default();
    s.ignore.push(h(&m, "Library/Caches/com.example.Browser"));
    s.downloads_days = 300;
    s.keep_languages = vec!["de".into()];
    let p = scan(&m, &s);
    assert!(!has(&p, &h(&m, "Library/Caches/com.example.Browser")));
    assert!(!has(&p, &h(&m, "Downloads/Old Installer.dmg")), "200 days < 300");
    let res = m.env.sys_path("/Applications/Atlas.app/Contents/Resources");
    assert!(!has(&p, &res.join("de.lproj")));
    assert!(has(&p, &res.join("fr.lproj")));
}

#[test]
fn missing_tool_answers_are_honest() {
    let m = FakeMac::new();
    kit::app_with_languages(&m, "Atlas", &["en", "it"], 10_000);
    // No answers for defaults, uname, lipo: nothing is guessed.
    let p = scan(&m, &Settings::default());
    assert!(p.notes.iter().any(|n| n.contains("system languages could not be read")));
    assert!(p.notes.iter().any(|n| n.contains("processor type could not be read")));
    let res = m.env.sys_path("/Applications/Atlas.app/Contents/Resources");
    assert!(has(&p, &res.join("it.lproj")), "only English kept");
    assert!(!p.items.iter().any(|i| i.category == "universal_binaries"));
}

#[test]
fn saved_plan_cannot_unlock_report_only_items() {
    let m = junk_mac();
    let s = Settings::default();
    let mut p = scan(&m, &s);
    // Someone edits a saved plan: unlocks and selects a language folder and a deleted user.
    for it in p.items.iter_mut() {
        if it.category == "language_files" || it.category == "deleted_users" {
            it.locked = None;
            it.selected = true;
        } else {
            it.selected = false;
        }
    }
    let targets: Vec<PathBuf> = p.selected().map(|i| i.path.clone()).collect();
    assert!(!targets.is_empty());
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    assert!(rep.moved.is_empty());
    assert_eq!(rep.skipped.len(), targets.len());
    assert!(
        rep.skipped
            .iter()
            .all(|(_, r)| matches!(r, SkipReason::Refused(_)))
    );
    assert!(targets.iter().all(|t| t.exists()));
}
