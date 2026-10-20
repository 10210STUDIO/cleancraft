# design4, round 4 report

Branch `agent4/design4` (base 4e23d58). Merged: `agent4/ux4` (up to 40bb670), `agent4/review4`
(78912fd), `agent4/usability4` (cc1530b). New tests: `ux_render::every_control_shows_the_pointing_hand`,
`ux_render::pages_use_kit_controls_only`, `tests/ux_design4.rs` (6), `tests/ux_design4_lists.rs` (10).
Renders: `docs/screens/v2/*-{1060x680,1280x820,1600x1000}{,@2x}.png`.

## 1. Pointer (the user's report)

| Item | Status | Evidence |
|---|---|---|
| Pointing hand on every enabled clickable control (pills, round Scan, sidebar rows, switcher segments, checkboxes, radios, switches, category rows, result rows, app rows, Disk Map blocks and folder links, dialog X, sort pop-up and its entries, menus, sliders); I-beam over text fields (the whole search field); arrow over disabled controls, labels (labels are no longer selectable) and the title bar strip | Fixed | `ux_render::every_control_shows_the_pointing_hand` hovers every painted, unclipped control in all 38 render scenes (300+), reads `platform_output.cursor_icon`; `review_c1` (review4's probe, un-ignored); `u16` |
| One central helper | Done | `kit::feel` / `kit::feel_ring` / `kit::pointer` used by every kit widget and the custom rows; egui's own buttons via `visuals.interact_cursor`; `kit::slider`, `kit::menu_item`, `kit::menu_choice` |
| Lint against raw egui controls on pages | Done | `ux_render::pages_use_kit_controls_only` (fails on `ui.button`, `ui.checkbox`, `ui.radio`, `selectable_*`, `menu_button`, `egui::Slider` outside `kit::slider`, ComboBox, links) |

## 2. Hover, press, focus

| Item | Status | Evidence |
|---|---|---|
| Hover (eased `motion::tr::HOVER`) and press (`tr::PRESS`: fill shrinks and dims, switch knob stretches, rows and segments tint) on every kit control and custom row | Fixed | `kit::Feel`; renders; review4 Round D marks C34 fixed for kit/app/plan_page |
| Keyboard focus ring (2 pt, 3 pt outside the shape, section colour lifted) | Fixed | `review_c2` un-ignored |
| Result rows: click anywhere toggles; one Tab stop per row; Reveal/Ignore show while the keyboard is on the row | Fixed | `review_c3` un-ignored |

## 3. Open items from design3-r3

| Item | Status | Evidence |
|---|---|---|
| 27 truncation at 1060 | Fixed for category names (size moves to line 2 in the narrow column, left column 260 at 1060); path re-shortened before hover actions | `review_c4`, `review_c13` un-ignored; `system-junk-results-1060x680.png` |
| 18 title jump: Quick Sweep | Fixed: one title row in every phase; the start screen's large headline says what the scan does | `ux_design4::quick_sweep_title_stays_and_the_grid_is_full` |
| 18 title jump: Live Monitor | Handed to ux4 (owner of monitor.rs since 09:58) | board |
| Sounds for App Remover, Startup, Disk Map, Tune-up | Done by ux4 in their pages (commit 1891479) | ux4-r4 |
| Tap on switches and checkboxes | Fixed: kit notes the toggle, the shell plays `Cue::Tap` once (only with "Click sounds on switches") | `ux_design4::toggles_play_the_tap_only_when_turned_on` |

## Lead's high-priority items

| Item | Status | Evidence |
|---|---|---|
| Lists not updated after removal (plan pages, Quick Sweep, cross-page) | Fixed: `CleanApp::files_moved` drops moved paths (and anything inside) from every plan page and Quick Sweep tile, re-measures containing items, drops one-copy duplicate sets; Done returns to the remaining list; Undo marks lists stale and they rescan when shown; the shell follows the action log (`JournalWatch`) so moves and Undos from App Remover, Disk Map, History and Startup reach the plan pages too | `ux_design4_lists` (10 e2e tests: every plan module, list equals a fresh scan; cross-page Duplicates to Large & Old; Quick Sweep both ways; Undo; moves through the log); `u17` |
| App Remover detail (user screenshot) | ux4 owns the page; kit parts done: `kit::row_tip` (tooltip under or above its row), `kit::check_or_lock` (padlock), one disabled pill style (flat, faint text), `kit::banner` (button inside) | `ux_design4::row_tooltips_do_not_cover_their_row`, `disabled_buttons_have_one_clear_style` |

## REVIEW-C (review4)

| # | Status | Evidence |
|---|---|---|
| C1 C2 C3 C6 C8 C9 C13 C16 | Fixed | tests un-ignored |
| C4 | Fixed | `review_c4` |
| C5 | Fixed: inert background, Cancel focused on open, Tab then reaches the action, Return presses the action, Esc cancels | `review_c5` (Tab expectation adjusted to the target "Cancel focused when the dialog opens"), `u10_return_confirms_the_dialog` |
| C10 | Fixed for the total (exact from frame 1; changes glide) | `review_c10`; `ux_design3::results_total_is_exact_from_the_first_frame` (was the count-up test) |
| C12 | Fixed: confirmations up to 860 pt wide, fade and "N items in all. Scroll the list to see every one." | renders `*-confirm-*` |
| C15 | Fixed: one title treatment, rows share the tiles evenly and widen the last row | `ux_design4::quick_sweep_title_stays_and_the_grid_is_full` |
| C17 | Fixed: one toolbar row; a "More" menu holds the module's buttons when narrow | `review_b1` still passes; `privacy-traces-results-1060x680.png` |
| C18 | Fixed: tags neutral for ordinary reasons, amber for loss, deeper amber for own files, never red; the tooltip says what the colour means | `kit::risk_note` |
| C19 C20 | Partly: plan pages and Quick Sweep follow one bar and one header rule; App Remover, Disk Map, Startup, App Updates are ux4's | renders |
| C21 | Partly: toolbar in reading order, one stop per row; the bottom bar is still the first Tab stop (egui needs the panel first) | |
| C22 | Fixed: `kit::banner` | `system-junk-error-*.png` |
| C23 | Fixed: hero up to 576 pt on big windows | `*-idle-1600x1000.png` |
| C24 | Fixed: ink within 25% of the median, centred within 1.5 px at 56 px | `ux_design4::section_icons_have_even_weight_and_sit_centred` |
| C30 | Partly: `kit::empty_state` for list empties (plan search); ux4's pages to adopt it | |
| C31 C32 C35 | Fixed: no "0 skipped"; custom sort entries with a check mark and no repeated "Sort by"; overlay scroll bars | `qa_cleanup_junk`, `qa_sweep` updated wording |
| C33 | Partly: segments announce as radio buttons; switches stay checkboxes (egui has no switch role); row checkboxes keep the file name only | |
| C34 | Fixed for kit, app, plan_page, sweep | |
| C7 C11 C14 C25-C29 C36 | ux4 | |
| C37-C39 | Partly: tile error text as a sentence; the rest ux4/core | |

## REVIEW-U (usability4)

| # | Status | Evidence |
|---|---|---|
| U3 | Fixed: Trash Bins hides Select All/None, category checkboxes, Ignore and Reveal on whole Trashes; Empty Trash in Finder sits in the bottom bar with its hint | `u03` |
| U4 | Fixed: Done screens say where the space comes back (Trash or quarantine by method) and offer "Empty Trash..." with Finder's confirmation | `u04` |
| U8 | Fixed: notice and "Stop Ignoring" on the row | `u08` |
| U9 | Fixed: reason plus next step, "Try Again" (re-confirms only the skipped items), "Open Privacy Settings" for permission errors | `u09` |
| U10 | Fixed: Return presses the dialog action, Cmd+F search, Cmd+, Settings, Cmd+1..7 sections | `u10_*` |
| U11 | Fixed: "Kept" tag and "Keep This One" | `u11` |
| U14 | Fixed: notices clear on navigation; FDA banner only on Quick Sweep, Clean and Protect pages, with "Not Now" | `u14_*` |
| U16 U17 U18 | Fixed (Sort by date on Large & Old Files) | `u16`, `u17`, `u18` |
| U20 | Partly: the confirmation says once "You will be signed out of websites in ..."; preselection of cookies is core `privacy.rs` (not mine) | |
| U23 U25 U27 | Fixed | renders, `ux_design4_lists` |
| U13 U19 U21 U22 U24 U26 U28, U1 U2 U5 U6 U7 U12 | ux4 / apps4 | |

## Not done or partly

- C19/C20/C30 on ux4's pages, C21 bottom-bar Tab order, C33 switch role and row descriptions, U20 core preselection.
- 17 (banding) and 48 still need a look on the real Mac.
