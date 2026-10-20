//! sfix7: the fixes of release check 7's safety findings, each proved on a FakeMac.
//! CHK7-1 (links above an allow-list folder), CHK7-2 (a folder holding an ignored item),
//! CHK7-5 (the quarantine moves only the confirmed list), CHK7-6 (no-replace Trash move), DV1
//! (the lock reason of a code folder reached through a link). Details in
//! docs/agents/reports/sfix7-r7.md.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, Method};
use cleancraft_core::features::{self, CAT_LINKED, Feature, dev_clutter, space_lens, system_junk};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::policy::{self, Refusal};
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{FeatureId, Plan, Settings};

fn link(target: &Path, at: &Path) {
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, at).unwrap();
}

/// The dot folders users link elsewhere (big caches and models), and a file System Junk would
/// offer inside each (relative to the dot folder).
const DOT_FOLDERS: &[(&str, &str)] = &[
    (".npm", "_cacache/content-v2/aa/blob"),
    (".cargo", "registry/cache/index.crates.io/serde-1.0.0.crate"),
    (".gradle", "caches/modules-2/files/lib.jar"),
    (".lmstudio", "models/acme/model-7b/model.gguf"),
    (".cache", "lm-studio/models/acme/model-7b/model.gguf"),
];

/// Where the link leads: (folder relative to home, words the lock reason must contain).
const PLACES: &[(&str, &str)] = &[
    ("Documents/store", "the contents of Documents"),
    ("Desktop/store", "the contents of Desktop"),
    (
        "Library/Mobile Documents/com~apple~CloudDocs/store",
        "iCloud Drive",
    ),
    (
        "Pictures/Photos Library.photoslibrary/resources/store",
        "a Photos library",
    ),
    // A safe place, but outside every allow-list folder: refused all the same.
    ("BigDisk/store", "outside the folders this feature cleans"),
];

fn select_all(plan: &mut Plan) {
    for i in 0..plan.items.len() {
        plan.set_selected(i, true);
    }
}

/// CHK7-1: each dot folder linked into Documents, Desktop, iCloud Drive, a Photos library and a
/// safe place outside the allow-list. The fence refuses every item under it, the scan offers
/// nothing there and lists the link locked with the plain reason and the next step, and a
/// System Junk run with everything selectable selected moves nothing out of the real place.
#[test]
fn chk7_1_dot_folders_linked_anywhere_are_never_cleaned() {
    for (dot, rel) in DOT_FOLDERS {
        for (place, words) in PLACES {
            let mac = FakeMac::new();
            let store = mac.env.home_path(place);
            let precious = store.join(rel);
            mac.write(&precious, &vec![3u8; 400_000]);
            age(&precious, 400);
            let at = mac.env.home_path(dot);
            link(&store, &at);
            let ctx = format!("{dot} -> ~/{place}");
            let before = tree_size(&store);

            // The fence, for the file and for each folder above it under the link.
            let allow = system_junk::SystemJunk.allow_roots(&mac.env);
            let mut p = at.join(rel);
            while p != at {
                assert!(
                    policy::check(&mac.env, &allow, &p).is_err(),
                    "{ctx}: the fence lets {} move",
                    p.display()
                );
                p = p.parent().unwrap().to_path_buf();
            }

            // The scan: nothing under the link offered; the link listed locked, with why.
            let settings = Settings::default();
            let mut plan = features::scan(&mac.env, &settings, FeatureId::SystemJunk).unwrap();
            assert!(
                !plan
                    .items
                    .iter()
                    .any(|i| i.locked.is_none() && i.path.starts_with(&at)),
                "{ctx}: offered {:?}",
                plan.items.iter().map(|i| &i.path).collect::<Vec<_>>()
            );
            let row = plan
                .items
                .iter()
                .find(|i| i.path == at)
                .unwrap_or_else(|| panic!("{ctx}: the link is not listed"));
            assert_eq!(row.category, CAT_LINKED);
            let why = row.locked.as_deref().expect("locked");
            assert!(
                why.contains(&format!("/Users/tester/{dot} is ")),
                "{ctx}: {why}"
            );
            assert!(why.contains(words), "{ctx}: {why}");
            assert!(
                why.contains("never cleans through a link")
                    && why.contains("make /Users/tester/")
                    && why.contains("a real folder again, then scan again"),
                "{ctx}: no next step: {why}"
            );

            // The run: everything that can be selected is, and nothing leaves the real place.
            select_all(&mut plan);
            let rep =
                features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {})
                    .unwrap();
            assert!(precious.exists(), "{ctx}: moved {:?}", rep.moved);
            assert_eq!(tree_size(&store), before, "{ctx}: the real place changed");
        }
    }
}

/// CHK7-1 at move time: the scan saw a real folder; before the run the folder was replaced by
/// a link into Documents. The executor refuses, nothing moves.
#[test]
fn chk7_1_a_link_made_after_the_scan_is_refused_at_move_time() {
    let mac = FakeMac::new();
    let npm = mac.env.home_path(".npm");
    let file = npm.join("_cacache/content-v2/aa/blob");
    mac.write(&file, &vec![1u8; 300_000]);
    age(&file, 400);
    let settings = Settings::default();
    let plan = features::scan(&mac.env, &settings, FeatureId::SystemJunk).unwrap();
    assert!(
        plan.selected().any(|i| i.path.starts_with(&npm)),
        "the npm cache is offered while it is a real folder"
    );
    let store = mac.env.home_path("Documents/npm-store");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::rename(&npm, &store).unwrap();
    link(&store, &npm);
    let rep =
        features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(store.join("_cacache/content-v2/aa/blob").exists());
    assert!(rep.moved.is_empty(), "{:?}", rep.moved);
    let why = rep
        .skipped
        .iter()
        .find(|(p, _)| p.starts_with(&npm))
        .map(|(_, w)| w.to_string())
        .expect("skipped with a reason");
    assert!(
        why.contains("the contents of Documents") && why.contains("reached through the link"),
        "{why}"
    );
}

/// CHK7-1: an allow-list folder that is itself a link (not only a folder above it).
#[test]
fn chk7_1_an_allow_list_folder_that_is_a_link_is_refused() {
    let mac = FakeMac::new();
    let store = mac.env.home_path("Desktop/derived");
    mac.write(&store.join("App-abc/Build/x.o"), &vec![1u8; 200_000]);
    let derived = mac.env.home_path("Library/Developer/Xcode/DerivedData");
    link(&store, &derived);
    let allow = system_junk::SystemJunk.allow_roots(&mac.env);
    let r = policy::check(&mac.env, &allow, &derived.join("App-abc"));
    assert!(
        matches!(&r, Err(Refusal::Protected(w)) if w.contains("Desktop")),
        "{r:?}"
    );
    let l = policy::link_at_or_above(&mac.env, &derived).expect("a link");
    assert_eq!(l.link, derived);
    assert!(l.protected.as_deref().unwrap().contains("Desktop"));
    // macOS's own links are not a way around and not a reason to refuse.
    assert!(policy::link_above(&mac.env, &mac.env.home_path("Library/Caches/x")).is_none());
}

/// DV1: a code folder reached through a link into Documents: the build folder is locked with
/// the right reason (the link, Documents) and a next step that works; a link to a safe place
/// says to add the real folder in Settings.
#[test]
fn dv1_code_folder_through_a_link_says_why_and_what_to_do() {
    let mac = FakeMac::new();
    let real = mac.env.home_path("Documents/Work");
    let proj = real.join("code/alpha");
    mac.write(&proj.join("Cargo.toml"), b"[package]\n");
    mac.write(&proj.join("src/main.rs"), b"fn main() {}\n");
    mac.write(&proj.join("target/debug/app"), &vec![1u8; 300_000]);
    link(&real, &mac.env.home_path("Work"));
    let settings = Settings {
        dev_roots: vec![mac.env.home_path("Work/code")],
        ..Settings::default()
    };
    let plan = dev_clutter::plan(&mac.env, &settings).unwrap();
    let it = plan
        .items
        .iter()
        .find(|i| i.path.ends_with("alpha/target"))
        .expect("listed");
    let why = it.locked.as_deref().expect("locked");
    assert!(
        !why.contains("Not inside one of your code folders"),
        "{why}"
    );
    assert!(
        why.contains("reached through the link /Users/tester/Work")
            && why.contains("the contents of Documents")
            && why.contains(dev_clutter::PROTECTED_STEPS),
        "{why}"
    );

    // The same layout with the real folder outside Documents: add the real folder.
    let mac = FakeMac::new();
    let real = mac.env.home_path("Volumes2/Work");
    let proj = real.join("code/alpha");
    mac.write(&proj.join("Cargo.toml"), b"[package]\n");
    mac.write(&proj.join("target/debug/app"), &vec![1u8; 300_000]);
    link(&real, &mac.env.home_path("Work"));
    let settings = Settings {
        dev_roots: vec![mac.env.home_path("Work/code")],
        ..Settings::default()
    };
    let why = dev_clutter::root_lock(&mac.env, &mac.env.home_path("Work/code")).expect("locked");
    assert!(
        why.contains("Add the real folder /Users/tester/Volumes2/Work/code under Developer Clutter in Settings"),
        "{why}"
    );
    let plan = dev_clutter::plan(&mac.env, &settings).unwrap();
    assert!(plan.selected().next().is_none());
    // The next step works: with the real folder added, the build folder can be cleaned.
    let settings = Settings {
        dev_roots: vec![real.join("code")],
        ..Settings::default()
    };
    assert!(dev_clutter::root_lock(&mac.env, &real.join("code")).is_none());
    let plan = dev_clutter::plan(&mac.env, &settings).unwrap();
    let it = plan
        .items
        .iter()
        .find(|i| i.path.ends_with("alpha/target"))
        .expect("listed");
    assert!(
        it.locked.as_deref().is_none_or(|w| !w.contains("link")),
        "{:?}",
        it.locked
    );
}

/// CHK7-2 at scan time (Disk Map): the home folder's `Projects` holds an ignored folder. The
/// scan offers what is in `Projects` minus the ignored folder, never `Projects` itself; a run
/// with everything selected keeps the ignored folder.
#[test]
fn chk7_2_disk_map_offers_the_other_items_never_the_ignored_one() {
    let mac = FakeMac::new();
    let projects = mac.env.home_path("Projects");
    let keep = projects.join("keep");
    mac.write(&keep.join("notes.txt"), &vec![7u8; 50_000]);
    mac.write(&projects.join("old/big.bin"), &vec![8u8; 200_000]);
    mac.write(&projects.join("loose.bin"), &vec![9u8; 30_000]);
    let settings = Settings {
        ignore: vec![keep.clone()],
        ..Settings::default()
    };
    let mut plan = features::scan(&mac.env, &settings, FeatureId::SpaceLens).unwrap();
    let paths: Vec<&PathBuf> = plan.items.iter().map(|i| &i.path).collect();
    assert!(!paths.contains(&&projects), "{paths:?}");
    assert!(!paths.iter().any(|p| p.starts_with(&keep)), "{paths:?}");
    assert!(paths.contains(&&projects.join("old")), "{paths:?}");
    assert!(paths.contains(&&projects.join("loose.bin")), "{paths:?}");
    // Ids stay positions; sizes are the tree sizes.
    for (n, it) in plan.items.iter().enumerate() {
        assert_eq!(it.id, n);
    }
    let old = plan
        .items
        .iter()
        .find(|i| i.path == projects.join("old"))
        .unwrap();
    assert_eq!(old.size, tree_size(&projects.join("old")).0);
    select_all(&mut plan);
    let rep =
        features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(keep.join("notes.txt").exists());
    assert!(!projects.join("old").exists(), "{:?}", rep.skipped);
    assert!(!projects.join("loose.bin").exists());
}

/// CHK7-2 when the feature does not take the parts (System Junk offers whole cache folders):
/// the folder holding the ignored file stays listed, locked with the reason and what to do;
/// the shared check refuses it too in an edited plan.
#[test]
fn chk7_2_a_cache_folder_holding_an_ignored_file_is_locked_or_narrowed_never_moved() {
    let mac = FakeMac::new();
    let cache = mac.env.home_path("Library/Caches/com.acme.App");
    let kept = cache.join("keep.db");
    mac.write(&kept, &vec![1u8; 80_000]);
    mac.write(&cache.join("blobs/a"), &vec![2u8; 200_000]);
    let settings = Settings {
        ignore: vec![kept.clone()],
        ..Settings::default()
    };
    let mut plan = features::scan(&mac.env, &settings, FeatureId::SystemJunk).unwrap();
    assert!(!plan.items.iter().any(|i| i.path == kept));
    if let Some(it) = plan.items.iter().find(|i| i.path == cache) {
        let why = it.locked.as_deref().expect("locked");
        assert!(
            why.contains("on your ignore list") && why.contains("Settings"),
            "{why}"
        );
    }
    select_all(&mut plan);
    features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(kept.exists());

    // An edited plan that selects the folder anyway: refused by the shared check.
    let mut edited = Plan::new(FeatureId::SystemJunk);
    edited.add_category("user_caches", "User caches", "", cleancraft_core::Risk::Low);
    edited.push(
        cache.clone(),
        tree_size(&cache).0,
        "user_caches",
        "Cache",
        cleancraft_core::Risk::Low,
        true,
    );
    let rep = features::run_plan(
        &mac.env,
        &settings,
        &edited,
        &edited.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(kept.exists());
    let why = rep.skipped[0].1.to_string();
    assert!(why.contains("on your ignore list"), "{why}");
}

/// CHK7-2: Space Lens's own check also refuses a folder holding an ignored item (Disk Map
/// sends plans of selected folders straight to the run).
#[test]
fn chk7_2_disk_map_plan_of_a_parent_is_refused_with_the_reason() {
    let mac = FakeMac::new();
    let projects = mac.env.home_path("Projects");
    let keep = projects.join("a/b/keep.txt");
    mac.write(&keep, b"keep me");
    let settings = Settings {
        ignore: vec![keep.clone()],
        ..Settings::default()
    };
    let mut plan = Plan::new(FeatureId::SpaceLens);
    plan.add_category(
        space_lens::CATEGORY,
        "Home folder",
        "",
        cleancraft_core::Risk::High,
    );
    plan.push(
        projects.clone(),
        tree_size(&projects).0,
        space_lens::CATEGORY,
        "Folder",
        cleancraft_core::Risk::High,
        true,
    );
    let rep =
        features::run_plan(&mac.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert!(keep.exists());
    assert!(rep.moved.is_empty());
    assert!(
        rep.skipped[0]
            .1
            .to_string()
            .contains("/Users/tester/Projects/a/b/keep.txt, which is on your ignore list")
    );
}

/// CHK7-5: the quarantine moves exactly the list the person confirmed; an item that came in
/// after the list was shown stays in the quarantine.
#[test]
fn chk7_5_quarantine_moves_only_the_confirmed_list() {
    let mac = FakeMac::new();
    let a = mac.env.home_path("Library/Caches/com.a/data");
    mac.write(&a, &vec![1u8; 10_000]);
    let mut plan = Plan::new(FeatureId::SystemJunk);
    plan.add_category("c", "c", "", cleancraft_core::Risk::Low);
    plan.push(
        a.parent().unwrap().to_path_buf(),
        tree_size(a.parent().unwrap()).0,
        "c",
        "r",
        cleancraft_core::Risk::Low,
        true,
    );
    let allow = vec![mac.env.home_path("Library/Caches")];
    executor::execute(
        &mac.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Quarantine,
        &mut |_, _| {},
    )
    .unwrap();
    let list = executor::quarantine_list(&mac.env);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].1, tree_size(&list[0].0).0);
    // Another cleanup puts something in the quarantine after the dialog opened.
    let late = mac.env.quarantine_dir().join("zz-later/late.bin");
    mac.write(&late, &vec![2u8; 5_000]);
    let confirmed: Vec<PathBuf> = list.iter().map(|(p, _)| p.clone()).collect();
    let rep =
        executor::move_quarantine_items_to_trash(&mac.env, &confirmed, &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 1);
    assert!(late.exists(), "an item that was not in the list moved");
    assert!(!confirmed[0].exists());
    assert!(mac.env.user_trash().join("com.a").exists());
}

/// CHK7-6: the Trash already holds an item with the same name; the move picks a fresh name
/// and never replaces it, and Undo brings the item back.
#[test]
fn chk7_6_trash_move_never_replaces_and_undo_works() {
    let mac = FakeMac::new();
    let item = mac.env.home_path("Library/Caches/com.dup");
    mac.write(&item.join("x"), b"mine");
    let theirs = mac.env.user_trash().join("com.dup");
    mac.write(&theirs.join("y"), b"already in the Trash");
    let mut plan = Plan::new(FeatureId::SystemJunk);
    plan.add_category("c", "c", "", cleancraft_core::Risk::Low);
    plan.push(
        item.clone(),
        tree_size(&item).0,
        "c",
        "r",
        cleancraft_core::Risk::Low,
        true,
    );
    let allow = vec![mac.env.home_path("Library/Caches")];
    let rep = executor::execute(
        &mac.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 1);
    assert!(theirs.join("y").exists());
    assert!(mac.env.user_trash().join("com.dup 2/x").exists());
    executor::undo(&mac.env, &rep.moved[0].id).unwrap();
    assert_eq!(std::fs::read(item.join("x")).unwrap(), b"mine");
    assert!(theirs.join("y").exists());
}
