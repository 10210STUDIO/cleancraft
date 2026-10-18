//! Privacy Traces (P3 to P6) by clicks: per-browser numbers against disk, safe defaults,
//! cleaning, running browsers, Wi-Fi Settings, Reset Permissions with tccutil, and errors.

mod common;

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::privacy;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::{self as kit, privacy_mac};
use cleancraft_core::{Fault, FeatureId, Plan};
use common::*;

const CHROME: &str = "Library/Application Support/Google/Chrome";
const FF: &str = "Library/Application Support/Firefox/Profiles/abcd.default-release";

fn plan(h: &H) -> Plan {
    h.state().plans[&FeatureId::Privacy]
        .plan
        .clone()
        .expect("plan")
}

fn open(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    click(&mut h, "Privacy Traces");
    click(&mut h, "Scan");
    h
}

#[test]
fn browsers_numbers_and_clean() {
    let mac = privacy_mac();
    let mut h = open(&mac);
    let p = plan(&h);
    let total: u64 = p
        .items
        .iter()
        .map(|i| tree_size(&i.path).0 * u64::from(i.size > 0))
        .sum();
    assert_eq!(total, p.total_size());
    assert!(has(&h, &human_size(total)));
    for (cat, title) in [
        ("safari", "Safari"),
        ("chrome", "Google Chrome"),
        ("brave", "Brave"),
        ("firefox", "Firefox"),
        ("recent_items", "Recent items"),
    ] {
        let s: u64 = p
            .items
            .iter()
            .filter(|i| i.category == cat)
            .map(|i| tree_size(&i.path).0)
            .sum();
        assert!(has(&h, &format!("{title}  {}", human_size(s))), "{title}");
    }
    // Firefox only: history (with bookmarks) and passwords are not selected.
    click(
        &mut h,
        &format!(
            "Firefox  {}",
            human_size(
                p.items
                    .iter()
                    .filter(|i| i.category == "firefox")
                    .map(|i| i.size)
                    .sum(),
            )
        ),
    );
    assert!(has_text(
        &h,
        "browsing history, download list and bookmarks"
    ));
    assert!(has(&h, "places.sqlite") && has(&h, "logins.json"));
    let sel: Vec<_> = p
        .selected()
        .map(|i| (i.path.clone(), tree_size(&i.path).0))
        .collect();
    let sel_size: u64 = sel.iter().map(|(_, s)| s).sum();
    assert!(has(
        &h,
        &format!("{} selected, {}", sel.len(), human_size(sel_size))
    ));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    for (path, _) in &sel {
        assert!(!path.exists(), "{} not moved", path.display());
    }
    for keep in [
        format!("{CHROME}/Default/Login Data"),
        format!("{CHROME}/Default/Bookmarks"),
        format!("{FF}/places.sqlite"),
        format!("{FF}/logins.json"),
        "Library/Safari/Bookmarks.plist".to_string(),
        "Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.FavoriteItems.sfl3".to_string(),
    ] {
        assert!(mac.env.home_path(&keep).exists(), "{keep}");
    }
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(sel_size))
    ));
}

#[test]
fn running_browser_and_errors() {
    let mac = privacy_mac();
    kit::set_running(&mac.env, "Google Chrome", "/Applications/Google Chrome.app");
    let mut h = open(&mac);
    assert!(has_text(
        &h,
        "Google Chrome is open. Quit it and scan again"
    ));
    assert!(has_text(
        &h,
        "(cannot be selected: Quit Google Chrome first)"
    ));
    let p = plan(&h);
    assert!(
        p.items
            .iter()
            .filter(|i| i.category == "chrome")
            .all(|i| !i.selected)
    );
    // Faults on three selected files.
    let safari = mac.env.home_path("Library/Safari/History.db");
    let session = mac.env.home_path("Library/Safari/LastSession.plist");
    let ff = mac.env.home_path(&format!("{FF}/cookies.sqlite"));
    mac.env.inject_fault(&safari, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&session);
    mac.env.inject_fault(&ff, Fault::DiskFull);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(safari.exists() && session.exists() && ff.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has(&h, "the disk is full"));
    assert!(
        mac.env
            .home_path(&format!("{CHROME}/Default/History"))
            .exists()
    );

    // A browser opened after the scan: the cleanup is refused.
    let mac = privacy_mac();
    let mut h = open(&mac);
    kit::set_running(&mac.env, "Safari", "/Applications/Safari.app");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(has(&h, "Safari is open. Quit it, then scan again."));
    assert!(plan(&h).selected().all(|i| i.path.exists()));
}

#[test]
fn wifi_settings_button() {
    let mac = privacy_mac();
    let mut h = open(&mac);
    click(&mut h, "Wi-Fi networks  Zero KB");
    assert!(has(&h, "Home Net") && has(&h, "Cafe Guest"));
    assert!(has_text(
        &h,
        "macOS removes known networks in System Settings"
    ));
    click(&mut h, "Wi-Fi Settings");
    let urls = mac.env.fake_sys().unwrap().with(|s| s.opened_urls.clone());
    assert_eq!(urls, vec![privacy::WIFI_SETTINGS_URL.to_string()]);
    assert!(has_text(&h, "System Settings is open at Wi-Fi"));
}

#[test]
fn reset_permissions_flow() {
    let mac = privacy_mac();
    let fake = mac.env.fake_sys().unwrap();
    fake.answer(
        "/usr/bin/tccutil reset Camera us.zoom.xos",
        "Successfully reset\n",
    );
    let mut h = open(&mac);
    click(&mut h, "App permissions  Zero KB");
    assert!(has_text(
        &h,
        "Camera access granted to us.zoom.xos (cannot be selected"
    ));
    assert!(has_text(
        &h,
        "Screen recording access granted to com.example.Recorder"
    ));
    // Open, cancel: nothing runs.
    click(&mut h, "Reset Permissions");
    assert!(has(&h, "Reset app permissions"));
    assert!(!has(&h, "Reset Selected"), "nothing checked yet");
    click(&mut h, "Cancel");
    assert!(!fake.calls().iter().any(|c| c.contains("tccutil")));
    // Check Camera and Screen recording, reset.
    click(&mut h, "Reset Permissions");
    click(&mut h, "Camera: us.zoom.xos");
    click(&mut h, "Screen recording: com.example.Recorder");
    let scans_before = plan(&h).id;
    click(&mut h, "Reset Selected");
    let calls: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|c| c.contains("tccutil"))
        .collect();
    assert_eq!(
        calls,
        vec![
            "/usr/bin/tccutil reset Camera us.zoom.xos".to_string(),
            "/usr/bin/tccutil reset ScreenCapture com.example.Recorder".to_string(),
        ]
    );
    // The second had no scripted success: reported as failed, both logged.
    assert!(has_text(&h, "1 permissions reset, 1 failed"));
    let cmds: Vec<(String, bool)> = executor::history(&mac.env)
        .into_iter()
        .filter_map(|r| match r.kind {
            ActionKind::Command { command, ok, .. } => Some((command, ok)),
            _ => None,
        })
        .collect();
    assert_eq!(
        cmds,
        vec![
            ("tccutil reset Camera us.zoom.xos".to_string(), true),
            (
                "tccutil reset ScreenCapture com.example.Recorder".to_string(),
                false
            ),
        ]
    );
    assert_ne!(plan(&h).id, scans_before, "scanned again");
    // Log unwritable: nothing runs.
    mac.env
        .inject_fault(&executor::journal_path(&mac.env), Fault::DiskFull);
    let n = fake.calls().len();
    click(&mut h, "App permissions  Zero KB");
    click(&mut h, "Reset Permissions");
    click(&mut h, "Camera: us.zoom.xos");
    click(&mut h, "Reset Selected");
    assert!(has_text(&h, "the action log could not be written"));
    assert!(!fake.calls()[n..].iter().any(|c| c.contains("tccutil")));
}

#[test]
fn permissions_without_full_disk_access() {
    let mac = privacy_mac();
    kit::answer_tcc_denied(&mac.env, &mac.env.home_path(privacy::TCC_USER));
    kit::answer_tcc_denied(&mac.env, &mac.env.sys_path(privacy::TCC_SYSTEM));
    let h = open(&mac);
    assert!(has(&h, privacy::NOTE_TCC));
    assert!(!has(&h, "Reset Permissions"));
}
