//! Parsers of the Live Monitor on sample outputs. The samples follow the format of macOS 14
//! and 15 tools as documented in their man pages; they are marked SAMPLE and must be compared
//! with a real Mac (see docs/agents/reports/monitor.md, hand checks).

use cleancraft_core::monitor::*;
use cleancraft_core::testkit_monitor::*;

#[test]
fn sysctl_values() {
    let m = parse_sysctl(SYSCTL);
    assert_eq!(m["hw.memsize"], "17179869184");
    assert_eq!(m["hw.ncpu"], "8");
    assert_eq!(parse_boottime(&m["kern.boottime"]), Some(1_760_000_000));
    assert_eq!(parse_loadavg(&m["vm.loadavg"]), Some([1.85, 2.10, 2.30]));
    assert_eq!(
        parse_swapusage(&m["vm.swapusage"]),
        Some(((1024.5 * 1048576.0) as u64, 2048 * 1048576))
    );
    assert_eq!(
        parse_pressure_level(&m["kern.memorystatus_vm_pressure_level"]),
        Some(Pressure::Normal)
    );
    assert_eq!(parse_pressure_level("2"), Some(Pressure::Warning));
    assert_eq!(parse_pressure_level("4"), Some(Pressure::Critical));
    assert_eq!(parse_pressure_level("x"), None);
    // An unknown key prints only to stderr; the others still parse.
    assert_eq!(
        parse_sysctl("sysctl: unknown oid 'x'\nhw.ncpu: 4\n").len(),
        1
    );
    assert_eq!(parse_boottime("garbage"), None);
    assert_eq!(parse_loadavg("{ 1.0 }"), None);
}

#[test]
fn top_cpu_uses_last_sample() {
    let (load, user, sys) = parse_top_cpu(TOP).unwrap();
    assert!((load - 17.4).abs() < 1e-9, "{load}");
    assert_eq!((user, sys), (7.69, 10.25));
    assert_eq!(parse_top_cpu("Processes: 1 total\n"), None);
}

#[test]
fn ps_usage_and_top_lists() {
    let apps = parse_ps_usage(PS).unwrap();
    assert_eq!(apps.len(), 6);
    assert_eq!(apps[2].name, "Google Chrome Helper");
    assert_eq!(apps[2].memory, 300_000 * 1024);
    assert_eq!(apps[5].cpu, 1.5, "decimal comma accepted");
    let cpu = top_by_cpu(&apps, 2);
    assert_eq!(cpu[0].name, "Google Chrome Helper");
    assert_eq!(cpu[1].name, "Safari");
    let mem = top_by_memory(&apps, 2);
    assert_eq!(mem[0].name, "Xcode");
    assert_eq!(mem[1].name, "Safari");
    assert_eq!(parse_ps_usage("not a table\n"), None);
    assert_eq!(parse_ps_usage(""), None);
}

#[test]
fn vm_stat_memory_like_activity_monitor() {
    let (page, m) = parse_vm_stat(VM_STAT).unwrap();
    assert_eq!(page, 16384);
    assert_eq!(m["Translation faults"], 912_345_678);
    let mem = memory_from_vm_stat(17_179_869_184, page, &m).unwrap();
    assert_eq!(mem.app, (340_000 - 10_000) * 16384);
    assert_eq!(mem.wired, 130_000 * 16384);
    assert_eq!(mem.compressed, 50_000 * 16384);
    assert_eq!(mem.used, mem.app + mem.wired + mem.compressed);
    assert_eq!(mem.cached, (250_000 + 10_000) * 16384);
    assert_eq!(mem.available(), 17_179_869_184 - mem.used);
    assert!(parse_vm_stat("Pages free: 1.\n").is_none(), "no header");
    assert!(
        parse_vm_stat("Mach Virtual Memory Statistics: (page size of 4096 bytes)\n").is_none(),
        "no counters"
    );
}

#[test]
fn df_total_and_free() {
    assert_eq!(parse_df(DF), Some((488_245_288 * 1024, 80_000_000 * 1024)));
    assert_eq!(parse_df("Filesystem\n"), None);
}

#[test]
fn pmset_states() {
    let b = parse_pmset(PMSET_BATT).unwrap().unwrap();
    assert_eq!(b.percent, 64);
    assert_eq!(b.state, "discharging");
    assert!(!b.on_ac);
    assert_eq!(b.remaining_minutes, Some(252));
    let c = parse_pmset(PMSET_CHARGING).unwrap().unwrap();
    assert_eq!(
        (c.percent, c.state.as_str(), c.on_ac),
        (87, "charging", true)
    );
    assert_eq!(c.remaining_minutes, None);
    assert_eq!(
        parse_pmset(PMSET_DESKTOP),
        Some(None),
        "desktop: no battery"
    );
    assert_eq!(parse_pmset("pmset: error\n"), None);
}

#[test]
fn battery_health_both_kinds() {
    let io = parse_ioreg_ints(IOREG_AS);
    assert_eq!(io["CycleCount"], 213, "top-level key, not the nested one");
    assert_eq!(battery_health(&io), (Some(213), Some(95)));
    assert_eq!(
        battery_health(&parse_ioreg_ints(IOREG_INTEL)),
        (Some(640), Some(76))
    );
    // Only the percent figure: health unknown rather than a wrong 2%.
    let only_pct = parse_ioreg_ints("  \"MaxCapacity\" = 100\n  \"DesignCapacity\" = 4382\n");
    assert_eq!(battery_health(&only_pct), (None, None));
}

#[test]
fn netstat_counters_and_rate() {
    let a = parse_netstat(NETSTAT_1).unwrap();
    assert_eq!(a.rx, 6_000_000_000 + 20_000);
    assert_eq!(a.tx, 400_000_000 + 30_000);
    let b = parse_netstat(NETSTAT_2).unwrap();
    let r = net_rate(a, b, 2.0).unwrap();
    assert_eq!(r.down, 2_000_000);
    assert_eq!(r.up, 100_000);
    assert_eq!(net_rate(b, a, 2.0), None, "counter went back");
    assert_eq!(net_rate(a, b, 0.0), None);
    assert_eq!(parse_netstat("Name Mtu\n"), None);
}

#[test]
fn texts() {
    assert_eq!(uptime_text(3 * 86_400 + 4 * 3600 + 5), "3 days 4 h");
    assert_eq!(uptime_text(86_400 + 2 * 3600), "1 day 2 h");
    assert_eq!(uptime_text(375 * 86_400 + 5 * 3600), "375 days");
    assert_eq!(uptime_text(2 * 86_400 + 59), "2 days");
    assert_eq!(uptime_text(5 * 3600 + 12 * 60), "5 h 12 min");
    assert_eq!(uptime_text(720), "12 min");
    assert_eq!(rate_text(1_200_000), "1.20 MB/s");
    assert_eq!(pct(12.4), "12%");
}
