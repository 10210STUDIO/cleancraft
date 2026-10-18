//! Safety rows the builders' suites did not cover (FEATURES S2, S5): items on another disk go
//! to that disk's Trash, and iCloud Drive and SIP-protected system folders are refused even
//! when an allow-list would include them.

use cleancraft_core::executor::{self, trash_dir_for};
use cleancraft_core::policy::{self, Refusal};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Method, Plan, Risk};

#[test]
fn an_item_on_another_disk_goes_to_that_disks_trash_and_comes_back() {
    let m = FakeMac::new();
    let root = m.env.sys_path("/Volumes/USB/Old");
    let item = m.sys_file("/Volumes/USB/Old/movie.mov", 400_000);
    let mut plan = Plan::new(FeatureId::LargeOldFiles);
    plan.add_category("old", "Old", "Old files.", Risk::High);
    plan.push(item.clone(), 400_000, "old", "old movie", Risk::High, true);
    let rep = executor::execute(
        &m.env,
        &plan,
        std::slice::from_ref(&root),
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .expect("execute");
    assert_eq!(rep.moved.len(), 1, "{:?}", rep.skipped);
    let trash = trash_dir_for(&m.env, &item);
    assert!(
        trash.starts_with(m.env.sys_path("/Volumes/USB/.Trashes")),
        "{trash:?}"
    );
    assert!(!item.exists());
    assert!(trash.join("movie.mov").exists(), "not in the disk's Trash");
    assert!(!m.env.user_trash().join("movie.mov").exists());
    let (n, errs) = executor::undo_plan(&m.env, &rep.plan_id);
    assert_eq!((n, errs.len()), (1, 0));
    assert!(item.exists());
}

#[test]
fn icloud_drive_and_system_folders_are_refused_even_inside_an_allow_list() {
    let m = FakeMac::new();
    let library = m.env.home_path("Library");
    let icloud = m.home_file("Library/Mobile Documents/com~apple~CloudDocs/a.txt", 10);
    assert!(matches!(
        policy::check(&m.env, std::slice::from_ref(&library), &icloud),
        Err(Refusal::Protected(_))
    ));
    let usr = m.env.sys_path("/usr");
    for p in ["/usr/bin/true", "/usr/lib/libx.dylib", "/usr/libexec/x"] {
        let f = m.sys_file(p, 10);
        assert!(
            matches!(
                policy::check(&m.env, std::slice::from_ref(&usr), &f),
                Err(Refusal::Protected(_))
            ),
            "{p}"
        );
    }
    // The executor refuses them too, even in a plan that lists them.
    let mut plan = Plan::new(FeatureId::SystemJunk);
    plan.add_category("x", "X", "x", Risk::Low);
    plan.push(icloud.clone(), 10, "x", "forged", Risk::Low, true);
    let rep = executor::execute(
        &m.env,
        &plan,
        std::slice::from_ref(&library),
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .expect("execute");
    assert!(rep.moved.is_empty());
    assert!(icloud.exists());
}
