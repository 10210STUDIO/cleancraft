//! Release check 7 (independent checker), UI side: defects proved by clicks on a FakeMac.
//! `#[ignore]` until fixed; run with
//! `cargo test -p cleancraft-ui-egui --test check7_ui -- --ignored`.
//! Details in docs/agents/reports/check7.md.

mod common;

use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit_cleanup::trash_mac;
use common::*;
use egui_kittest::kittest::Queryable;

fn count(h: &H, needle: &str) -> usize {
    h.query_all_by_label_contains(needle).count()
}

/// CHK7-3 (preview): "Empty Trash in Finder" makes Finder remove everything in the Trash for
/// good, the one permanent removal CleanCraft starts. The user's rule is a preview with the
/// exact files and sizes before anything is removed; the confirmation shows only the total.
/// It must list each item that Finder will erase, with its size.
#[test]
fn chk7_3_empty_trash_confirmation_lists_the_exact_items() {
    let mac = trash_mac();
    let mut h = harness(&mac);
    click(&mut h, "Clean");
    click(&mut h, "Trash Bins");
    click(&mut h, "Scan");
    let user = mac.env.user_trash();
    let uid = cleancraft_core::features::trash::uid(&mac.env);
    let usb = mac.env.sys_path(&format!("/Volumes/USB/.Trashes/{uid}"));
    let items = [
        user.join("old.zip"),
        user.join("Folder"),
        usb.join("movie.mov"),
    ];
    let before: Vec<usize> = items
        .iter()
        .map(|p| count(&h, &p.file_name().unwrap().to_string_lossy()))
        .collect();
    click(&mut h, "Empty Trash in Finder");
    assert!(has_text(&h, "Empty the Trash ("), "confirmation not shown");
    for (p, b) in items.iter().zip(before) {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            count(&h, &name) > b,
            "{name} is not listed in the confirmation"
        );
        assert!(
            has_text(&h, &human_size(tree_size(p).0)),
            "the size of {name} is not shown"
        );
    }
}
