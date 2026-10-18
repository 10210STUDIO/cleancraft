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

fn flat(t: &cleancraft_core::space::Tree) -> Vec<(std::path::PathBuf, u64, u64)> {
    fn walk(n: &cleancraft_core::space::Node, out: &mut Vec<(std::path::PathBuf, u64, u64)>) {
        out.push((n.path.clone(), n.size, n.files));
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut v = Vec::new();
    if let Some(r) = &t.root {
        walk(r, &mut v);
    }
    v.sort();
    v
}

fn build(p: &std::path::Path) -> cleancraft_core::space::Tree {
    cleancraft_core::space::build(p, &std::sync::atomic::AtomicBool::new(false), &|_, _| {})
}

/// DAT-4: after moving a folder holding the second link, every node of the patched map equals
/// a new scan (the link now met first, in the Trash, carries the size), and undo too.
#[test]
fn disk_map_refresh_equals_a_new_scan_node_by_node() {
    use cleancraft_core::features::{self, space_lens::plan_for};
    let mac = FakeMac::new();
    let a = mac.home_file("Alpha/data.bin", 400_000);
    let beta = mac.env.home_path("Beta");
    fs::create_dir_all(&beta).unwrap();
    fs::hard_link(&a, beta.join("data link.bin")).unwrap();
    mac.home_file("Beta/notes.txt", 20_000);
    let mut t = build(mac.env.home());
    let plan = plan_for(&mac.env, &[(beta.clone(), t.find(&beta).unwrap().size)]);
    let s = cleancraft_core::Settings::default();
    let rep = features::run_plan(&mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    t.refresh_after(&mac.env, &rep.moved);
    assert_eq!(flat(&t), flat(&build(mac.env.home())));
    let undone = executor::undo_plan(&mac.env, &rep.plan_id);
    assert_eq!(undone.0, 1);
    let recs: Vec<_> = executor::history(&mac.env)
        .into_iter()
        .filter(|r| r.plan_id == rep.plan_id)
        .collect();
    t.refresh_after(&mac.env, &recs);
    assert_eq!(flat(&t), flat(&build(mac.env.home())));
    // A subtree opened from the map keeps its link paths.
    let sub = t.subtree(&beta).unwrap();
    assert_eq!(sub.root.as_ref().unwrap().size, t.find(&beta).unwrap().size);
}
