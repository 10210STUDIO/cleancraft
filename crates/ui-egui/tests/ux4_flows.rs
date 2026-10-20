//! ux4 round 4, end to end on a fake Mac:
//! - after an uninstall every list follows at once (the user's bug): the app leaves All Apps
//!   and its groups with counts equal to a fresh look, the page returns to the list, Disk Map
//!   drops the moved blocks with sizes equal to a fresh scan, History shows the action, and
//!   Undo from History brings the app back on the next look;
//! - a running app: one banner says it once (no "Quit X first" on every row), Quit X asks
//!   it to quit, the rows unlock by themselves when it is gone, Force Quit asks first.

mod common;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use cleancraft_core::apps;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::space;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
use cleancraft_core::testkit_cleanup;
use cleancraft_core::testkit_quit::FakeMacQuit;
use common::*;
use egui_kittest::kittest::{NodeT, Queryable};

struct Mac {
    mac: FakeMac,
    editor: PathBuf,
    leftovers: Vec<PathBuf>,
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
    mac.last_opened(&editor, 400);
    mac.last_opened(&viewer, 300);
    mac.spotlight_finds_nothing();
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Documents/keep.txt", 10_000);
    let f = mac.env.fake_sys().unwrap();
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
    let leftovers = vec![
        mac.env
            .home_path("Library/Application Support/com.example.Editor"),
        mac.env.home_path("Library/Caches/com.example.Editor"),
        mac.env
            .home_path("Library/Preferences/com.example.Editor.plist"),
    ];
    Mac {
        mac,
        editor,
        leftovers,
    }
}

fn go_remover(h: &mut H) {
    click(h, "Apps");
    click(h, "App Remover");
}

/// The All Apps and Unused rows as a fresh look at the disk would show them.
fn fresh_group_texts(m: &FakeMac) -> (String, String) {
    let list = apps::inventory(&m.env, true, &|_| {});
    let now = chrono::Utc::now().timestamp();
    let all: u64 = list.iter().map(|a| a.size).sum();
    let unused: Vec<_> = list.iter().filter(|a| a.unused(now) && !a.apple).collect();
    let unused_size: u64 = unused.iter().map(|a| a.size).sum();
    (
        format!("All Apps  {}  ·  {}", list.len(), human_size(all)),
        if unused.is_empty() {
            "Unused  0".to_string()
        } else {
            format!("Unused  {}  ·  {}", unused.len(), human_size(unused_size))
        },
    )
}

fn disk_map_root_size(h: &H) -> u64 {
    h.state()
        .disk_map
        .tree()
        .and_then(|t| t.root.as_ref())
        .map(|n| n.size)
        .expect("a mapped tree")
}

fn fresh_home_size(m: &FakeMac) -> u64 {
    space::build(m.env.home(), &AtomicBool::new(false), &|_, _| {})
        .root
        .map(|n| n.size)
        .unwrap_or(0)
}

#[test]
fn uninstall_updates_every_list_and_undo_brings_it_back() {
    let m = mac();
    let mut h = harness(&m.mac);
    // Disk Map measured before the uninstall.
    click(&mut h, "Files");
    h.state_mut().navigate("disk_map");
    steps(&mut h);
    click(&mut h, "Map Home Folder");
    assert_eq!(disk_map_root_size(&h), fresh_home_size(&m.mac));
    let caches = m.mac.env.home_path("Library/Caches");
    assert!(
        h.state()
            .disk_map
            .tree()
            .unwrap()
            .find(&m.leftovers[1])
            .is_some()
    );

    // App Remover: Editor is in All Apps and Unused.
    go_remover(&mut h);
    let (all, unused) = fresh_group_texts(&m.mac);
    assert!(has_text(&h, &all), "{all}");
    assert!(has_text(&h, &unused), "{unused}");
    assert!(has(&h, "Editor"));
    // Scan, select, confirm, done, back.
    click(&mut h, "Editor");
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(!m.editor.exists());
    assert!(m.leftovers.iter().all(|p| !p.exists()));
    click(&mut h, "Done");
    assert!(!has(&h, "Back to Apps"), "Done returns to the list");
    assert!(!has(&h, "Editor"), "the removed app left the list");
    let (all, unused) = fresh_group_texts(&m.mac);
    assert!(has_text(&h, &all), "All Apps equals a fresh look: {all}");
    assert!(
        has_text(&h, &unused),
        "Unused equals a fresh look: {unused}"
    );
    // The Unused group no longer lists it either.
    h.get_by_label_contains("Unused  ").click();
    steps(&mut h);
    assert!(!has(&h, "Editor"));
    assert!(has(&h, "Viewer"));

    // Disk Map: the moved blocks are gone, the sizes equal a fresh scan.
    h.state_mut().navigate("disk_map");
    steps(&mut h);
    let tree = h.state().disk_map.tree().unwrap();
    assert!(
        tree.find(&m.leftovers[1]).is_none(),
        "moved cache still mapped"
    );
    assert!(tree.find(&caches).is_some());
    assert_eq!(disk_map_root_size(&h), fresh_home_size(&m.mac));

    // History: the uninstall is the newest action, with each file.
    click(&mut h, "History");
    assert!(has_text(&h, "App Remover"));
    assert!(has(&h, &m.mac.env.display(&m.editor)));
    assert!(has(&h, "Undo All"));
    click(&mut h, "Undo All");
    assert!(m.editor.exists() && m.leftovers.iter().all(|p| p.exists()));
    assert!(has_text(&h, "4 items put back."));

    // Back in App Remover, the next look lists it again with the right numbers.
    go_remover(&mut h);
    assert!(has(&h, "Editor"), "undo brings the app back");
    let (all, _) = fresh_group_texts(&m.mac);
    assert!(has_text(&h, &all), "{all}");
    // And Disk Map maps it again.
    h.state_mut().navigate("disk_map");
    steps(&mut h);
    assert!(
        h.state()
            .disk_map
            .tree()
            .unwrap()
            .find(&m.leftovers[1])
            .is_some()
    );
    assert_eq!(disk_map_root_size(&h), fresh_home_size(&m.mac));
}

#[test]
fn removing_leftover_files_updates_their_group() {
    let m = mac();
    // Files of an app that is gone: the Leftovers group.
    let orphan = m
        .mac
        .home_file("Library/Caches/com.gone.OldApp/blob", 250_000);
    let orphan_dir = orphan.parent().unwrap().to_path_buf();
    let mut h = harness(&m.mac);
    go_remover(&mut h);
    let before = h.state().apps.extras.as_ref().unwrap().items.len();
    if !h
        .state()
        .apps
        .extras
        .as_ref()
        .unwrap()
        .items
        .iter()
        .any(|i| i.path == orphan_dir)
    {
        panic!(
            "not listed: {:?}",
            h.state()
                .apps
                .extras
                .as_ref()
                .unwrap()
                .items
                .iter()
                .map(|i| &i.path)
                .collect::<Vec<_>>()
        );
    }
    h.get_by_label_contains("Leftovers  ").click();
    steps(&mut h);
    let shown = m.mac.env.display(&orphan_dir);
    if !h
        .state()
        .apps
        .extras
        .as_ref()
        .unwrap()
        .items
        .iter()
        .any(|i| i.path == orphan_dir && i.selected)
    {
        click(&mut h, &shown);
    }
    click(&mut h, "Remove Selected");
    click(&mut h, "Move to Trash");
    click(&mut h, "Done");
    assert!(!orphan_dir.exists());
    assert!(!has(&h, &shown), "moved leftover still listed");
    assert_eq!(
        h.state().apps.extras.as_ref().unwrap().items.len(),
        before - 1
    );
}

#[test]
fn running_app_banner_quits_and_rows_unlock() {
    let m = mac();
    // The app's own program runs (as `ps` shows it).
    m.mac.user_id(501);
    m.mac
        .run_program(&m.editor.join("Contents/MacOS/Editor"), 4242, 501);
    let f = m.mac.env.fake_sys().unwrap();
    f.answer("/usr/bin/osascript", "");
    f.answer("/bin/kill", "");
    let mut h = harness(&m.mac);
    go_remover(&mut h);
    click(&mut h, "Editor");
    // Said once, in the banner; the rows carry a padlock, not the sentence.
    assert!(has(&h, "Editor is running"));
    assert!(!has_text(&h, "(Quit Editor first)"));
    assert!(has(&h, "Quit Editor"));
    // Every leftover is locked while it runs.
    for p in &m.leftovers {
        let shown = m.mac.env.display(p);
        assert!(
            h.get_by_label(&shown).accesskit_node().is_disabled(),
            "{shown} should be locked"
        );
    }

    // Quit is asked, but the app is still open (a save dialog): Force Quit is offered.
    click(&mut h, "Quit Editor");
    assert!(
        f.calls()
            .iter()
            .any(|c| c.contains("osascript") && c.contains("Editor"))
    );
    assert!(has(&h, "Editor did not quit"));
    assert!(has(&h, "Force Quit"));
    // Force Quit asks first; Cancel changes nothing.
    click(&mut h, "Force Quit");
    assert!(has(&h, "Force quit Editor?"));
    click(&mut h, "Cancel");
    assert!(!f.calls().iter().any(|c| c.starts_with("/bin/kill")));

    // The person quits it themselves: the rows unlock with no click, defaults selected.
    testkit_cleanup::clear_running(&m.mac.env);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while has(&h, "Editor did not quit") && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(200));
        steps(&mut h);
    }
    assert!(
        !has(&h, "Editor did not quit"),
        "banner stayed after the app quit"
    );
    assert!(has_text(&h, "Editor has quit."));
    for p in &m.leftovers {
        let shown = m.mac.env.display(p);
        assert!(
            !h.get_by_label(&shown).accesskit_node().is_disabled(),
            "{shown} still locked"
        );
    }
    // Files named after the id are selected for you again.
    assert!(has_text(&h, "4 selected"));
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    assert!(!m.editor.exists());
}

#[test]
fn force_quit_after_confirmation() {
    let m = mac();
    // The app's own program runs (as `ps` shows it).
    m.mac.user_id(501);
    m.mac
        .run_program(&m.editor.join("Contents/MacOS/Editor"), 4242, 501);
    let f = m.mac.env.fake_sys().unwrap();
    f.answer("/usr/bin/osascript", "");
    f.answer("/bin/kill", "");
    let mut h = harness(&m.mac);
    go_remover(&mut h);
    click(&mut h, "Editor");
    click(&mut h, "Quit Editor");
    click(&mut h, "Force Quit");
    // The dialog's button.
    let n = h.query_all_by_label("Force Quit").count();
    h.get_all_by_label("Force Quit").nth(n - 1).unwrap().click();
    steps(&mut h);
    assert!(f.calls().iter().any(|c| c.starts_with("/bin/kill")));
}
