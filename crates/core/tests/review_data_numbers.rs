//! Data-qa review (reviewer "data"): every number CleanCraft shows, recalculated independently
//! from the fake disk or from the sample tool output. Ignored tests prove a wrong number
//! (`DAT-n` on the board); the others protect numbers found right.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cleancraft_core::executor::{self, ActionKind};
use cleancraft_core::features::large_old::{Location, scan_location};
use cleancraft_core::features::space_lens::plan_for;
use cleancraft_core::features::{self, ScanCtx, trash};
use cleancraft_core::fsutil::{entry_size, human_size, tree_size};
use cleancraft_core::monitor::{self, Sampler};
use cleancraft_core::space::{self, Node, Tree};
use cleancraft_core::sweep;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_monitor::*;
use cleancraft_core::{Fault, FeatureId, Plan, Risk, Settings};

fn build(root: &Path) -> Tree {
    let never = AtomicBool::new(false);
    space::build(root, &never, &|_, _| {})
}

/// Every node as (path, size, files), for comparing a patched tree with a fresh scan.
fn flat(t: &Tree) -> Vec<(PathBuf, u64, u64)> {
    fn walk(n: &Node, out: &mut Vec<(PathBuf, u64, u64)>) {
        out.push((n.path.clone(), n.size, n.files));
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut v = Vec::new();
    if let Some(r) = &t.root {
        walk(r, &mut v);
    }
    v.sort();
    v
}

fn alloc(p: &Path) -> u64 {
    entry_size(&fs::symlink_metadata(p).unwrap())
}

// ------------------------------------------------------------------------------------------
// human_size: decimal units like Finder.

#[test]
fn human_size_boundaries_that_are_right() {
    assert_eq!(human_size(0), "Zero KB");
    assert_eq!(human_size(999), "999 bytes");
    assert_eq!(human_size(1_000), "1 KB");
    assert_eq!(human_size(999_499), "999 KB");
    assert_eq!(human_size(1_000_000), "1.00 MB");
    assert_eq!(human_size(1_000_000_000), "1.00 GB");
    // Decimal, never binary.
    assert_eq!(human_size(1_024_000), "1.02 MB");
}

#[test]
fn human_size_rolls_over_after_rounding() {
    // Rounding to the shown precision must not produce a value that belongs to the next
    // unit or the next precision band. Finder shows 999,999 bytes as "1 MB".
    let cases = [
        (999_500u64, "1.00 MB"),
        (999_999, "1.00 MB"),
        (999_999_999, "1.00 GB"),
        (9_999_000, "10.0 MB"),
        (99_990_000, "100 MB"),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(b, want)| human_size(*b) != *want)
        .map(|(b, want)| format!("{b} bytes: shown '{}', expected '{want}'", human_size(*b)))
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn singular_counts_and_sizes() {
    assert_eq!(human_size(1), "1 byte");
    let mut p = Plan::new(FeatureId::SystemJunk);
    p.push("/a".into(), 5_000, "c", "r", Risk::Low, true);
    // CLI `cleancraft scan` / `clean` print this line.
    assert_eq!(p.summary(), "1 item, 1 selected (5 KB)");
}

// ------------------------------------------------------------------------------------------
// Plan totals.

#[test]
fn plan_totals_follow_selection_and_locks() {
    let mut p = Plan::new(FeatureId::SystemJunk);
    p.push("/a".into(), 1_000, "x", "r", Risk::Low, true);
    p.push("/b".into(), 2_000, "x", "r", Risk::Low, true);
    p.push("/c".into(), 4_000, "y", "r", Risk::Low, false);
    let locked = p.push("/d".into(), 8_000, "y", "r", Risk::Low, true);
    p.lock(locked, "admin");
    assert_eq!(p.selected_size(), 3_000);
    assert_eq!(p.selected_count(), 2);
    assert_eq!(p.total_size(), 15_000);
    assert_eq!(p.category_size("y"), 12_000);
    assert_eq!(p.category_count("y"), 2);
    p.set_category_selected("y", true);
    assert_eq!(
        p.selected_size(),
        7_000,
        "a locked item is never counted as selected"
    );
}

/// System Junk on the sample Mac: every item's size is its tree size on disk, and no item is
/// inside another item (a nested pair would be counted twice in every total).
#[test]
fn system_junk_items_are_disk_sizes_and_never_nested() {
    let mac = FakeMac::sample();
    mac.home_file(
        "Library/Caches/com.example.App/org.sparkle-project.Sparkle/u.zip",
        300_000,
    );
    mac.home_file("Library/Caches/com.example.App/data.db", 120_000);
    mac.home_file("Library/Caches/Homebrew/downloads/x.tar.gz", 200_000);
    mac.home_file("Library/Logs/Claude/main.log", 30_000);
    let plan = features::scan(&mac.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
    assert!(!plan.items.is_empty());
    for it in &plan.items {
        assert_eq!(it.size, tree_size(&it.path).0, "{}", it.path.display());
        for other in &plan.items {
            assert!(
                other.id == it.id || !it.path.starts_with(&other.path),
                "{} is inside {}",
                it.path.display(),
                other.path.display()
            );
        }
    }
}

// ------------------------------------------------------------------------------------------
// Done screens: freed = sum of what really moved.

#[test]
fn freed_is_the_sum_of_really_moved_items_when_some_are_skipped() {
    let mac = FakeMac::sample();
    let s = Settings::default();
    let plan = features::scan(&mac.env, &s, FeatureId::SystemJunk).unwrap();
    let sel: Vec<_> = plan.selected().cloned().collect();
    assert!(sel.len() >= 3);
    // One refused by permissions, one open in an app.
    mac.env.inject_fault(&sel[0].path, Fault::PermissionDenied);
    mac.env.fake_sys().unwrap().set_open(&sel[1].path);
    let before: Vec<(PathBuf, u64)> = sel
        .iter()
        .map(|i| (i.path.clone(), tree_size(&i.path).0))
        .collect();
    let rep = features::run_plan(&mac.env, &s, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.skipped.len(), 2);
    assert_eq!(rep.moved.len(), sel.len() - 2);
    let expect: u64 = before
        .iter()
        .filter(|(p, _)| !p.exists())
        .map(|(_, s)| *s)
        .sum();
    assert_eq!(rep.freed, expect);
    // Journal sizes: each record is the size of what now sits in the Trash.
    let hist = executor::history(&mac.env);
    let journal_sum: u64 = hist
        .iter()
        .filter(|r| r.plan_id == plan.id)
        .map(|r| r.size)
        .sum();
    assert_eq!(journal_sum, rep.freed);
    for r in &hist {
        if let ActionKind::Moved { stored, .. } = &r.kind {
            assert_eq!(r.size, tree_size(stored).0, "{}", stored.display());
        }
    }
}

// ------------------------------------------------------------------------------------------
// Disk Map: hard links once, refresh equals a fresh scan, Done equals the map.

#[test]
fn disk_map_freed_matches_the_map_for_hard_links() {
    let mac = FakeMac::new();
    let a = mac.home_file("Shots/a.bin", 400_000);
    fs::hard_link(&a, mac.env.home_path("Shots/a copy.bin")).unwrap();
    let folder = mac.env.home_path("Shots");
    let t = build(mac.env.home());
    let shown = t.find(&folder).unwrap().size;
    // Recalculated: the folder plus one copy of the data.
    assert_eq!(shown, alloc(&folder) + alloc(&a));
    let plan = plan_for(&mac.env, &[(folder.clone(), shown)]);
    let rep = features::run_plan(
        &mac.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 1);
    assert_eq!(
        rep.freed,
        shown,
        "map and confirmation said {}, Done says {} freed",
        human_size(shown),
        human_size(rep.freed)
    );
}

#[test]
fn disk_map_refresh_counts_hard_links_once() {
    let mac = FakeMac::new();
    let a = mac.home_file("Alpha/data.bin", 400_000);
    let beta = mac.env.home_path("Beta");
    fs::create_dir_all(&beta).unwrap();
    fs::hard_link(&a, beta.join("data link.bin")).unwrap();
    mac.home_file("Beta/notes.txt", 20_000);
    let mut t = build(mac.env.home());
    let before = t.root.as_ref().unwrap().size;
    // The user removes the folder Beta in the Disk Map; the page patches the tree.
    let plan = plan_for(&mac.env, &[(beta.clone(), t.find(&beta).unwrap().size)]);
    let rep = features::run_plan(
        &mac.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 1);
    t.refresh_after(&mac.env, &rep.moved);
    let fresh = build(mac.env.home());
    assert_eq!(
        t.root.as_ref().unwrap().size,
        fresh.root.as_ref().unwrap().size,
        "home was {} before; after moving Beta to the Trash the map shows {}, a new scan {}",
        human_size(before),
        human_size(t.root.as_ref().unwrap().size),
        human_size(fresh.root.as_ref().unwrap().size)
    );
}

#[test]
fn disk_map_refresh_after_removing_the_first_link() {
    let mac = FakeMac::new();
    let a = mac.home_file("Alpha/data.bin", 400_000);
    fs::create_dir_all(mac.env.home_path("Beta")).unwrap();
    let b = mac.env.home_path("Beta/data link.bin");
    fs::hard_link(&a, &b).unwrap();
    let mut t = build(mac.env.home());
    // Beta's link was met second, so it shows 0 bytes.
    assert_eq!(t.find(&b).unwrap().size, 0);
    let plan = plan_for(&mac.env, &[(a.clone(), t.find(&a).unwrap().size)]);
    let rep = features::run_plan(
        &mac.env,
        &Settings::default(),
        &plan,
        &plan.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(rep.moved.len(), 1);
    t.refresh_after(&mac.env, &rep.moved);
    // The data now sits in the Trash (met first in a new scan), Beta's link still 0.
    assert_eq!(
        flat(&t),
        flat(&build(mac.env.home())),
        "patched tree differs from a new scan"
    );
}

#[test]
fn disk_map_sizes_match_a_recalculation_with_links() {
    let mac = FakeMac::new();
    let a = mac.home_file("P/a.bin", 300_000);
    mac.home_file("P/b.bin", 100_000);
    fs::hard_link(&a, mac.env.home_path("P/z-link.bin")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(mac.env.home_path("P"), mac.env.home_path("P/loop")).unwrap();
    let t = build(mac.env.home());
    let p = t.find(&mac.env.home_path("P")).unwrap();
    let mut expect =
        alloc(&mac.env.home_path("P")) + alloc(&a) + alloc(&mac.env.home_path("P/b.bin"));
    #[cfg(unix)]
    {
        expect += alloc(&mac.env.home_path("P/loop"));
    }
    assert_eq!(p.size, expect);
    assert_eq!(
        p.files, 4,
        "files are counted per path (link and symlink included)"
    );
}

#[test]
fn disk_map_used_plus_free_is_the_disk() {
    let mac = FakeMac::new();
    mac.env.fake_sys().unwrap().answer("df", DF);
    let d = space::disk_use(&mac.env, mac.env.home());
    let (total, used, free) = (d.total.unwrap(), d.used.unwrap(), d.free.unwrap());
    // Recalculated from the sample line: 488245288 KiB blocks, 80000000 KiB available.
    assert_eq!(total, 488_245_288 * 1024);
    assert_eq!(free, 80_000_000 * 1024);
    // Live Monitor draws used = total - free; Finder's Used is the same. The Disk Map must
    // agree, otherwise "X used of T, F free" does not add up.
    assert_eq!(
        used,
        total - free,
        "Disk Map says {} used of {}, {} free",
        human_size(used),
        human_size(total),
        human_size(free)
    );
}

// ------------------------------------------------------------------------------------------
// Large & Old: ages.

fn local_midnight_today() -> SystemTime {
    use chrono::{Local, TimeZone};
    let now = Local::now();
    let midnight = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let t = Local
        .from_local_datetime(&midnight)
        .earliest()
        .unwrap_or(now)
        .timestamp();
    UNIX_EPOCH + Duration::from_secs(t as u64)
}

#[test]
fn last_opened_uses_calendar_days() {
    let mac = FakeMac::new();
    let f = mac.home_file("Downloads/movie.mov", 200_000);
    // One minute before local midnight: yesterday on the user's calendar, always less than
    // 24 hours ago.
    let t = local_midnight_today() - Duration::from_secs(60);
    let file = fs::File::options().write(true).open(&f).unwrap();
    file.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t))
        .unwrap();
    drop(file);
    let s = Settings {
        large_min_bytes: 100_000,
        ..Settings::default()
    };
    let never = AtomicBool::new(false);
    let ctx = ScanCtx {
        env: &mac.env,
        settings: &s,
        cancel: &never,
        progress: &|_| {},
    };
    let plan = scan_location(&ctx, &Location::Downloads).unwrap();
    let it = plan
        .items
        .iter()
        .find(|i| i.path == f)
        .expect("listed as large");
    assert!(
        it.reason.ends_with("last opened yesterday"),
        "reason: {}",
        it.reason
    );
}

#[test]
fn old_files_need_the_full_number_of_days() {
    let mac = FakeMac::new();
    let old = mac.home_file("Downloads/old.zip", 2_000_000);
    let young = mac.home_file("Downloads/young.zip", 2_000_000);
    let now = SystemTime::now();
    for (p, secs) in [(&old, 365 * 86_400 + 3_600), (&young, 365 * 86_400 - 3_600)] {
        let t = now - Duration::from_secs(secs);
        let f = fs::File::options().write(true).open(p).unwrap();
        f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t))
            .unwrap();
    }
    let s = Settings::default();
    let never = AtomicBool::new(false);
    let ctx = ScanCtx {
        env: &mac.env,
        settings: &s,
        cancel: &never,
        progress: &|_| {},
    };
    let plan = scan_location(&ctx, &Location::Downloads).unwrap();
    let o = plan
        .items
        .iter()
        .find(|i| i.path == old)
        .expect("old listed");
    assert_eq!(o.reason, "Not opened for 365 days");
    assert_eq!(o.size, alloc(&old));
    assert!(
        plan.items.iter().all(|i| i.path != young),
        "364 days is not old"
    );
    assert!(plan.items.iter().all(|i| !i.selected));
}

// ------------------------------------------------------------------------------------------
// Live Monitor.

fn sampled() -> (FakeMac, monitor::Snapshot) {
    let m = FakeMac::new();
    answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    let mut s = Sampler::new();
    let snap = s.sample(&m.env, 1_760_086_400.0);
    (m, snap)
}

#[test]
fn monitor_numbers_recalculated_from_the_samples() {
    let (_m, s) = sampled();
    let mem = s.memory.as_ref().unwrap();
    let page = 16_384u64;
    // vm_stat sample: anonymous 340000, purgeable 10000, wired 130000, compressor 50000,
    // file-backed 250000.
    assert_eq!(mem.app, (340_000 - 10_000) * page);
    assert_eq!(mem.wired, 130_000 * page);
    assert_eq!(mem.compressed, 50_000 * page);
    assert_eq!(mem.cached, (250_000 + 10_000) * page);
    assert_eq!(mem.used, mem.app + mem.wired + mem.compressed);
    assert_eq!(mem.total, 17_179_869_184);
    // sysctl: 1024.50M used of 2048.00M swap, binary megabytes.
    assert_eq!(mem.swap_used, Some((1024.5 * 1_048_576.0) as u64));
    assert_eq!(mem.swap_total, Some(2048 * 1_048_576));
    // CPU: 100 - idle of the LAST top sample (82.6% idle).
    let cpu = s.cpu.as_ref().unwrap();
    assert!((cpu.load - 17.4).abs() < 1e-9);
    assert_eq!(cpu.cores, Some(8));
    // Uptime from kern.boottime sec = 1760000000.
    assert_eq!(s.uptime_secs, Some(86_400));
    // Battery health = AppleRawMaxCapacity / DesignCapacity = 4180 / 4382.
    let Some(monitor::BatteryReading::Present(b)) = &s.battery else {
        panic!("battery")
    };
    assert_eq!(b.health, Some(((4180.0 / 4382.0) * 100.0f64).round() as u8));
    assert_eq!(
        b.cycles,
        Some(213),
        "top-level CycleCount, not the nested one"
    );
    assert_eq!(b.remaining_minutes, Some(4 * 60 + 12));
}

#[test]
fn network_speed_divides_by_the_real_time_between_readings() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    answer_all(f);
    let mut s = Sampler::new();
    let first = s.sample(&m.env, 1_000.0);
    assert!(first.network.is_none(), "one reading has no speed");
    f.answer("netstat", NETSTAT_2);
    // 2.5 seconds later (a slow reading), not the nominal 2.
    let second = s.sample(&m.env, 1_002.5);
    let n = second.network.unwrap();
    // en0 + utun0, loopback left out: rx 4,000,000 bytes, tx 200,000 bytes.
    assert_eq!(n.down, (4_000_000.0f64 / 2.5).round() as u64);
    assert_eq!(n.up, (200_000.0f64 / 2.5).round() as u64);
}

#[test]
fn memory_total_reads_like_the_mac() {
    let (_m, s) = sampled();
    // hw.memsize 17179869184 = 16 GiB, sold and shown by macOS as 16 GB.
    let lines = monitor::menu_bar_entries(&s);
    let mem = &lines[0];
    assert!(
        mem.ends_with("of 16 GB used")
            || mem.ends_with("of 16.0 GB used")
            || mem.ends_with("of 16.00 GB used"),
        "menu line: {mem}"
    );
}

#[test]
fn monitor_trash_equals_trash_bins() {
    let (m, s) = sampled();
    m.home_file(".Trash/folder/a.bin", 300_000);
    let mut sm = Sampler::new();
    let s2 = sm.sample(&m.env, 1_760_086_400.0);
    let _ = s;
    let (bins, _) = trash::contents_size(&m.env.user_trash());
    // Recalculated: each entry of the Trash with the shared formula.
    let expect: u64 = fs::read_dir(m.env.user_trash())
        .unwrap()
        .flatten()
        .map(|e| tree_size(&e.path()).0)
        .sum();
    assert_eq!(bins, expect);
    assert_eq!(
        s2.storage.trash,
        Some(bins),
        "Live Monitor shows {}, Trash Bins {}",
        human_size(s2.storage.trash.unwrap_or(0)),
        human_size(bins)
    );
}

// ------------------------------------------------------------------------------------------
// Quick Sweep, CLI and MCP agree with the module plans.

#[test]
fn sweep_tiles_and_text_match_each_module_plan() {
    let m = sweep_mac();
    let s = Settings::default();
    let src = sweep_sources();
    let res = sweep::scan(&m.env, &s, &src, &AtomicBool::new(false), &|_| {});
    let junk = res
        .tile(FeatureId::SystemJunk)
        .and_then(|t| t.plan())
        .unwrap();
    let direct = features::scan(&m.env, &s, FeatureId::SystemJunk).unwrap();
    assert_eq!(junk.selected_size(), direct.selected_size());
    assert_eq!(junk.items.len(), direct.items.len());
    // Recalculated from disk.
    let disk: u64 = junk.selected().map(|i| tree_size(&i.path).0).sum();
    assert_eq!(junk.selected_size(), disk);
    let line = sweep::describe(res.tile(FeatureId::SystemJunk).unwrap());
    assert_eq!(
        line,
        format!(
            "System Junk: {} selected in {} of {} items",
            human_size(disk),
            junk.selected_count(),
            junk.items.len()
        )
    );
    // Ready = junk + mail (Trash never runs from the sweep).
    let mail = res
        .tile(FeatureId::MailAttachments)
        .and_then(|t| t.plan())
        .unwrap();
    assert_eq!(res.ready_bytes(&[]), disk + mail.selected_size());
    assert_eq!(res.ready_bytes(&[FeatureId::MailAttachments]), disk);
}

#[test]
fn app_last_opened_uses_calendar_days() {
    let now = chrono::Utc::now().timestamp();
    let t = local_midnight_today()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 60;
    assert_eq!(
        cleancraft_core::apps::last_used_text(Some(t), now),
        "Yesterday"
    );
}
