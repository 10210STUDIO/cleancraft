//! Duplicates and Similar Images by clicks: set headers with numbers recalculated from disk,
//! the suggested copy kept, every copy can never be selected (UI and core), Select Copies
//! menu, files really moved, undo.

mod common;

use cleancraft_core::FeatureId;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::testkit_files::{ImageFormat, save_scene};
use common::*;

fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_add(7);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

fn plan(h: &H, f: FeatureId) -> cleancraft_core::Plan {
    h.state().plans[&f].plan.clone().expect("plan")
}

fn selected(h: &H, f: FeatureId, p: &std::path::Path) -> bool {
    plan(h, f)
        .items
        .iter()
        .find(|i| i.path == p)
        .unwrap()
        .selected
}

#[test]
fn duplicate_sets_keep_one_copy() {
    let mac = FakeMac::new();
    let da = bytes(1, 300_000);
    // Rows inside a set follow plan order: Downloads, Music, Projects.
    let a_dl = mac.env.home_path("Downloads/a.bin");
    let a_music = mac.env.home_path("Music/old/a.bin");
    let a_keep = mac.env.home_path("Projects/a.bin");
    for p in [&a_dl, &a_music, &a_keep] {
        mac.write(p, &da);
    }
    let db = bytes(2, 200_000);
    let b1 = mac.env.home_path("Pictures/b1.jpg");
    let b2 = mac.env.home_path("Pictures/b2.jpg");
    mac.write(&b1, &db);
    mac.write(&b2, &db);
    age(&b1, 5);
    age(&b2, 10);
    let mut h = harness(&mac);
    click(&mut h, "Duplicates");
    assert!(has(&h, "Home folder"), "location picker missing");
    click(&mut h, "Scan");
    let sa = tree_size(&a_dl).0;
    let sb = tree_size(&b1).0;
    assert!(has(&h, &human_size(3 * sa + 2 * sb)), "total");
    assert!(has(
        &h,
        &format!(
            "3 identical files: 1 kept, {} to remove",
            human_size(2 * sa)
        )
    ));
    assert!(has(
        &h,
        &format!("2 identical files: 1 kept, {} to remove", human_size(sb))
    ));
    assert!(!selected(&h, FeatureId::Duplicates, &a_keep));
    assert!(selected(&h, FeatureId::Duplicates, &a_dl));
    assert!(
        !selected(&h, FeatureId::Duplicates, &b2),
        "older copy is the suggestion"
    );
    // The kept copy cannot be selected while the others are.
    click_nth(&mut h, "a.bin", 2);
    assert!(!selected(&h, FeatureId::Duplicates, &a_keep));
    assert!(has_text(&h, "Keep at least one file in each set"));
    // Deselect another copy first, then it works.
    click_nth(&mut h, "a.bin", 0);
    click_nth(&mut h, "a.bin", 2);
    assert!(selected(&h, FeatureId::Duplicates, &a_keep));
    assert!(!selected(&h, FeatureId::Duplicates, &a_dl));
    // Select All still keeps one per set.
    click(&mut h, "Select All");
    let p = plan(&h, FeatureId::Duplicates);
    for ids in cleancraft_core::features::duplicates::groups(&p).values() {
        assert_eq!(ids.iter().filter(|i| !p.items[**i].selected).count(), 1);
    }
    assert!(has_text(&h, "stays unselected"));
    // Select Copies > Keep Newest.
    click(&mut h, "Select Copies");
    click(&mut h, "Keep Newest");
    assert!(!selected(&h, FeatureId::Duplicates, &b1));
    assert!(selected(&h, FeatureId::Duplicates, &b2));
    click(&mut h, "Select Copies");
    click(&mut h, "Keep Suggested");
    assert!(!selected(&h, FeatureId::Duplicates, &a_keep));
    let p = plan(&h, FeatureId::Duplicates);
    let expect: u64 = p.selected().map(|i| tree_size(&i.path).0).sum();
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(a_keep.exists() && !a_dl.exists() && !a_music.exists());
    assert!(b2.exists() && !b1.exists());
    assert!(has(&h, &format!("{} freed", human_size(expect))));
    click(&mut h, "Undo");
    assert!(a_dl.exists() && a_music.exists() && b1.exists());
}

#[test]
fn every_copy_selected_is_refused_by_core() {
    let mac = FakeMac::new();
    let d = bytes(3, 150_000);
    let x = mac.env.home_path("Movies/x.bin");
    let y = mac.env.home_path("Projects/x.bin");
    mac.write(&x, &d);
    mac.write(&y, &d);
    let mut h = harness(&mac);
    click(&mut h, "Duplicates");
    click(&mut h, "Scan");
    // Bypass the UI guard: the run itself refuses.
    if let Some(p) = h
        .state_mut()
        .plan_state(FeatureId::Duplicates)
        .plan
        .as_mut()
    {
        p.set_selected(0, true);
        p.set_selected(1, true);
    }
    steps(&mut h);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(x.exists() && y.exists());
    assert!(has_text(&h, "Keep at least one copy in each set"));
}

#[test]
fn similar_pictures_keep_the_biggest() {
    let mac = FakeMac::new();
    let a = mac.env.home_path("Pictures/Trip/a.png");
    let big = mac.env.home_path("Downloads/a_big.tiff");
    let small = mac.env.home_path("Pictures/a_small.bmp");
    let other = mac.env.home_path("Pictures/other.png");
    save_scene(&a, 640, 480, 0, 0, 1, ImageFormat::Png);
    save_scene(&big, 800, 600, 0, 5, 3, ImageFormat::Tiff);
    save_scene(&small, 320, 240, 0, -6, 4, ImageFormat::Bmp);
    save_scene(&other, 640, 480, 1, 0, 5, ImageFormat::Png);
    let mut h = harness(&mac);
    click(&mut h, "Similar Images");
    assert!(has_text(&h, "Pictures and Downloads"));
    click(&mut h, "Scan");
    let rm = tree_size(&a).0 + tree_size(&small).0;
    assert!(has(
        &h,
        &format!("3 similar pictures: 1 kept, {} to remove", human_size(rm))
    ));
    assert!(!has(&h, "other.png"));
    assert!(has(&h, "Looks like 2 other pictures, 800 x 600"));
    assert!(!selected(&h, FeatureId::SimilarPhotos, &big));
    click(&mut h, "a_big.tiff");
    assert!(!selected(&h, FeatureId::SimilarPhotos, &big));
    assert!(has_text(&h, "Keep at least one file in each set"));
    click(&mut h, "Select Copies");
    assert!(has(&h, "Keep Largest File"));
    click(&mut h, "Keep Suggested");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(big.exists() && !a.exists() && !small.exists() && other.exists());
    assert!(has(&h, &format!("{} freed", human_size(rm))));
}
