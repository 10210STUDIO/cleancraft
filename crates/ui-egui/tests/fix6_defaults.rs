//! Defaults the user chose (fix6): browser cookies (removing them signs the user out of
//! websites) and Mail attachments are never preselected. The user can still tick them, the run
//! moves exactly what was ticked, the sign-out warning appears only when cookies are ticked,
//! and Quick Sweep keeps the same defaults: its header counts only what is selected while the
//! Mail tile still says what was found.

mod common;

use cleancraft_core::features::privacy;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::testkit_cleanup::{mail_mac, privacy_mac};
use cleancraft_core::{FeatureId, Plan, Risk};
use cleancraft_ui_egui::app::Page;
use cleancraft_ui_egui::pages::{plan_page, sweep};
use common::*;
use egui_kittest::kittest::Queryable;

const CHROME: &str = "Library/Application Support/Google/Chrome";
const FF: &str = "Library/Application Support/Firefox/Profiles/abcd.default-release";
const SIGN_OUT: &str = "You will be signed out of websites";

fn page_plan(h: &H, f: FeatureId) -> Plan {
    h.state().plans[&f].plan.clone().expect("plan")
}

fn sweep_plan(h: &H, f: FeatureId) -> Plan {
    h.state()
        .sweep
        .result
        .as_ref()
        .expect("scanned")
        .tile(f)
        .and_then(|t| t.plan())
        .expect("plan tile")
        .clone()
}

/// Click an item's checkbox (named after the file) on the plan page.
fn tick(h: &mut H, name: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, name)
        .click();
    steps(h);
}

#[test]
fn cookies_unselected_and_sign_out_warning_only_when_ticked() {
    let mac = privacy_mac();
    let mut h = harness(&mac);
    click(&mut h, "Protect");
    click(&mut h, "Privacy Traces");
    click(&mut h, "Scan");
    let p = page_plan(&h, FeatureId::Privacy);
    let cookies: Vec<_> = p
        .items
        .iter()
        .filter(|i| i.group.as_deref() == Some("Cookies"))
        .map(|i| i.path.clone())
        .collect();
    assert!(cookies.len() >= 3, "Safari, Chrome and Firefox cookies");
    assert!(
        !p.selected().any(privacy::signs_out),
        "a cookie is preselected"
    );
    // History and caches keep their defaults.
    for rel in [
        "Library/Safari/History.db",
        "Library/Caches/com.apple.Safari",
    ] {
        let it = p.items.iter().find(|i| i.path == mac.env.home_path(rel));
        assert!(it.is_some_and(|i| i.selected), "{rel}");
    }

    // Defaults: the confirmation has no sign-out warning.
    click(&mut h, "Review and Clean");
    assert!(has(&h, "Move to Trash"));
    assert!(!has_text(&h, SIGN_OUT), "warning without a ticked cookie");
    click(&mut h, "Cancel");
    assert!(cookies.iter().all(|c| c.exists()));

    // The user ticks Firefox's cookies: the warning names Firefox only.
    let ff_size: u64 = p
        .items
        .iter()
        .filter(|i| i.category == "firefox")
        .map(|i| i.size)
        .sum();
    click(&mut h, &format!("Firefox  {}", human_size(ff_size)));
    tick(&mut h, "cookies.sqlite");
    let p = page_plan(&h, FeatureId::Privacy);
    let ff_cookies = mac.env.home_path(&format!("{FF}/cookies.sqlite"));
    assert!(p.items.iter().any(|i| i.path == ff_cookies && i.selected));
    click(&mut h, "Review and Clean");
    assert!(has(&h, "You will be signed out of websites in Firefox."));
    let sel: Vec<_> = p.selected().map(|i| i.path.clone()).collect();
    click(&mut h, "Move to Trash");
    // Exactly the selection moved: Firefox's cookies, not the other browsers'.
    assert!(!ff_cookies.exists());
    for i in &p.items {
        if i.size > 0 {
            assert_eq!(
                i.path.exists(),
                !sel.contains(&i.path),
                "{}",
                i.path.display()
            );
        }
    }
    assert!(
        mac.env
            .home_path(&format!("{CHROME}/Default/Cookies"))
            .exists()
    );
}

#[test]
fn sign_out_note_is_for_ticked_privacy_cookies_only() {
    let mut p = Plan::new(FeatureId::Privacy);
    p.add_category("chrome", "Google Chrome", "", Risk::Medium);
    p.add_category("safari", "Safari", "", Risk::Medium);
    let c = p.push(
        "/u/Chrome/Default/Cookies".into(),
        10,
        "chrome",
        "Google Chrome (Default profile) cookies (you will be signed out of websites): Cookies",
        Risk::Medium,
        false,
    );
    p.items[c].group = Some("Cookies".into());
    let s = p.push(
        "/u/Safari/Cookies.binarycookies".into(),
        10,
        "safari",
        "Safari cookies (you will be signed out of websites): Cookies.binarycookies",
        Risk::Medium,
        false,
    );
    p.items[s].group = Some("Cookies".into());
    let hist = p.push(
        "/u/Safari/History.db".into(),
        10,
        "safari",
        "Safari browsing history and download list: History.db",
        Risk::Medium,
        true,
    );
    p.items[hist].group = Some("History".into());
    assert_eq!(plan_page::signed_out_note(&p), None);
    p.set_selected(c, true);
    assert_eq!(
        plan_page::signed_out_note(&p).as_deref(),
        Some("You will be signed out of websites in Google Chrome.")
    );
    p.set_selected(s, true);
    assert_eq!(
        plan_page::signed_out_note(&p).as_deref(),
        Some("You will be signed out of websites in Google Chrome and Safari.")
    );
    // An app's leftover Cookies folder in App Remover is not a browser sign-out.
    let mut a = Plan::new(FeatureId::Uninstaller);
    a.add_category("leftovers", "Leftovers", "", Risk::High);
    a.push(
        "/u/Library/Cookies/com.example.app.binarycookies".into(),
        10,
        "leftovers",
        "Cookies of Example",
        Risk::High,
        true,
    );
    assert_eq!(plan_page::signed_out_note(&a), None);
}

#[test]
fn quick_sweep_keeps_mail_unselected_and_shows_what_was_found() {
    let mac = mail_mac();
    mac.home_file("Library/Caches/com.example.Browser/Cache.db", 1_200_000);
    let mut h = harness(&mac);
    click(&mut h, "Scan");
    let mail = sweep_plan(&h, FeatureId::MailAttachments);
    let junk = sweep_plan(&h, FeatureId::SystemJunk);
    assert!(mail.items.len() >= 4);
    assert_eq!(mail.selected_count(), 0, "Quick Sweep reselected mail");
    // The tile: nothing ready to clean, but what was found is shown.
    assert!(has(&h, &human_size(0)));
    let line = sweep::sweep_tile_line(&mail);
    assert_eq!(
        line,
        format!(
            "0 of {} items selected, {} found",
            mail.items.len(),
            human_size(mail.total_size())
        )
    );
    assert!(has(&h, &line));
    // "ready to clean" counts only the selection (System Junk here).
    let res = h.state().sweep.result.clone().unwrap();
    assert_eq!(
        res.ready_bytes(&h.state().settings.sweep_skip),
        junk.selected_size()
    );
    assert!(has(&h, "ready to clean"));
    assert!(
        res.to_run(&[])
            .iter()
            .all(|p| p.feature != Some(FeatureId::MailAttachments)),
        "Run would include unticked mail"
    );

    // Review opens Mail Downloads with nothing ticked; the user ticks one attachment there.
    let ids: Vec<FeatureId> = res.tiles.iter().map(|t| t.id).collect();
    let mail_i = ids
        .iter()
        .position(|i| *i == FeatureId::MailAttachments)
        .unwrap();
    let l = tile_label(&h, "Review", mail_i);
    click(&mut h, &l);
    assert_eq!(h.state().page, Page::Feature(FeatureId::MailAttachments));
    assert_eq!(
        page_plan(&h, FeatureId::MailAttachments).selected_count(),
        0
    );
    assert!(has(&h, "0 selected, 0 KB"));
    // Only the opened copies, so the row is on screen.
    let opened: u64 = page_plan(&h, FeatureId::MailAttachments)
        .items
        .iter()
        .filter(|i| i.category == "mail_opened")
        .map(|i| i.size)
        .sum();
    click(
        &mut h,
        &format!("Opened attachments  {}", human_size(opened)),
    );
    tick(&mut h, "Invoice.pdf");
    let p = page_plan(&h, FeatureId::MailAttachments);
    assert_eq!(p.selected_count(), 1);
    let inv = p.selected().next().unwrap().path.clone();
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(!inv.exists());
    for i in p.items.iter().filter(|i| i.path != inv) {
        assert!(i.path.exists(), "{} moved without a tick", i.path.display());
    }

    // Back in Quick Sweep the Mail tile drops the moved file; scanning again still selects
    // no attachment.
    click(&mut h, "Quick Sweep");
    let mail_after = sweep_plan(&h, FeatureId::MailAttachments);
    assert_eq!(mail_after.items.len(), mail.items.len() - 1);
    assert!(has(&h, &sweep::sweep_tile_line(&mail_after)));
    click(&mut h, "Scan Again");
    let rescanned = sweep_plan(&h, FeatureId::MailAttachments);
    assert_eq!(rescanned.items.len(), mail.items.len() - 1);
    assert_eq!(rescanned.selected_count(), 0);
}
