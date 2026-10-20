//! Release check 7 (independent checker): defects found by reading the code, each proved by a
//! failing test on a FakeMac. The tests are `#[ignore]` until the defect is fixed; run them with
//! `cargo test -p cleancraft-core --test check7 -- --ignored`. Details in
//! docs/agents/reports/check7.md.

use std::path::{Path, PathBuf};

use cleancraft_core::features::{self, Feature, dev_clutter, space_lens, system_junk};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::policy;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{FeatureId, Plan, Risk, Settings};

#[cfg(unix)]
fn link(target: &Path, at: &Path) {
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, at).unwrap();
}

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

/// CHK7-1 (fence): an allow-list folder reached through a symbolic link ABOVE it. `~/.npm` is
/// a link into `~/Documents`; System Junk's allow-list folder `~/.npm/_cacache` then lies in
/// Documents on disk, but `policy::check` only looks for links from the item up to the
/// allow-list folder, and `protected_reason` compares the path as written. The fence must
/// refuse it.
#[cfg(unix)]
#[test]
#[ignore = "CHK7-1: links above an allow-list folder are not checked"]
fn chk7_1_fence_refuses_an_allow_root_reached_through_a_link_into_documents() {
    let mac = FakeMac::new();
    let store = mac.env.home_path("Documents/npm-store");
    mac.write(&store.join("_cacache/content-v2/aa"), &vec![1u8; 600_000]);
    link(&store, &mac.env.home_path(".npm"));
    let allow = system_junk::SystemJunk.allow_roots(&mac.env);
    let item = mac.env.home_path(".npm/_cacache/content-v2");
    assert!(
        policy::check(&mac.env, &allow, &item).is_err(),
        "the fence lets {} move although it is the contents of Documents",
        item.display()
    );
}

/// CHK7-1, end to end: System Junk proposes and preselects the npm cache entries that really
/// live in `~/Documents` (through the `~/.npm` link), and a default clean moves them out of
/// Documents into the Trash.
#[cfg(unix)]
#[test]
#[ignore = "CHK7-1: System Junk moves Documents contents through a linked ~/.npm"]
fn chk7_1_system_junk_never_moves_documents_contents_through_a_linked_dot_folder() {
    let mac = FakeMac::new();
    let store = mac.env.home_path("Documents/npm-store");
    let precious = store.join("_cacache/content-v2/aa");
    mac.write(&precious, &vec![1u8; 600_000]);
    link(&store, &mac.env.home_path(".npm"));
    let settings = Settings::default();
    let plan = features::scan(&mac.env, &settings, FeatureId::SystemJunk).unwrap();
    let rep =
        features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(
        precious.exists(),
        "moved out of Documents: {:?}",
        rep.moved.iter().map(|r| &r.kind).collect::<Vec<_>>()
    );
}

/// Guard (not a defect): Developer Clutter with a code folder under a linked parent that points
/// into Documents. The build folder is listed locked and never moves. (Its lock reason, "Not
/// inside one of your code folders", is misleading: see the report, CHK7-1 note.)
#[cfg(unix)]
#[test]
fn chk7_1_developer_clutter_never_moves_documents_contents_through_a_linked_parent() {
    let mac = FakeMac::new();
    let real = mac.env.home_path("Documents/Work");
    let proj = real.join("code/alpha");
    mac.write(&proj.join("Cargo.toml"), b"[package]\n");
    mac.write(&proj.join("src/main.rs"), b"fn main() {}\n");
    mac.write(&proj.join("target/debug/app"), &vec![1u8; 300_000]);
    age_tree(&proj, 90);
    link(&real, &mac.env.home_path("Work"));
    let settings = Settings {
        dev_roots: vec![mac.env.home_path("Work/code")],
        ..Settings::default()
    };
    let plan = dev_clutter::plan(&mac.env, &settings).unwrap();
    let _ = features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {});
    assert!(
        proj.join("target/debug/app").exists(),
        "a build folder inside Documents was moved to the Trash"
    );
}

/// CHK7-2 (ignore list): the ignore list only refuses an item that is ON the list or INSIDE an
/// entry (`Settings::is_ignored`). A folder that HOLDS an ignored entry is not refused, so the
/// ignored entry moves to the Trash with its parent. Shown with Disk Map (any folder of the
/// home folder can be selected there); System Junk, Large & Old and Developer Clutter behave
/// the same.
#[test]
#[ignore = "CHK7-2: a folder holding an ignored item is moved with it"]
fn chk7_2_a_folder_holding_an_ignored_item_is_not_moved() {
    let mac = FakeMac::new();
    let projects = mac.env.home_path("Projects");
    let keep = projects.join("keep");
    mac.write(&keep.join("notes.txt"), &vec![7u8; 50_000]);
    mac.write(&projects.join("old/big.bin"), &vec![8u8; 200_000]);
    let settings = Settings {
        ignore: vec![keep.clone()],
        ..Settings::default()
    };
    assert!(settings.is_ignored(&keep.join("notes.txt")));
    let mut plan = Plan::new(FeatureId::SpaceLens);
    plan.add_category(space_lens::CATEGORY, "Home folder", "", Risk::High);
    plan.push(
        projects.clone(),
        tree_size(&projects).0,
        space_lens::CATEGORY,
        "Folder",
        Risk::High,
        true,
    );
    let _ = features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {});
    assert!(
        keep.join("notes.txt").exists(),
        "the ignored folder went to the Trash with its parent"
    );
}

/// Not a defect, kept as a guard: every registered feature refuses each of its own allow-list
/// folders as a whole (an item equal to an allow-list folder is never moved).
#[test]
fn allow_list_folders_themselves_are_never_movable() {
    let mac = FakeMac::sample();
    for f in features::registry() {
        let allow: Vec<PathBuf> = f.allow_roots(&mac.env);
        for r in &allow {
            let inner = allow.iter().any(|o| o != r && r.starts_with(o));
            if inner {
                // Nested folders (~/Library/Caches/Homebrew in ~/Library/Caches) are one
                // entry of the outer folder: J1 offers them on purpose.
                continue;
            }
            assert!(
                policy::check(&mac.env, &allow, r).is_err(),
                "{:?} may move its allow-list folder {}",
                f.id(),
                r.display()
            );
        }
    }
}
