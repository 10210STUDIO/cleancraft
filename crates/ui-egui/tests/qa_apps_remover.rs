//! App Remover end to end, by clicks and typing: list and groups checked against the disk,
//! uninstall and reset with the exact files in the confirmation, files really moved, undo,
//! running apps refused, Apple apps locked, leftovers and installers removed.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::executor;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use common::*;
use egui_kittest::kittest::Queryable;

struct Mac {
    mac: FakeMac,
    editor: PathBuf,
    viewer: PathBuf,
    sure: Vec<PathBuf>,
    maybe: PathBuf,
}

fn mac() -> Mac {
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        size: 3_000_000,
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    let viewer = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Viewer", "com.maker.Viewer", "1.0")
    });
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.last_opened(&editor, 2);
    mac.last_opened(&viewer, 300);
    mac.spotlight_finds_nothing();
    let sure = vec![
        mac.env
            .home_path("Library/Application Support/com.example.Editor"),
        mac.env.home_path("Library/Caches/com.example.Editor"),
        mac.env
            .home_path("Library/Preferences/com.example.Editor.plist"),
    ];
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    let maybe = mac.env.home_path("Library/Logs/Editor");
    mac.home_file("Library/Logs/Editor/run.log", 5_000);
    Mac {
        mac,
        editor,
        viewer,
        sure,
        maybe,
    }
}

fn shown(m: &Mac, p: &Path) -> String {
    m.mac.env.display(p)
}

fn count(h: &H, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

/// Click the one widget whose label contains `needle` (group rows carry their counts).
fn click_contains(h: &mut H, needle: &str) {
    let n = h.query_all_by_label_contains(needle).count();
    assert_eq!(n, 1, "'{needle}' matches {n} widgets");
    let node = h.get_by_label_contains(needle);
    let r = node.rect();
    assert!(
        r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5,
        "'{needle}' outside the window"
    );
    node.click();
    steps(h);
}

#[test]
fn list_groups_and_numbers_match_the_disk() {
    let m = mac();
    let mut h = harness(&m.mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    // Apps of macOS are hidden until asked for (lists7).
    assert!(!has(&h, "Safari"));
    click(&mut h, "Show apps that are part of macOS");
    for n in ["Editor", "Viewer", "Safari"] {
        assert!(has(&h, n), "{n} missing");
    }
    // Sizes recalculated from disk.
    assert!(has(&h, &human_size(tree_size(&m.editor).0)));
    let all: u64 = [&m.editor, &m.viewer]
        .iter()
        .map(|p| tree_size(p).0)
        .sum::<u64>()
        + tree_size(&m.mac.env.sys_path("/Applications/Safari.app")).0;
    assert!(has_text(
        &h,
        &format!("All Apps  3  ·  {}", human_size(all))
    ));
    assert!(has_text(&h, "Part of macOS"));
    // Unused: only Viewer (300 days).
    click_contains(&mut h, "Unused  ");
    assert!(has(&h, "Viewer"));
    assert!(!has(&h, "Editor"));
    click_contains(&mut h, "From App Store  ");
    assert!(has(&h, "Viewer") && !has(&h, "Editor"));
    click_contains(&mut h, "All Apps  ");
    // Search, then Enter opens the first match, Esc goes back.
    type_into(&mut h, "Search apps", "edi");
    assert!(has(&h, "Editor") && !has(&h, "Viewer"));
    h.key_press(egui::Key::Enter);
    steps(&mut h);
    assert!(has(&h, "Back to Apps"), "Enter opens the app");
    assert!(has(&h, &shown(&m, &m.sure[1])));
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(!has(&h, "Back to Apps"), "Esc goes back");
}

#[test]
fn uninstall_moves_app_and_leftovers_then_undo() {
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    // Every file listed, high-confidence ones selected, the name match not.
    for p in &m.sure {
        assert!(has(&h, &shown(&m, p)), "{} not listed", p.display());
    }
    assert!(has(&h, &shown(&m, &m.maybe)));
    let sel_size: u64 = std::iter::once(&m.editor)
        .chain(&m.sure)
        .map(|p| tree_size(p).0)
        .sum();
    assert!(
        has(&h, &format!("4 selected, {}", human_size(sel_size))),
        "selection total"
    );
    // Keep the cache.
    click(&mut h, &shown(&m, &m.sure[1]));
    let moved = [m.editor.clone(), m.sure[0].clone(), m.sure[2].clone()];
    let size: u64 = moved.iter().map(|p| tree_size(p).0).sum();
    assert!(has(&h, &format!("3 selected, {}", human_size(size))));
    click(&mut h, "Uninstall");
    assert!(has_text(&h, "Uninstall Editor? 3 items"));
    // Listed in the dialog and behind it: twice. The kept cache: once (behind).
    assert_eq!(count(&h, &shown(&m, &m.sure[0])), 2);
    assert_eq!(
        count(&h, &shown(&m, &m.editor)),
        2,
        "dialog lists the bundle"
    );
    assert_eq!(count(&h, &shown(&m, &m.sure[1])), 1);
    click(&mut h, "Move to Trash");
    for p in &moved {
        assert!(!p.exists(), "{} not moved", p.display());
    }
    assert!(m.sure[1].exists() && m.maybe.exists());
    assert!(m.mac.env.user_trash().join("Editor.app").exists());
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    assert_eq!(executor::history(&m.mac.env).len(), 3);
    click(&mut h, "Undo");
    for p in &moved {
        assert!(p.exists(), "{} not restored", p.display());
    }
    assert!(has_text(&h, "3 items put back."));
    // The app is back and its detail shows again.
    assert!(has(&h, "Back to Apps"));
    // Uninstall for real, then Done returns to the list without it.
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    click(&mut h, "Done");
    assert!(!m.editor.exists());
    assert!(!has(&h, "Editor"), "removed app gone from the list");
    assert!(has(&h, "Viewer"));
}

fn m_h(h: &mut H) -> &mut H {
    // The tool sits in the "Apps" section of the sidebar.
    click(h, "Apps");
    click(h, "App Remover");
    h
}

#[test]
fn reset_keeps_the_app() {
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    click(&mut h, "Reset App...");
    assert!(has_text(&h, "Reset Editor? 3 items"));
    assert_eq!(
        count(&h, &shown(&m, &m.editor)),
        1,
        "bundle not in the dialog"
    );
    click(&mut h, "Move to Trash");
    assert!(m.editor.exists());
    for p in &m.sure {
        assert!(!p.exists());
    }
    assert!(has_text(&h, "Reset done. 3 items moved."));
}

#[test]
fn cancel_and_escape_move_nothing() {
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    click(&mut h, "Uninstall");
    click(&mut h, "Cancel");
    assert!(!has(&h, "Move to Trash"));
    click(&mut h, "Uninstall");
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(!has(&h, "Move to Trash"), "Esc closes the dialog");
    assert!(has(&h, "Back to Apps"), "and stays on the app");
    assert!(m.editor.exists() && m.sure.iter().all(|p| p.exists()));
}

#[test]
fn running_app_is_refused() {
    let m = mac();
    // Running before opening: everything locked, Uninstall disabled.
    m.mac.run_app(&m.editor);
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    // Said once, in the banner, with the way forward.
    assert!(has_text(&h, "of Editor is running") || has(&h, "Editor is running"));
    assert!(has(&h, "Quit Editor"));
    h.get_by_label("Uninstall").click();
    steps(&mut h);
    assert!(
        !has(&h, "Move to Trash"),
        "disabled button opened the dialog"
    );
    assert!(m.editor.exists());

    // Opened after the list was shown: refused when moving, nothing moved.
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    click(&mut h, "Uninstall");
    m.mac.run_app(&m.editor);
    click(&mut h, "Move to Trash");
    assert!(has_text(&h, "Quit Editor first. Nothing was moved."));
    assert!(m.editor.exists() && m.sure.iter().all(|p| p.exists()));
    assert!(executor::history(&m.mac.env).is_empty());
    // The detail now shows the lock.
    assert!(has(&h, "Quit Editor"));
}

#[test]
fn apple_app_is_locked() {
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Show apps that are part of macOS");
    click(&mut h, "Safari");
    assert!(has_text(
        &h,
        "Safari is part of macOS and cannot be removed"
    ));
    h.get_by_label("Uninstall").click();
    steps(&mut h);
    assert!(!has(&h, "Move to Trash"));
    assert!(m.mac.env.sys_path("/Applications/Safari.app").exists());
}

#[test]
fn file_errors_are_listed_per_file() {
    let m = mac();
    let mut h = harness(&m.mac);
    click(m_h(&mut h), "Editor");
    m.mac
        .env
        .inject_fault(&m.sure[0], cleancraft_core::Fault::PermissionDenied);
    m.mac
        .env
        .fake_sys()
        .unwrap()
        .set_open(&m.sure[1].join("blob"));
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(m.sure[0].exists() && m.sure[1].exists());
    assert!(!m.editor.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has_text(&h, "2 items moved. 2 skipped."));
}

#[test]
fn leftovers_and_installers_groups() {
    let m = mac();
    let gone = m.mac.env.home_path("Library/Caches/com.gone.Writer");
    m.mac.home_file("Library/Caches/com.gone.Writer/c", 60_000);
    let pkg = m.mac.home_file("Downloads/Tool-1.0.pkg", 80_000);
    let pane = m
        .mac
        .env
        .home_path("Library/PreferencePanes/Thing.prefPane");
    m.mac
        .home_file("Library/PreferencePanes/Thing.prefPane/Contents/x", 20_000);
    let mut h = harness(&m.mac);
    // The tool sits in the "Apps" section of the sidebar.
    click(&mut h, "Apps");
    click(&mut h, "App Remover");
    assert!(has_text(
        &h,
        &format!("Leftovers  1  ·  {}", human_size(tree_size(&gone).0))
    ));
    click_contains(&mut h, "Leftovers  ");
    click(&mut h, &shown(&m, &gone));
    click_contains(&mut h, "Installers  ");
    click(&mut h, &shown(&m, &pkg));
    click_contains(&mut h, "Extensions  ");
    assert!(has_text(&h, "Settings pane"));
    assert!(has_text(&h, "Safari extensions could not be listed."));
    click(&mut h, &shown(&m, &pane));
    let size = tree_size(&gone).0 + tree_size(&pkg).0 + tree_size(&pane).0;
    assert!(has(&h, &format!("3 selected, {}", human_size(size))));
    click(&mut h, "Remove Selected");
    assert!(has_text(&h, "Move 3 items"));
    click(&mut h, "Move to Trash");
    assert!(!gone.exists() && !pkg.exists() && !pane.exists());
    assert!(has(&h, &format!("{} moved to the Trash", human_size(size))));
    click(&mut h, "Done");
    // Refreshed: the groups are empty now.
    assert!(has_text(&h, "Leftovers  0"));
    assert!(has_text(&h, "Installers  0"));
    assert!(has_text(&h, "Extensions  0"));
}
