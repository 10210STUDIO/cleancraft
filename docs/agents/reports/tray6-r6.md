# tray6, round 6: menu bar agent, login item, scheduled scans, notifications

Branch agent6/tray6. Rows with status and tests: docs/features-r6/tray6.md.

## What was built

- **`cleancraft agent`** (apps/cleancraft/src/agent.rs, tray.rs). A worker thread samples CPU, memory, disk and Trash every 5 s (core `monitor::Sampler`), runs the schedule and disk checks every 60 s (core `schedule::Agent::check`) and carries out menu choices. The main thread runs the status item. Title "CPU 12% RAM 61%", plus "Disk low" under the threshold. Menu: CPU, Memory, Free space and Trash lines; Review items (what a scan or watcher found, newest first, at most 3); Open CleanCraft; Quick Sweep...; Pause/Resume Notifications; Quit Menu Bar Agent. The agent refuses to start while the menu bar is off, and exits when the plist disappears (off means off). One agent at a time (agent.pid + `ps -p`).
- **Login item** (core login_agent.rs). The Settings switch "Show CleanCraft in the menu bar" (off by default) writes ~/Library/LaunchAgents/com.cristianizzo.cleancraft.agent.plist (plist crate: ProgramArguments [helper binary, "agent"], RunAtLoad true, KeepAlive false, LimitLoadToSessionType Aqua, log at ~/Library/Logs/CleanCraft/agent.log) and runs `launchctl bootstrap gui/<uid> <plist>`. Off: `launchctl bootout gui/<uid>/<label>`, then the plist moves to ~/.Trash (Finder-style free name). Both are journaled as `Toggled` with an undo command, and there is Undo in the notice and in History. A launchctl failure keeps the plist and says it starts at next login. An unwritable journal changes nothing.
- **Scheduled scans** (core schedule.rs). Off / Every day / Every week, hour, weekday. The schedule is armed when first seen or changed, so slots before the change do not count. Missed slots after sleep give one scan. The scan is `sweep::scan` + `plans::save` only. A "CleanCraft found X to clean" notification and a Review item open Quick Sweep. Nothing is shown when 0 bytes are found.
- **Disk almost full**. Threshold setting (default 10% free, 0 off), at most one notification per local day (the day is saved, so this survives restarts).
- **Notifications** (core notify.rs). `osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1 of argv)' -e 'end run' -- <title> <body>`. The texts are arguments, never script (safety6 R2/R7), with control characters turned into spaces and a 200-character cap. **Why osascript:** UNUserNotificationCenter needs a signed bundle with a notification entitlement prompt, a delegate class for clicks and objc2 calls that are `unsafe` (the workspace forbids unsafe code). The ad hoc signature from install-app.sh is also known to make authorization fail on some systems. osascript needs nothing, but macOS attributes the notification to Script Editor, and a click opens Script Editor (or nothing) and never CleanCraft. So every notification that leads somewhere also leaves a Review item in the menu, and Settings says this in one line.
- **Opening the app on a page**. `cleancraft open <route>` / `--page <route>` / `cleancraft open 'cleancraft://open/<route>'` writes data_dir/open-request.json {route, time, seq} and runs `open -b com.cristianizzo.cleancraft`. Launch Services keeps a single instance: it focuses a running app or starts it. The app reads a fresh request (under 2 minutes) once, switches page and sends ViewportCommand::Focus (`pages::settings::follow_open_requests`, called from `CleanApp::logic`). The agent runs as a separate helper app (own bundle id, LSUIElement), so `open -b` of the main id never lands on the agent.
- **install-app.sh**. CFBundleURLTypes for `cleancraft`; helper Contents/Library/LoginItems/CleanCraftAgent.app (bundle id com.cristianizzo.cleancraft.agent, LSUIElement, a copy of the binary); ad hoc signing of the helper, then the app; lsregister. The script never starts the agent.
- **Settings page**: new card "Menu bar and notifications" with the menu bar switch, Scan automatically (radios), At (hour slider), On (weekday radios, weekly only), next-scan line, disk alert switch and threshold slider, Pause notifications, and the note about notification clicks. These controls are disabled while the menu bar is off.

## Safety

The agent calls only `sweep::scan`, `plans::save`, osascript and `open -b`. It never calls the executor, `run_plan`, sweep Run or admin commands (tray_schedule::a_scheduled_scan_only_looks_then_notifies checks that the journal is empty, the files are in place and no admin call was made). Routes only navigate. No `remove_file` / `remove_dir`: the plist goes to the Trash.

## Unverified until built on the Mac (lead)

- **apps/cleancraft/src/tray.rs, `mac` module, has never been compiled.** It is written against tray-icon 0.26.1 (`TrayIconBuilder::with_title/with_tooltip/build`, `set_title`, `set_menu`), muda 0.21.2 via `tray_icon::menu` (`Menu::new/append`, `MenuItem::with_id/set_text/set_enabled`, `PredefinedMenuItem::separator`, `MenuEvent::receiver`) and winit 0.30.13 (`EventLoop::builder().with_activation_policy(Accessory).with_default_menu(false)`, `ApplicationHandler::new_events` with `StartCause::Init`, `ControlFlow::WaitUntil`), as read from their sources in ~/.cargo/registry. It is a macOS-only dependency in apps/cleancraft/Cargo.toml (`tray-icon` default-features off, `winit` default-features off). Build it with `cargo build --release -p cleancraft` and run `bash scripts/install-app.sh`, then turn the switch on in Settings.
- To check by hand: the status item appears with no Dock icon; the title updates every 5 s; menu items act; Quit ends it; turning the switch off removes it; `launchctl print gui/$(id -u)/com.cristianizzo.cleancraft.agent` while on; a notification appears (as Script Editor) for `Scan automatically` at the next hour; `cleancraft open history` focuses the running app on History, and starts it when closed.
- The `cleancraft://` link from a browser only opens/focuses the app. winit delivers no open-URL Apple Event, so the page in the link is not applied (PARTIAL). `cleancraft open 'cleancraft://...'` routes it.

## Integrated after merging main (fix6, watch6, health6 core)

- watch6: the agent starts `watch::Watcher` from the two new Settings switches (`watch_trash_offer`, `watch_new_items`, off by default), restarts it when they change and drops it when both are off. Events become notifications plus Review items with `ev.open.to_arg()`.
- health6: `space_history::record` runs on every agent check (it limits itself to 1 an hour), and `health::note_plan` runs for each plan of a scheduled scan. The `cleancraft mcp-setup ...` subcommand passes its arguments to `mcp_setup::cli`, so the journal's Undo command works.

## Waiting on others

- health6's UI `CleanApp::open_route` (439520a) is not on main yet. `pages::settings::open_route` strips "scan:" and calls `navigate`, so a Review item lands on the page without starting its scan, and `uninstall:` routes do nothing yet. When it lands, replace the body of `pages::settings::open_route` with `if let Some(r) = AppRoute::parse(route) { app.open_route(&r) }`. The Settings links "How to connect an AI agent" / "Connect AI Apps..." also wait for that (no link is shown without a page behind it).
- dev6: the Developer Clutter settings group, once `dev_roots` / `dev_inactive_days` are on main.

## Shared-file changes

- crates/core/src/lib.rs: `pub mod login_agent; pub mod notify; pub mod schedule;`.
- crates/core/src/settings.rs: fields `scan_every`, `scan_hour`, `scan_weekday`, `disk_low_percent`, `notifications_paused`, `watch_trash_offer`, `watch_new_items` (serde default, old files load) and consts `DEFAULT_SCAN_HOUR`, `DEFAULT_DISK_LOW_PERCENT`.
- crates/ui-egui/src/app.rs: one line in `CleanApp::logic`, `pages::settings::follow_open_requests(self, ctx);` (announced to health6 on the board).
- apps/cleancraft/Cargo.toml: chrono; macOS-only tray-icon and winit.
- New files: core notify.rs, schedule.rs, login_agent.rs; apps agent.rs, tray.rs; tests core tray_schedule.rs, tray_login.rs, ui tray6_settings.rs.
