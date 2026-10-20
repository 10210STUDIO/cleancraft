# watch6 round 6 report: watchers (drag-to-Trash offer, new app / startup item watch)

Branch agent6/watch6. Rows: docs/features-r6/watch6.md (W1-W6).

## What was built

`crates/core/src/watch.rs` (new):
- `Watcher::start(env, WatchSettings, sink: impl Fn(WatchEvent)) -> io::Result<WatchHandle>`: one thread, `notify::recommended_watcher` (FSEvents on macOS, inotify here). Folders are watched and their contents remembered before `start` returns.
- `WatchHandle`: `set_paused`, `is_paused`, `status() -> WatchStatus { trash, threats: Access (Watching | NoFullDiskAccess | Off | Failed), paused, notes }`, `stop()` (also on drop).
- `WatchSettings { trash_offer, threat_watch, quiet_hours: Option<(u8,u8)>, debounce (2 s), coalesce (5 s), tick, clock }`.
- `WatchEvent { id, kind: WatchKind, title, body, open: route, time, paths }`.
- `trashed_app_plan(env, settings, trashed) -> Plan`: App Remover plan of a trashed app's leftovers (Uninstaller feature, its allow-list, its usual preselection), refused unless the path is a real `.app` folder directly in `~/.Trash`.
- `preselect_threat(&mut plan, path)`: ticks the Threat Check item at that path only when the scan listed it.

Engine rules: Trash non-recursive, new `.app` names only; apps folders recursive (an app one folder down counts); launchd folders non-recursive, `.plist` only. An item is examined when its size, file count and newest change stayed the same for `debounce` (a Trash bundle without an Info.plist waits up to 60 s). Ignored: items CleanCraft moved (journal `Moved.stored`) or put back by Undo, links, apps of macOS, untrusted bundle ids (empty, no dot, `com.apple.`), ids still installed elsewhere, apps that left nothing. Seen-list `watch-seen.json` in the data folder: Trash offers once per bundle id per 24 h, findings once per (item, reasons). Findings within `coalesce` are merged per kind. Quiet hours hold events and send them when they end. Pause drops events (no replay). Missing folders and missing Full Disk Access are retried every 2 s.

## Shared-file changes
- `Cargo.toml` (workspace): `notify = "8"`. `crates/core/Cargo.toml`: `notify = { workspace = true }`. `crates/core/src/lib.rs`: `pub mod watch;`.
- Behaviour-neutral extractions so the watcher reuses the existing rules instead of copying them (existing tests unchanged):
  - `features/uninstaller.rs` (apps6 owns it now): `add_leftover_categories`, `push_leftovers` (the loop formerly inside `uninstall_plan`).
  - `startup.rs`: `launch_item_at(env, domain, plist, overrides)` (the body of `list_launch_items`' loop).
  - `features/malware.rs`: `launch_item_findings(&LaunchItem)`, `app_findings(env, app)` (from `Finder::launch_item` and `Finder::apps`).

## Tests
crates/core/tests/watch_events.rs (real notify events on FakeMac), plus unit tests in watch.rs (quiet hours, route round trip, name lists). See the rows file for the mapping.

## Hand checks on a real Mac
1. FSEvents delivers a drag to `~/.Trash` (Finder's move) as an event on the new bundle or on the Trash folder (both handled).
2. Without Full Disk Access `read_dir(~/.Trash)` fails with EPERM (PermissionDenied); after granting, the watch starts within 2 s without restarting the agent.
3. Debounce on a large app copied from a DMG (Xcode-sized): no event before the copy ends; CPU while signatures are recomputed every second.
4. `/Library/LaunchDaemons` watch works for a non-admin user (read-only is enough).
5. codesign and spctl sample outputs are the same as Threat Check's (testkit_perf), unverified there too.

## Not done here
- Showing notifications and hosting the watcher in the agent: tray6.
- Opening the routes in the app (App Remover on `trashed_app_plan`, Threat Check with `preselect_threat`, Full Disk Access): health6 / apps6.
