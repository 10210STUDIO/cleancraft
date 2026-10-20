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
/// the journal. None of them may be built as program arguments in app code (a command shown
/// as text for the person to copy is fine; the dynamic test below checks what runs).
#[test]
fn d456_no_permanent_developer_cleanup_arguments() {
    let bad = offending(
        |_| true,
        &[
            "\"delete\", \"unavailable\"",
            "\"simctl\", \"delete\"",
            "\"simctl\", \"erase\"",
            "\"prune\"",
            "\"autoremove\"",
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

// ----------------------------------------------------------------------------------------
// shared helpers for behaviour tests

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{Settings, executor};

/// Every regular file under `dir` (links not followed), with its length.
fn files_under(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(md) = fs::symlink_metadata(&p) else { continue };
            if md.is_dir() {
                stack.push(p);
            } else if md.is_file() {
                out.push((p, md.len()));
            }
        }
    }
    out.sort();
    out
}

// ----------------------------------------------------------------------------------------
// tray6: notifications, schedule, login agent

/// How `osascript` reads its command line (BSD getopt, options "e:l:is:"): options until the
/// first argument that does not start with '-', or until "--"; "-eTEXT" and "-e TEXT" both
/// give a statement. Returns (statements, argv).
fn osascript_getopt(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut st = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        let flag = &a[1..2];
        let rest = &a[2..];
        match flag {
            "e" | "l" | "s" => {
                let v = if rest.is_empty() {
                    i += 1;
                    args.get(i).cloned().unwrap_or_default()
                } else {
                    rest.to_string()
                };
                if flag == "e" {
                    st.push(v);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (st, args[i.min(args.len())..].to_vec())
}

/// T2 / R7: a file or app name never becomes AppleScript code. Names that start with "-e"
/// matter because the texts follow the -e options as plain arguments: getopt reads
/// "-e do shell script ..." as one more statement unless "--" ends the options first.
#[test]
#[ignore = "SAF6-6: a notification text starting with -e is read by osascript as a statement (no -- before argv)"]
fn saf6_6_notification_texts_never_become_statements() {
    use cleancraft_core::notify::{Notification, osascript_args};
    // An app named like this in the Trash gives the title "<name> was moved to the Trash".
    let name = r#"-e do shell script "touch /tmp/pwn" --"#;
    for (title, body, sub) in [
        (format!("{name} was moved to the Trash"), "x".to_string(), None),
        ("CleanCraft".to_string(), name.to_string(), None),
        ("CleanCraft".to_string(), "x".to_string(), Some(name.to_string())),
        (r#"Evil" & (do shell script "id") & ""#.to_string(), "b".into(), None),
    ] {
        let n = Notification {
            title: title.clone(),
            subtitle: sub.clone(),
            body: body.clone(),
            route: None,
        };
        let (st, argv) = osascript_getopt(&osascript_args(&n));
        assert_eq!(st.len(), 3, "statements: {st:?}");
        assert_eq!(st[0], "on run argv");
        assert_eq!(st[2], "end run");
        assert!(!st.iter().any(|s| s.contains("shell")), "{st:?}");
        assert_eq!(argv.first(), Some(&title), "title is argv 1: {argv:?}");
    }
}

/// T1: the agent's scheduled scan on a Mac full of junk (every Quick Sweep module, with its
/// preselection) moves nothing and writes no journal line; only plans and its own state.
#[test]
fn t1_scheduled_scan_moves_nothing() {
    use cleancraft_core::schedule::{Agent, ScanEvery};
    use cleancraft_core::sweep::Sources;
    use cleancraft_core::testkit_monitor::{answer_all, sweep_mac};
    let m = sweep_mac();
    let fake = m.env.fake_sys().unwrap();
    answer_all(fake);
    fake.answer("osascript", "");
    let before = files_under(m.env.root());
    let s = Settings {
        scan_every: ScanEvery::Daily,
        scan_hour: 10,
        ..Settings::default()
    };
    let mut a = Agent::new(&m.env, Sources::registered());
    let at = |t: &str| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M").unwrap();
    a.check(&m.env, &s, None, at("2026-10-20 09:00"));
    let rep = a.check(&m.env, &s, None, at("2026-10-20 10:05"));
    assert!(rep.scanned.is_some_and(|b| b > 0), "{rep:?}");
    assert!(executor::history(&m.env).is_empty(), "the agent wrote a journal line");
    let data = m.env.data_dir().to_path_buf();
    let after: Vec<_> = files_under(m.env.root())
        .into_iter()
        .filter(|(p, _)| !p.starts_with(&data))
        .collect();
    let before: Vec<_> = before.into_iter().filter(|(p, _)| !p.starts_with(&data)).collect();
    assert_eq!(before, after, "files outside CleanCraft's data changed");
    assert!(
        fake.admin_calls().is_empty(),
        "administrator commands: {:?}",
        fake.admin_calls()
    );
    let bad: Vec<_> = fake
        .calls()
        .into_iter()
        .filter(|c| {
            let words: Vec<&str> = c.split_whitespace().collect();
            let changing_launchctl = words.first().is_some_and(|w| w.ends_with("launchctl"))
                && words.get(1).is_some_and(|w| {
                    ["bootout", "bootstrap", "load", "unload", "disable", "enable", "remove", "kill"]
                        .contains(w)
                });
            changing_launchctl || c.contains("/bin/mv") || c.contains("rm ")
        })
        .collect();
    assert!(bad.is_empty(), "commands that change things: {bad:?}");
}

/// T4: the LaunchAgent is a plist with ProgramArguments (no shell), its path XML-escaped,
/// and it never runs a cleanup at load.
#[test]
fn t4_login_agent_plist_is_an_argument_array() {
    use cleancraft_core::login_agent::plist_xml;
    let prog = Path::new("/Applications/A&B <x>/CleanCraft.app/Contents/MacOS/CleanCraft");
    let xml = plist_xml(prog, Path::new("/Users/t/Library/Logs/CleanCraft/agent.log"));
    let v: plist::Value = plist::from_bytes(xml.as_bytes()).expect("valid plist");
    let d = v.as_dictionary().unwrap();
    let args: Vec<&str> = d["ProgramArguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_string().unwrap())
        .collect();
    assert_eq!(args, vec![prog.to_str().unwrap(), "agent"]);
    assert!(d.get("Program").is_none());
    for bad in ["clean", "--yes", "sh", "-c"] {
        assert!(!args.contains(&bad), "{args:?}");
    }
}

// ----------------------------------------------------------------------------------------
// watch6: drag-to-Trash offer and routes

fn editor_with_data(m: &FakeMac) -> (PathBuf, Vec<PathBuf>) {
    let app = m.app("Editor", "com.example.Editor", "2.0");
    let data = vec![
        m.home_file("Library/Application Support/com.example.Editor/db.sqlite", 300_000),
        m.home_file("Library/Caches/com.example.Editor/cache.bin", 200_000),
        m.home_file("Library/Preferences/com.example.Editor.plist", 2_000),
    ];
    (app, data)
}

/// R6 (checked, right): a route is untrusted input (a cleancraft:// link from any web page, a
/// plain JSON file). AppRemoverLeftovers{trashed} pointing at an app that is still installed
/// must not preselect that app's data as "leftovers".
#[test]
fn w6_leftovers_route_for_an_installed_app_selects_nothing() {
    use cleancraft_core::watch::{AppRoute, trashed_app_plan};
    let m = FakeMac::new();
    let (app, data) = editor_with_data(&m);
    let route = AppRoute::AppRemoverLeftovers {
        app_name: "Editor".into(),
        bundle_id: "com.example.Editor".into(),
        trashed: app.clone(),
    };
    let AppRoute::AppRemoverLeftovers { trashed, .. } =
        AppRoute::decode(&route.encode()).expect("decodes")
    else {
        unreachable!()
    };
    match trashed_app_plan(&m.env, &Settings::default(), &trashed) {
        Err(_) => {}
        Ok(p) => {
            let sel: Vec<_> = p.selected().map(|i| i.path.clone()).collect();
            assert!(
                sel.is_empty(),
                "an installed app's data is preselected as leftovers: {sel:?} (notes {:?})",
                p.notes
            );
        }
    }
    assert!(data.iter().all(|d| d.exists()));
}

/// W3: a link in the Trash pointing to the installed app is not a trashed app: its data
/// stays unselected (the installed copy is a twin).
#[cfg(unix)]
#[test]
fn w3_link_in_trash_to_installed_app_selects_nothing() {
    use cleancraft_core::watch::trashed_app_plan;
    let m = FakeMac::new();
    let (app, _) = editor_with_data(&m);
    let link = m.env.user_trash().join("Editor.app");
    std::os::unix::fs::symlink(&app, &link).unwrap();
    if let Ok(p) = trashed_app_plan(&m.env, &Settings::default(), &link) {
        let sel: Vec<_> = p.selected().map(|i| i.path.clone()).collect();
        assert!(sel.is_empty(), "{sel:?}");
    }
}

/// W1: the real case still works: an app really in the Trash, its data preselected for
/// review (not moved).
#[test]
fn w1_really_trashed_app_offers_its_leftovers_without_moving_them() {
    use cleancraft_core::watch::trashed_app_plan;
    let m = FakeMac::new();
    let (app, data) = editor_with_data(&m);
    let in_trash = m.env.user_trash().join("Editor.app");
    fs::rename(&app, &in_trash).unwrap();
    let p = trashed_app_plan(&m.env, &Settings::default(), &in_trash).expect("plan");
    assert!(p.selected().count() > 0, "{:?}", p.items);
    assert!(data.iter().all(|d| d.exists()));
    assert!(executor::history(&m.env).is_empty());
}

// ----------------------------------------------------------------------------------------
// health6: connect AI apps (mcp_setup)

/// SAF6-5 S2: ~/.claude.json kept as a link into a dotfiles folder (common). Connecting
/// writes a temporary file and renames it over the link: the link is replaced by a plain
/// file, the dotfiles copy silently stops being the one in use. Expected: refuse with a note,
/// or write through to the link's target when it is inside the home folder.
#[cfg(unix)]
#[test]
#[ignore = "SAF6-5: connecting replaces a symlinked ~/.claude.json with a plain file"]
fn saf6_5_connect_keeps_a_linked_settings_file_a_link() {
    use cleancraft_core::mcp_setup::{APP_BINARY, CODE_CONFIG, ClientKind, connect};
    let m = FakeMac::new();
    m.sys_file(APP_BINARY, 1_000);
    let real = m.env.home_path("dotfiles/claude.json");
    m.write(&real, b"{\n  \"numStartups\": 3\n}\n");
    let link = m.env.home_path(CODE_CONFIG);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let r = connect(&m.env, ClientKind::ClaudeCode);
    let still_link = fs::symlink_metadata(&link).unwrap().file_type().is_symlink();
    assert!(
        r.is_err() || still_link,
        "connected ({:?}) and {} is no longer a link",
        r.map(|c| c.message),
        link.display()
    );
}

/// (b) the binary path reaches the JSON escaped: the result parses and keeps other keys.
#[test]
fn h_connect_writes_valid_json_and_keeps_other_servers() {
    use cleancraft_core::mcp_setup::{APP_BINARY, DESKTOP_CONFIG, ClientKind, connect};
    let m = FakeMac::new();
    m.sys_file(APP_BINARY, 1_000);
    let cfg = m.env.home_path(DESKTOP_CONFIG);
    let before = r#"{"mcpServers":{"other":{"command":"/x","env":{"KEY":"secret"}}},"theme":"dark"}"#;
    m.write(&cfg, before.as_bytes());
    connect(&m.env, ClientKind::ClaudeDesktop).expect("connect");
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
    assert_eq!(v["theme"], "dark");
    assert_eq!(v["mcpServers"]["other"]["env"]["KEY"], "secret");
    assert!(v["mcpServers"]["cleancraft"]["command"].is_string());
    assert!(executor::history(&m.env).len() == 1, "journaled once");
}

// ----------------------------------------------------------------------------------------
// dev6: Developer Clutter

mod dev {
    use super::*;
    use cleancraft_core::features::{self, dev_clutter as dc};
    use cleancraft_core::testkit::age;
    use cleancraft_core::{FeatureId, Plan};

    pub fn age_tree(dir: &Path, days: u64) {
        for (p, _) in files_under(dir) {
            age(&p, days);
        }
    }

    pub fn rust(m: &FakeMac, dir: &Path, days: u64) -> PathBuf {
        m.write(&dir.join("Cargo.toml"), b"[package]\nname = \"x\"\n");
        m.write(&dir.join("src/main.rs"), b"fn main() {}\n");
        m.write(&dir.join("target/debug/app"), &vec![7u8; 200_000]);
        age_tree(dir, days);
        dir.join("target")
    }

    pub fn scan(m: &FakeMac, s: &Settings) -> Plan {
        features::scan(&m.env, s, FeatureId::DevClutter).expect("scan")
    }

    fn item<'a>(p: &'a Plan, path: &Path) -> &'a cleancraft_core::Item {
        p.items
            .iter()
            .find(|i| i.path == path)
            .unwrap_or_else(|| panic!("{} not listed: {:?}", path.display(), p.items))
    }

    /// SAF6-2 (verify): a project folder whose name is a git pattern ("app [old]"): git is
    /// asked with literal pathspecs, so the tracked target/ is locked.
    #[test]
    fn saf6_2_bracket_named_project_tracked_target_is_locked() {
        let m = FakeMac::new();
        let repo = m.env.home_path("Projects/mono");
        m.write(&repo.join(".git/HEAD"), b"ref: refs/heads/main\n");
        let t = rust(&m, &repo.join("app [old]"), 90);
        age(&repo.join(".git/HEAD"), 90);
        m.sys_file("/opt/homebrew/bin/git", 10);
        let fake = m.env.fake_sys().unwrap();
        let repo_s = repo.to_string_lossy();
        // What git prints when the pathspec is taken literally.
        fake.answer(
            &format!("/opt/homebrew/bin/git --literal-pathspecs -C {repo_s} ls-files -z -- app [old]/target"),
            "app [old]/target/debug/app\0",
        );
        // Without --literal-pathspecs '[old]' is a character class: git prints nothing.
        fake.answer(
            &format!("/opt/homebrew/bin/git -C {repo_s} ls-files -z -- app [old]/target"),
            "",
        );
        let p = scan(&m, &Settings::default());
        let it = item(&p, &t);
        assert_eq!(it.locked.as_deref(), Some(dc::LOCK_TRACKED), "{it:?}");
        assert!(!it.selected);
    }

    /// target/ holding its own repository is locked at scan time, not only at the move.
    #[test]
    fn d1_target_with_its_own_git_is_locked_in_the_scan() {
        let m = FakeMac::new();
        let t = rust(&m, &m.env.home_path("Projects/a"), 90);
        m.write(&t.join(".git/HEAD"), b"ref: refs/heads/main\n");
        m.write(&t.join("notes.md"), b"my work");
        age_tree(&t, 90);
        let p = scan(&m, &Settings::default());
        let it = item(&p, &t);
        assert!(it.locked.is_some() && !it.selected, "{it:?}");
    }

    /// SAF6-3 (verify): a venv in an inactive project is listed, never selected.
    #[test]
    fn saf6_3_venvs_never_selected() {
        let m = FakeMac::new();
        let py = m.env.home_path("Projects/py");
        m.write(&py.join("main.py"), b"print(1)");
        m.write(&py.join(".venv/pyvenv.cfg"), b"home = /usr/bin");
        m.write(&py.join(".venv/lib/site.py"), &vec![1u8; 40_000]);
        age_tree(&py, 200);
        let p = scan(&m, &Settings::default());
        let it = item(&p, &py.join(".venv"));
        assert!(!it.selected, "{it:?}");
    }

    /// D2: code folders in Documents or Desktop: listed locked, and an edited plan that selects
    /// them is refused at the move (files stay).
    #[test]
    fn d2_projects_in_documents_never_move() {
        let m = FakeMac::new();
        let code = m.env.home_path("Documents/code");
        let t = rust(&m, &code.join("game"), 120);
        let s = Settings {
            dev_roots: vec![code.clone()],
            ..Settings::default()
        };
        s.save(&m.env).unwrap();
        let mut p = scan(&m, &s);
        let i = item(&p, &t).id;
        assert!(p.items[i].locked.is_some());
        p.items[i].locked = None;
        p.items[i].selected = true;
        let d = p.digest();
        let rep = features::run_plan(&m.env, &s, &p, &d, &mut |_, _| {});
        assert!(t.join("debug/app").exists(), "{rep:?}");
    }

    /// D3: a link inside a code folder to a project elsewhere is never followed.
    #[cfg(unix)]
    #[test]
    fn d3_linked_project_is_not_followed() {
        let m = FakeMac::new();
        let elsewhere = m.env.home_path("Archive/old");
        let t = rust(&m, &elsewhere, 200);
        fs::create_dir_all(m.env.home_path("Projects")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, m.env.home_path("Projects/old")).unwrap();
        let p = scan(&m, &Settings::default());
        assert!(
            p.items.iter().all(|i| !i.path.starts_with(m.env.home_path("Projects/old"))
                && i.path != t),
            "{:?}",
            p.items
        );
    }

    /// D4 D5 D6 (dynamic): scanning, scanning the tools and cleaning everything selected never
    /// runs a tool's own delete (simctl delete/erase, docker prune, brew cleanup other than -n).
    #[test]
    fn d456_no_tool_delete_ever_runs() {
        let m = FakeMac::new();
        rust(&m, &m.env.home_path("Projects/a"), 90);
        let dev = m.env.home_path(dc::SIM_DEVICES);
        let udid = "11111111-2222-3333-4444-555555555555";
        m.write(&dev.join(udid).join("data/x"), &vec![1u8; 10_000]);
        for p in ["/usr/local/bin/docker", "/opt/homebrew/bin/brew"] {
            m.sys_file(p, 10);
        }
        let fake = m.env.fake_sys().unwrap();
        fake.answer(
            "xcrun simctl list devices unavailable -j",
            &format!(
                r#"{{"devices":{{"com.apple.CoreSimulator.SimRuntime.iOS-14-0":[{{"udid":"{udid}","name":"iPhone 8","isAvailable":false,"state":"Shutdown","availabilityError":"runtime profile not found"}}]}}}}"#
            ),
        );
        let s = Settings::default();
        let _ = dc::scan_tools(&m.env);
        let mut p = scan(&m, &s);
        for i in 0..p.items.len() {
            if p.items[i].locked.is_none() {
                p.items[i].selected = true;
            }
        }
        let d = p.digest();
        let _ = features::run_plan(&m.env, &s, &p, &d, &mut |_, _| {});
        let bad: Vec<_> = fake
            .calls()
            .into_iter()
            .filter(|c| {
                c.contains("simctl delete")
                    || c.contains("simctl erase")
                    || c.contains("prune")
                    || (c.contains("brew") && c.contains("cleanup") && !c.contains(" -n"))
                    || c.contains("autoremove")
            })
            .collect();
        assert!(bad.is_empty(), "{bad:?}");
        assert!(fake.admin_calls().is_empty());
    }
}
