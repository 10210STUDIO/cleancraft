# ux4 round 4 report

Branch `agent4/ux4`. Pages owned: App Remover, App Updates and the app detail (`pages/apps.rs`),
Startup Items, Tune-up Tasks, Disk Map, Live Monitor, History, Settings. Merged on the way:
agent4/design4 (up to dc35380), agent4/apps4 (f35df8f), agent4/review4, agent4/usability4.

Renders: `docs/screens/v2/ux4/<scene>-<w>x<h>[@2x].png` at 1060x680, 1280x820 and 1600x1000, 1x
and 2x, 18 scenes (App Remover list, detail, long names, running app with a helper, part of
macOS, confirm, done, leftovers; App Updates idle and results; Startup; Tune-up; Disk Map idle
and map; Live Monitor; History and empty History; Settings). Test file
`crates/ui-egui/tests/ux4_render.rs` (`ux4_pages_fit_every_size` checks no text leaves the
window in any scene at any size; `ux4_pages_render` writes the PNGs with
`CLEANCRAFT_SNAPSHOTS=1`).

## 1. App Remover detail (the user's screenshot)

| Item | Status | Evidence |
|---|---|---|
| Tooltip "The app itself." covering the path | fixed | No tooltip on group headings; each group's meaning is a dim line under it (none for the app). Row tooltips use `kit::row_tip` (opens under or above the row, never over it) and only when a name or folder was shortened. Render `app-remover-detail-long-*` |
| Locked rows look like empty checkboxes | fixed | Padlock in place of the box (still a disabled checkbox for VoiceOver); test `ux4_flows::running_app_banner_quits_and_rows_unlock` checks every leftover is disabled |
| "(Quit X first)" on every row | fixed | Said once in the banner; rows show only their kind. Same test asserts no "(Quit Editor first)" text |
| Reasons cut "(..." | fixed | Rows show the reason's kind ("Cache", "Preferences", "App container"); the group heading says the rest; full reason announced |
| Full /Users/... paths in large type | fixed | File name prominent (15 pt), folder under it dim, "~" for home, shortened in the middle; full path in the checkbox name and the row tooltip |
| Disabled Uninstall in pale red | fixed | Blocked actions are quiet secondary pills (design4's one disabled style too), with a hover reason |
| Banner had no action | fixed | apps4's `app_quit`: headline from `running_parts` ("PhotoStudio_Pro and 1 background helper are running"), one line per helper (name, role), `Quit <App>` primary, a password question first when a system service runs ("Quit and Enter Password" / "Quit the Rest"), spinner and wait text while quitting, "Force Quit" (with a confirm) when it did not quit or after 3 s, rows unlock by themselves (running_parts every 1.5 s in a Job), the app and id-named files selected again. Part of macOS: neutral padlock banner, no Ignore or Reset. Ignored app: banner with "Remove from Ignore List". Tests `ux4_flows::running_app_banner_quits_and_rows_unlock`, `force_quit_after_confirmation`, `qa_apps_remover::running_app_is_refused`, usability `u01`, `u02` un-ignored and passing; render `app-remover-running-*` |

## 2. Lists not updated after removal (user bug)

Fixed. `pages/history.rs` has `JournalWatch` (prime when data is read, `changes()` = records new or
undone since, one stat of journal.jsonl per call) and `moved_paths`. It catches moves and undos
from any page, History or an MCP agent. design4 now uses it in app.rs for plan pages and Quick
Sweep; my pages also call `app.files_moved` / `files_back`.

- App Remover: on Done the moved app leaves All Apps, Unused and its group at once and the counts
  follow (before the slow re-inventory finishes); Done returns to the list; Leftovers/Installers/
  Extensions drop moved files; Undo brings it back on the next look.
- Disk Map: trees re-measure only the moved places (and the sound cache), so moved blocks leave
  the map and sizes equal a fresh scan.
- History and Startup reload when the log changed.
- Test `ux4_flows::uninstall_updates_every_list_and_undo_brings_it_back`: map home, uninstall
  Editor (scan, select, confirm, done, back), All Apps and Unused rows equal a fresh
  `apps::inventory`, Unused no longer lists it, Disk Map has no moved node and its root size
  equals a fresh `space::build`, History lists the action, Undo All puts 4 items back, App
  Remover and Disk Map show it again with fresh numbers. `removing_leftover_files_updates_their_group`
  covers the Leftovers group.

## 3. Pointer and states

Fixed for my pages: every control is a kit control (design4's `kit::feel`, `feel_ring`,
`slider`, `dialog_actions`); app rows, file rows, Disk Map blocks and folder links use
`kit::feel_ring` (pointing hand, eased hover and press, keyboard focus ring). File rows toggle on
a click anywhere. Confirm dialogs (Uninstall/Reset/Remove, Force Quit, startup Quit and Remove,
Tune-up Run, Disk Map, Free Up RAM) use `kit::dialog_actions` (Cancel first in Tab order).
Evidence: design4's `ux_render::every_control_shows_the_pointing_hand` and
`pages_use_kit_controls_only` pass on this branch; REVIEW-C `review_c1` passes.

## 4. review4 and usability4 items for my pages

| Item | Status | Evidence |
|---|---|---|
| U1 running app, no way forward | fixed | u01 un-ignored |
| U2 helper not named | fixed | u02 un-ignored |
| U6 History one card per file | fixed | One card per action with Undo All; actions over 8 items show 5 and "Show All N Items"; u06 un-ignored |
| U7 Undo for items emptied from the Trash | fixed | "Emptied" tag with a reason, no Undo; u07 un-ignored; `review_reliability_ui::history_undo_after_trash_emptied_says_why` updated to the new behaviour |
| U12 ignore list folder picker | fixed | "Choose Folder..." (macOS dialog through osascript and env.sys, Cancel = nothing), typed paths checked (missing path, already listed), Enter adds; u12 un-ignored |
| U13 five names for rescanning | fixed for my pages | "Scan Again" in App Remover (list and detail), Startup, Tune-up, Disk Map; History has none (reloads itself); u13 un-ignored |
| U19 raw -1743 error | fixed | Startup says where to turn on Automation; u19 un-ignored |
| U5 quarantine invisible | partly | Settings shows what the quarantine holds and "Show Quarantine in Finder". No "Empty Quarantine": it would delete for good, which STANDARD.md forbids in app code (a core "move quarantine to Trash" would be needed). u05 stays ignored |
| U15 Reset / Ignore / Download unclear | partly | Hover hints on Reset, Ignore, Update ("opens the app's updater / App Store"), "Download" renamed "Download Only" with a hint. Labels "Reset" and "Ignore" kept (tests and the hints explain them) |
| U21 startup rows by bundle id | fixed | Title is the owning app's name with the id beside it; Remove has a hint; "Background helpers that your apps installed for you" |
| U22 Free Up RAM duplicates the task | open | Left as is |
| U4 Done offers Empty Trash, U9 skipped files get Try Again | open | Both live in `plan_page::done_view` (design4), which App Remover's Done screen reuses; nothing App Remover specific was added |
| U3, U8, U10, U11, U20 | not mine | Trash Bins, plan page Ignore, dialog keys, Duplicates, Privacy: design4's pages and kit |
| U24 Disk Map "use System Junk or App Remover" as links | open | |
| C7 Monitor pill hit area | fixed | review_c7 un-ignored |
| C11 unnamed slider value fields | fixed | Values are labels announced with their setting ("Large files start at: 50 MB"), sizes on a log scale; review_c11 un-ignored |
| C14 Tune-up cards tall | fixed | Title row with tags and last run, description left and command right from 720 pt of width; actions in a bottom bar ("N tasks selected", Run Selected, Scan Again); "Run 1 task?" singular. Commands stay Inter (the theme has no monospace face) |
| C19 duplicate "Selected" figure in Disk Map | fixed | Shown only when something is selected |
| C20 header styles | partly | App Updates headline now the same 22 pt text as Startup and History |
| C25 monitor gauge gap and flat sparkline | fixed | 12 pt gap, history line only after two different readings |
| C26 "of 16.0 GB" baseline | open | |
| C27 startup tiles 32 vs 28, copy | fixed | Same tile size, new wording (see U21) |
| C28 Disk Map glyphs and "Locked: protected:" | fixed | Kind icons with a colour bar, lock line says the reason once |
| C29 Disk Map Map Folder looks dead | fixed | "Choose Folder..." next to it, always enabled |
| C30 empty states | partly | App Remover and History empty states have a title and a hint; one shared kit component would be design4's |
| C31 "0 skipped" | fixed | App Remover Done uses `kit::moved_summary` |
| C36 Updates cards uneven | open | |

## 5. Own pass

- App Remover list: group names whole at 1060 ("App Store", "Other Sources" shown, full name
  announced); empty states with a hint (search vs group); Leftovers notes moved to Extensions,
  where they belong; "Tick files to remove them." in an empty file bottom bar.
- Startup: a change in progress keeps the list on screen (inline spinner, no full-page flash);
  plain empty states; warnings as strips; dialogs without monospace.
- Settings: quarantine line, folder chooser, path checks, agent switch explained, scroll fade.
- Sounds (design3's open list): ScanStart and ScanDone on App Updates and Disk Map, CleanDone or
  Error on uninstall, CleanDone on Disk Map removal and Tune-up, Trash on Startup Remove.

## Tests

New: `tests/ux4_flows.rs` (4), `tests/ux4_render.rs` (2). Updated without weakening:
`qa_apps_remover` (banner text), `qa_perf_tasks` ("2 tasks selected", "Scan Again"),
`qa_shell` (scrolls back before Remove), `review_reliability_ui` (emptied items: no Undo, the tag
explains). Un-ignored: review_usability u01 u02 u06 u07 u12 u13 u19, review_design_c c7 c11.
Full `cargo test --workspace` run on 5bd0e3c+: three failures found and fixed after it
(Settings sliders' pointing hand, the Disk Map folder row at 1280 in qa_every_control, the new
partial-size test's fixture); those targets re-run green, workspace clippy -D warnings and fmt
clean. Final full `cargo test --workspace` on 40bb670: all 80 test targets green.
