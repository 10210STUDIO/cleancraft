//! DAT-3: one size formula for hard links everywhere. A file with several links has its data
//! on disk once: tree sizes, plan totals and the executor's freed bytes count it once, like the
//! Disk Map.

use std::fs;

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::{entry_size, tree_size};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Plan, Risk};

#[test]
fn two_items_sharing_a_file_count_it_once_in_the_plan_and_the_report() {
    let mac = FakeMac::new();
    let a = mac.home_file("Library/Caches/com.example.A/blob", 300_000);
    let b_dir = mac.env.home_path("Library/Caches/com.example.B");
    fs::create_dir_all(&b_dir).unwrap();
    fs::hard_link(&a, b_dir.join("blob")).unwrap();
    let a_dir = a.parent().unwrap().to_path_buf();
    let data = entry_size(&fs::metadata(&a).unwrap());
    let dir = |p: &std::path::Path| entry_size(&fs::symlink_metadata(p).unwrap());

    let mut p = Plan::new(FeatureId::SystemJunk);
    p.push(
        a_dir.clone(),
        tree_size(&a_dir).0,
        "c",
        "A",
        Risk::Low,
        true,
    );
    p.push(
        b_dir.clone(),
        tree_size(&b_dir).0,
        "c",
        "B",
        Risk::Low,
        true,
    );
    // Each folder alone holds the data.
    assert_eq!(p.items[1].size, dir(&b_dir) + data);
    p.count_links_once();
    // Recalculated: two folders, one copy of the data.
    assert_eq!(p.items[0].size, dir(&a_dir) + data);
    assert_eq!(p.items[1].size, dir(&b_dir));
    let shown = p.selected_size();
    assert_eq!(shown, dir(&a_dir) + dir(&b_dir) + data);

    let allow = vec![mac.env.home_path("Library/Caches")];
    let rep = executor::execute(
        &mac.env,
        &p,
        &allow,
        &p.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 2);
    assert_eq!(rep.freed, shown, "Done says what the confirmation said");
}
