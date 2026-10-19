# Parity lead report

Branch `agent/parity`. Built and tested in a private target dir (`/home/claude/wt/parity/target`) because the shared target mixes worktrees. Everything below was run on the branch after merging main at fix1 (`d5ebc01`): `cargo test --workspace` passed (326 passed, 0 failed, 2 ignored: PAR-3 and a timing probe since removed), `cargo clippy --workspace --all-targets -- -D warnings` was clean, and `cargo fmt --all` was applied.

## 1. Click-every-control test (S12)

File: `crates/ui-egui/tests/qa_every_control.rs`.

How it works:
- Each sidebar page gets a fake Mac with data for it, built from the builders' helpers: `junk_mac`, `mail_mac`, `trash_mac`, `privacy_mac` (testkit_cleanup), `startup_mac` (testkit_perf, used for Threat Check and Startup Items), `sweep_mac` with the monitor answers (testkit_monitor), app bundles, leftovers, installers, appcasts and Spotlight answers (testkit_apps), generated pictures (testkit_files), a journal with a real cleanup, a command and a toggled startup item for History, and a saved ignore list for Settings. FakeSys answers `open`, `sh`, `tccutil` and Finder's empty trash, so hand-offs work.
- The test explores each page breadth first. It starts at the page's first screen, clicks every control, and adds any state with a new set of controls to the queue. That covers results, popups, confirm dialogs, done pages, categories, tabs, App Remover groups and app detail, Disk Map folders, and so on.
- In every state it uses each control that is new in that state (text boxes get typed into, everything else gets clicked) on a fresh copy of the state. The copy is the page's fake Mac (files, times, FakeSys answers and running apps), rebuilt and replayed from the start.
- It fails when:
  - (a) a control lies outside the window even after scrolling to it;
  - (b) nothing happened: `action_count()` did not grow and the accessibility tree (labels, values, check states) did not change;
  - (c) the app panics;
  - (d) any text on screen contains "not available", "coming soon", "not implemented", "TODO" or an em dash, or uses the whole word "nor".
- When a dialog is open, only the controls in the top dialog count. Controls behind it cannot be clicked, as on macOS.
- Sidebar rows, the Full Disk Access banner and the notice's Dismiss are tested once, in `shell_controls_respond`.
- A control is first clicked where it is drawn. If that does nothing, the test tries again after scrolling it into view, because a row can be drawn but clipped by its list.
- Run with `--nocapture` to print the full table, one row per control: state path, control, result.

Results on the branch (1280 x 820 window):

| Page | States | Controls listed | Clicked | Failed |
|---|---|---|---|---|
| Quick Sweep | 27 | 378 | 91 | 0 |
| System Junk | 40 | 2400 | 237 | 0 |
| Mail Downloads | 10 | 162 | 43 | 0 |
| Trash Bins | 8 | 110 | 21 | 0 |
| Threat Check | 12 | 202 | 43 | 0 |
| Privacy Traces | 23 | 711 | 114 | 0 |
| Tune-up Tasks | 3 | 25 | 12 | 0 |
| Startup Items | 4 | 34 | 34 | 0 |
| App Remover | 31 | 349 | 78 | 0 |
| App Updates | 2 | 8 | 8 | 0 |
| Large & Old Files | 12 | 148 | 38 | 0 |
| Duplicates | 10 | 114 | 38 | 0 |
| Similar Images | 9 | 87 | 29 | 0 |
| Disk Map | 40 | 444 | 162 | 0 |
| Live Monitor | 14 | 81 | 41 | 0 |
| History | 1 | 9 | 9 | 0 |
| Settings | 3 | 40 | 13 | 0 |
| Total | 249 | 5302 | 1011 | 0 |

At the smallest window the app allows (1060 x 680, `with_min_inner_size` in main.rs), with at most 6 states per page (`every_page_at_the_smallest_window`):

| Page | States | Clicked | Failed |
|---|---|---|---|
| Quick Sweep | 6 | 59 | 0 |
| System Junk | 6 | 78 | 0 |
| Mail Downloads | 6 | 32 | 0 |
| Trash Bins | 6 | 17 | 0 |
| Threat Check | 6 | 36 | 0 |
| Privacy Traces | 6 | 52 | 0 |
| Tune-up Tasks | 3 | 12 | 0 |
| Startup Items | 4 | 34 | 0 |
| App Remover | 6 | 18 | 0 |
| App Updates | 2 | 8 | 0 |
| Large & Old Files | 6 | 30 | 0 |
| Duplicates | 6 | 30 | 0 |
| Similar Images | 6 | 25 | 0 |
| Disk Map | 6 | 49 | 0 |
| Live Monitor | 6 | 24 | 0 |
| History | 1 | 9 | 0 |
| Settings | 3 | 13 | 0 |

Also in this file:
- `shell_controls_respond`: all 17 sidebar rows, the banner's two buttons and Dismiss.
- `every_page_text_follows_the_rules`: no banned text on any page's first screen.
- The focused tests for the fixes below.

Runtime: the last run took 282 s of wall time with the machine shared by other agents (load average about 10). An earlier measured run used about 100 s of user CPU and 60 s of system CPU, so roughly 1.5 min of wall time on an idle 2-core machine. Most of the time goes to System Junk (40 states, 237 clicks, 126 s).

Limits (why S12 stays PARTIAL):
- At most 40 states per page, and 6 at the smallest window. System Junk and Disk Map reach that cap.
- Long lists only draw the rows in view, so rows further down are clicked only when a category or folder brings them into view.
- A control that a state inherits from the state before it (same role, label and position among equal labels) is clicked once, in the first state that shows it.
- PAR-3 is still open.

## 2. Defects

Fixed, each with a focused test:

| Id | Where | What was wrong | Fix | Test |
|---|---|---|---|---|
| PAR-1 | `pages/plan_page.rs` results toolbar | Privacy Traces adds Wi-Fi Settings and Reset Permissions to the toolbar. The toolbar then grew past the window's right edge and pushed Select None and every row's Ignore button out of reach (x up to 1332 in a 1280 window). | The toolbar wraps (`horizontal_wrapped`), and the list height comes from the space left under it | `qa_every_control::results_toolbar_and_rows_fit_the_window`, `qa_every_control::privacy_traces` |
| PAR-2 | `pages/plan_page.rs`, `pages/apps.rs` sort menus | The combo boxes had no accessible name, so VoiceOver read nothing | Named "Sort", value unchanged ("Sort by size") | `qa_every_control::sort_menus_have_a_name` |
| PAR-4 | `pages/disk_map.rs` folder names | egui reports a visible link as a plain label (text selection overrides the role). The folder links were not announced as links, and the walk could not see them. | `widget_info` sets the Link role again after `ui.link` | `qa_every_control::disk_map_folder_names_are_links_that_open_the_folder`, `qa_every_control::disk_map` |
| PAR-5 | `pages/settings.rs` ignore list | The buttons read "Remove 1", "Remove 2", which does not say what they remove | Visible text "Remove", accessible name "Remove <path>" | `qa_shell::settings_are_saved_and_change_what_happens` |

Open, routed to the coordinator (an ignored failing test, plus a board line):

| Id | Defect | Test |
|---|---|---|
| PAR-3 | Settings shows "Show the live monitor in the menu bar", but nothing reads `Settings::menu_bar` and no status item exists (R6 is PARTIAL). The checkbox saves a value that changes nothing, which is a dead control under rule 6. It needs a decision: build the status item, or hide the option until there is one. | `review_parity::menu_bar_option_only_when_there_is_a_menu_bar_item` (ignored) |

## 3. Inventory audit (docs/FEATURES.md)

Every test name in the Test column was checked against `cargo test --workspace -- --list`, with `{a,b}` and `*` expanded and `::name` continuing the previous file. Every referenced test exists. K8's reference sits inside a parenthesis that the checker splits, and was checked by hand. All of them passed in the full run.

New tests for rows that had none:
- `crates/ui-egui/tests/qa_shell.rs` covers U2, U3 and U4:
  - History: every moved item is shown with its size recalculated from the stored copy and where it went. Undo of one row brings back exactly that item. When something took an item's place, Undo says so and keeps the stored copy.
  - Settings: method, ignore list add and remove, agents, menu bar and threshold are saved to disk. The quarantine method and the ignore list change the next cleanup and scan. A settings file that cannot be written shows a warning.
  - Full Disk Access banner: opens the pane URL, and Check Again hides the banner once access is on.
- `crates/ui-egui/tests/qa_parity_flows.rs` covers Q2 on the real modules: Quick Sweep with the registered System Junk, Trash Bins, Mail Downloads and Threat Check. Tile numbers are checked against the disk, Run lists and moves exactly the selected files, and Undo puts Mail Downloads back. The builders' Run test used stand-ins.
- `crates/core/tests/parity_safety.rs` covers S2 and S5: an item on another disk goes to that disk's `.Trashes/<uid>` and comes back, and iCloud Drive and `/usr/bin`, `/usr/lib`, `/usr/libexec` are refused even inside an allow-list, by both policy and the executor.

Status corrections and fills:
- S1 to S12, previously PLANNED, now have tests and statuses:
  - WORKS: S1, S3, S4, S5, S8, S9, S10, S11
  - PARTIAL: S2 (the fallback from the Trash to the quarantine is untested: tests run as root, and faults apply only to the moved item) and S12 (limits above)
  - MAC-CHECK: S6 (real `lsof` and `ps`) and S7 (real detection and pane URL)
- U1 to U7, previously PLANNED:
  - WORKS: U1, U2
  - PARTIAL: U3 (PAR-3, plus REL-8 noted), U5 (`scan`, `clean`, `undo`, `history`, `features`, `access` have no test), U6 (`list_features`, `scan`, `get_plan`, `undo`, `history`, `full_disk_access` tools have no test; stdio is not driven) and U7 (the install script cannot run here)
  - MAC-CHECK: U4
- Q1: the merge kept main's newer text (fix1 added the tune-up and updates tiles) and adds the real-module test name.
- Q2: WORKS is now backed by a test on the real modules. The note says which error paths still go through stand-ins.
- D2: WORKS changed to MAC-CHECK, because the bar's numbers come from parsing `df -Pk`, and the macOS columns are a sample.
- S11: WORKS with a note on REL-8 (reliability's open defect: a damaged settings file is read as defaults, so the ignore list is lost).
- The WORKS rows I read (J1 to J5, J8, J9, J12 to J19, J21, M1, T1, A6, Q3, C2, C3, D3) do check files on disk and the numbers against a recalculation in their UI tests. They are left as they are.

## 4. Hand checks on a real Mac

- S7, U4: start CleanCraft without Full Disk Access. The banner shows. Open Privacy Settings should open System Settings > Privacy & Security > Full Disk Access (`x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles`). Turn access on and click Check Again: the banner disappears and "Full Disk Access is on." appears.
- S6: `lsof -F n` on a busy Mac. Check the time it takes and that a file open in Preview is skipped with "open in a running app".
- S2: put a folder in the Trash's place without write rights (for example `chmod 500 ~/.Trash`), then clean. Items should go to `~/Library/Application Support/CleanCraft/Quarantine`.
- U7: `bash scripts/install-app.sh`, then open `/Applications/CleanCraft.app`.
- Smallest window: resize to 1060 x 680 and click through System Junk results. Select None and the row buttons stay visible.

## 5. Shared-file changes

- `crates/ui-egui/src/pages/plan_page.rs`: the results toolbar wraps, the list height follows from it, and the sort combo has a name (announced on the board at 22:02).
- `crates/ui-egui/src/pages/apps.rs`: the sort combo has a name.
- `crates/ui-egui/src/pages/settings.rs`: the ignore-list Remove buttons are named after their path.
- `crates/ui-egui/src/pages/disk_map.rs`: the folder link keeps its Link role.
- `docs/FEATURES.md`: S and U rows, plus the Q1, Q2, D2 and S11 corrections above.
- New files:
  - `crates/ui-egui/tests/qa_every_control.rs`
  - `crates/ui-egui/tests/qa_shell.rs`
  - `crates/ui-egui/tests/qa_parity_flows.rs`
  - `crates/ui-egui/tests/review_parity.rs`
  - `crates/core/tests/parity_safety.rs`
