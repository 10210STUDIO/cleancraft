# design3, round 3 report

Branch `agent3/design3` (base ec752a6). Merged: `agent3/icons3` (cf3f46d), `agent3/sound3`
(11e489e), `agent3/motion3` (6d0e94a). Renders: `docs/screens/v2/*-{1060x680,1280x820,1600x1000}{,@2x}.png`.
New tests: `crates/ui-egui/tests/ux_design3.rs` (13 tests). `review_design_a.rs` and
`review_design_b.rs` run without `#[ignore]` now (all pass).

## Lead's request (sidebar, sections, page sizing, sounds)

| Part | Status | Evidence |
|---|---|---|
| Sidebar: 7 section entries + History and Settings, 44 pt rows, 15 pt labels, 28 pt 3D section icons (motion3), soft selected pill, no small-caps headers, 240 pt wide, traffic-light band and logo on top | Done | `ux_design3::sidebar_sections_open_their_last_tool` (44 pt rows, all inside 680 pt, a section reopens its last tool), `check_shell`, `qa_every_control::shell_controls_respond`; any render |
| Section switcher for Clean, Protect, Speed, Apps, Files; each tool keeps its own flow and controls | Done | `qa_every_control::shell_controls_respond` clicks every segment of every section; `ux_design3::section_switcher_stays_in_place_across_phases` |
| Tests navigating by old sidebar labels | Updated honestly: `click("Clean")` then `click("System Junk")` (59 places in 23 files); `qa_every_control::nav` goes section then segment, and segments are kept out of page exploration (tested in the shell test) | full test run |
| 34 pt start-screen titles, 15 pt body, two-column start screens from 900 pt content width (object left, title, explanation, what the scan covers, Scan right), one column at 1060 | Done | `system-junk-idle-1280x820.png`, `app-updates-idle-1600x1000.png`, `*-idle-1060x680.png`; `pages::covers` |
| 1600 x 1000 renders | Done | `ux_render::SIZES` |
| Sounds (sound3): cues and Settings group | Done for plan pages, Quick Sweep and warnings; Settings "Sounds" card (Play sounds, Volume, Preview sound, Click sounds on switches) | `sound_flow.rs` (ignores removed, passes), `ux_design3::settings_sounds_group_saves_and_previews`. Not wired: Tap on switches (kit::switch has no Env), cues for App Remover, Startup, Disk Map and Tune-up jobs |

## REVIEW-B items

| # | Status | Evidence |
|---|---|---|
| B1 Privacy toolbar overlap | Fixed (already by b704427: extras go to a second row) | `review_design_b::review_b1_results_toolbar_buttons_never_overlap` un-ignored, passes |
| B2 Memory figure elided | Fixed (b704427) | `review_b2_monitor_figures_are_not_elided` passes |
| 4 Danger pill contrast | Fixed: ink label, `theme::DANGER_A/B`, `danger_label()` >= 4.5:1 on both ends | `ux_design3::danger_pill_label_reads_on_its_whole_gradient` |
| 53 Empty sparkline | Fixed: readings span the box, quarter grid lines | `ux_design3::sparkline_spans_the_box_from_two_readings`, `live-monitor-*.png` |
| 66 Motion on pages | Fixed: totals count up (`kit::display_count`, `motion::count_to`), rows and tiles fade in staggered (`kit::appear`, `motion::appear`), dialogs fade and rise (`kit::modal` -> `Dialog`, `motion::tr::OVERLAY`); all instant with Reduce motion (Motion::progress) | `ux_design3::results_total_counts_up_to_the_exact_figure`, `dialog_close_button_and_esc_cancel`; the stagger has no test of its own |
| 27 Truncation | Partly: System Junk names and reasons fit at 1060 (reason runs under the size, drops a repeated name); Privacy reasons longer than about 55 characters still end in "..." at 1060 (full text in the tooltip and for VoiceOver). New at 1060 with 15 pt body: long category names in the left column are shortened ("Start...k Trash", "Goo...hrome") | `system-junk-results-1060x680.png`, `privacy-traces-results-1060x680.png` |
| 29 Locked rows | Fixed: padlock instead of the checkbox on plan pages and Disk Map; Disk Map reason shows whole or its first clause, never an open parenthesis | `disk-map-1060x680.png` |
| 34 Trash item "0" | Fixed: the cause was the UI naming rows after the folder (`.Trashes/<uid>`); rows are named after their Trash ("Trash of USB"), reason "1 item" | `ux_design3::trash_rows_are_named_after_their_disk`, core `cleanup_trash::every_trash_is_measured_and_locked` |
| 31 "Zero KB" | Fixed in core `fsutil::units`: "0 KB" | `review_design_b::review_a31_no_zero_kb_wording`, `fsutil` and `review_data_numbers` unit tests |
| 56 Formats | Fixed: "375 days", "3 days 4 h", "1 day 2 h" (`monitor::uptime_text`), "Discharging", "0 KB/s" | `monitor_parsers`, `qa_monitor`, `review_data_screens` |
| 43 Scan progress | Fixed: Quick Sweep scanning lists every module as done / scanning / waiting, the hero's progress follows | `ux_design3::quick_sweep_scan_lists_each_module_with_its_state`, `quick-sweep-scanning-*.png` |
| 18 Title jump | Partly: in sections with several tools the switcher is the fixed anchor from start screen to results (tested); Quick Sweep and Live Monitor have a large title on the start screen and the compact header afterwards, so their title still moves (the lead's 34 pt start-screen title) | `ux_design3::section_switcher_stays_in_place_across_phases` |
| 8 Select Copies | Fixed (pill since b704427); its menu items are egui menu buttons | `duplicates-results-*.png` |
| 50 Startup switch | Fixed: `kit::switch` (38 x 22), named after the action ("Turn Off Dropbox"), Remove with a Trash glyph | `ux_design3::startup_items_use_one_switch`, `qa_perf_startup` |
| B5 Empty state | Fixed: done hero, "Nothing to clean here.", one line, notes behind Details, no 0 total | `ux_design3::empty_result_keeps_notes_behind_details`, `system-junk-empty-*.png` |
| B6 Error banner | Fixed: plain sentence for permission errors plus "Open Privacy Settings"; other errors keep their words without the OS error number | `ux_design3::scan_errors_read_as_plain_sentences`, `system-junk-error-*.png` |
| B7 Sidebar header spacing | Fixed by the new sidebar (no group labels) | renders |
| B8 Trash tile sub-line | Fixed (wraps, b704427) | `quick-sweep-results-1060x680.png` |
| B9 macOS card | Fixed (b704427) | `app-updates-results-1060x680.png` |
| B10 "1 items" | Fixed | see 34 |

## S3 items

| # | Status | Evidence |
|---|---|---|
| 5 Radio ring | Fixed by colour (#8C96A5, 5.0:1 as a colour); not sampled from pixels | `settings-*.png` |
| 15 Icon weights | Fixed: sidebar now uses motion3's 3D icons; vector Quick Sweep star outlined, Tune-up dot lighter, Trash raised 0.8 unit | `icons.rs` |
| 17 Banding | Partly: 0 to 2 levels of white noise over the backdrop (`kit::dither`); not measured on a Retina panel | renders |
| 23 "Run" | Fixed: "Review and Clean" (tests that clicked "Run" on Quick Sweep updated) | `qa_sweep`, `ux_flows`, `qa_parity_flows` |
| 24 List fade | Fixed: `kit::scroll_fade` (lists in cards) and `scroll_fade_page` (lists on the backdrop) | `system-junk-results-*.png`, `live-monitor-1060x680.png` |
| 26 Done composition | Fixed: Quick Sweep Done is one centred block like module Done screens | `quick-sweep-done-*.png` |
| 37 Dialog close | Fixed: X (28 pt, "Close dialog") and Esc on every dialog | `ux_design3::dialog_close_button_and_esc_cancel` |
| 42 Disk Map idle | Fixed (two columns at 1280; one column at 1060 without crowding) | `disk-map-idle-*.png` |
| 48 App Remover last row | Partly: fade on the list; not checked on the real Mac | `app-remover-*.png` |
| 51 Startup paths | Fixed: middle truncation keeps the executable | `startup-items-*.png` |
| 55 Gauge size | Fixed: 128 pt arc, number inside | `live-monitor-*.png` |
| 59 Command wells | Partly: commands in Inter Medium under a "COMMAND" eyebrow; command output logs keep the monospace font | `tune-up-tasks-*.png` |

## Shared files and core

- core `fsutil.rs` ("0 KB"), `monitor.rs` (`uptime_text`), `features/trash.rs` ("1 item"), with their tests (`fsutil`, `monitor_parsers`, `review_data_numbers`, `cleanup_trash`).
- `icons.rs` (three glyph tweaks), `lib.rs` (merge of motion3 and sound3 module lines).
- Tests outside my pages that changed labels or routes: listed above; no assertion was weakened.

## Open

- 27 Privacy long reasons and the category column at 1060 with 15 pt body.
- 18 for Quick Sweep and Live Monitor.
- Sounds for App Remover, Startup, Disk Map and Tune-up jobs; Tap on switches.
- 17 and 48 need a look on the real Mac.
