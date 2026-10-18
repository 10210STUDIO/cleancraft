//! A5, A6, A11: the App Remover's own scan (leftovers of removed apps, installers,
//! extensions), carried out through features::run_plan.

use cleancraft_core::executor;
use cleancraft_core::extensions::CAT_EXTENSIONS;
use cleancraft_core::features::{self, leftovers};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::FakeMacApps;
use cleancraft_core::{FeatureId, Settings};

fn scan(mac: &FakeMac) -> cleancraft_core::Plan {
    features::scan(&mac.env, &Settings::default(), FeatureId::Uninstaller).unwrap()
}

#[test]
fn leftovers_of_removed_apps_are_conservative() {
    let mac = FakeMac::new();
    mac.spotlight_finds_nothing();
    mac.app("Chrome", "com.google.Chrome", "120");
    let gone = [
        mac.home_file("Library/Application Support/com.gone.Writer/data", 30_000),
        mac.home_file("Library/Preferences/com.gone.Writer.plist", 1_000),
        mac.home_file("Library/Caches/com.gone.Writer/c", 9_000),
    ];
    let keep = [
        // Same maker as an installed app.
        mac.home_file("Library/Caches/com.google.Keystone/c", 9_000),
        // Apple's own.
        mac.home_file("Library/Preferences/com.apple.finder.plist", 1_000),
        // Not a bundle id.
        mac.home_file("Library/Application Support/Writer/x", 1_000),
        // Startup agents are left to Startup Items.
        mac.home_file("Library/LaunchAgents/com.gone.Writer.agent.plist", 300),
    ];
    let plan = scan(&mac);
    let orphans: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.category == leftovers::CAT_ORPHANS)
        .collect();
    assert_eq!(orphans.len(), 3, "{orphans:#?}");
    for g in &gone {
        assert!(orphans.iter().any(|i| g.starts_with(&i.path)));
    }
    assert!(orphans.iter().all(|i| !i.selected));
    assert!(
        orphans
            .iter()
            .all(|i| i.group.as_deref() == Some("com.gone.Writer"))
    );
    for k in &keep {
        assert!(
            !plan.items.iter().any(|i| k.starts_with(&i.path)),
            "{}",
            k.display()
        );
    }
    assert!(
        mac.env.fake_sys().unwrap().calls().contains(
            &"/usr/bin/mdfind kMDItemCFBundleIdentifier == 'com.gone.Writer'".to_string()
        )
    );
}

#[test]
fn app_found_elsewhere_or_spotlight_failure_proposes_nothing() {
    let mac = FakeMac::new();
    mac.home_file("Library/Caches/com.gone.Writer/c", 9_000);
    // No Spotlight answer: the command fails.
    let plan = scan(&mac);
    assert_eq!(plan.category_count(leftovers::CAT_ORPHANS), 0);
    assert!(plan.notes.iter().any(|n| n.contains("Spotlight")));
    // Spotlight finds the app on another disk.
    mac.env
        .fake_sys()
        .unwrap()
        .answer("/usr/bin/mdfind", "/Volumes/Backup/Writer.app\n");
    let plan = scan(&mac);
    assert_eq!(plan.category_count(leftovers::CAT_ORPHANS), 0);
}

#[test]
fn installers_and_extensions_are_listed_and_removable() {
    let mac = FakeMac::new();
    mac.spotlight_finds_nothing();
    let pkg = mac.home_file("Downloads/Tool-1.0.pkg", 50_000);
    let dmg = mac.home_file("Downloads/Editor.DMG", 70_000);
    let other = mac.home_file("Downloads/photo.jpg", 70_000);
    let pane = mac.home_file("Library/PreferencePanes/Thing.prefPane/Contents/x", 9_000);
    let saver = mac.sys_file("/Library/Screen Savers/Waves.saver/Contents/y", 9_000);
    let ql = mac.home_file("Library/QuickLook/Code.qlgenerator/z", 2_000);
    let blocker = mac.app("Blocker", "com.example.Blocker", "2.1");
    mac.write(
        &blocker.join("Contents/PlugIns/Blocker Extension.appex/Contents/Info.plist"),
        b"x",
    );
    mac.env.fake_sys().unwrap().answer(
        "/usr/bin/pluginkit -mAvvv -p com.apple.Safari.web-extension",
        "+    com.example.Blocker.Extension(2.1)\n\t            Path = /Applications/Blocker.app/Contents/PlugIns/Blocker Extension.appex\n\t   Parent Bundle = /Applications/Blocker.app\n\t    Display Name = Blocker Extension\n",
    );
    let mut plan = scan(&mac);
    let find = |p: &std::path::Path| plan.items.iter().find(|i| p.starts_with(&i.path)).cloned();
    let i_pkg = find(&pkg).expect("pkg");
    assert_eq!(i_pkg.category, leftovers::CAT_INSTALLERS);
    assert_eq!(i_pkg.size, tree_size(&pkg).0);
    assert!(find(&dmg).is_some());
    assert!(find(&other).is_none());
    let i_pane = find(&pane).expect("pane");
    assert_eq!(i_pane.category, CAT_EXTENSIONS);
    assert!(i_pane.reason.contains("Settings pane"));
    assert!(find(&saver).unwrap().reason.contains("all users"));
    assert!(find(&ql).is_some());
    let safari = plan
        .items
        .iter()
        .find(|i| i.reason.contains("Safari extension Blocker Extension"))
        .expect("safari ext");
    assert!(
        safari
            .locked
            .as_deref()
            .unwrap()
            .contains("Part of Blocker")
    );
    assert_eq!(plan.selected_count(), 0, "nothing preselected");

    for p in [&pkg, &pane] {
        let id = plan
            .items
            .iter()
            .find(|i| p.starts_with(&i.path))
            .unwrap()
            .id;
        plan.set_selected(id, true);
    }
    let rep = features::run_plan(
        &mac.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 2);
    assert!(!pkg.exists() && !pane.exists());
    assert!(dmg.exists() && saver.exists());
    executor::undo_plan(&mac.env, &plan.id);
    assert!(pkg.exists() && pane.exists());
}

#[test]
fn pluginkit_failure_is_a_note() {
    let mac = FakeMac::new();
    let plan = scan(&mac);
    assert!(plan.notes.iter().any(|n| n.contains("Safari extensions")));
}
