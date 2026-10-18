//! Privacy Traces (P3 to P6): browser files per browser and profile, recent items lists,
//! Wi-Fi networks, app permissions, running browsers, cleaning and resetting.

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, ActionKind, SkipReason};
use cleancraft_core::features::{self, Feature, privacy};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, privacy_mac};
use cleancraft_core::{Fault, FeatureId, Item, Plan, Risk, Settings, policy};

fn scan(m: &FakeMac) -> Plan {
    features::scan(&m.env, &Settings::default(), FeatureId::Privacy).unwrap()
}

fn item<'a>(p: &'a Plan, path: &Path) -> &'a Item {
    p.items
        .iter()
        .find(|i| i.path == path)
        .unwrap_or_else(|| panic!("{} not listed", path.display()))
}

fn has(p: &Plan, path: &Path) -> bool {
    p.items.iter().any(|i| i.path == path)
}

fn h(m: &FakeMac, rel: &str) -> PathBuf {
    m.env.home_path(rel)
}

const CHROME: &str = "Library/Application Support/Google/Chrome";
const FF: &str = "Library/Application Support/Firefox/Profiles/abcd.default-release";

#[test]
fn browsers_are_listed_with_safe_defaults() {
    let m = privacy_mac();
    let p = scan(&m);
    let check = |rel: &str, cat: &str, group: &str, selected: bool| {
        let it = item(&p, &h(&m, rel));
        assert_eq!(it.category, cat, "{rel}");
        assert_eq!(it.group.as_deref(), Some(group), "{rel}");
        assert_eq!(it.selected, selected, "{rel}");
        assert!(it.locked.is_none(), "{rel}");
    };
    // Safari.
    check("Library/Safari/History.db", "safari", "History", true);
    check(
        "Library/Safari/LastSession.plist",
        "safari",
        "Last session",
        true,
    );
    check(
        "Library/Safari/Downloads.plist",
        "safari",
        "Download list",
        true,
    );
    check("Library/Safari/Form Values", "safari", "Form values", true);
    check(
        "Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies",
        "safari",
        "Cookies",
        true,
    );
    check("Library/Caches/com.apple.Safari", "safari", "Cache", true);
    assert!(!has(&p, &h(&m, "Library/Safari/Bookmarks.plist")));
    assert!(p.notes.iter().any(|n| n.contains("Passwords app")));
    // Chrome, two profiles; passwords and autofill (with saved addresses) not preselected.
    check(
        &format!("{CHROME}/Default/History"),
        "chrome",
        "History",
        true,
    );
    check(
        &format!("{CHROME}/Default/Cookies"),
        "chrome",
        "Cookies",
        true,
    );
    check(
        &format!("{CHROME}/Default/Current Session"),
        "chrome",
        "Last session",
        true,
    );
    check(
        &format!("{CHROME}/Default/Web Data"),
        "chrome",
        "Form values",
        false,
    );
    check(
        &format!("{CHROME}/Default/Login Data"),
        "chrome",
        "Saved passwords",
        false,
    );
    assert_eq!(
        item(&p, &h(&m, &format!("{CHROME}/Default/Login Data"))).risk,
        Risk::High
    );
    check(
        &format!("{CHROME}/Profile 1/History"),
        "chrome",
        "History",
        true,
    );
    check(
        "Library/Caches/Google/Chrome/Default",
        "chrome",
        "Cache",
        true,
    );
    for kept in [
        "Default/Bookmarks",
        "Default/Preferences",
        "System Profile/History",
    ] {
        assert!(!has(&p, &h(&m, &format!("{CHROME}/{kept}"))), "{kept}");
    }
    assert!(
        item(&p, &h(&m, &format!("{CHROME}/Profile 1/History")))
            .reason
            .contains("Profile 1 profile")
    );
    // Brave.
    check(
        "Library/Application Support/BraveSoftware/Brave-Browser/Default/History",
        "brave",
        "History",
        true,
    );
    // Firefox: places.sqlite also holds bookmarks, so it is never preselected.
    check(&format!("{FF}/places.sqlite"), "firefox", "History", false);
    assert_eq!(
        item(&p, &h(&m, &format!("{FF}/places.sqlite"))).risk,
        Risk::High
    );
    check(&format!("{FF}/cookies.sqlite"), "firefox", "Cookies", true);
    check(
        &format!("{FF}/formhistory.sqlite"),
        "firefox",
        "Form values",
        true,
    );
    check(
        &format!("{FF}/sessionstore.jsonlz4"),
        "firefox",
        "Last session",
        true,
    );
    check(
        &format!("{FF}/logins.json"),
        "firefox",
        "Saved passwords",
        false,
    );
    check(
        "Library/Caches/Firefox/Profiles/abcd.default-release",
        "firefox",
        "Cache",
        true,
    );
    assert!(!has(&p, &h(&m, &format!("{FF}/prefs.js"))));
    // Every file item: size from disk, inside the fence.
    let allow = privacy::Privacy.allow_roots(&m.env);
    for it in p.items.iter().filter(|i| i.size > 0) {
        assert_eq!(it.size, tree_size(&it.path).0, "{}", it.path.display());
        assert_eq!(policy::check(&m.env, &allow, &it.path), Ok(()));
    }
    assert!(
        !p.items
            .iter()
            .any(|i| i.path.starts_with(h(&m, "Documents")))
    );
}

#[test]
fn recent_items_wifi_and_permissions() {
    let m = privacy_mac();
    let p = scan(&m);
    let sfl = h(&m, privacy::SHARED_FILE_LIST);
    let recent: Vec<&Item> = p
        .items
        .iter()
        .filter(|i| i.category == "recent_items")
        .collect();
    assert_eq!(recent.len(), 4, "{recent:#?}");
    assert!(recent.iter().all(|i| i.selected && i.risk == Risk::Low));
    assert!(!has(
        &p,
        &sfl.join("com.apple.LSSharedFileList.FavoriteItems.sfl3")
    ));
    assert_eq!(
        item(
            &p,
            &sfl.join("com.apple.LSSharedFileList.RecentDocuments.sfl3")
        )
        .reason,
        "Recently opened documents"
    );
    // Wi-Fi: listed, locked, nothing to move.
    let wifi: Vec<&Item> = p.items.iter().filter(|i| i.category == "wifi").collect();
    assert_eq!(wifi.len(), 2);
    assert!(
        wifi.iter()
            .all(|i| i.locked.as_deref() == Some(privacy::LOCK_WIFI))
    );
    assert!(
        wifi.iter()
            .any(|i| i.reason == "Known Wi-Fi network Home Net")
    );
    let calls = m.env.fake_sys().unwrap().calls();
    assert!(
        calls.contains(&"/usr/sbin/networksetup -listpreferredwirelessnetworks en0".to_string())
    );
    // Permissions: granted ones only, from both databases, locked.
    let perms: Vec<&Item> = p
        .items
        .iter()
        .filter(|i| i.category == "permissions")
        .collect();
    assert_eq!(perms.len(), 3, "{perms:#?}");
    assert!(
        perms
            .iter()
            .all(|i| i.locked.as_deref() == Some(privacy::LOCK_PERMISSION))
    );
    let reasons: Vec<&str> = perms.iter().map(|i| i.reason.as_str()).collect();
    assert!(reasons.contains(&"Camera access granted to us.zoom.xos"));
    assert!(reasons.contains(&"Screen recording access granted to com.example.Recorder"));
    assert!(!reasons.iter().any(|r| r.contains("com.example.Old")));
    let cam = perms
        .iter()
        .find(|i| i.reason.starts_with("Camera"))
        .unwrap();
    assert_eq!(
        privacy::permission_of(&m.env, &cam.path),
        Some(("kTCCServiceCamera".into(), "us.zoom.xos".into()))
    );
    assert_eq!(
        privacy::permission_of(&m.env, &h(&m, "Library/Safari/History.db")),
        None
    );
    // None of these pseudo items is inside the fence.
    let allow = privacy::Privacy.allow_roots(&m.env);
    for it in wifi.iter().chain(perms.iter()) {
        assert!(policy::check(&m.env, &allow, &it.path).is_err());
    }
    // The TCC databases were read read-only.
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("/usr/bin/sqlite3 -readonly"))
    );
}

#[test]
fn running_browser_is_locked_and_blocks_cleaning() {
    let m = privacy_mac();
    kit::set_running(&m.env, "Google Chrome", "/Applications/Google Chrome.app");
    let p = scan(&m);
    let chrome: Vec<&Item> = p.items.iter().filter(|i| i.category == "chrome").collect();
    assert!(!chrome.is_empty());
    assert!(
        chrome
            .iter()
            .all(|i| i.locked.as_deref() == Some("Quit Google Chrome first"))
    );
    assert!(
        p.items
            .iter()
            .filter(|i| i.category == "safari")
            .all(|i| i.locked.is_none())
    );
    assert!(
        p.notes
            .iter()
            .any(|n| n.starts_with("Google Chrome is open"))
    );

    // Scanned while quit, opened before the cleanup: refused, nothing moved.
    let m = privacy_mac();
    let p = scan(&m);
    kit::set_running(&m.env, "Firefox", "/Applications/Firefox.app");
    let s = Settings::default();
    let err = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap_err();
    assert!(err.to_string().contains("Firefox is open"));
    assert!(p.selected().all(|i| i.path.exists()));
}

#[test]
fn clean_moves_selected_traces_and_reports_errors() {
    let m = privacy_mac();
    let s = Settings::default();
    let p = scan(&m);
    let hist = h(&m, &format!("{CHROME}/Default/History"));
    let cookies = h(&m, &format!("{CHROME}/Default/Cookies"));
    let safari = h(&m, "Library/Safari/History.db");
    m.env.inject_fault(&hist, Fault::PermissionDenied);
    m.env.fake_sys().unwrap().set_open(&cookies);
    m.env.inject_fault(&safari, Fault::DiskFull);
    let rep = features::run_plan(&m.env, &s, &p, &p.digest(), &mut |_, _| {}).unwrap();
    let why = |path: &Path| {
        rep.skipped
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, r)| r.clone())
    };
    assert_eq!(why(&hist), Some(SkipReason::PermissionDenied));
    assert_eq!(why(&cookies), Some(SkipReason::InUse));
    assert_eq!(why(&safari), Some(SkipReason::DiskFull));
    assert_eq!(rep.moved.len(), p.selected_count() - 3);
    let expected: u64 = p
        .selected()
        .filter(|i| ![&hist, &cookies, &safari].contains(&&i.path))
        .map(|i| i.size)
        .sum();
    assert_eq!(rep.freed, expected);
    // Passwords, bookmarks and not-selected files stay.
    for keep in [
        format!("{CHROME}/Default/Login Data"),
        format!("{CHROME}/Default/Bookmarks"),
        format!("{CHROME}/Default/Web Data"),
        format!("{FF}/places.sqlite"),
        format!("{FF}/logins.json"),
        "Library/Safari/Bookmarks.plist".to_string(),
    ] {
        assert!(h(&m, &keep).exists(), "{keep}");
    }
    assert!(!h(&m, "Library/Safari/LastSession.plist").exists());
}

#[test]
fn permissions_need_full_disk_access() {
    let m = privacy_mac();
    let db = h(&m, privacy::TCC_USER);
    kit::answer_tcc_denied(&m.env, &db);
    kit::answer_tcc_denied(&m.env, &m.env.sys_path(privacy::TCC_SYSTEM));
    let p = scan(&m);
    assert!(p.notes.contains(&privacy::NOTE_TCC.to_string()));
    assert!(!p.items.iter().any(|i| i.category == "permissions"));
}

#[test]
fn wifi_listing_failure_is_noted() {
    let m = privacy_mac();
    kit::answer_fail(
        &m.env,
        privacy::NETWORKSETUP,
        &["-listpreferredwirelessnetworks", "en0"],
        "** Error: Error obtaining wireless information.",
    );
    let p = scan(&m);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("Wi-Fi networks could not be listed"))
    );
    assert!(!p.items.iter().any(|i| i.category == "wifi"));
}

#[test]
fn reset_permission_runs_tccutil_and_logs() {
    let m = privacy_mac();
    let fake = m.env.fake_sys().unwrap();
    fake.answer(
        "/usr/bin/tccutil reset Camera us.zoom.xos",
        "Successfully reset Camera approval status for us.zoom.xos\n",
    );
    let msg = privacy::reset_permission(&m.env, "kTCCServiceCamera", "us.zoom.xos").unwrap();
    assert!(msg.contains("Camera access of us.zoom.xos was reset"));
    assert!(
        fake.calls()
            .contains(&"/usr/bin/tccutil reset Camera us.zoom.xos".to_string())
    );
    let h = executor::history(&m.env);
    match &h[0].kind {
        ActionKind::Command { command, ok, .. } => {
            assert_eq!(command, "tccutil reset Camera us.zoom.xos");
            assert!(ok);
        }
        k => panic!("{k:?}"),
    }
    // tccutil fails: error, logged as failed.
    let err =
        privacy::reset_permission(&m.env, "kTCCServiceMicrophone", "us.zoom.xos").unwrap_err();
    assert!(err.contains("could not reset Microphone"));
    assert_eq!(executor::history(&m.env).len(), 2);
    // Names that are not plain identifiers never reach a command.
    let before = fake.calls().len();
    assert!(privacy::reset_permission(&m.env, "kTCCServiceCamera", "x; rm -rf /").is_err());
    assert_eq!(fake.calls().len(), before);
    // Log unwritable: nothing runs.
    m.env
        .inject_fault(&executor::journal_path(&m.env), Fault::DiskFull);
    assert!(privacy::reset_permission(&m.env, "kTCCServiceCamera", "us.zoom.xos").is_err());
    assert_eq!(fake.calls().len(), before);
}

#[test]
fn unreadable_safari_folder_is_noted() {
    let m = privacy_mac();
    kit::make_unlistable(&h(&m, privacy::SAFARI_DIR));
    let p = scan(&m);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("Library/Safari could not be read"))
    );
    assert!(!p.items.iter().any(|i| i.category == "safari"));
}
