//! Sample outputs of the macOS tools the Live Monitor reads, and helpers that make
//! `FakeSys` answer them. The samples follow the documented formats of macOS 14 and 15 tools;
//! each is marked SAMPLE and must be compared with a real Mac (hand checks in
//! docs/agents/reports/monitor.md).

use std::path::PathBuf;
use std::sync::Arc;

use crate::env::Env;
use crate::features::{self, Feature, ScanCtx};
use crate::fsutil::{children, tree_size};
use crate::plan::{FeatureId, Plan, Risk};
use crate::settings::Settings;
use crate::sweep::{CountSource, Sources};
use crate::sys::{CmdOut, FakeSys};
use crate::testkit::FakeMac;

// SAMPLE: `sysctl hw.memsize hw.ncpu kern.boottime vm.loadavg vm.swapusage
// kern.memorystatus_vm_pressure_level` on an Apple silicon Mac.
pub const SYSCTL: &str = "hw.memsize: 17179869184
hw.ncpu: 8
kern.boottime: { sec = 1760000000, usec = 123456 } Thu Oct  9 08:53:20 2025
vm.loadavg: { 1.85 2.10 2.30 }
vm.swapusage: total = 2048.00M  used = 1024.50M  free = 1023.50M  (encrypted)
kern.memorystatus_vm_pressure_level: 1
";

// SAMPLE: `top -l 2 -n 0 -s 1` (two samples; the first covers time since boot).
pub const TOP: &str = "Processes: 612 total, 3 running, 609 sleeping, 3012 threads
2025/10/18 10:00:00
Load Avg: 1.85, 2.10, 2.30
CPU usage: 4.10% user, 3.20% sys, 92.70% idle
SharedLibs: 512M resident, 98M data, 41M linkedit.
PhysMem: 15G used (2100M wired, 900M compressor), 1000M unused.

Processes: 612 total, 3 running, 609 sleeping, 3012 threads
2025/10/18 10:00:01
Load Avg: 1.85, 2.10, 2.30
CPU usage: 7.69% user, 10.25% sys, 82.6% idle
";

// SAMPLE: `ps -axco pid=,%cpu=,rss=,comm=`.
pub const PS: &str = "    1   0.3  14000 launchd
  512  12.5 812000 Safari
  640  48.0 300000 Google Chrome Helper
  700   0.0 2100000 Xcode
  801   2.0  50000 WindowServer
  900   1,5  40000 Finder
";

// SAMPLE: `vm_stat` on Apple silicon (16 KiB pages).
pub const VM_STAT: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                9000.
Pages active:                            300000.
Pages inactive:                          290000.
Pages speculative:                         5000.
Pages throttled:                              0.
Pages wired down:                        130000.
Pages purgeable:                          10000.
\"Translation faults\":                 912345678.
Pages copy-on-write:                   12345678.
Pages zero filled:                    456789012.
Pages reactivated:                      1234567.
Pages purged:                            234567.
File-backed pages:                       250000.
Anonymous pages:                         340000.
Pages stored in compressor:              150000.
Pages occupied by compressor:             50000.
Decompressions:                         3456789.
Compressions:                           4567890.
Pageins:                                5678901.
Pageouts:                                 12345.
Swapins:                                      0.
Swapouts:                                     0.
";

// SAMPLE: `df -Pk /Users/me`.
pub const DF: &str = "Filesystem     1024-blocks      Used Available Capacity  Mounted on
/dev/disk3s1     488245288 400000000  80000000    84%    /System/Volumes/Data
";

// SAMPLE: `pmset -g batt` on a MacBook, on battery.
pub const PMSET_BATT: &str = "Now drawing from 'Battery Power'
 -InternalBattery-0 (id=4653155)\t64%; discharging; 4:12 remaining present: true
";

// SAMPLE: `pmset -g batt` on a MacBook, charging, no estimate yet.
pub const PMSET_CHARGING: &str = "Now drawing from 'AC Power'
 -InternalBattery-0 (id=4653155)\t87%; charging; (no estimate) present: true
";

// SAMPLE: `pmset -g batt` on a Mac mini.
pub const PMSET_DESKTOP: &str = "Now drawing from 'AC Power'
";

// SAMPLE: `ioreg -rn AppleSmartBattery` on Apple silicon (excerpt, nested dicts on one line).
pub const IOREG_AS: &str = "+-o AppleSmartBattery  <class AppleSmartBattery, id 0x100000abc, registered, matched, active, busy 0 (0 ms), retain 8>
    {
      \"PostChargeWaitSeconds\" = 120
      \"BatteryData\" = {\"DesignCapacity\"=4382,\"CycleCount\"=99}
      \"AppleRawMaxCapacity\" = 4180
      \"MaxCapacity\" = 100
      \"CycleCount\" = 213
      \"DesignCapacity\" = 4382
      \"IsCharging\" = No
      \"Temperature\" = 3051
    }
";

// SAMPLE: `ioreg -rn AppleSmartBattery` on an Intel MacBook (mAh in MaxCapacity).
pub const IOREG_INTEL: &str = "    {
      \"MaxCapacity\" = 5000
      \"CycleCount\" = 640
      \"DesignCapacity\" = 6559
    }
";

// SAMPLE: `netstat -ib`. utun rows have no Address; lo0 is skipped.
pub const NETSTAT_1: &str = "Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
lo0        16384 <Link#1>                        812345     0  123456789   812345     0  123456789     0
lo0        16384 127           localhost         812345     -  123456789   812345     -  123456789     -
en0        1500  <Link#11>   aa:bb:cc:dd:ee:ff  5000000     0 6000000000  3000000     0  400000000     0
en0        1500  192.168.1     192.168.1.20     5000000     - 6000000000  3000000     -  400000000     -
utun0      1380  <Link#16>                          100     0      20000      120     0      30000     0
";

pub const NETSTAT_2: &str = "Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
lo0        16384 <Link#1>                        812999     0  223456789   812999     0  223456789     0
en0        1500  <Link#11>   aa:bb:cc:dd:ee:ff  5001000     0 6004000000  3000500     0  400200000     0
utun0      1380  <Link#16>                          100     0      20000      120     0      30000     0
";


/// Answer every Live Monitor command with the samples (network: the first sample).
pub fn answer_all(fake: &FakeSys) {
    fake.answer("sysctl", SYSCTL);
    fake.answer("top", TOP);
    fake.answer("ps", PS);
    fake.answer("vm_stat", VM_STAT);
    fake.answer("df", DF);
    fake.answer("pmset", PMSET_BATT);
    fake.answer("ioreg", IOREG_AS);
    fake.answer("netstat", NETSTAT_1);
}

/// Make one tool fail as if it were missing or broken: exit status 1, nothing on stdout.
pub fn break_tool(fake: &FakeSys, program: &str) {
    fake.answer_full(
        program,
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: format!("{program}: failed"),
        },
    );
}

// Quick Sweep stand-ins.

/// A stand-in Mail module for Quick Sweep tests (the real one is another builder's):
/// proposes every entry of `~/Library/Mail Downloads`, the ones named `keep*` unselected.
pub struct TestMail;

impl Feature for TestMail {
    fn id(&self) -> FeatureId {
        FeatureId::MailAttachments
    }
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.home_path("Library/Mail Downloads")]
    }
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut p = Plan::new(FeatureId::MailAttachments);
        p.add_category("dl", "Mail downloads", "Copies of attachments.", Risk::Medium);
        for c in children(&ctx.env.home_path("Library/Mail Downloads")) {
            let keep = c.file_name().unwrap().to_string_lossy().starts_with("keep");
            p.push(
                c.clone(),
                tree_size(&c).0,
                "dl",
                "attachment copy",
                Risk::Medium,
                !keep,
            );
        }
        Ok(p)
    }
}

/// A module whose scan always fails.
pub struct Broken;

impl Feature for Broken {
    fn id(&self) -> FeatureId {
        FeatureId::Malware
    }
    fn allow_roots(&self, _env: &Env) -> Vec<PathBuf> {
        Vec::new()
    }
    fn scan(&self, _ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        anyhow::bail!("rules could not be loaded")
    }
}

/// Trash stand-in: proposes what is in ~/.Trash.
pub struct TestTrash;

impl Feature for TestTrash {
    fn id(&self) -> FeatureId {
        FeatureId::TrashBins
    }
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.user_trash()]
    }
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut p = Plan::new(FeatureId::TrashBins);
        p.add_category("t", "Trash", "In the Trash.", Risk::Low);
        for c in children(&ctx.env.user_trash()) {
            p.push(c.clone(), tree_size(&c).0, "t", "in the Trash", Risk::Low, true);
        }
        Ok(p)
    }
}

pub fn three_updates(_: &Env, _: &Settings) -> anyhow::Result<usize> {
    Ok(3)
}

/// System Junk (real) plus the stand-ins above and an "updates" counter.
pub fn sweep_sources() -> Sources {
    Sources {
        plans: vec![
            Arc::from(features::get(FeatureId::SystemJunk).unwrap()),
            Arc::new(TestTrash),
            Arc::new(TestMail),
            Arc::new(Broken),
        ],
        counts: vec![CountSource {
            id: FeatureId::Updater,
            noun: "updates",
            count: three_updates,
        }],
    }
}

/// The sample Mac plus Mail downloads (two selected, `keep-c.doc` not) and a Trash file.
pub fn sweep_mac() -> FakeMac {
    let m = FakeMac::sample();
    m.home_file("Library/Mail Downloads/a/report.pdf", 400_000);
    m.home_file("Library/Mail Downloads/b.zip", 250_000);
    m.home_file("Library/Mail Downloads/keep-c.doc", 90_000);
    m.home_file(".Trash/old.dmg", 800_000);
    m
}

