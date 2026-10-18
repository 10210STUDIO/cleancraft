//! A1, A3, A7: the app list on a fake Mac.

use cleancraft_core::apps::{self, Store};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};

#[test]
fn lists_every_kind_of_app() {
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        build: "455",
        size: 2_000_000,
        feed: Some("https://example.com/cast.xml"),
        ..AppSpec::new("Editor", "com.example.Editor", "4.5")
    });
    let store = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Notes Pro", "com.maker.NotesPro", "2.0")
    });
    let setapp = mac.app_spec(&AppSpec {
        dir: "/Applications/Setapp",
        ..AppSpec::new("Clipper", "com.setapp.Clipper", "1.1")
    });
    let mine = mac.app_spec(&AppSpec {
        dir: "~/Applications",
        binary_plist: true,
        ..AppSpec::new("Tool", "org.tools.Tool", "0.9")
    });
    let util = mac.app_spec(&AppSpec {
        dir: "/Applications/Utilities",
        ..AppSpec::new("Disk Helper", "com.apple.DiskHelper", "1.0")
    });
    let sys = mac.app_spec(&AppSpec {
        dir: "/System/Applications",
        ..AppSpec::new("Calculator", "com.apple.calculator", "11.0")
    });
    // Not apps: a plain folder, a file named .app, a broken bundle.
    mac.write(&mac.env.sys_path("/Applications/readme.app"), b"x");
    std::fs::create_dir_all(mac.env.sys_path("/Applications/Broken.app/Contents")).unwrap();

    mac.last_opened(&editor, 3);
    mac.last_opened(&store, 400);
    let list = apps::inventory(&mac.env, true, &|_| {});
    let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Calculator",
            "Clipper",
            "Disk Helper",
            "Editor",
            "Notes Pro",
            "Tool"
        ]
    );
    let get = |p: &std::path::Path| list.iter().find(|a| a.path == p).unwrap();
    let e = get(&editor);
    assert_eq!(e.bundle_id, "com.example.Editor");
    assert_eq!(e.version, "4.5");
    assert_eq!(e.build, "455");
    assert_eq!(e.size, tree_size(&editor).0);
    assert_eq!(e.vendor, "Example");
    assert_eq!(e.store, Store::Other);
    assert_eq!(e.feed_url.as_deref(), Some("https://example.com/cast.xml"));
    assert!(!e.apple);
    let now = chrono::Utc::now().timestamp();
    assert!(!e.unused(now));
    assert!(get(&store).unused(now), "400 days is unused");
    assert_eq!(get(&store).store, Store::AppStore);
    assert_eq!(get(&setapp).store, Store::Setapp);
    assert!(get(&setapp).last_used.is_none());
    assert!(!get(&setapp).unused(now), "unknown date is never unused");
    assert_eq!(get(&mine).version, "0.9", "binary Info.plist read");
    assert!(get(&util).apple, "com.apple id is Apple");
    assert!(get(&sys).apple, "/System/Applications is Apple");
    assert_eq!(get(&sys).vendor, "Apple");
}

#[test]
fn apple_signature_marks_app_as_apple() {
    let mac = FakeMac::new();
    let app = mac.app("Pages Helper", "com.thirdparty.Helper", "1.0");
    mac.env.fake_sys().unwrap().answer_full(
        "/usr/bin/codesign -dv --verbose=2 /Applications/Pages Helper.app",
        cleancraft_core::sys::CmdOut {
            status: 0,
            stdout: String::new(),
            stderr: "Executable=x\nAuthority=Software Signing\nAuthority=Apple Root CA\n".into(),
        },
    );
    let list = apps::inventory(&mac.env, true, &|_| {});
    assert!(list.iter().find(|a| a.path == app).unwrap().apple);
}

#[test]
fn quick_listing_runs_no_commands() {
    let mac = FakeMac::new();
    mac.app("Editor", "com.example.Editor", "1.0");
    let list = apps::inventory(&mac.env, false, &|_| {});
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].size, 0);
    assert!(mac.env.fake_sys().unwrap().calls().is_empty());
}

#[test]
fn find_by_name_id_or_path() {
    let mac = FakeMac::new();
    mac.app("Editor", "com.example.Editor", "1.0");
    let list = apps::inventory(&mac.env, false, &|_| {});
    assert!(apps::find(&mac.env, &list, "editor").is_some());
    assert!(apps::find(&mac.env, &list, "Editor.app").is_some());
    assert!(apps::find(&mac.env, &list, "com.example.editor").is_some());
    assert!(apps::find(&mac.env, &list, "/Applications/Editor.app").is_some());
    assert!(apps::find(&mac.env, &list, "Other").is_none());
}
