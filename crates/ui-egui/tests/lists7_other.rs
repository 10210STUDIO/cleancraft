//! lists7: the shared list toolbar on Startup Items, Disk Map, History and Live Monitor, by
//! clicks on a FakeMac: search narrows the list and the counts, each filter and sort works
//! and is remembered after a relaunch, Clear Filters restores, and in Disk Map a ticked item
//! the filters hide is still counted and listed in the confirmation before it moves.

mod common;

use cleancraft_core::executor::{self, ActionKind, ActionRecord, Method};
use cleancraft_core::proc_names;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor;
use cleancraft_core::testkit_perf;
use cleancraft_core::{FeatureId, Settings};
use common::*;
use egui_kittest::kittest::Queryable;

fn settle(h: &mut H) {
    for _ in 0..3 {
        steps(h);
    }
}

fn filter(h: &mut H, entry: &str) {
    click(h, "Filter");
    click(h, entry);
    h.key_press(egui::Key::Escape);
    settle(h);
}

fn sort(h: &mut H, entry: &str) {
    click(h, "Sort");
    click(h, entry);
}

/// Top-to-bottom order of the labels found.
fn order(h: &H, labels: &[String]) -> Vec<String> {
    let mut v: Vec<(f32, String)> = labels
        .iter()
        .filter_map(|l| {
            h.query_all_by_label(l)
                .next()
                .map(|n| (n.rect().center().y, l.clone()))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, l)| l).collect()
}

// ---------- Startup Items ----------

fn startup(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    h.state_mut().navigate(FeatureId::LoginItems.as_str());
    settle(&mut h);
    h
}

#[test]
fn startup_items_kind_state_unsigned_search_and_sort() {
    let mac = testkit_perf::startup_mac();
    let mut h = startup(&mac);
    let data = h.state().startup.data.clone().expect("read");
    let login = data.login.as_ref().map(|l| l.len()).unwrap_or(0);
    let off_login = data.removed_login.len();
    let agents = data
        .launch
        .iter()
        .filter(|i| i.domain != cleancraft_core::startup::Domain::Daemon)
        .count();
    let daemons = data.launch.len() - agents;
    let unsigned = data.launch.iter().filter(|i| i.signer.is_weak()).count();
    let on = data.launch.iter().filter(|i| i.enabled).count() + login;
    let off = data.launch.len() + login + off_login - on;
    let total = data.launch.len() + login + off_login;
    assert!(login > 0 && agents > 0 && daemons > 0 && unsigned > 0);
    assert!(has(&h, "Login items"), "section shown");
    filter(&mut h, &format!("Daemons ({daemons})"));
    assert!(has(&h, &format!("Showing {daemons} of {total} items")));
    assert!(!has(&h, "Login items"), "other kinds leave");
    assert!(has(&h, "Launch daemons") || has_text(&h, "aemons"));
    click(&mut h, "Clear Filters");
    assert!(has(&h, "Login items"));
    filter(&mut h, &format!("Login items ({})", login + off_login));
    assert!(has(
        &h,
        &format!("Showing {} of {total} items", login + off_login)
    ));
    click(&mut h, "Clear Filters");
    filter(&mut h, &format!("On ({on})"));
    assert!(has(&h, &format!("Showing {on} of {total} items")));
    click(&mut h, "Remove filter On");
    filter(&mut h, &format!("Off ({off})"));
    click(&mut h, "Clear Filters");
    filter(
        &mut h,
        &format!("Not signed by a known developer ({unsigned})"),
    );
    assert!(has(&h, &format!("Showing {unsigned} of {total} items")));
    assert!(!has(&h, "Login items"));
    click(&mut h, "Clear Filters");
    // Search: the agent's label, program or app.
    type_into(&mut h, "Search startup items", "genieo");
    let n = data
        .launch
        .iter()
        .filter(|i| {
            i.label.to_lowercase().contains("genieo")
                || i.program
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("genieo")
        })
        .count();
    assert!(n > 0);
    assert!(has(&h, &format!("Showing {n} of {total} items")));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().startup.search.is_empty());
    // Sort by signer, the other way round; remembered with the filter after a relaunch.
    sort(&mut h, "Sort by signer");
    sort(&mut h, "Z to A");
    filter(&mut h, &format!("Agents ({agents})"));
    let h2 = startup(&mac);
    assert_eq!(
        h2.get_by_label("Sort").value().as_deref(),
        Some("Sort by signer, Z to A")
    );
    assert!(has(&h2, &format!("Showing {agents} of {total} items")));
    let p = &Settings::load(&mac.env).lists["startup"];
    assert_eq!((p.sort.as_str(), p.reverse), ("signer", true));
    assert_eq!(p.filters, ["agents"]);
}

// ---------- Disk Map ----------

fn disk_mac() -> FakeMac {
    let mac = FakeMac::sample();
    mac.home_file("Movies/film.mov", 3_000_000);
    mac.home_file("Projects/app/src/main.rs", 40_000);
    mac.home_file("notes.txt", 20_000);
    mac.home_file("zeta.log", 10_000);
    mac.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    mac
}

fn disk_map(mac: &FakeMac) -> H {
    let mut h = harness(mac);
    click(&mut h, "Files");
    click(&mut h, "Disk Map");
    click(&mut h, "Map Home Folder");
    settle(&mut h);
    h
}

#[test]
fn disk_map_list_filters_sorts_and_keeps_hidden_ticks() {
    let mac = disk_mac();
    let mut h = disk_map(&mac);
    let home = mac.env.home();
    let kids: Vec<String> = cleancraft_core::fsutil::children(home)
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| !n.starts_with('.'))
        .collect();
    let node = h.state().disk_map.current_node().cloned().expect("node");
    let total = node.children.len();
    let files = node.children.iter().filter(|c| !c.is_dir).count();
    assert!(files >= 2 && total > files);
    // Tick Movies, then show files only: Movies is hidden but still counted.
    click(&mut h, "Select Movies");
    filter(&mut h, &format!("Files ({files})"));
    assert!(has(&h, &format!("Showing {files} of {total} items")));
    assert!(!has(&h, "Select Movies") && has(&h, "Select notes.txt"));
    assert!(has_text(&h, "1 selected (1 hidden by filters)"));
    // Sort by name: notes.txt above zeta.log, then the other way round.
    sort(&mut h, "Sort by name");
    let names = vec![
        "Select notes.txt".to_string(),
        "Select zeta.log".to_string(),
    ];
    assert_eq!(order(&h, &names), names);
    sort(&mut h, "Z to A");
    assert_eq!(
        order(&h, &names),
        vec![
            "Select zeta.log".to_string(),
            "Select notes.txt".to_string()
        ]
    );
    // Search narrows.
    type_into(&mut h, "Search this folder", "zeta");
    assert!(has(&h, &format!("Showing 1 of {total} items")));
    // The confirmation lists the hidden ticked folder; it moves only after it.
    let movies = mac.env.home_path("Movies");
    assert!(movies.exists());
    click(&mut h, "Review and Remove");
    assert!(has(&h, &mac.env.display(&movies)) || has_text(&h, "Movies"));
    click(&mut h, "Move to Trash");
    settle(&mut h);
    assert!(!movies.exists());
    // Remembered after a relaunch.
    let p = &Settings::load(&mac.env).lists["disk_map"];
    assert_eq!(p.filters, ["files"]);
    assert_eq!((p.sort.as_str(), p.reverse), ("name", true));
    let mut h2 = disk_map(&mac);
    assert!(!has(&h2, "Select Projects"), "folders still filtered out");
    click(&mut h2, "Clear Filters");
    assert!(has(&h2, "Select Projects"));
    let _ = kids;
}

// ---------- History ----------

fn record(env: &cleancraft_core::Env, id: &str, f: FeatureId, days: i64, size: u64, name: &str) {
    let t = chrono::Utc::now().timestamp() - days * 86_400;
    let original = env.home_path(&format!("Downloads/{name}"));
    executor::append_journal(
        env,
        &ActionRecord {
            id: id.into(),
            plan_id: format!("plan-{id}"),
            feature: Some(f),
            time: t,
            size,
            kind: ActionKind::Moved {
                original: original.clone(),
                stored: env.home_path(&format!(".Trash/{name}")),
                method: Method::Trash,
                stored_id: None,
            },
            undone: None,
        },
    )
    .expect("journal");
}

#[test]
fn history_search_module_date_and_order() {
    let mac = FakeMac::new();
    record(
        &mac.env,
        "a",
        FeatureId::SystemJunk,
        0,
        1_000,
        "today.cache",
    );
    record(
        &mac.env,
        "b",
        FeatureId::LargeOldFiles,
        3,
        9_000_000,
        "week.zip",
    );
    record(&mac.env, "c", FeatureId::SystemJunk, 20, 5_000, "month.log");
    record(&mac.env, "d", FeatureId::Duplicates, 90, 2_000, "old.bin");
    let mut h = harness(&mac);
    h.state_mut().navigate("history");
    settle(&mut h);
    assert!(has(&h, "4 actions"));
    let titles = |h: &H| -> Vec<String> {
        let mut v: Vec<(f32, String)> = ["today.cache", "week.zip", "month.log", "old.bin"]
            .iter()
            .filter_map(|n| {
                h.query_all_by_label_contains(n)
                    .next()
                    .map(|x| (x.rect().center().y, n.to_string()))
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, n)| n).collect()
    };
    assert_eq!(
        titles(&h),
        ["today.cache", "week.zip", "month.log", "old.bin"]
    );
    filter(&mut h, "System Junk (2)");
    assert_eq!(titles(&h), ["today.cache", "month.log"]);
    assert!(has(&h, "Showing 2 of 4 actions"));
    filter(&mut h, "Last 7 days (2)");
    assert_eq!(titles(&h), ["today.cache"]);
    click(&mut h, "Clear Filters");
    filter(&mut h, "Today (1)");
    assert_eq!(titles(&h), ["today.cache"]);
    click(&mut h, "Remove filter Today");
    filter(&mut h, "Last 30 days (3)");
    assert_eq!(titles(&h), ["today.cache", "week.zip", "month.log"]);
    click(&mut h, "Clear Filters");
    sort(&mut h, "Oldest first");
    assert_eq!(
        titles(&h),
        ["old.bin", "month.log", "week.zip", "today.cache"]
    );
    sort(&mut h, "Sort by size");
    assert_eq!(titles(&h)[0], "week.zip");
    sort(&mut h, "Sort by date");
    type_into(&mut h, "Search history", "zip");
    assert_eq!(titles(&h), ["week.zip"]);
    assert!(has(&h, "Showing 1 of 4 actions"));
    click(&mut h, "Clear Filters");
    assert_eq!(titles(&h).len(), 4);
    // Remembered.
    filter(&mut h, "Duplicates (1)");
    let mut h2 = harness(&mac);
    h2.state_mut().navigate("history");
    settle(&mut h2);
    assert_eq!(titles(&h2), ["old.bin"]);
}

// ---------- Live Monitor ----------

#[test]
fn live_monitor_top_lists_sort_by_cpu_memory_or_name() {
    let mac = FakeMac::new();
    testkit_monitor::answer_all(mac.env.fake_sys().expect("fake sys"));
    let mut h = harness(&mac);
    h.state_mut().navigate("monitor");
    for _ in 0..3 {
        h.state_mut().monitor.interval = std::time::Duration::ZERO;
        settle(&mut h);
    }
    h.state_mut().monitor.interval = std::time::Duration::from_secs(3600);
    settle(&mut h);
    let procs = h
        .state()
        .monitor
        .last
        .as_ref()
        .expect("reading")
        .processes
        .clone();
    let top = proc_names::top_groups_by_cpu(&procs, cleancraft_core::monitor::TOP_APPS);
    assert!(top.len() > 2);
    let labels: Vec<String> = top
        .iter()
        .map(|g| {
            if g.processes > 1 {
                format!("{} ({} processes)", g.name, g.processes)
            } else {
                g.name.clone()
            }
        })
        .collect();
    // The CPU card's list: highest CPU first.
    let first_cpu = order(&h, &labels);
    assert_eq!(first_cpu.first(), labels.first(), "highest CPU first");
    // The CPU card's Sort is the first one on the page.
    let sort_node = h.get_all_by_label("Sort").next().expect("sort");
    assert_eq!(sort_node.value().as_deref(), Some("Sort by CPU"));
    sort_node.click();
    settle(&mut h);
    click(&mut h, "Sort by name");
    let mut want = labels.clone();
    want.sort_by_key(|l| l.to_lowercase());
    // Labels may appear in both cards: compare the CPU card (top of the page) by the first
    // node of each.
    assert_eq!(order(&h, &labels), want, "A to Z");
    let p = &Settings::load(&mac.env).lists["monitor.cpu"];
    assert_eq!(p.sort, "name");
}
