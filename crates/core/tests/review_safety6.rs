//! Round 6 safety review (reviewer safety6): attempts to make the new round 6 features
//! (Developer Clutter, menu bar agent and schedules, watchers, Mac Health, Duplicate Downloads,
//! Disk Map for the whole disk) lose data, act in the background, leave their allow-list or
//! run injected commands, on a FakeMac only. Ignored tests prove open defects (SAF6-n, see
//! docs/review/SAFETY-R6.md); the others protect behaviour checked and found right.
//!
//! Run the defects: `cargo test -p cleancraft-core --test review_safety6 -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};

// ----------------------------------------------------------------------------------------
// source helpers (static guards for code that must never exist)

/// Every `.rs` file under `crates/*/src` and `apps/*/src` (test helpers left out), with the
/// text before its `#[cfg(test)]` module.
fn app_sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for top in ["crates", "apps"] {
        for e in fs::read_dir(root.join(top)).unwrap().flatten() {
            stack.push(e.path().join("src"));
        }
    }
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if name.starts_with("testkit") {
                    continue;
                }
                let text = fs::read_to_string(&p).unwrap();
                let app = text.split("#[cfg(test)]").next().unwrap().to_string();
                out.push((p, app));
            }
        }
    }
    assert!(out.len() > 30, "sources not found: {}", out.len());
    out
}

fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// Lines of app code containing any of `needles`, in files accepted by `which`.
fn offending(which: impl Fn(&Path) -> bool, needles: &[&str]) -> Vec<String> {
    let mut bad = Vec::new();
    for (p, text) in app_sources() {
        if !which(&p) {
            continue;
        }
        for (n, l) in code_lines(&text) {
            for b in needles {
                if l.contains(b) {
                    bad.push(format!("{}:{}: {}", p.display(), n + 1, l.trim()));
                }
            }
        }
    }
    bad
}

// ----------------------------------------------------------------------------------------
// static guards

/// X2: programs start only in `sys.rs` (RealSys), so every command has a FakeSys answer in
/// tests and nothing new can run a real tool.
#[test]
fn x2_processes_start_only_in_sys() {
    let bad = offending(
        |p| file_name(p) != "sys.rs",
        &["Command::new", "process::Command", "std::process::Command"],
    );
    assert!(bad.is_empty(), "process started outside sys.rs:\n{}", bad.join("\n"));
}

/// D4 D5 D6: developer tools' own cleanup commands delete permanently, without the Trash or
/// the journal. None of them may appear in app code.
#[test]
fn d456_no_permanent_developer_cleanup_commands() {
    let bad = offending(
        |_| true,
        &[
            "\"delete\", \"unavailable\"",
            "simctl delete",
            "simctl erase",
            "\"erase\"",
            "\"prune\"",
            "system prune",
            "image prune",
            "volume prune",
            "builder prune",
            "\"cleanup\"",
            "brew cleanup",
            "\"autoremove\"",
            "Docker.raw\")",
        ],
    );
    assert!(
        bad.is_empty(),
        "permanent developer cleanup in app code:\n{}",
        bad.join("\n")
    );
}

/// The files that run in the background (menu bar agent, schedules, notifications, watchers).
fn background_file(p: &Path) -> bool {
    matches!(
        file_name(p).as_str(),
        "agent.rs" | "tray.rs" | "schedule.rs" | "notify.rs" | "watch.rs" | "space_history.rs"
    )
}

/// T1 W1 W4: background code never reaches anything that moves, removes, unloads or asks for
/// the administrator password.
#[test]
fn t1_background_code_never_reaches_removal() {
    let bad = offending(
        background_file,
        &[
            "executor::execute",
            "execute_with(",
            "run_plan",
            "sweep::run",
            "::run_selected",
            "run_admin",
            "move_quarantine_to_trash",
            "uninstall(",
            "bootout",
            "\"unload\"",
            "remove_item",
            "--yes",
        ],
    );
    assert!(
        bad.is_empty(),
        "background code reaches a removal path:\n{}",
        bad.join("\n")
    );
}
