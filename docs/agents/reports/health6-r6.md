# health6, round 6 report

Branch agent6/health6. Rows with status and tests: docs/features-r6/health6.md (H1 to H16).

## What was built

1. **Mac Health** (pages/health.rs, core health.rs). A second tool of the Live Monitor section (switcher "Live Monitor | Mac Health"), so the sidebar keeps its 7 entries and Cmd+1..7. Facts: free space and 7-day change, Trash, last junk scan and when, last Threat Check and age, updates, apps not opened for 180 days, startup items turned on and the unsigned ones, battery wear and cycles, memory pressure, macOS version; "Unknown" when unread. No score: the state is Good or Needs attention by the 13 rules in `health::RULES` (shown under "How this is decided"); only Attention and Act soon cards change it, and the summary line names them. Each card has one button that opens a page (`AppRoute`); a "Scan ..." card starts a scan that only looks. Nothing runs from the page (test checks the journal stays empty).
2. **Free space history** (core space_history.rs): one sample per launch (`CleanApp::launch_checks`, background thread) and per agent tick (tray6 calls `space_history::record`), at most one an hour, 90 days, capped at 2160 samples, numbers only, written tmp + rename, a damaged file kept as space-history.damaged.json. Chart (`kit::time_chart`) over 7 / 30 / 90 days from zero, a dashed "Getting low" line at the Attention threshold, hover value, "Collecting" when empty.
3. **What grew**: app.rs keeps a two-level summary of each finished Disk Map tree (`health::summarize` / `keep_map_summary`, two latest per folder, 6 folders), written when the person leaves Disk Map (or the app quits) so CleanCraft's own file never makes the map on screen one file out of date (ux4_flows recalculates the map from disk), Mac Health compares the two latest and names the inner folder when it carries 80% of its parent's growth. files6 needed no change.
4. **Scan notes**: app.rs `observe()` notes every plan shown (plan pages and Quick Sweep tiles; the note follows what is left after cleanups) and every update check, in scan-notes.json. `health::note_plan` is public for tray6's scheduled scans.
5. **Routes** (core route.rs): `AppRoute { Feature, Scan, Health, History, Settings, AgentGuide, ConnectAi, UnusedApps, UninstallApp(PathBuf) }` with a string form; `CleanApp::open_route`; `navigate(str)` takes the same strings. watch6 re-exports it.
6. **AI agents guide** (docs/guide-ai-agents.md) rendered in the app (pages/guide.rs, text built in); a test fails if an MCP tool is missing from its table.
7. **AI apps step** (core mcp_setup.rs, pages/welcome.rs), the lead's new request: first launch (real app only) opens it once; Not Now and Done are remembered (welcome.json; a damaged file never brings it back). Detects Claude Desktop and Claude Code; each row shows the exact file and entry, or the exact `claude mcp add` command, before Connect. Desktop: text insertion of one key, re-parse and compare (everything else equal), refusal for invalid JSON, a non-object, an existing "cleancraft" entry, a linked file; timestamped backup (mode 600) in mcp-backups/; tmp + rename keeping permissions; journal `Toggled` with Undo `'<bin>' mcp-setup remove '<file>' [--drop-empty]`, which removes only our key the same careful way (`mcp_setup::cli`, only for the two known files). Claude Code: `claude mcp add --scope user cleancraft -- <bin> serve` by argv from fixed install places, Undo `claude mcp remove --scope user cleancraft`; without the command the same JSON edit on ~/.claude.json. The "Let AI agents clean after a preview" switch is separate, off by default, and connecting never changes it. safety6's items a to f are built in (board 18:08).
8. **Live Monitor CPU** (pages/monitor.rs, core monitor.rs, motion.rs, app.rs). Causes found: (a) while a reading ran (top -l 2 -s 1 takes over a second of every 2), `poll` reported busy and the shell repainted every 100 ms, about 11 frames per reading; (b) every reading walked the whole Trash and ran ioreg; (c) spinners requested a frame every pass (60 fps); (d) the column-levelling loop could chase a pixel forever; (e) ambient loops ignored minimised and occluded windows. Fixes: the reading's thread wakes the window when done (no polling frames), the next reading is one `request_repaint_after(2 s)`, readings use egui's clock and stop while the window is minimised or covered or another page shows, Trash size and battery wear are read every 30 s, spinners at 30 fps, levelling stops after 3 tries, `motion::window_hidden` stops ambient loops.
   Measured (health6_ui::idle_pages_stay_within_the_repaint_budget, 60 Hz event loop simulated, 20 s): History, Settings, Mac Health, guide 0.1 frames/s (no frame unless something changes); Live Monitor 1.4 frames/s (budget 1.6: the 2 s tick plus the wake-up, egui draws twice for it); System Junk start screen 30.4 frames/s (ambient hero, budget 31); unfocused start screen: no further frame. Before, by the code: Live Monitor about 6 frames/s (11 frames during each 1.1 s reading every 2 s, plus 60 fps for the first readings' spinner), every reading 8 commands plus a full walk of the Trash. Sampler now: 10 readings and 70 commands in 20 s (7 per reading; ioreg and the Trash every 30 s), none off the page or minimised (sampler_runs_at_most_every_two_seconds_and_only_when_visible). The real CPU percentage must be read again on the Mac.
9. **Process names** (core proc_names.rs): plain words for WindowServer, kernel_task, Spotlight (mds, mds_stores, mdworker...), backupd, photoanalysisd, cloudd/bird, softwareupdated, coreaudiod, launchd and others; "<App> Helper (Renderer)" = "A tab or page in <App>". Top lists group helpers under their app ("Brave Browser (3 processes)") with "Show Each Process" / "Group by App"; the explanation shows as a second line when there is room and on hover; macOS processes never get a quit link, app rows show "Quit in Startup Items..." on hover (the existing confirmed quit).

## Tests

- core: tests/health6.rs (17), tests/mcp_setup6.rs (17), route.rs, space_history.rs, proc_names.rs unit tests.
- ui: tests/health6_ui.rs (21: every card opens its page on a FakeMac, rules shown, chart ranges, what grew, routes, guide, first launch, connect with Undo, Claude Code, invalid JSON, not installed, process names, quit links, sampler rate, repaint budget); pages::guide tests; ux_render gets 4 scenes (mac-health, mac-health-good, connect-ai-apps, agent-guide), so the pointer test covers the new controls and renders run at 1060, 1280 and 1600.
- Renders: docs/screens/v2/health/.

## Gate

`cargo test --workspace` (all 82 targets, the last 24 rerun with --no-fail-fast after two fixes: qa_monitor now expects grouped process names; the guide's list items wrap), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` green. target/ deleted afterwards.

## Shared-file changes

- core lib.rs: `pub mod` lines for health, mcp_setup, proc_names, route, space_history, testkit_health.
- core monitor.rs: `Snapshot::processes` (serde skip, not sent to agents), Sampler caches Trash size and ioreg for 30 s, `COMMANDS_PER_READING`, `SLOW_FACTS_SECS`.
- ui motion.rs: `window_hidden`, `spinner_repaint`; `ambient_repaint` also stops when minimised or occluded.
- ui pages/mod.rs: `pub mod guide, health, welcome`.
- ui tests/ux_render.rs: 4 scenes. ui tests/qa_monitor.rs: the top-list check follows the grouped names (round 6 behaviour).
- app.rs and kit.rs are mine this round (EXTRA_TOOLS, page_group, section_pages, page_look, open_route, observe, launch_checks, `now`, `window_hidden`, `egui_ctx`; kit::choice_pills, time_chart, nice_ceiling).

## Waiting on others

- tray6: cli.rs dispatch `"mcp-setup" => exit(mcp_setup::cli(&env, &args))` (History's Undo of a Desktop connection needs it); Settings links to `AppRoute::AgentGuide` and `AppRoute::ConnectAi`; `space_history::record` and `health::note_plan` on each agent tick / scheduled scan.
- dev6: the Clean group line for FeatureId::DevClutter and its feature_look / Section arms (approved on the board; the variant is on their branch).
- apps6: `pages::apps::open_app` should build the plan with `watch::trashed_app_plan` for an app already in the Trash (watch6's drag-to-Trash route).

## Check on a real Mac

- CPU of CleanCraft on Live Monitor (was 23%) and on each page; WindowServer's share.
- `claude mcp list` output format (CleanCraft looks for a line starting "cleancraft:"); `claude mcp add --scope user` writes ~/.claude.json; the guide's commands were written from Claude Code's documented syntax, the docs page could not be fetched here.
- Claude Desktop reading the edited claude_desktop_config.json after Quit and reopen.
- `/usr/bin/open -t <file>` opens the settings file in TextEdit.
- Time of `health::gather` on a full Mac (mdls per app, codesign per startup item, top for a second).
- egui's `occluded` and `minimized` viewport flags on macOS (winit) for the pauses.
