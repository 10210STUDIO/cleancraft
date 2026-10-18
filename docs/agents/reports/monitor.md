# monitor: Live Monitor and Quick Sweep

Rows R1-R7, Q1-Q3. Branch `agent/monitor`.

## What works (tests)

Core (`cargo test -p cleancraft-core --test monitor_parsers --test monitor_sampler --test sweep_run`):

- `monitor::Sampler::sample` reads, through `env.sys` only, read-only tools:
  `/usr/sbin/sysctl hw.memsize hw.ncpu kern.boottime vm.loadavg vm.swapusage kern.memorystatus_vm_pressure_level`,
  `/usr/bin/top -l 2 -n 0 -s 1` (CPU load from the second sample; falls back to the `ps` sum divided by cores),
  `/bin/ps -axco pid=,%cpu=,rss=,comm=` (top 5 by CPU and by memory), `/usr/bin/vm_stat`,
  `/bin/df -Pk <home>`, `~/.Trash` walked with `fsutil::tree_size`, `/usr/bin/pmset -g batt`,
  `/usr/sbin/ioreg -rn AppleSmartBattery`, `/usr/sbin/netstat -ib`.
  `monitor_sampler::full_snapshot_from_tools` checks every value and the exact command list.
- Unknown is never zero: a missing tool, a failing tool or output that does not parse gives `None` plus a plain-words entry in `Snapshot::problems` (`every_tool_missing_means_unknown_not_zero`, `garbage_output_is_unknown`).
- Memory follows Activity Monitor: used = (anonymous - purgeable) + wired + compressor pages, page size from the `vm_stat` header (`vm_stat_memory_like_activity_monitor`).
- Battery health: `AppleRawMaxCapacity / DesignCapacity` on Apple silicon, `MaxCapacity / DesignCapacity` on Intel; when only the percent-style MaxCapacity exists, health is Unknown instead of a wrong number (`battery_health_both_kinds`). A desktop Mac shows "No battery" and `ioreg` is not called (`desktop_mac_has_no_battery`).
- Network speed = byte counter difference of all non-loopback `<Link#>` rows over the time between two readings; a counter that goes back gives Unknown (`netstat_counters_and_rate`, `network_speed_from_two_samples`).
- Trash size without Full Disk Access: Unknown with "The Trash size needs Full Disk Access" (`trash_without_full_disk_access`).
- Alerts (`alerts_at_thresholds`): free < 10% of the disk or < 10 GB; memory pressure warning/critical or > 90% used; Trash > 5 GB. Thresholds are constants in `monitor.rs` (`LOW_FREE_FRACTION`, `LOW_FREE_BYTES`, `HEAVY_MEMORY_FRACTION`, `LARGE_TRASH_BYTES`), not Settings.
- Free Up RAM (`monitor::free_up_ram`): checks the journal is writable first (else nothing runs), runs `/usr/sbin/purge` through `Sys::run_admin`, logs `ActionKind::Command` (no undo); a cancelled password dialog (`-128` / "User canceled") runs nothing more and logs nothing; a failure is logged with ok=false and reported (`free_up_ram_*`).
- Quick Sweep core (`sweep.rs`): `Sources::registered()` takes `features::get` of System Junk, Trash Bins, Mail Downloads, Threat Check (missing ones get no tile) plus `count_sources()`; `scan` gives one tile per module (plan, count, or "could not scan"), `run` carries out each confirmed `(plan, digest)` with the new `features::run_plan_with`; a changed list or a missing module is refused and the others still run (`sweep_run::*`).

UI (`cargo test -p cleancraft-ui-egui --test qa_monitor --test qa_sweep`), by clicks on FakeMac + FakeSys:

- `qa_monitor::cards_show_the_parsed_reading`: every figure on the CPU, Memory, Storage, Battery cards equals the snapshot formatting; memory used and Trash size recalculated from the sample and from disk.
- `qa_monitor::network_speed_after_two_readings`, `unreadable_tools_show_unknown_not_zero` (at least 6 "Unknown", no "0%"), `trash_without_full_disk_access_says_so`, `desktop_mac_shows_no_battery`, `alerts_appear`, `free_up_space_opens_quick_sweep`, `free_up_ram_asks_then_runs_purge_and_logs`, `free_up_ram_dialog_cancel_runs_nothing`, `free_up_ram_password_cancelled`, `free_up_ram_failure_is_shown`, `no_readings_on_other_pages` (no tool runs unless the Live Monitor page is open).
- Readings run in a `Job` (thread in the app, inline in tests) every 2 s while the page is open; the page asks egui to repaint after the interval.
- `qa_sweep::tiles_show_each_module_plan`: each tile number equals the sum of that module's selected items recalculated from disk; header = sum of runnable included tiles; Trash tile shows the Trash total; count and failed tiles.
- `qa_sweep::run_lists_everything_moves_exactly_that_and_undo_per_module`: ONE dialog lists every selected file of every included module once, no unselected or Trash file; Move to Trash moves exactly those; "X freed" equals the recalculated sum; Undo on the Mail card puts back only Mail.
- `qa_sweep::include_toggle_skips_a_module_and_is_saved` (saved in `Settings::sweep_skip`, skipped module untouched), `review_opens_the_module_with_its_plan` (plan put into `app.plans` with `Phase::Results`, plan page shows "Review and Clean"), `cancel_moves_nothing`, `file_in_use_is_skipped_and_shown`, `registered_modules_only` (real registry: tiles = registered modules only).

MCP (`crates/mcp/src/monitor.rs`, `cargo test -p cleancraft-mcp --lib`): `system_status` (reading, alerts, summary; speed from the second call) and `sweep_preview` (scans, saves each plan, returns plan_id + digest for `execute_plan`). Tests: `monitor::tests::status_reports_reading_and_alerts`, `sweep_preview_saves_plans_with_digests`.

CLI (`apps/cleancraft/src/cli_monitor.rs`): `cleancraft status [--json]` (two readings one second apart) and `cleancraft sweep` (prints tiles, saves plans for `cleancraft clean <plan> --yes`). Tests: `cli_monitor::tests::*`.

## PARTIAL and why

- Q1: only System Junk is registered on this branch. Mail, Trash and Threat Check tiles appear by themselves once their owners register them (`features::get`); tested meanwhile with stand-ins in `core/src/testkit_monitor.rs`. Tune-up suggestions and app updates need one line each in `sweep::count_sources()` from perf and apps, for example `CountSource { id: FeatureId::Maintenance, noun: "suggestions", count: crate::tasks::suggestion_count }`. Duplicate downloads are not included (no Downloads-only duplicates scan exists).
- Trash Bins tile: shows the Trash size and Review opens the Trash page with the plan loaded; it is never emptied by Run (as assigned).
- R6: no status item. `tray-icon` is not in the dependency cache and cannot be compiled for macOS here, and TEAM.md asks for zero macOS-only code in our files, so `apps/cleancraft/src/menubar.rs` was not written. The content is ready and tested: `monitor::menu_bar_title` ("CPU 17% RAM 49%") and `monitor::menu_bar_entries` (memory, free space, Trash, battery, network, alerts, "Open CleanCraft", "Quit CleanCraft"). Steps on a Mac: add `tray-icon = "0.21"` as a macOS-only dependency of `apps/cleancraft`, build a `TrayIconBuilder` with `with_title(menu_bar_title(..))` and a `Menu` of `MenuItem`s from `menu_bar_entries` after eframe's event loop starts (in the creation closure), read clicks from `MenuEvent::receiver()` in `logic`, refresh the title from `app.monitor.last` every 2 s, and honour `Settings::menu_bar`.
- R7: alerts are shown only on the Live Monitor page while it is open; no macOS notifications, no Settings thresholds.
- Quick Sweep scans skipped modules too (fast and lets the toggle work instantly); skipped tiles are dimmed and left out of Run and the total.

## Hand checks on a real Mac

Run each and compare with the SAMPLE in `crates/core/src/testkit_monitor.rs`:

1. `sysctl hw.memsize hw.ncpu kern.boottime vm.loadavg vm.swapusage kern.memorystatus_vm_pressure_level` : "name: value" lines; boottime `{ sec = N, usec = N } ...`; swapusage `total = 2048.00M  used = ...M  free = ...M`; pressure level 1, 2 or 4. If `kern.memorystatus_vm_pressure_level` is unknown, Pressure shows Unknown and the rest still works.
2. `top -l 2 -n 0 -s 1 | grep "CPU usage"` : two lines `CPU usage: 7.69% user, 10.25% sys, 82.6% idle`; the Live Monitor CPU figure should match the second within a few percent of Activity Monitor.
3. `ps -axco pid=,%cpu=,rss=,comm=` : pid, cpu, rss (KiB), name (may contain spaces).
4. `vm_stat` : header `(page size of 16384 bytes)` on Apple silicon (4096 on Intel); keys "Anonymous pages", "Pages purgeable", "Pages wired down", "Pages occupied by compressor", "File-backed pages". Memory Used should be close to Activity Monitor > Memory > "Memory Used".
5. `df -Pk ~` : the Available column; Finder's free space is larger because it adds purgeable space. Decide if that difference is acceptable.
6. Live Monitor Storage card without Full Disk Access: Trash shows Unknown with "The Trash size needs Full Disk Access"; with it, the size matches `du -sk ~/.Trash` times 1024 (within block rounding).
7. `pmset -g batt` on a MacBook (on battery, charging, charged) and a desktop Mac; `ioreg -rn AppleSmartBattery | grep -E '"(CycleCount|MaxCapacity|AppleRawMaxCapacity|DesignCapacity)" ='` : top-level lines. Health should match System Settings > Battery > Battery Health > Maximum Capacity (within 1-2 points; Apple rounds differently), cycles must match System Information > Power.
8. `netstat -ib` : header contains Ibytes and Obytes; `<Link#n>` rows; during a download the Download figure should match Activity Monitor > Network "Data received/sec".
9. Free Up RAM: click it, then "Free Up RAM Now": the macOS password dialog appears; Cancel shows "Cancelled. Nothing was run." and History has no entry; with the password, History shows a `/usr/sbin/purge` command entry.
10. `cleancraft status` and `cleancraft sweep` in Terminal; MCP `system_status` twice (second call has a network speed) and `sweep_preview`.

## Shared-file changes

- `crates/core/src/lib.rs`: `pub mod monitor;`, `pub mod sweep;`, `pub mod testkit_monitor;`.
- `crates/core/src/features/mod.rs`: `run_plan` now looks up the feature and calls the new `run_plan_with(&dyn Feature, ...)` (same digest check, allow-list, method override, prepare, executor; plus a check that the plan belongs to that feature).
- `crates/core/src/settings.rs`: new field `sweep_skip: Vec<FeatureId>` (default empty; old settings files still load because the struct is `#[serde(default)]`).
- `crates/core/src/policy.rs`: test lines 195 and 198 only, `&[caches.clone()]` -> `std::slice::from_ref(&caches)` (clippy `cloned_ref_to_slice_refs` failed on main with Rust 1.97). No logic change.
- `crates/mcp/src/lib.rs`: `pub mod monitor;` and `tool_router: Self::tool_router() + Self::monitor_router()`.
- `apps/cleancraft/src/cli.rs`: subcommands `Status { --json }` and `Sweep`, two match arms calling `cli_monitor`.
- `apps/cleancraft/src/main.rs`: `mod cli_monitor;`.
- No change to `app.rs` or `pages/mod.rs` (they already had `monitor` and `sweep` state and poll). Note for the coordinator: in the 1280x820 test window the sidebar row "Live Monitor" is at y 794-826, outside the window; `qa_monitor` scrolls to it first (`scroll_to_me`).
