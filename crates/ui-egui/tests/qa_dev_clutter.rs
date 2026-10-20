//! Developer Clutter end to end, by clicks: scan the code folders, check the numbers against a
//! recalculation from disk, confirm the exact list, see the folders really move (and only the
//! selected ones), undo; errors per item; and the developer tools' commands, which run only
//! after a confirmation showing the exact command, and are reported honestly when a tool is
//! missing or not running.

mod common;

use std::path::{Path, PathBuf};

use cleancraft_core::features::dev_clutter as dev;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit::{FakeMac, age};
use cleancraft_core::{Fault, FeatureId};
use cleancraft_ui_egui::pages::dev_clutter as page;
use common::*;
use egui_kittest::kittest::Queryable;

fn age_tree(dir: &Path, days: u64) {
    let mut v = Vec::new();
    cleancraft_core::fsutil::walk_files(dir, &|_| false, &mut v);
    for (p, _) in v {
        age(&p, days);
    }
}

fn rust(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("Cargo.toml"), b"[package]\n");
    m.write(&dir.join("src/main.rs"), b"fn main() {}\n");
    m.write(&dir.join("target/debug/app"), &vec![1u8; 300_000]);
    age_tree(dir, days);
    dir.join("target")
}

fn node(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
    m.write(&dir.join("package.json"), b"{}");
    m.write(&dir.join("node_modules/a/index.js"), &vec![2u8; 120_000]);
    age_tree(dir, days);
    dir.join("node_modules")
}

fn open_page(h: &mut H) {
    click(h, "Clean");
    click(h, "Developer Clutter");
}

fn plan(h: &H) -> cleancraft_core::Plan {
    h.state().plans[&FeatureId::DevClutter]
        .plan
        .clone()
        .expect("plan")
}

#[test]
fn scan_review_confirm_moves_selected_and_numbers_match() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Projects");
    let a = rust(&mac, &p.join("alpha"), 90);
    let b = node(&mac, &p.join("beta"), 90);
    let c = rust(&mac, &p.join("gamma"), 0);
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let pl = plan(&h);
    assert_eq!(pl.items.len(), 3);
    // The big number is every proposed folder, recalculated from disk.
    let total: u64 = [&a, &b, &c].iter().map(|x| tree_size(x).0).sum();
    assert!(has(&h, &human_size(total)), "total {}", human_size(total));
    // The active project is listed, not selected, and says why.
    let gamma = pl.items.iter().find(|i| i.path == c).unwrap();
    assert!(!gamma.selected && gamma.reason.contains(dev::ACTIVE));
    assert!(has_text(&h, dev::ACTIVE));
    let moved: u64 = [&a, &b].iter().map(|x| tree_size(x).0).sum();
    assert!(has(&h, &human_size(moved)), "selected size not shown");
    click(&mut h, "Review and Clean");
    let count = |h: &H, p: &Path| h.query_all_by_label(&mac.env.display(p)).count();
    assert_eq!(
        count(&h, &a),
        2,
        "selected folder missing from the confirmation"
    );
    assert_eq!(
        count(&h, &c),
        1,
        "active project listed in the confirmation"
    );
    click(&mut h, "Move to Trash");
    assert!(!a.exists() && !b.exists());
    assert!(c.exists(), "the active project's folder moved");
    assert!(p.join("alpha/Cargo.toml").exists() && p.join("beta/package.json").exists());
    assert!(has(
        &h,
        &format!("{} moved to the Trash", human_size(moved))
    ));
    // In the Trash, with the same size.
    let trashed: u64 = cleancraft_core::fsutil::children(&mac.env.user_trash())
        .iter()
        .map(|x| tree_size(x).0)
        .sum();
    assert_eq!(trashed, moved);
    click(&mut h, "Undo");
    assert!(a.exists() && b.exists());
}

#[test]
fn permission_denied_and_file_in_use_are_shown() {
    let mac = FakeMac::new();
    let p = mac.env.home_path("Code");
    let a = rust(&mac, &p.join("alpha"), 90);
    let b = node(&mac, &p.join("beta"), 90);
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    mac.env.inject_fault(&a, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&b.join("a/index.js"));
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(a.exists() && b.exists());
    assert!(has_text(&h, "permission denied"));
    assert!(has_text(&h, "open in a running app"));
}

#[test]
fn git_tracked_folder_is_shown_locked_and_never_moves() {
    let mac = FakeMac::new();
    let repo = mac.env.home_path("work/app");
    let t = rust(&mac, &repo, 90);
    mac.write(&repo.join(".git/HEAD"), b"ref");
    age(&repo.join(".git/HEAD"), 90);
    mac.write(&mac.env.sys_path("/opt/homebrew/bin/git"), b"#!");
    mac.env
        .fake_sys()
        .unwrap()
        .answer("git", "target/vendored.txt\0");
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let it = plan(&h).items[0].clone();
    assert_eq!(it.path, t);
    assert_eq!(it.locked.as_deref(), Some(dev::LOCK_TRACKED));
    assert!(!has(&h, "Review and Clean"));
    assert!(t.exists());
}

const SIMCTL_JSON: &str = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-14-0":[{"udid":"AAAA-1111","isAvailable":false,"name":"iPhone 8","availabilityError":"runtime profile not found","state":"Shutdown"}]}}"#;

#[test]
fn unavailable_simulator_is_listed_unticked_and_moves_only_when_ticked() {
    let mac = FakeMac::new();
    let sim = mac.env.home_path(dev::SIM_DEVICES).join("AAAA-1111");
    mac.write(&sim.join("data/big.bin"), &vec![3u8; 400_000]);
    mac.env
        .fake_sys()
        .unwrap()
        .answer("xcrun simctl list devices unavailable -j", SIMCTL_JSON);
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    let it = plan(&h).items[0].clone();
    assert_eq!(it.path, sim);
    assert!(!it.selected, "never selected for you");
    assert!(has(&h, &human_size(tree_size(&sim).0)));
    assert!(has_text(&h, "iPhone 8 (iOS 14.0)"));
    assert!(!has(&h, "Review and Clean"));
    // Tick it by hand: it moves to the Trash, nothing else runs.
    click(&mut h, "AAAA-1111");
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(!sim.exists());
    let calls = mac.env.fake_sys().unwrap().calls();
    assert!(
        calls.iter().all(|c| !c.contains("simctl delete")),
        "{calls:?}"
    );
    click(&mut h, "Undo");
    assert!(sim.exists());
}

#[test]
fn docker_and_brew_are_reported_with_the_command_never_run() {
    let mac = FakeMac::new();
    rust(&mac, &mac.env.home_path("Projects/alpha"), 90);
    let fake = mac.env.fake_sys().unwrap();
    mac.write(&mac.env.sys_path("/usr/local/bin/docker"), b"#!");
    mac.write(&mac.env.sys_path("/opt/homebrew/bin/brew"), b"#!");
    fake.answer_full(
        "docker system df",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?".into(),
        },
    );
    fake.answer_full(
        "brew cleanup -n",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "Error: Cannot write to /opt/homebrew/Cellar".into(),
        },
    );
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    assert!(has_text(&h, "nothing to clean right now"));
    click(&mut h, page::TOOLS_BUTTON);
    assert!(has_text(&h, "Docker is installed but not running"));
    assert!(has_text(&h, "Cannot write to /opt/homebrew/Cellar"));
    assert!(!has(&h, dev::ToolId::DockerImages.copy_label()));
    // Docker starts and Homebrew works: Check Again shows sizes and the commands.
    fake.answer(
        "docker system df",
        "TYPE TOTAL ACTIVE SIZE RECLAIMABLE\nImages 3 1 4GB 1.5GB (37%)\nBuild Cache 9 0 800MB 800MB\n",
    );
    fake.answer(
        "brew cleanup -n",
        "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,000 files, 60.5MB)\n==> This operation would free approximately 60.5MB of disk space.\n",
    );
    click(&mut h, page::CHECK_AGAIN);
    assert!(has(&h, dev::DOCKER_CACHE_CMD));
    assert!(has(&h, dev::BREW_CMD));
    assert!(has(&h, &human_size(800_000_000)));
    assert!(has_text(&h, "cannot go to the Trash"));
    click(&mut h, dev::ToolId::DockerBuildCache.copy_label());
    assert!(has_text(&h, "Copied \"docker builder prune\""));
    click(&mut h, page::CLOSE);
    assert!(!has(&h, page::CHECK_AGAIN));
    // Nothing that deletes for good ever ran; the project folder is untouched.
    for c in fake.calls() {
        assert!(
            !c.contains("prune") && !c.ends_with("brew cleanup"),
            "ran {c}"
        );
    }
    assert!(mac.env.home_path("Projects/alpha/target").exists());
}

#[test]
fn nothing_found_says_where_it_looked() {
    let mac = FakeMac::new();
    let mut h = harness(&mac);
    open_page(&mut h);
    click(&mut h, "Scan");
    assert!(has(&h, "Nothing to clean here"));
    click(&mut h, "Details");
    assert!(has_text(&h, "No code folders found"));
    // No tool on this Mac: no banner, no dead button.
    assert!(!has(&h, page::TOOLS_BUTTON));
}
