# apps6, round 6: uninstall several apps at once

User feedback: "here I cannot select the apps I want to remove, I need to go one by one?" The App Remover list only opened one app's detail.

## What was built

### List (pages/apps.rs)
- A checkbox on every app row ("Select <App>"); the row click still opens the detail. Cmd-click toggles, Shift-click ticks the range from the last ticked app (the range takes the anchor's state), Space on the focused row ticks it (Return still opens it). Rows have stable ids so the focus survives the selection bar appearing.
- Apps that are part of macOS: a padlock (disabled checkbox) with the reason on hover; never ticked by any path (checkbox, Cmd/Shift-click, group select all).
- Group rows use plan_page::category_row's existing tri-state checkbox ("Select all in <group>"). All Apps never ticks in one click: "Select all N apps?" with the count of removable apps. File groups (Leftovers, Installers, Extensions) tick their files.
- Bottom bar while apps are ticked: "3 apps selected, 4.2 GB with their leftovers" (bundle plus the files named after each app's id, measured in a background job, "measuring" until done), Review and Uninstall, Clear Selection.

### Review (core uninstaller::batch_plan + page)
- One plan built from each app's own uninstall plan (same preselection), every item tagged with its app (`Item::group = "app:<bundle path>"`), plan id prefix `uninstaller-batch`. A file two apps list appears once (first app; selected if either selects, locked if either locks). Apps of macOS and ignored apps are refused.
- Grouped by app (biggest first), each group foldable ("Hide files of X"), per-app selected count and size, "needs password" note, Leave Out per app, totals in the header and the bar ("N selected from K apps").
- Running apps: only their group is locked, with the per-app quit banner (headline and helpers from app_quit::running_parts, Quit X, Try Again, Force Quit with confirmation, the password question for system services). Quit All when two or more run. A running check every 1.5 s unlocks a group as soon as its app quits (selection defaults restored for it, choices kept for the others).
- One confirmation listing every file under its app; the administrator note once (the executor moves every admin item of the plan with one dialog); which running apps stay.

### Run and result
- core uninstaller::run_batch remembers the app groups (data_dir/uninstall-batches.json, last 100 batches) then runs features::run_plan. Uninstaller::validate refuses per item the files of an app of a batch that runs (by group, or by bundle path/bundle id read from disk when an edited plan drops the groups); prepare no longer refuses a whole batch for a running app whose items are not selected (a single-app plan still refuses as before).
- One result screen: per app Uninstalled / Not removed / Kept, size moved, every file that stayed with its reason; Undo All (executor::undo_plan, one action) and Undo per app (uninstaller::undo_batch_app). History: the batch card shows one line per app with its own Undo next to the card's Undo All.
- The list updates at once (forget_moved also drops ticked apps that are gone); undo brings them back through the journal watch and a new look.

### Sort by size
Sizes are the whole bundle (`tree_size`, allocated blocks) and the list sorts biggest first; iMovie at the top of the user's screenshot is consistent with that (it is usually the biggest bundle). Tested: core apps_batch::app_sizes_include_the_whole_bundle_and_sort_biggest_first, ui qa_apps_batch::keyboard_and_pointer_selection. No change needed.

### Also this round
- Merged agent6/watch6 (it changed uninstaller.rs first): watch6's add_leftover_categories / push_leftovers kept, my plan_for calls them; the single-app plan is unchanged.
- health6's ask: open_app on an app already in the Trash (drag-to-Trash offer route) builds watch::trashed_app_plan; the detail shows "X is already in the Trash..." and a Remove Leftovers bar (no Uninstall or Reset). Test: ui qa_apps_batch::an_app_already_in_the_trash_offers_its_leftovers_only.
- safety6 R4/R5: CleanCraft itself is never tickable or in a batch (uninstaller::is_cleancraft, refused by batch_plan); select all while searching ticks only the apps shown. Tests: core apps_batch::cleancraft_itself_is_never_in_a_batch, ui qa_apps_batch::select_all_while_searching_ticks_only_the_apps_shown_and_never_cleancraft.
- The result screen says why a running app was not removed ("Not removed: it was running. Quit it, then uninstall it.").
- Renders: docs/screens/v2/apps6/batch-{list,review,confirm,done}-{1060x680,1280x820}.png (CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test qa_apps_batch renders).

### SAF6-8 (safety6): an edited uninstall plan could take another app's files
Fixed in core (uninstaller::own_files_only, called from Uninstaller::validate_set, so nothing moves when it fails): for an uninstall, reset or Trash-leftovers plan, right before anything moves, every selected item must be one of the plan's own apps (one app in a plan for one app; in a batch each app item must be the app its group names; every app must be installed in the Applications folders) or a path find_leftovers returns for those apps now, read again from disk (paths compared canonically). A plan without an app item (app already in the Trash) takes only the leftovers of the apps in ~/.Trash. Anything else: "<path> is not a file of the apps in this plan, so this list was not made by CleanCraft for these apps. Nothing was moved." The feature's own scan (leftovers of removed apps, installers, extensions) is not affected. Tests: safety6's review_safety6_apps::a5_edited_batch_plan_cannot_take_other_apps_files and ::a5b_edited_single_plan_cannot_take_other_apps_files (merged from agent6/safety6, #[ignore] removed, passing), core apps_batch::an_edited_plan_may_only_take_its_apps_own_files; an edited batch plan with its groups stripped is now refused as a whole (apps_batch::an_app_opened_after_the_review_keeps_its_files_and_the_others_go).

Merged main (fix6 + watch6) at the lead's request.

## Tests
- core crates/core/tests/apps_batch.rs (11): union of single plans and numbers vs disk, apps of macOS refused (also edited plan), running app locks only its group, app opened after the review (also with groups stripped), one admin dialog plus undo, undo one app then the rest, permission denied and in use per item, shared file once, bundle sizes and sort.
- ui crates/ui-egui/tests/qa_apps_batch.rs (12, one of them the renders): e2e scan, tick 3, bar size vs disk, review, fold, confirmation lists every file, all gone, results, Undo per app, Undo All, counts recalculated; History per-app and Undo All; Apple apps not selectable (checkbox, Cmd-click, select all count, pushed into the set by hand); keyboard Space, Cmd/Shift-click range, plain click opens, pointing hand on the checkbox, sort order; running apps and Quit All (one quits, one does not, Force Quit confirmation), Force Quit after confirmation; admin apps in one prompt; permission denied and file in use per file; Leave Out and Leftovers select all.

## Shared-file changes (outside my files)
- crates/core/src/executor.rs: new `pub fn undo_actions(env, ids)`; undo_plan now collects its ids and calls it (same behaviour: admin moves go back with one dialog). Nothing in the move path changed.
- crates/ui-egui/src/pages/history.rs: one line in group_card calling `crate::pages::apps::history_batch_rows` (shows nothing for other plans).
- No kit.rs or app.rs change.

- crates/ui-egui/tests/icons_ui.rs: the two pixel helpers sample the app icon 62 pt into the row (it moved right of the new checkbox); no assertion changed.
- crates/core/tests/review_safety6_apps.rs (safety6's): the two #[ignore]s removed as asked, one clippy fix (slice::from_ref).

## Gate
Workspace tests green, run in chunks because of the shared disk: core all 38 targets, ui-egui every test target (8 per chunk) plus lib, mcp and the app; clippy --workspace --all-targets -D warnings and fmt --check clean. target/ deleted.

## Check on a real Mac
- Ticking 3 apps and the bar size; Review with one running app (Quit and Force Quit), two package-installed apps giving one password dialog, Undo All from History giving one password dialog.
- Shift-click and Cmd-click feel (Cmd is the Command key on macOS in egui).
