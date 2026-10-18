//! The click-every-control test (FEATURES S12).
//!
//! Every sidebar page is opened on a fake Mac with data for it (the builders' `testkit_*`
//! helpers, FakeSys answering the tools each page asks). From the page's first state the test
//! explores every state a user can reach by clicking (scanned results, popups, confirm
//! dialogs, done pages, tabs, groups, app details), breadth first. In each state it lists every
//! control (button, checkbox, radio button, combo box, link, text box, slider) and, for every
//! control that state shows for the first time, it rebuilds the state from scratch, clicks it
//! (text boxes: types into them) and fails when:
//! - the control lies outside the window even after scrolling it into view;
//! - nothing happened: `CleanApp::action_count()` did not grow and the accessibility tree
//!   (labels, values, check states) did not change;
//! - the app panicked;
//! - any text on screen says "not available", "coming soon", "not implemented", "TODO",
//!   contains an em dash, or uses the word "nor".
//!
//! Controls shared by every page (sidebar rows, the Full Disk Access banner, the notice's
//! Dismiss button) are tested once in `shell_controls_respond`. A control a state inherits from
//! the state before it (the same role, label and position among equal labels) was already
//! clicked there; the table lists it as inherited.
//!
//! Run with `--nocapture` to see the table of every control per page.

mod common;

use std::collections::{BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use cleancraft_core::executor::{self, ActionKind, ActionRecord};
use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps, appcast};
use cleancraft_core::{FeatureId, Settings, features, testkit_cleanup, testkit_monitor};
use cleancraft_core::{testkit_perf as tp, updater};
use common::*;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};

// ---------------------------------------------------------------------------------------
// Fake Macs, one per page, each with something for every control to act on.
// ---------------------------------------------------------------------------------------

fn base_answers(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    // Reveal in Finder, hand-offs to other apps.
    f.answer("/usr/bin/open", "");
    // Undo of a toggled setting runs its undo command through the shell.
    f.answer("sh", "");
}

fn mac_sweep() -> FakeMac {
    let m = testkit_monitor::sweep_mac();
    testkit_monitor::answer_all(m.env.fake_sys().unwrap());
    base_answers(&m);
    m
}

fn mac_junk() -> FakeMac {
    let m = testkit_cleanup::junk_mac();
    base_answers(&m);
    m
}

fn mac_mail() -> FakeMac {
    let m = testkit_cleanup::mail_mac();
    base_answers(&m);
    m
}

const EMPTY_CALL: &str = "/usr/bin/osascript -e tell application \"Finder\" to empty trash";

fn mac_trash() -> FakeMac {
    let m = testkit_cleanup::trash_mac();
    base_answers(&m);
    m.env.fake_sys().unwrap().answer(EMPTY_CALL, "");
    m
}

fn mac_privacy() -> FakeMac {
    let m = testkit_cleanup::privacy_mac();
    base_answers(&m);
    m.env
        .fake_sys()
        .unwrap()
        .answer("tccutil", "Successfully reset\n");
    m
}

fn mac_startup() -> FakeMac {
    let m = tp::startup_mac();
    base_answers(&m);
    m
}

fn mac_tasks() -> FakeMac {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    tp::mail_index(&m, 300_000);
    base_answers(&m);
    m
}

fn mac_remover() -> FakeMac {
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        size: 3_000_000,
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    let viewer = mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Viewer", "com.maker.Viewer", "1.0")
    });
    mac.app_spec(&AppSpec {
        dir: "/Applications/Setapp",
        ..AppSpec::new("Tidy", "com.setapp.Tidy", "1.2")
    });
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.last_opened(&editor, 2);
    mac.last_opened(&viewer, 300);
    mac.spotlight_finds_nothing();
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    mac.home_file("Library/Preferences/com.example.Editor.plist", 3_000);
    mac.home_file("Library/Logs/Editor/run.log", 5_000);
    mac.home_file("Library/Caches/com.gone.Writer/c", 60_000);
    mac.home_file("Downloads/Tool-1.0.pkg", 80_000);
    mac.home_file("Library/PreferencePanes/Thing.prefPane/Contents/x", 20_000);
    base_answers(&mac);
    mac
}

const CAST: &str = "https://example.com/editor/cast.xml";
const DOWN: &str = "https://down.example.org/cast.xml";

fn mac_updates() -> FakeMac {
    let mac = FakeMac::new();
    mac.app_spec(&AppSpec {
        build: "200",
        feed: Some(CAST),
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.app_spec(&AppSpec {
        mas: true,
        ..AppSpec::new("Notes Pro", "com.maker.NotesPro", "3.1")
    });
    mac.app_spec(&AppSpec {
        build: "9",
        feed: Some(DOWN),
        ..AppSpec::new("Viewer", "org.example.Viewer", "0.9")
    });
    mac.web(
        CAST,
        &appcast(&[("210", "2.1", "<ul><li>Faster search</li></ul>")]),
    );
    mac.web(
        &updater::lookup_url("com.maker.NotesPro"),
        r#"{"resultCount":1,"results":[{"version":"3.2","releaseNotes":"Sync fixes"}]}"#,
    );
    mac.web_fails(DOWN, 6);
    mac.env.fake_sys().unwrap().answer(
        "/usr/sbin/softwareupdate --list",
        "* Label: macOS 15.1-24B83\n\tTitle: macOS Sequoia 15.1, Version: 15.1, Size: 2000000KiB, Recommended: YES, Action: restart,\n",
    );
    base_answers(&mac);
    mac
}

fn mac_files() -> FakeMac {
    let m = FakeMac::sample();
    m.home_file("Movies/big.mov", 60_000_000);
    m.home_file("Downloads/a.zip", 300_000);
    m.home_file("Downloads/copy of a.zip", 300_000);
    base_answers(&m);
    m
}

fn mac_monitor() -> FakeMac {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    base_answers(&m);
    m
}

/// A Mac with history: a System Junk cleanup that really ran, a maintenance command and a
/// startup item turned off.
fn mac_history() -> FakeMac {
    let m = FakeMac::sample();
    base_answers(&m);
    let settings = Settings::default();
    let plan = features::scan(&m.env, &settings, FeatureId::SystemJunk).expect("junk scan");
    let rep = features::run_plan(&m.env, &settings, &plan, &plan.digest(), &mut |_, _| {})
        .expect("junk clean");
    assert!(!rep.moved.is_empty(), "history needs moved items");
    let now = chrono::Utc::now().timestamp();
    executor::append_journal(
        &m.env,
        &ActionRecord {
            id: "cmd-1".into(),
            plan_id: "task-1".into(),
            feature: Some(FeatureId::Maintenance),
            time: now,
            size: 0,
            kind: ActionKind::Command {
                command: "dscacheutil -flushcache".into(),
                ok: true,
                output: String::new(),
            },
            undone: None,
        },
    )
    .expect("journal");
    executor::append_journal(
        &m.env,
        &ActionRecord {
            id: "tog-1".into(),
            plan_id: "startup-1".into(),
            feature: Some(FeatureId::LoginItems),
            time: now,
            size: 0,
            kind: ActionKind::Toggled {
                what: "Turned off com.example.helper".into(),
                undo_command: Some("launchctl enable gui/501/com.example.helper".into()),
            },
            undone: None,
        },
    )
    .expect("journal");
    m
}

fn mac_settings() -> FakeMac {
    let m = FakeMac::new();
    let s = Settings {
        ignore: vec![
            m.env.home_path("Library/Caches/com.example.Keep"),
            m.env.home_path("Downloads/keep"),
        ],
        ..Settings::default()
    };
    s.save(&m.env).expect("save settings");
    base_answers(&m);
    m
}

// ---------------------------------------------------------------------------------------
// The accessibility tree as plain data.
// ---------------------------------------------------------------------------------------

const CLICKABLE: &[Role] = &[
    Role::Button,
    Role::CheckBox,
    Role::RadioButton,
    Role::ComboBox,
    Role::Link,
    Role::TextInput,
    Role::MultilineTextInput,
    Role::SearchInput,
    Role::Slider,
    Role::SpinButton,
];

/// The sidebar is the left panel, 232 points wide.
const SIDEBAR_RIGHT: f32 = 232.0;
/// Shell controls tested once by `shell_controls_respond`, not on every page.
const SHELL_LABELS: &[&str] = &["Dismiss"];

#[derive(Clone, Debug, PartialEq)]
struct N {
    role: Role,
    label: String,
    value: String,
    toggled: String,
    disabled: bool,
    rect: Option<egui::Rect>,
    /// Inside the top modal dialog, or no dialog is open.
    interactive: bool,
    /// Position in the tree walk, to find the kittest node again.
    idx: usize,
}

fn nodes(h: &H) -> Vec<N> {
    let modal = h.ctx.memory(|m| m.top_modal_layer());
    let modal_id = modal.map(|l| l.id.accesskit_id());
    let ppp = h.ctx.pixels_per_point();
    let mut out = Vec::new();
    for (idx, n) in h.root().children_recursive().enumerate() {
        let a = n.accesskit_node();
        if a.is_hidden() {
            continue;
        }
        let interactive = match modal_id {
            None => true,
            Some(id) => {
                let mut cur = Some(n);
                let mut inside = false;
                while let Some(c) = cur {
                    if c.accesskit_node().locate().0 == id {
                        inside = true;
                        break;
                    }
                    cur = c.parent();
                }
                inside
            }
        };
        out.push(N {
            role: a.role(),
            label: a.label().unwrap_or_default(),
            value: a.value().unwrap_or_default(),
            toggled: a.toggled().map(|t| format!("{t:?}")).unwrap_or_default(),
            disabled: a.is_disabled(),
            rect: a.bounding_box().map(|r| {
                egui::Rect::from_min_max(
                    egui::pos2(r.x0 as f32 / ppp, r.y0 as f32 / ppp),
                    egui::pos2(r.x1 as f32 / ppp, r.y1 as f32 / ppp),
                )
            }),
            interactive,
            idx,
        });
    }
    out
}

/// One control: its role, label and position among controls with the same role and label.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key {
    role: String,
    label: String,
    nth: usize,
}

impl Key {
    fn show(&self) -> String {
        let label = if self.label.is_empty() {
            "(no label)"
        } else {
            &self.label
        };
        if self.nth > 0 {
            format!("{} '{}' #{}", self.role, label, self.nth + 1)
        } else {
            format!("{} '{}'", self.role, label)
        }
    }
}

fn role_name(r: Role) -> String {
    match r {
        Role::Button => "Button".into(),
        Role::CheckBox => "CheckBox".into(),
        Role::RadioButton => "Radio".into(),
        Role::ComboBox => "ComboBox".into(),
        Role::Link => "Link".into(),
        Role::TextInput | Role::MultilineTextInput | Role::SearchInput => "TextInput".into(),
        Role::Slider => "Slider".into(),
        Role::SpinButton => "Spin".into(),
        other => format!("{other:?}"),
    }
}

/// The page's own controls (not the sidebar, not shell controls), in tree order, with keys.
fn controls(ns: &[N], with_sidebar: bool) -> Vec<(Key, N)> {
    let mut out: Vec<(Key, N)> = Vec::new();
    for n in ns {
        if !CLICKABLE.contains(&n.role) || !n.interactive {
            continue;
        }
        let in_sidebar = n.rect.is_some_and(|r| r.max.x <= SIDEBAR_RIGHT + 0.5);
        if in_sidebar != with_sidebar {
            continue;
        }
        if !with_sidebar && SHELL_LABELS.contains(&n.label.as_str()) {
            continue;
        }
        let role = role_name(n.role);
        let nth = out
            .iter()
            .filter(|(k, _)| k.role == role && k.label == n.label)
            .count();
        out.push((
            Key {
                role,
                label: n.label.clone(),
                nth,
            },
            n.clone(),
        ));
    }
    out
}

/// What a state looks like for exploration: the set of its controls (role and label), check
/// boxes left out so that ticking items does not count as a new state.
fn signature(ctls: &[(Key, N)]) -> BTreeSet<(String, String)> {
    ctls.iter()
        .filter(|(k, _)| k.role != "CheckBox")
        .map(|(k, _)| (k.role.clone(), k.label.clone()))
        .collect()
}

/// Everything the tree says, for "did anything change".
fn tree_text(ns: &[N]) -> Vec<(Role, String, String, String, bool)> {
    ns.iter()
        .map(|n| {
            (
                n.role,
                n.label.clone(),
                n.value.clone(),
                n.toggled.clone(),
                n.disabled,
            )
        })
        .collect()
}

const BANNED: &[&str] = &[
    "not available",
    "coming soon",
    "not implemented",
    "TODO",
    "\u{2014}",
];

/// Texts on screen that break the rules.
fn bad_texts(ns: &[N]) -> Vec<String> {
    let mut bad = Vec::new();
    for n in ns {
        for s in [&n.label, &n.value] {
            if s.is_empty() {
                continue;
            }
            let lower = s.to_lowercase();
            for b in BANNED {
                let hit = if b.chars().all(|c| c.is_ascii_uppercase()) {
                    s.contains(b)
                } else {
                    lower.contains(&b.to_lowercase())
                };
                if hit {
                    bad.push(format!("{b:?} in {s:?}"));
                }
            }
            if lower
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w == "nor")
            {
                bad.push(format!("the word 'nor' in {s:?}"));
            }
        }
    }
    bad
}

// ---------------------------------------------------------------------------------------
// Driving the app.
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Step {
    /// Click a control (text boxes: type into them).
    Use(Key),
}

fn inside_window(r: egui::Rect) -> bool {
    r.min.x >= -0.5 && r.min.y >= -0.5 && r.max.x <= W + 0.5 && r.max.y <= HGT + 0.5
}

/// Find a control by key, scroll it into view, check it is inside the window, use it.
fn use_control(h: &mut H, key: &Key, with_sidebar: bool) -> Result<(), String> {
    let find = |h: &H| controls(&nodes(h), with_sidebar).into_iter().find(|(k, _)| k == key);
    let Some((_, n)) = find(h) else {
        return Err(format!("{} is gone", key.show()));
    };
    if !n.rect.is_some_and(inside_window) {
        scroll_to(h, key, with_sidebar);
    }
    let Some((_, n)) = find(h) else {
        return Err(format!("{} is gone after scrolling", key.show()));
    };
    match n.rect {
        Some(r) if inside_window(r) => {}
        r => return Err(format!("{} is outside the window: {r:?}", key.show())),
    }
    let is_text = key.role == "TextInput";
    with_node(h, key, with_sidebar, |node| {
        if is_text {
            node.focus();
            node.type_text("zz");
        } else {
            node.click();
        }
    });
    steps(h);
    Ok(())
}

fn scroll_to(h: &mut H, key: &Key, with_sidebar: bool) {
    with_node(h, key, with_sidebar, |n| n.scroll_to_me());
    steps(h);
}

/// Run `f` on the kittest node of `key`.
fn with_node(h: &H, key: &Key, with_sidebar: bool, f: impl FnOnce(&egui_kittest::Node<'_>)) {
    let Some((_, n)) = controls(&nodes(h), with_sidebar)
        .into_iter()
        .find(|(k, _)| k == key)
    else {
        return;
    };
    if let Some(node) = h.root().children_recursive().nth(n.idx) {
        f(&node);
    }
}

struct PageSpec {
    title: &'static str,
    mac: fn() -> FakeMac,
}

/// A fresh app on a fresh fake Mac, on the page, after `path`.
fn build(p: &PageSpec, path: &[Step]) -> Result<(FakeMac, H), String> {
    let mac = (p.mac)();
    let mut h = harness(&mac);
    // Page tests never show the Full Disk Access banner; the shell test covers it.
    h.state_mut().fda = Access::Granted;
    steps(&mut h);
    match p.title {
        "History" | "Settings" => {
            let page = p.title.to_lowercase();
            h.state_mut().navigate(&page);
        }
        t => {
            h.get_by_label(t).scroll_to_me();
            steps(&mut h);
            click(&mut h, t);
        }
    }
    steps(&mut h);
    for s in path {
        let Step::Use(k) = s;
        use_control(&mut h, k, false)?;
    }
    Ok((mac, h))
}

#[derive(Clone, Debug)]
enum Outcome {
    Works(&'static str),
    Disabled,
    Inherited,
    Fail(String),
}

struct Row {
    state: String,
    key: Key,
    outcome: Outcome,
}

struct Explored {
    rows: Vec<Row>,
    states: usize,
    failures: Vec<String>,
}

const MAX_STATES: usize = 24;
const MAX_DEPTH: usize = 6;

fn path_name(path: &[Step]) -> String {
    if path.is_empty() {
        return "start".into();
    }
    path.iter()
        .map(|Step::Use(k)| k.show())
        .collect::<Vec<_>>()
        .join(" > ")
}

/// Explore a page breadth first and use every control of every state.
fn explore(p: &PageSpec) -> Explored {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let mut seen: BTreeSet<BTreeSet<(String, String)>> = BTreeSet::new();
    let mut queue: VecDeque<(Vec<Step>, Vec<Key>)> = VecDeque::new();
    queue.push_back((Vec::new(), Vec::new()));
    let mut states = 0;
    while let Some((path, parent_keys)) = queue.pop_front() {
        if states >= MAX_STATES {
            break;
        }
        let built = catch_unwind(AssertUnwindSafe(|| build(p, &path)));
        let (_mac, h) = match built {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                failures.push(format!("{}: {}: cannot rebuild: {e}", p.title, path_name(&path)));
                continue;
            }
            Err(_) => {
                failures.push(format!("{}: {}: panic while rebuilding", p.title, path_name(&path)));
                continue;
            }
        };
        let ns = nodes(&h);
        let ctls = controls(&ns, false);
        if states == 0 {
            seen.insert(signature(&ctls));
        }
        states += 1;
        for b in bad_texts(&ns) {
            failures.push(format!("{}: {}: {b}", p.title, path_name(&path)));
        }
        drop(h);
        let state_name = path_name(&path);
        let keys: Vec<Key> = ctls.iter().map(|(k, _)| k.clone()).collect();
        for (key, n) in &ctls {
            if parent_keys.contains(key) {
                rows.push(Row {
                    state: state_name.clone(),
                    key: key.clone(),
                    outcome: Outcome::Inherited,
                });
                continue;
            }
            if n.disabled {
                rows.push(Row {
                    state: state_name.clone(),
                    key: key.clone(),
                    outcome: Outcome::Disabled,
                });
                continue;
            }
            let res = catch_unwind(AssertUnwindSafe(|| {
                let (_mac, mut h) = build(p, &path)?;
                let before_n = nodes(&h);
                let before = (h.state().action_count(), tree_text(&before_n));
                use_control(&mut h, key, false)?;
                let after_n = nodes(&h);
                let acted = h.state().action_count() > before.0;
                let changed = tree_text(&after_n) != before.1;
                let sig = signature(&controls(&after_n, false));
                Ok::<_, String>((acted, changed, sig, bad_texts(&after_n)))
            }));
            let outcome = match res {
                Err(_) => Outcome::Fail("panic".into()),
                Ok(Err(e)) => Outcome::Fail(e),
                Ok(Ok((acted, changed, sig, bad))) => {
                    for b in bad {
                        failures.push(format!(
                            "{}: {state_name} > {}: {b}",
                            p.title,
                            key.show()
                        ));
                    }
                    if path.len() < MAX_DEPTH && !seen.contains(&sig) {
                        seen.insert(sig);
                        let mut next = path.clone();
                        next.push(Step::Use(key.clone()));
                        queue.push_back((next, keys.clone()));
                    }
                    match (acted, changed) {
                        (true, true) => Outcome::Works("acted, screen changed"),
                        (true, false) => Outcome::Works("acted"),
                        (false, true) => Outcome::Works("screen changed"),
                        (false, false) => Outcome::Fail("nothing happened".into()),
                    }
                }
            };
            if let Outcome::Fail(why) = &outcome {
                failures.push(format!("{}: {state_name} > {}: {why}", p.title, key.show()));
            }
            rows.push(Row {
                state: state_name.clone(),
                key: key.clone(),
                outcome,
            });
        }
    }
    if !queue.is_empty() {
        println!(
            "{}: stopped after {MAX_STATES} states, {} more not explored",
            p.title,
            queue.len()
        );
    }
    Explored {
        rows,
        states,
        failures,
    }
}

fn print_table(title: &str, e: &Explored, secs: f32) {
    let tested = e
        .rows
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Works(_) | Outcome::Fail(_)))
        .count();
    let failed = e
        .rows
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
        .count();
    println!(
        "\n== {title}: {} states, {} controls listed, {tested} clicked, {failed} failed ({secs:.1} s)",
        e.states,
        e.rows.len()
    );
    println!("| State | Control | Result |");
    println!("|---|---|---|");
    for r in &e.rows {
        let res = match &r.outcome {
            Outcome::Works(w) => format!("ok ({w})"),
            Outcome::Disabled => "disabled".into(),
            Outcome::Inherited => "inherited".into(),
            Outcome::Fail(w) => format!("FAIL: {w}"),
        };
        if matches!(r.outcome, Outcome::Inherited) {
            continue;
        }
        println!("| {} | {} | {res} |", r.state, r.key.show());
    }
}

fn run_page(p: PageSpec) {
    let t0 = Instant::now();
    let e = explore(&p);
    print_table(p.title, &e, t0.elapsed().as_secs_f32());
    assert!(e.states > 0, "{}: no state could be built", p.title);
    assert!(
        e.failures.is_empty(),
        "{}: {} problems:\n{}",
        p.title,
        e.failures.len(),
        e.failures.join("\n")
    );
}

macro_rules! page_test {
    ($name:ident, $title:expr, $mac:expr) => {
        #[test]
        fn $name() {
            run_page(PageSpec {
                title: $title,
                mac: $mac,
            });
        }
    };
}

page_test!(quick_sweep, "Quick Sweep", mac_sweep);
page_test!(system_junk, "System Junk", mac_junk);
page_test!(mail_downloads, "Mail Downloads", mac_mail);
page_test!(trash_bins, "Trash Bins", mac_trash);
page_test!(threat_check, "Threat Check", mac_startup);
page_test!(privacy_traces, "Privacy Traces", mac_privacy);
page_test!(tune_up_tasks, "Tune-up Tasks", mac_tasks);
page_test!(startup_items, "Startup Items", mac_startup);
page_test!(app_remover, "App Remover", mac_remover);
page_test!(app_updates, "App Updates", mac_updates);
page_test!(live_monitor, "Live Monitor", mac_monitor);
page_test!(history, "History", mac_history);
page_test!(settings, "Settings", mac_settings);

// Pages still being built by "files" (Large & Old, Duplicates, Similar Images, Disk Map):
// run once main has them.
page_test!(large_old_files, "Large & Old Files", mac_files);
page_test!(duplicates, "Duplicates", mac_files);
page_test!(similar_images, "Similar Images", mac_files);
page_test!(disk_map, "Disk Map", mac_files);

/// Every sidebar row is inside the window and opens its page; the Full Disk Access banner's
/// buttons and the notice's Dismiss respond.
#[test]
fn shell_controls_respond() {
    // Sidebar: every row, from a fresh app.
    let mac = FakeMac::sample();
    base_answers(&mac);
    let h = harness(&mac);
    let rows: Vec<Key> = controls(&nodes(&h), true)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    drop(h);
    let mut names: Vec<String> = Vec::new();
    for (_, feats) in cleancraft_ui_egui::app::GROUPS {
        for f in *feats {
            names.push(f.title().to_string());
        }
    }
    names.push("History".into());
    names.push("Settings".into());
    let labels: Vec<&str> = rows.iter().map(|k| k.label.as_str()).collect();
    assert_eq!(labels, names, "sidebar rows");
    let mut failures = Vec::new();
    println!("\n== Sidebar");
    println!("| Control | Result |");
    println!("|---|---|");
    for k in &rows {
        let mut h = harness(&mac);
        // Start somewhere else so every row, Quick Sweep included, has a page to switch from.
        h.state_mut().navigate("settings");
        steps(&mut h);
        let before = h.state().action_count();
        let page_before = h.state().page;
        let r = use_control(&mut h, k, true);
        let ok = r.is_ok() && h.state().action_count() > before && h.state().page != page_before
            || (k.label == "Settings" && h.state().action_count() > before);
        let title_shown = has(&h, &k.label);
        println!(
            "| {} | {} |",
            k.show(),
            if ok && title_shown { "ok" } else { "FAIL" }
        );
        if let Err(e) = r {
            failures.push(e);
        } else if !ok {
            failures.push(format!("{}: page did not change", k.show()));
        }
        failures.extend(bad_texts(&nodes(&h)));
    }

    // Full Disk Access banner: shown when access is missing.
    let mac = FakeMac::new();
    base_answers(&mac);
    mac.home_file("Library/Safari/Bookmarks.plist", 10);
    mac.env.inject_fault(
        &mac.env.home_path("Library/Safari/Bookmarks.plist"),
        cleancraft_core::Fault::PermissionDenied,
    );
    for label in ["Open Privacy Settings", "Check Again"] {
        let mut h = harness(&mac);
        assert_eq!(h.state().fda, Access::Missing);
        let before = h.state().action_count();
        let k = Key {
            role: "Button".into(),
            label: label.into(),
            nth: 0,
        };
        let r = use_control(&mut h, &k, false);
        let ok = r.is_ok() && h.state().action_count() > before;
        println!("| banner {} | {} |", k.show(), if ok { "ok" } else { "FAIL" });
        if !ok {
            failures.push(format!("banner {label}: {r:?}"));
        }
        if label == "Open Privacy Settings" {
            let urls = mac.env.fake_sys().unwrap().with(|s| s.opened_urls.clone());
            assert!(
                urls.iter()
                    .any(|u| u == cleancraft_core::permissions::FDA_SETTINGS_URL),
                "{urls:?}"
            );
        } else {
            assert!(has(&h, "Full Disk Access is still off."));
        }
        failures.extend(bad_texts(&nodes(&h)));
    }

    // Notice: Dismiss removes it.
    let mut h = harness(&mac);
    h.state_mut().tell("Something happened.");
    steps(&mut h);
    let before = h.state().action_count();
    click(&mut h, "Dismiss");
    assert!(h.state().action_count() > before);
    assert!(h.state().notice.is_none());
    assert!(!has(&h, "Something happened."));
    println!("| notice Button 'Dismiss' | ok |");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every sidebar page in one app: no forbidden text on any first screen.
#[test]
fn every_page_text_follows_the_rules() {
    let mac = testkit_cleanup::junk_mac();
    base_answers(&mac);
    let mut h = harness(&mac);
    let mut bad = Vec::new();
    let titles: Vec<&str> = cleancraft_ui_egui::app::GROUPS
        .iter()
        .flat_map(|(_, f)| f.iter().map(|f| f.title()))
        .chain(["History", "Settings"])
        .collect();
    for t in titles {
        h.get_by_label(t).scroll_to_me();
        steps(&mut h);
        click(&mut h, t);
        for b in bad_texts(&nodes(&h)) {
            bad.push(format!("{t}: {b}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
