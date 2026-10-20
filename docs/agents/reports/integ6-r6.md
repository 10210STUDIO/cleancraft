# integ6, round 6: integration of the round 6 branches

Branch agent6/integ6, based on main d345c11 (all of round 6 merged).

## What was joined

### 1. One router for every route
- `pages::settings::open_route` (the agent's open requests, `cleancraft open`) now calls health6's `CleanApp::open_route` through `AppRoute::parse`, so every way in (sidebar, Mac Health cards, guide, menu bar Review items, watcher notifications, CLI) goes through one router. A route only opens and looks.
- `scan:quick_sweep` now starts Quick Sweep's scan when the page shows nothing (`sweep::start_scan_if_idle`); results on screen stay. Before, Quick Sweep was skipped as an "own page".
- `uninstall:<app in ~/.Trash>` opens App Remover on `watch::trashed_app_plan` (apps6's `open_app` already chose that plan for an app in the Trash). Paths from the agent are used as found when they exist (the fake Mac's root is not added twice), typed macOS paths go through `env.sys_path`.
- New `AppRoute` variants asked by watch6: `ThreatReview(path)` (`threat:<path>`) and `FullDiskAccess` (`full_disk_access`, also `fda`). Round trip and refusals (relative path, `..`) in route.rs unit tests.
  - `ThreatReview`: Threat Check opens, its scan starts when nothing is shown, and once the plan is there `watch::preselect_threat` ticks that finding only (`CleanApp::pending_threat`, applied in `logic`). The notice says it is selected for review, or that the scan did not list it this time (gone or fixed), or that it is listed but locked with the reason.
  - `FullDiskAccess`: Settings, new "Full Disk Access" group scrolled into view (status in plain words, Open Privacy Settings, Check Again; access read again on arrival).
- watch6's events switched: a single startup item or app finding opens `ThreatReview(<path>)` (several at once still `Scan(Malware)`); the Trash watch's missing Full Disk Access event opens `FullDiskAccess`. watch_events assertions updated.

### 2. Settings
- "Code folders for Developer Clutter" group (not named "Developer Clutter": ux_render treats a label equal to a module title as its sidebar row): the usual folders named, own code folders listed with Remove (announced "Remove code folder <path>"), a path field with Add Code Folder, Choose Code Folder... (macOS folder dialog through osascript), refusals with the reason (empty, missing, a file or a link, the home folder or a folder holding it, already listed), inactivity days slider (7 to 365). Saved to `dev_roots` / `dev_inactive_days`.
- AI assistants group: "Connect AI Apps..." (the welcome step, `AppRoute::ConnectAi`) and "How to connect an AI agent" (`AppRoute::AgentGuide`).
- Full Disk Access group (see above). Subtitle names the new groups.

### 3. cleancraft:// links
Checked winit 0.30.13 and eframe 0.36.2 sources: no open-URL event; winit's docs show adding `application:openURLs:` to the app delegate with objc2 class declaration, which needs unsafe code (forbidden in the workspace). Documented in FEATURES.md M4; `cleancraft open 'cleancraft://...'` routes links (tray6, tested). No extra work.

### 4. docs/FEATURES.md
Rows of docs/features-r6/*.md merged: Q4 to Q6 (Duplicate Downloads tile, Developer Clutter tile, scheduled scan route) and Q1 updated; T1 updated, T3 Erase in Mail; new 2.4 Developer Clutter (DV1 to DV13); A12 to A15, A1 note; C1 updated, C2b APFS clones; D1, D3 updated, D5 to D8; R6, R7 replaced; new sections 10 Menu bar agent and schedules (tray6's T1 to T8 renamed M1 to M8, since T is Trash Bins), 11 Watchers (W1 to W6), 12 Mac Health, 13 AI apps setup; U3, U4 updated, U8 one router. Statuses moved only where this round's integration tests back it (DV1, H11 to WORKS; W1, W4, H10 to MAC-CHECK). Totals line added: 152 rows, 64 WORKS, 75 MAC-CHECK, 11 PARTIAL, 2 EXCLUDED. Hand-check list items 11 to 18 added (menu bar glue never compiled for macOS, Script Editor notifications, FSEvents on ~/.Trash and /Library, Erase in Mail via Accessibility, diskutil external and firmlinks, simctl JSON, docker and brew output, claude mcp, CPU, batch uninstall dialogs).

### 5. Renders
`crates/ui-egui/tests/integ6_render.rs` (CLEANCRAFT_SNAPSHOTS=1) writes docs/screens/v2/r6/ at 1280x820 and 1600x1000: mac-health, developer-clutter, settings-developer-clutter, settings-ai-agents, settings-full-disk-access, trash-bins, app-remover-multi-select, disk-map-location-picker, large-old-groups, welcome-connect-ai. Looked at each: no clipping or overlap found beyond the expected (the location picker menu covers the storage bar while open; App Remover's bar hint is cut with an ellipsis at 1280).

## Tests (new)
- ui `integ6_routes.rs` (7): the_scheduled_scan_route_opens_quick_sweep_and_starts_its_scan, the_trash_offer_route_opens_app_remover_on_what_the_app_left, the_threat_route_selects_the_finding_and_says_when_it_is_gone, the_full_disk_access_route_lands_on_its_group_with_the_way_forward, developer_clutter_code_folders_are_added_checked_and_removed, inactivity_days_slider_is_saved, ai_agents_group_links_to_the_guide_and_the_connect_step. Every route test checks the journal is empty and no admin call ran.
- core route.rs unit tests extended; watch_events updated for the new routes.

## Gate (focused, the lead runs the full suite)
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all`: clean.
- core watch_events + lib: green. cleancraft (app crate): green.
- ui tray6_settings, health6_ui, qa_every_control, integ6_routes, ux_render, qa_shell, review_parity, review_reliability_ui, review_safety6_ui, qa_apps_batch, qa_cleanup_trash, ux4_flows, qa_dev_clutter, qa_files_r6: green.
- One test changed for the layout: qa_shell::settings_are_saved_and_change_what_happens scrolls to the Large file slider before clicking it (the new groups moved it above the view).

## Still open
- SAF6-8 (apps6): `Uninstaller::validate` accepts items of another app in an edited plan (not in integ6's brief).
- Q5 / DV13: the Developer Clutter tile in Quick Sweep has no click-driven test.
- H12: `cleancraft mcp-setup` dispatch in cli.rs is not driven by a test.
- M4: links clicked in other apps do not change the page (needs an Apple Event handler, unsafe code).
- Everything in the hand-check list, above all the menu bar glue (`tray.rs`), which has never been compiled for macOS.
