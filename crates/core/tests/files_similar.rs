//! Similar Images with generated pictures that differ slightly (size, brightness, noise,
//! format), a different picture, and HEIC through a faked `sips`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cleancraft_core::features::duplicates::{self, KeepRule, apply_keep_rule};
use cleancraft_core::features::similar::{self, dhash, distance, heic_thumb_path};
use cleancraft_core::features::{self, Feature};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_files::scene;
use cleancraft_core::{FeatureId, Plan, Settings};
use image::{ImageFormat, RgbImage};

fn save(mac: &FakeMac, rel: &str, img: &RgbImage, fmt: ImageFormat) -> PathBuf {
    let p = mac.env.home_path(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save_with_format(&p, fmt).unwrap();
    assert!(
        std::fs::metadata(&p).unwrap().len() > 100_000,
        "{rel} too small for the test"
    );
    p
}

fn scan(mac: &FakeMac) -> Plan {
    features::scan(&mac.env, &Settings::default(), FeatureId::SimilarPhotos).unwrap()
}

fn sets(plan: &Plan) -> BTreeSet<BTreeSet<PathBuf>> {
    duplicates::groups(plan)
        .into_values()
        .map(|ids| ids.iter().map(|i| plan.items[*i].path.clone()).collect())
        .collect()
}

fn kept(plan: &Plan) -> BTreeSet<PathBuf> {
    plan.items
        .iter()
        .filter(|i| !i.selected)
        .map(|i| i.path.clone())
        .collect()
}

#[test]
fn hashes_tell_alike_from_different() {
    let a = image::DynamicImage::ImageRgb8(scene(640, 480, 0, 0, 1));
    let a2 = image::DynamicImage::ImageRgb8(scene(320, 240, 0, 8, 2));
    let b = image::DynamicImage::ImageRgb8(scene(640, 480, 1, 0, 1));
    let d_same = distance(dhash(&a), dhash(&a2));
    let d_diff = distance(dhash(&a), dhash(&b));
    assert!(d_same <= similar::MAX_DISTANCE, "alike: {d_same}");
    assert!(d_diff > similar::MAX_DISTANCE, "different: {d_diff}");
}

#[test]
fn groups_keep_most_pixels() {
    let mac = FakeMac::new();
    let a = save(
        &mac,
        "Pictures/Trip/a.png",
        &scene(640, 480, 0, 0, 1),
        ImageFormat::Png,
    );
    let big = save(
        &mac,
        "Downloads/a_big.tiff",
        &scene(800, 600, 0, 5, 3),
        ImageFormat::Tiff,
    );
    let small = save(
        &mac,
        "Pictures/a_small.bmp",
        &scene(320, 240, 0, -6, 4),
        ImageFormat::Bmp,
    );
    let other = save(
        &mac,
        "Pictures/other.png",
        &scene(640, 480, 1, 0, 5),
        ImageFormat::Png,
    );
    // Never looked at: a Photos library and Library.
    save(
        &mac,
        "Pictures/Photos Library.photoslibrary/originals/a.png",
        &scene(640, 480, 0, 0, 6),
        ImageFormat::Png,
    );
    save(
        &mac,
        "Movies/a.png",
        &scene(640, 480, 0, 0, 7),
        ImageFormat::Png,
    );
    let plan = scan(&mac);
    let want: BTreeSet<BTreeSet<PathBuf>> = [[a.clone(), big.clone(), small.clone()]
        .into_iter()
        .collect()]
    .into_iter()
    .collect();
    assert_eq!(sets(&plan), want);
    assert!(!plan.items.iter().any(|i| i.path == other));
    assert_eq!(kept(&plan), [big.clone()].into_iter().collect());
    assert_eq!(plan.selected_count(), 2);
    let r = &plan.items.iter().find(|i| i.path == big).unwrap().reason;
    assert_eq!(r, "Looks like 2 other pictures, 800 x 600");
    // Keep largest file / newest, then back to Smart.
    let mut p = plan.clone();
    apply_keep_rule(&mac.env, &mut p, KeepRule::Largest);
    assert_eq!(kept(&p).len(), 1);
    apply_keep_rule(&mac.env, &mut p, KeepRule::Smart);
    assert_eq!(kept(&p), kept(&plan));
    // Run: the two smaller copies move, the keeper stays; all selected is refused.
    let s = Settings::default();
    let mut all = plan.clone();
    for i in 0..all.items.len() {
        all.set_selected(i, true);
    }
    assert!(features::run_plan(&mac.env, &s, &all, &all.digest(), &mut |_, _| {}).is_err());
    assert!(a.exists() && small.exists() && big.exists());
    let rep = features::run_plan(&mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 2);
    assert!(big.exists() && !a.exists() && !small.exists());
}

#[test]
fn heic_goes_through_sips() {
    let mac = FakeMac::new();
    let a = save(
        &mac,
        "Pictures/a.png",
        &scene(640, 480, 0, 0, 1),
        ImageFormat::Png,
    );
    // A HEIC whose converted thumbnail is already cached (written after the picture).
    let heic = mac.home_file("Pictures/IMG_1.HEIC", 150_000);
    let thumb = heic_thumb_path(&mac.env, &heic);
    std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
    scene(256, 192, 0, 2, 9)
        .save_with_format(&thumb, ImageFormat::Png)
        .unwrap();
    // One without a thumbnail: sips "runs" but writes nothing (faked), so it is unreadable.
    let heic2 = mac.home_file("Downloads/IMG_2.heic", 150_000);
    let fake = mac.env.fake_sys().unwrap();
    fake.answer(
        &format!(
            "/usr/bin/sips -g pixelWidth -g pixelHeight {}",
            heic.display()
        ),
        &format!(
            "{}\n  pixelWidth: 4032\n  pixelHeight: 3024\n",
            heic.display()
        ),
    );
    fake.answer("sips", "");
    let plan = scan(&mac);
    assert_eq!(
        sets(&plan),
        [[a.clone(), heic.clone()].into_iter().collect()]
            .into_iter()
            .collect()
    );
    // The HEIC has the most pixels.
    assert_eq!(kept(&plan), [heic.clone()].into_iter().collect());
    let calls = fake.calls();
    let convert = format!(
        "/usr/bin/sips -s format png -Z 256 {} --out {}",
        heic2.display(),
        heic_thumb_path(&mac.env, &heic2).display()
    );
    assert!(calls.contains(&convert), "{calls:?}");
    assert!(
        !calls
            .iter()
            .any(|c| c.contains("-s format png") && c.contains("IMG_1")),
        "cached thumbnail converted again"
    );
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("1 pictures could not be read")),
        "{:?}",
        plan.notes
    );
    assert!(Path::new(&heic2).exists());
    assert_eq!(
        similar::SimilarImages.allow_roots(&mac.env),
        vec![
            mac.env.home_path("Pictures"),
            mac.env.home_path("Downloads")
        ]
    );
}
