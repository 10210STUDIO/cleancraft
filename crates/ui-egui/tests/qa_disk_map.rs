//! Disk Map by clicks: map the home folder, numbers against a recalculation, drill down by
//! list and by block, Back and breadcrumbs, locked places, select, review the exact list,
//! files really move, sizes refreshed, undo; typed folder checked; permission denied and file
//! in use.

mod common;

use cleancraft_core::Fault;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use common::*;
use egui_kittest::kittest::Queryable;

fn mac() -> FakeMac {
    let mac = FakeMac::sample();
    mac.home_file("Movies/film.mov", 3_000_000);
    mac.home_file("Projects/app/src/main.rs", 40_000);
    mac.home_file("Projects/app/target/big.bin", 1_200_000);
    mac.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    mac
}

fn current(h: &H) -> std::path::PathBuf {
    h.state().disk_map.current.clone().unwrap()
}

#[test]
fn map_drill_select_remove_undo() {
    let mac = mac();
    let env = mac.env.clone();
    let mut h = harness(&mac);
    // The tool sits in the "Files" section of the sidebar.
    click(&mut h, "Files");
    click(&mut h, "Disk Map");
    assert!(has(
        &h,
        &format!(
            "Startup disk: {} used of {}, {} free",
            human_size(300_000_000 * 1024),
            human_size(488_245_288 * 1024),
            human_size(188_245_288 * 1024)
        )
    ));
    click(&mut h, "Map Home Folder");
    let home = env.home().to_path_buf();
    assert!(has(&h, &human_size(tree_size(&home).0)), "home total");
    for c in cleancraft_core::fsutil::children(&home) {
        assert!(has(&h, &human_size(tree_size(&c).0)), "{}", c.display());
    }
    // Locked places cannot be ticked.
    click(&mut h, "Select Documents");
    click(&mut h, "Select Library");
    assert!(h.state().disk_map.selected.is_empty());
    assert!(has_text(&h, "locked: protected: the contents of Documents"));
    // Drill down by the list, then by a block.
    click(&mut h, "Projects");
    assert_eq!(current(&h), env.home_path("Projects"));
    assert!(has(&h, "Go to Projects"));
    click(&mut h, "Block app");
    assert_eq!(current(&h), env.home_path("Projects/app"));
    let target = env.home_path("Projects/app/target");
    assert!(has(&h, &human_size(tree_size(&target).0)));
    click(&mut h, "Select target");
    click(&mut h, "Back");
    assert_eq!(current(&h), env.home_path("Projects"));
    click(&mut h, "Go to Home folder");
    assert_eq!(current(&h), home);
    click(&mut h, "Movies");
    click(&mut h, "Select film.mov");
    let film = env.home_path("Movies/film.mov");
    let sel = tree_size(&target).0 + tree_size(&film).0;
    assert!(has(&h, &format!("2 selected, {}", human_size(sel))));
    assert!(has(&h, &format!("Selected: {}", human_size(sel))));
    click(&mut h, "Review and Remove");
    assert!(has(
        &h,
        &format!("Move 2 items ({}) to the Trash?", human_size(sel))
    ));
    assert!(has(&h, &env.display(&target)));
    assert!(has(&h, &env.display(&film)));
    click(&mut h, "Move to Trash");
    assert!(!target.exists() && !film.exists());
    assert!(env.user_trash().join("target").exists());
    assert!(has(&h, &format!("{} moved to the Trash", human_size(sel))));
    click(&mut h, "Done");
    // The map shows the new sizes.
    click(&mut h, "Go to Home folder");
    let projects = env.home_path("Projects");
    assert!(has(&h, &human_size(tree_size(&projects).0)));
    let t = h.state().disk_map.tree().unwrap().clone();
    assert_eq!(t.find(&projects).unwrap().size, tree_size(&projects).0);
    assert_eq!(t.find(&home).unwrap().size, tree_size(&home).0);
    assert!(t.find(&target).is_none());
    // Undo from a second removal's Done screen puts things back.
    click(&mut h, "Go to Home folder");
    click(&mut h, "Select Projects");
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(!projects.exists());
    click(&mut h, "Undo");
    assert!(projects.exists());
    assert!(has_text(&h, "1 item put back."));
    let t = h.state().disk_map.tree().unwrap().clone();
    assert_eq!(t.find(&home).unwrap().size, tree_size(&home).0);
}

#[test]
fn cancel_keeps_everything() {
    let mac = mac();
    let mut h = harness(&mac);
    // The tool sits in the "Files" section of the sidebar.
    click(&mut h, "Files");
    click(&mut h, "Disk Map");
    click(&mut h, "Map Home Folder");
    click(&mut h, "Select Movies");
    click(&mut h, "Review and Remove");
    click(&mut h, "Cancel");
    assert!(mac.env.home_path("Movies/film.mov").exists());
    assert!(has(&h, "Review and Remove"));
}

#[test]
fn typed_folder_and_errors() {
    let mac = mac();
    let env = mac.env.clone();
    let a = mac.home_file("Work/a.bin", 400_000);
    let b = mac.home_file("Work/b.bin", 500_000);
    let mut h = harness(&mac);
    // The tool sits in the "Files" section of the sidebar.
    click(&mut h, "Files");
    click(&mut h, "Disk Map");
    type_into(&mut h, "Folder to map", "~/Nope");
    click(&mut h, "Map Folder");
    assert!(has_text(&h, "There is no folder"));
    h.get_by_label("Folder to map").focus();
    h.key_press(egui::Key::Escape);
    steps(&mut h);
    assert!(!has_text(&h, "There is no folder"));
    type_into(&mut h, "Folder to map", "~/Work");
    h.key_press(egui::Key::Enter);
    steps(&mut h);
    assert_eq!(current(&h), env.home_path("Work"));
    assert!(has(&h, &human_size(tree_size(&env.home_path("Work")).0)));
    click(&mut h, "Select a.bin");
    click(&mut h, "Select b.bin");
    env.inject_fault(&a, Fault::PermissionDenied);
    env.fake_sys().unwrap().set_open(&b);
    click(&mut h, "Review and Remove");
    click(&mut h, "Move to Trash");
    assert!(a.exists() && b.exists());
    assert!(has(&h, "permission denied"));
    assert!(has(&h, "open in a running app"));
    assert!(has(&h, "0 items moved. 2 skipped."));
}
