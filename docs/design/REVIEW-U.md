# REVIEW-U: task-based usability review (round 4)

Reviewer: usability4. Base: main 4e23d58 (round 3, the build the user installed). Method: a first-time Mac user's real tasks, driven with egui_kittest on FakeMac at 1280 x 820 by clicks and keys only, every step logged; yardstick Nielsen's ten heuristics (H1..H10) and the Apple Human Interface Guidelines (HIG); flow quality (not the look) compared with top Mac utilities.

Evidence:
- Walkthroughs: `task_01..task_17` in `crates/ui-egui/tests/review_usability.rs` (pass today; the step log prints with `cargo test -p cleancraft-ui-egui --test review_usability -- --ignored --nocapture task_`).
- Proofs: `u01..u19` in the same file, `#[ignore]`d and failing today. Drop the `#[ignore]` when the finding is fixed.
- Renders: `docs/design/review-u/` (`u01`, `u08`, `u09`, `u17`, `u19` rendered by the tests with `CLEANCRAFT_REVIEW_U_RENDERS=1`; the others are crops of the round 3 renders in `docs/screens/v2`).

Real user feedback this round, and where it lands: pointer does not change over buttons (U16); could not uninstall an app because a helper of it was running, with no way forward (U1, U2); after removing something the list did not update (U17; other lists checked and do refresh, see "Checked and fine"); "better but needs polish" (U13..U28).

Severity: **S1** blocks the task. **S2** causes confusion or extra work. **S3** polish.

## Tasks

| # | Task | Result | Steps | Doubts | Findings |
|---|---|---|---|---|---|
| 1 | Free space quickly with Quick Sweep | Pass, but the goal is missed: 14.0 MB moved, 0 bytes freed; freeing it needs a detour to Clean > Trash Bins > Scan > Empty Trash in Finder > Empty Trash (5 more steps the app never suggests) | 3 (+5) | 2 | U4, U17 |
| 2 | Clean System Junk but keep one item | Pass (search, untick, the confirmation lists only the rest) | 7 | 0 | |
| 3 | Empty Trash Bins | Pass, after reading past a hint that asks to tick locked rows | 5 | 1 | U3 |
| 4 | Find and remove a big old file | Pass when the file is in view; cannot sort by age | 6 | 1 | U18 |
| 5 | Remove duplicate copies keeping the right one | Pass when the suggested copy is the right one (3 steps); choosing another copy is untick, refused hint, untick another, tick (about 7 steps) | 3 to 7 | 2 | U11 |
| 6 | Uninstall an app with its leftovers | Pass; Done returns to the list with the app gone | 6 | 0 | U15 |
| 7 | Uninstall an app that is running, or whose helper runs | **Fail**: "Quit Editor first", everything locked, no Quit button; with only the helper running the user is told to quit an app that is already quit | 3, dead end | 2 | U1, U2 |
| 8 | Reset an app | Pass (headline says "The app stays.") | 5 | 1 | U15 |
| 9 | Update apps | Pass to the hand-off; "Update" and "Download" side by side unexplained | 3 | 1 | U15, U19 |
| 10 | Turn off a startup item | Pass; an Automation refusal shows raw osascript text | 3 | 1 | U19, U21 |
| 11 | Run a tune-up task | Pass (tick, Run Selected, Run) | 5 | 1 | U28 |
| 12 | Find what fills the disk (Disk Map) | Pass (map, open Downloads, tick, Review and Remove; the map refreshes after Done) | 4 to 8 | 0 | U24 |
| 13 | Check threats and privacy traces | Pass; Privacy preselects cookies and history of every browser | 5 | 1 | U20 |
| 14 | Undo something from History | Pass for one file; a whole cleanup is 27 rows, 27 Undo clicks | 7 (one file), 33 (one cleanup) | 1 | U6, U7 |
| 15 | Change settings, including sounds | Pass (radio, switches, Preview sound) | 2 to 4 | 1 | U12, U26 |
| 16 | Work with Full Disk Access missing | Pass (banner, Open Privacy Settings, Check Again); banner on every page, cannot be closed | 2 | 1 | U14 |
| 17 | Recover from permission denied, file in use, disk full | **Fail** without help: bare reasons, no next step, no Try Again | 5, dead end | 1 | U9 |

**15 of 17 tasks pass, 2 fail** (7 and 17). Task 1 passes the clicks but misses the user's goal.

## Findings

Owners (COMMON4 "Round 4 ownership update"): design4 = theme, kit, app.rs shell, plan_page.rs, sweep.rs, extras pages; ux4 = apps.rs, startup.rs, tasks.rs, disk_map.rs, monitor.rs, history.rs, settings.rs; apps4 = core uninstaller running-app flow.

### S1: blocks the task

**U1 S1 App Remover detail, running app: no way forward** (apps4, ux4; H3 user control, H9 recover; HIG: offer the fix where the problem is)
Evidence: `u01_running_app_offers_quit_in_place`, `task_07`, render `review-u/u01-running-app.png`. The strip says "Quit Editor first"; every row is unticked, locked and repeats "(Quit Editor first)" truncated; Uninstall and Reset are disabled; the only buttons are Back, Check Again and Ignore. The user must leave CleanCraft, find the app, quit it, come back and press Check Again (which is not named as the next step).
Recommendation: in the strip, a "Quit Editor" button (the same polite quit as Startup > Heavy apps `quit_process`, asking first: "Quit Editor? It may ask you to save your work."); after it quits, re-check by itself (poll every second for up to 10 s, then unlock and keep the user's selection); offer "Force Quit" only when the app has not quit after 10 s. Rows keep the normal reasons; the lock reason shows once in the strip, not on every row. Disable Ignore while locked (it is the only active button and invites the wrong click).

**U2 S1 App Remover, a helper inside the bundle is reported as the app** (apps4 message, ux4 strip; H1 visibility, H2 match the real world)
Evidence: `u02_running_helper_is_named` (EditorHelper in `Editor.app/Contents/Library/LoginItems` runs, Editor is quit; the page says "Quit Editor first" and never names EditorHelper). This is the user's real case.
Recommendation: `app_lock` returns which process runs ("EditorHelper, part of Editor, is running"); the strip's button reads "Quit EditorHelper" (or "Quit Editor and its helpers" when several run); the same in the refusal "Nothing was moved." message.

### S2: confusion or extra work

**U3 S2 Trash Bins: selection controls for rows that cannot be selected** (design4: plan_page.rs, extras_cleanup.rs; H8 minimalist, H5 error prevention)
Evidence: `u03_trash_bins_has_no_dead_selection_controls`, `task_03`, `review-u/u03-trash-bins.png`. Every row is locked (correct: CleanCraft never deletes permanently), yet the bar says "Tick the items to clean.", the category checkboxes and Select All / Select None look active, every Trash row offers "Ignore" (ignoring a whole Trash) and "Reveal", and the one real action, "Empty Trash in Finder", is a small pill among the selection buttons.
Recommendation: on Trash Bins hide Select All, Select None, the category checkboxes and the row Ignore; the bottom bar carries the primary "Empty Trash..." with the size ("Empty Trash (3.38 MB)...") and the hint "Finder empties every Trash. CleanCraft cannot put these back."; the Mail trash row keeps its own hint.

**U4 S2 "Free space" ends with no space freed** (design4: plan_page.rs `done_view`, sweep.rs; ux4: monitor.rs; H1, H7 efficiency)
Evidence: `u04_done_offers_to_empty_the_trash`, `task_01`, `review-u/u04-done-note.png`, `review-u/u04-sweep-trash-tile.png`. Every Done screen (each module, Quick Sweep, Disk Map, App Remover) ends with "The space is freed when the Trash is emptied or the quarantine is cleared." and no button; the Quick Sweep Trash tile says "Emptied from its own page" and has no Include. The quarantine is mentioned although Settings uses the Trash. Live Monitor shows the Trash size and a "Free Up Space" button that opens Quick Sweep, not the Trash. the reference app's Smart Care frees the space in the same run; DaisyDisk's Delete frees it at once.
Recommendation: on every Done screen with something moved, a secondary "Empty Trash..." (Finder's own confirmation, same code as Trash Bins) next to Undo, with the Trash size after the move; once emptied, Undo is replaced by "Emptied" (see U7). Say "quarantine" only when the method is the quarantine (and then offer U5's action). On the Quick Sweep Trash tile: "Empty Trash..." in place of Include. Live Monitor: the button next to "Trash 2.50 MB" reads "Empty Trash..." and does that.

**U5 S2 The quarantine can never be seen or emptied** (ux4: settings.rs; H3, H1)
Evidence: `u05_quarantine_can_be_seen_and_emptied`. With "CleanCraft quarantine" chosen, and for every threat whatever the setting, items pile up in `~/Library/Application Support/CleanCraft/Quarantine`; no page shows its size or empties it, while Done tells the user to "clear" it.
Recommendation: in Settings > Removed items go to, a line "Quarantine holds 1.2 GB (34 items)" with "Show in Finder" and "Empty Quarantine..." (confirmation: "These items cannot be put back afterwards"; History marks their records as emptied). Same action reachable from the U4 Done button when the method is the quarantine.

**U6 S2 History: one row per file, no Undo for a cleanup** (ux4: history.rs; H3, H7, H4 consistency with Done)
Evidence: `u06_history_groups_a_cleanup_into_one_undo`, `task_14`, `review-u/u06-history.png`. One System Junk cleanup is 27 cards with 27 Undo buttons; the header says "27 actions". The Done screen's Undo puts back the whole run, History cannot.
Recommendation: one card per run (plan id): "System Junk · Today 22:14 · 27 items · 32.0 MB", "Undo All" on the card, a disclosure listing the files with per-file Undo; the header counts runs ("3 cleanups"). Tune-up commands and startup toggles stay one card each.

**U7 S2 History offers Undo for items no longer in the Trash** (ux4: history.rs, core `can_undo`; H5, H9)
Evidence: `u07_history_hides_undo_for_items_no_longer_in_the_trash` (stored files gone, 27 Undo buttons remain; clicking gives "Could not put it back: ...").
Recommendation: when listing, check that the stored path still exists; if not, no Undo and a dim "Emptied from the Trash; cannot be put back."; Undo All counts only what can come back ("Undo 12 of 27").

**U8 S2 Ignore acts at once, silently, with no way back** (design4: plan_page.rs row actions; H3, H1)
Evidence: `u08_ignore_can_be_undone_on_the_spot`, `review-u/u08-after-ignore.png`. One click adds the path to the ignore list, unticks and locks the row ("on your ignore list"); no notice, no Undo, no "Stop Ignoring"; the only way back is Settings, by finding and removing a typed path.
Recommendation: notice "Added to the ignore list." with an Undo button (removes it, restores the tick); rows that are ignored show "Stop Ignoring" in place of Ignore.

**U9 S2 Not moved: bare reasons, no next step, no retry** (design4: plan_page.rs `skipped_card`, sweep.rs; core SkipReason text; H9, H2)
Evidence: `u09_skipped_items_say_what_to_do_and_can_be_retried`, `task_17`, `review-u/u09-not-moved.png`. "permission denied", "open in a running app" (which app?), "the disk is full" (moving to the Trash of the same disk needs no space: what should the user do?). No Try Again; the user must rescan, find the items again and confirm again.
Recommendation: name the app holding the file (lsof gives the pid: "open in Safari"); give the next step in words ("Quit Safari, then Try Again"; "macOS did not allow it. Turn on Full Disk Access" with Open Privacy Settings; "The disk is full: empty the Trash first" with Empty Trash...); a "Try Again" button that re-checks only the skipped items and moves those that are now free (with the same confirmation listing them).

**U10 S2 Keyboard: no default button, no standard shortcuts** (design4: kit.rs `modal`, app.rs; H7; HIG: default button responds to Return, standard menu shortcuts)
Evidence: `u10_return_confirms_the_dialog` (Return in "Move 27 items to the Trash?" does nothing), `u10_standard_shortcuts` (Cmd+F does not focus Search, Cmd+, does not open Settings). Esc works in dialogs; Enter in the App Remover search opens the first app (good).
Recommendation: Return presses the dialog's default button (destructive dialogs may make Cancel the default as HIG allows; then Tab and Space must reach "Move to Trash"); Cmd+F focuses the page's search; Cmd+, opens Settings; Cmd+Z on a Done screen is Undo; Cmd+1..7 the sidebar sections.

**U11 S2 Duplicates: the kept copy is not marked and cannot be chosen in one click** (design4: plan_page.rs duplicates rows, extras_files.rs; H6 recognition, H7)
Evidence: `u11_duplicates_mark_and_choose_the_kept_copy`, `task_05`, `review-u/u11-duplicates.png`. Three rows all named "a.bin"; the kept one is only an unticked box; to keep another copy the user unticks it, is refused ("Keep at least one file in each set. Deselect another copy first, then select this one."), unticks another, ticks again. The sidebar category is "Other". Gemini and the reference app mark the kept file and switch it in one click.
Recommendation: a "Kept" tag on the kept row; a per-row "Keep This One" (hover action) that keeps it and ticks the rest of the set; rows lead with the folder ("Projects / a.bin"); drop the "Other" category when it is the only one.

**U12 S2 Ignore list: typed paths only** (ux4: settings.rs; H6, H5; HIG: use the open panel to pick files)
Evidence: `u12_ignore_list_has_a_folder_picker`, `review-u/u12-settings-ignore.png`. A text field "Path to ignore" and "Add to Ignore List"; no picker, no check that the path exists.
Recommendation: "Add Folder..." opens the macOS open panel (files and folders); a typed path that does not exist gets "No such file or folder" and is not added.

**U16 S2 Pointer stays an arrow over buttons** (design4, already planned; HIG pointers)
Evidence: `u16_pointing_hand_over_buttons` (hover Scan: `CursorIcon::Default`). User feedback.
Recommendation: PointingHand on every clickable control (buttons, pills, rows, segments, sidebar entries, Disk Map blocks), Text over text fields, Default on disabled controls.

**U17 S2 Quick Sweep tiles go stale after a module cleanup** (design4: sweep.rs; H1; the user's "the list did not update")
Evidence: `u17_quick_sweep_tiles_follow_a_module_cleanup`, `review-u/u17-stale-sweep.png`. Scan, Review the System Junk tile, clean there, Done, back to Quick Sweep: the tile still says 13.3 MB and the total 14.0 MB; Run would try to move files that are gone and report them as skipped.
Recommendation: when a module cleans, undoes or rescans, refresh its tile from that module's new plan (or show "Cleaned just now · Scan Again"); same for the App Updates tile after an update and the Trash tile after Empty Trash.

**U20 S2 Privacy Traces preselects cookies and history of every browser** (design4: plan_page.rs confirmation; core privacy defaults; H5)
Evidence: `review-u/u20-privacy-preselected.png`, `task_13`: 20 of 29 items preselected, cookies included (core `COOKIES.selected = true`); "you will be signed out of websites" lives only in each row's reason tag, truncated in the confirmation list.
Recommendation: preselect caches and sessions only; history and cookies ticked by the user per browser; when cookies are selected, the confirmation shows one strip "You will be signed out of websites in Safari and Chrome."

### S3: polish

**U13 S3 Rescan has five names** (design4, ux4; H4)
Evidence: `u13_one_name_for_scanning_again`: "Scan Again" (plan pages), "Look Again" (App Remover), "Check Again" (Tune-up, App Updates, App detail, FDA banner), "Refresh" (Startup, History), "Map Again" (Disk Map).
Recommendation: "Scan Again" everywhere a page re-reads the Mac; "Check Again" only for the Full Disk Access banner; History needs no button (reload when shown and after each Undo).

**U14 S3 Notices and the Full Disk Access banner follow the user** (design4: app.rs `banners`; H8)
Evidence: `u14_notice_clears_on_page_change` ("Put back." from History still shows on Settings), `u14_fda_banner_only_where_needed_and_dismissable` (banner on Quick Sweep and Settings, no way to close it), `review-u/u14-fda-banner.png` ("Check Again" wraps to a second line).
Recommendation: notices clear on page change, success notices also after 6 s; the banner only on pages that read protected data (System Junk, Mail Downloads, Trash Bins, Privacy, Threat Check, Quick Sweep), with "Not Now" (remembered for the session) and a re-check when the window becomes active again (the user comes back from System Settings); one line, buttons on the right.

**U15 S3 Unclear action names on App Remover and App Updates** (ux4: apps.rs; H2)
Evidence: `review-u/u15-app-detail-bar.png`, `review-u/u15-updates-buttons.png`. Detail bar "Ignore · Check Again · Reset · Uninstall": Reset what? Ignore what? Updates: "Download" and "Update" side by side; "Developer updates" under the version.
Recommendation: "Reset App..." and "Hide from List" (keep the tooltips); Updates: one primary "Update", "Download Only" as a secondary with a tooltip saying what each does; "From the developer" in place of "Developer updates".

**U18 S3 Large & Old Files cannot be sorted by age** (design4: plan_page.rs sort; H7)
Evidence: `u18_large_old_sorts_by_last_opened` (choices: size, name).
Recommendation: "Sort by last opened" (oldest first) on Large & Old Files, with the date on each row.

**U19 S3 Raw tool errors** (ux4: startup.rs, apps.rs updates; H9)
Evidence: `u19_login_item_error_is_in_plain_words`, `review-u/u19-raw-error.png` ("System Events did not remove it: execution error: Not authorized to send Apple events to System Events. (-1743)"); App Updates shows "Software Update could not check: <stderr>".
Recommendation: map -1743 to "CleanCraft needs permission to control System Events. Turn it on in System Settings > Privacy & Security > Automation." with an Open button; any other stderr behind a "Details" disclosure under a plain sentence.

**U21 S3 Startup rows named by bundle id, jargon tags, Remove next to the switch** (ux4: startup.rs; H2)
Evidence: `review-u/u22-startup-rows.png`. Rows "com.example.helper", "Ad hoc signature (no developer identity)", "Runs /Applications/.../ExampleHelper"; a Remove pill sits next to the on/off switch with nothing saying how they differ.
Recommendation: title = the owning app's name ("Example helper"), bundle id dim under it; "Not signed by a known developer" in place of "Ad hoc signature"; Remove under a "..." menu or with the tooltip "Move its file to the Trash; Turn Off keeps it".

**U22 S3 Live Monitor duplicates other tools with different flows** (ux4: monitor.rs; H4)
Evidence: render `docs/screens/v2/live-monitor-1280x820.png`. "Free Up RAM" (Live Monitor, dialog "Free Up RAM Now") and the "Free up RAM" task (Tune-up, checkbox and Run Selected) do the same thing two ways; "Free Up Space" opens Quick Sweep.
Recommendation: one flow and one name ("Free Up RAM...") reused from Tune-up; see U4 for Free Up Space.

**U23 S3 Quick Sweep failed tile has no retry** (design4: sweep.rs; H9)
Evidence: render `docs/screens/v2/quick-sweep-results-1280x820.png`, Threat Check tile: "Could not scan · rules could not be loaded" with only "Review".
Recommendation: "Try Again" on a failed tile, and the reason in plain words.

**U24 S3 Disk Map: locked Library points elsewhere without a link** (ux4: disk_map.rs; H7)
Evidence: render `disk-map`: "Locked: app data in Library (use System Junk or App Remover)".
Recommendation: make "System Junk" and "App Remover" links that open those tools.

**U25 S3 Done screens report "0 skipped"** (design4: plan_page.rs, apps.rs; H8)
Evidence: render `system-junk-done`: "27 items moved. 0 skipped."
Recommendation: mention skipped items only when there are some.

**U26 S3 Settings wording and controls** (ux4: settings.rs; H2, H4)
Evidence: render `settings`: "Let AI agents (MCP) carry out confirmed cleanups" is a checkbox among switches; the quarantine option shows a raw path; "Large files start at" runs 10 to 5000 MB on a linear slider, so 50 MB sits at the far left (also review4 C11).
Recommendation: a switch like the Sounds group; "Let AI assistants clean after you confirm (MCP)"; the path as a dim second line with Show in Finder; stepped values (10, 25, 50, 100, 250, 500 MB, 1, 2, 5 GB).

**U27 S3 Trash Bins and Privacy rows offer actions that make no sense there** (design4: plan_page.rs; H8)
Evidence: `task_03` buttons list: Ignore and Reveal on whole Trash folders; Privacy rows for Wi-Fi and permissions are hidden from Reveal (good) but keep Ignore.
Recommendation: row actions per feature: Trash rows "Show in Finder" only; Wi-Fi and permission rows no Ignore.

**U28 S3 Tune-up: Run Selected appears far from the ticked task** (ux4: tasks.rs; H1)
Evidence: `task_11`; at 1280 x 820 only three task cards fit, "Run Selected" appears in the top row while the user ticks a card below the fold.
Recommendation: a bottom bar like the plan pages ("1 selected · Run Selected") so the action appears where the eye is; see also review4 C14 (card height).

### Counts

| Severity | Count | Findings |
|---|---|---|
| S1 | 2 | U1, U2 |
| S2 | 13 | U3, U4, U5, U6, U7, U8, U9, U10, U11, U12, U16, U17, U20 |
| S3 | 13 | U13, U14, U15, U18, U19, U21, U22, U23, U24, U25, U26, U27, U28 |

Top 5: U1/U2 (running app or helper: dead end), U17 (stale Quick Sweep), U4 (no space actually freed), U9 (no recovery from skipped items), U6 (History cannot undo a cleanup).

## Checked and fine

- After Uninstall, Done returns to the list and the app is gone (`task_06`); Undo rebuilds it.
- Disk Map refreshes after a removal (Done shows the map without the file); Startup Items drops a removed agent and says "Moved 1 startup file. You can put it back from History."
- Trash Bins rescans after Finder empties the Trash.
- A login item turned off stays listed with "Turn On", also after CleanCraft is reopened.
- Confirmations list the exact files, sizes and destination; Esc and Cancel move nothing; the System Junk keep-one-item flow is clean (search, untick, confirm shows the rest).
- Section memory: a sidebar section reopens the tool used last.

## Flow quality compared with top Mac utilities

- Running apps: AppCleaner and Pearcleaner ask to quit the app (and its helpers) from the removal dialog; CleanCraft stops the user (U1, U2).
- Freeing space: the reference app's Smart Care and DaisyDisk end with space freed; CleanCraft's safety model (Trash first) is a strength, but it must offer the last step (U4, U5).
- Undo: CleanCraft is ahead of every competitor (Undo on Done, journal for every move); History should match the Done screen's granularity (U6, U7).
- Duplicates: Gemini's "keep this one" in one click is the bar (U11).
- Keyboard: all four reference apps honour Return for the default button and Cmd+F for search (U10).

## Verification

### Final verification on main d50de53 (design4 374a583, ux4 919987b, apps4 f35df8f)

Method: `git merge main` in this worktree, then `cargo test -p cleancraft-ui-egui --test review_usability -- --include-ignored --nocapture` (all 17 walkthroughs and all 20 proofs together): **36 of 37 pass**; only `u05` fails. The owners removed the `#[ignore]` of the proofs they fixed and did not change any assertion (checked with `git diff cc1530b HEAD -- crates/ui-egui/tests/review_usability.rs`: only `#[ignore]` lines removed). Items without a proof were checked in the source and in fresh renders (`review-u/main-*.png`, the same scenes as the round 3 `review-u/u*.png`).

**Tasks on main: 17 of 17 pass** (was 15 of 17). Task 7 (running app) now succeeds in 3 steps with no doubt, task 17 (recover from errors) succeeds with Try Again and Open Privacy Settings on the Done screen, task 16 has no doubt left. Doubts left: task 1 (the Quick Sweep Trash tile still says "Emptied from its own page" and the Quick Sweep Done screen has no Empty Trash) and task 14 (the History walkthrough still counts per-file Undo buttons inside the cards: 5, down from 27, next to Undo All).

| Item | Status | Evidence |
|---|---|---|
| U1 S1 running app: Quit in place | Fixed | `u01` passes; `task_07` success; `review-u/main-u01-running-app.png` ("Editor is open", Quit Editor, rows unlock after it quits) |
| U2 S1 helper named | Fixed | `u02` passes (EditorHelper is named) |
| U3 S2 Trash Bins dead selection | Fixed | `u03` passes (no "Tick the items", no Select All, no Ignore on Trash rows) |
| U4 S2 no space freed | Partly | `u04` passes: module Done screens have "Empty Trash..." and "The space comes back when the Trash is emptied." (`main-u09-not-moved.png`). Open: Quick Sweep Done has no Empty Trash (buttons Show History, Done, Undo per module) and its Trash tile still says "Emptied from its own page" (`main-u17-stale-sweep.png`); Live Monitor "Free Up Space" still opens Quick Sweep |
| U5 S2 quarantine | Partly | `u05` fails: Settings shows the quarantine size and "Show Quarantine in Finder", but nothing empties it in the app |
| U6 S2 History per cleanup | Fixed | `u06` passes (Undo All per action) |
| U7 S2 Undo for emptied items | Fixed | `u07` passes |
| U8 S2 Ignore without way back | Fixed | `u08` passes; notice "Demo-abc is on the ignore list now. Stop Ignoring on its row puts it back." (`main-u08-after-ignore.png`) |
| U9 S2 skipped items | Partly | `u09` passes: next step per reason, Try Again, Open Privacy Settings (`main-u09-not-moved.png`). Open: "open in a running app" still does not name the app ("Quit the app that uses it") |
| U10 S2 keyboard | Fixed | `u10_return_confirms_the_dialog` and `u10_standard_shortcuts` pass (Return, Cmd+F, Cmd+,) |
| U11 S2 duplicates keeper | Fixed | `u11` passes ("Keep This One" per row) |
| U12 S2 ignore list picker | Fixed | `u12` passes ("Choose Folder...") |
| U13 S3 rescan names | Fixed | `u13` passes (one name on every page) |
| U14 S3 notices and FDA banner | Fixed | both `u14` proofs pass (notices clear on navigation; banner only where needed, with Not Now) |
| U15 S3 action names | Partly | App Updates has "Download Only"; App Remover still "Reset" and "Ignore" (source apps.rs) |
| U16 S2 pointer | Fixed | `u16` passes |
| U17 S2 stale Quick Sweep | Fixed, with a new issue N1 | `u17` passes (System Junk tile follows the cleanup) |
| U18 S3 sort by age | Fixed | `u18` passes |
| U19 S3 raw errors | Fixed | `u19` passes: "CleanCraft needs permission to control System Events. Turn it on in System Settings > Privacy & Security > Automation, then try again." (`main-u19-raw-error.png`); no Open button in that notice (nit) |
| U20 S2 privacy preselection | Partly | the confirmation now says "You will be signed out of websites in ..." (plan_page.rs); cookies are still preselected (core `COOKIES.selected = true`) |
| U21 S3 startup rows | Partly | rows lead with the program name ("ExampleHelper com.example.helper"); "Ad hoc signature (no developer identity)" and Remove beside the switch unchanged (`main-u19-raw-error.png`) |
| U22 S3 Live Monitor duplicates | Open | "Free Up RAM" and "Free Up Space" unchanged (monitor.rs; Free Up Space now has a tooltip) |
| U23 S3 failed tile retry | Fixed | Threat Check tile has "Try Again" (`main-u17-stale-sweep.png`) |
| U24 S3 Disk Map links | Open | "app data in Library (use System Junk or App Remover)" is still plain text |
| U25 S3 "0 skipped" | Partly | plan pages and App Remover use `kit::moved_summary` (no "0 skipped"); Disk Map still prints "{n} moved. {m} skipped." always |
| U26 S3 Settings wording | Partly | slider values named and log scale for sizes; "Let AI agents (MCP) carry out confirmed cleanups" still a checkbox |
| U27 S3 row actions per module | Fixed | Trash rows have no Ignore or Reveal (`u03`, plan_page.rs) |
| U28 S3 Tune-up bottom bar | Fixed | tasks.rs uses `kit::bottom_bar`; `task_11` success in 4 steps |

**Counts on main:** fixed 18 (U1, U2, U3, U6, U7, U8, U10, U11, U12, U13, U14, U16, U17, U18, U19, U23, U27, U28), partly 8 (U4, U5, U9, U15, U20, U21, U25, U26), open 2 (U22, U24). By severity: S1 2 of 2 fixed; S2 9 of 13 fixed, 4 partly (U4, U5, U9, U20); S3 7 of 13 fixed, 4 partly, 2 open.

### New usability problems found on main

**N1 S3 Quick Sweep: a cleaned module becomes an empty tile** (design4: sweep.rs). After cleaning System Junk from its page, the tile reads "0 KB · 0 of 0 items selected" with an empty bar and an active Include checkbox (`main-u17-stale-sweep.png`). Recommendation: "Cleaned just now" with Scan Again, no Include.

**N2 S2 Quick Sweep: the Trash tile does not follow moves into the Trash** (design4: sweep.rs). After 13.3 MB of System Junk moved to the Trash, the Trash Bins tile still says 803 KB (`main-u17-stale-sweep.png`), so the one tile that tells the user how much space Empty Trash would free is wrong right when it matters (ties to U4). Recommendation: refresh the Trash tile from the action log like the module tiles (JournalWatch), and put "Empty Trash..." on it.

**N3 S3 Ignore notice has no Undo button** (design4: plan_page.rs). The notice explains "Stop Ignoring on its row puts it back", and the row action appears only on hover; an Undo in the notice is the shorter path (`main-u08-after-ignore.png`).

**N4 S3 text rule** (design4: plan_page.rs comment near the row actions for U27 uses the word the house rules ban). It is a code comment, not user-facing; reword when touched.

### Earlier passes

| Item | Branch and commit | Result |
|---|---|---|
| U16, U17 | agent4/design4 3645e69 | Fixed then |
| U1, U6, U7, U12 | agent4/ux4 ae18abe | Fixed then; U2 failed then (apps4 core only) |
| Combined | agent4/design4 86e756c | 23 of 37 passed then |
