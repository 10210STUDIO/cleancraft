# Reliability review (reviewer: reliability)

Branch `agent/reliability`, from main at 0a81073 (every module merged). Only two files added:
`crates/core/tests/review_reliability_core.rs` and `crates/ui-egui/tests/review_reliability_ui.rs`.
No src file changed, no shared file changed.

Run the checks: `cargo test -p cleancraft-core --test review_reliability_core` and
`cargo test -p cleancraft-ui-egui --test review_reliability_ui`; the defects with `-- --ignored`
(each fails at its main assertion for the reason in its ignore text; verified).

## Defects

| ID | Sev | Row | Test | Steps | Expected | Actual |
|---|---|---|---|---|---|---|
| REL-1 | S1 | S3, U2 | core `truncated_journal_line_does_not_swallow_the_next_move` | journal.jsonl ends with a line cut short (crash or power loss during a write), then run any cleanup | every move has a History line and can be undone | the first new record is appended to the damaged line, both are unreadable: the item is in the Trash with no History entry and no Undo (`append_journal` does not start on a fresh line) |
| REL-2 | S2 | S3, U2 | core `undo_keeps_journal_lines_it_cannot_read` | journal holds a line CleanCraft cannot parse (damaged, or written by a newer version), Undo any item | the action log is kept | `undo` rewrites the whole journal from the parsed records, every unreadable line is deleted for good |
| REL-3 | S2 | S3, U5, U6 | core `same_plan_run_again_after_undo_can_be_undone_again` | run a saved plan, undo it, run the same plan again (CLI `clean <id> --yes` twice, MCP `execute_plan` twice, Quick Sweep Run after a dead job), undo | items put back | action ids are `<plan>#<item>` and repeat; `undo` finds the first (already undone) record: "already put back" for all 7 items, they stay in the Trash |
| REL-4 | S1 | S1, S10 | core `mcp_only_items_cannot_reach_beyond_the_confirmed_selection` | MCP `execute_plan(plan_id, digest of the shown selection, only_items=[an item that was not selected])` | refused: not part of the confirmed list | the digest is checked before `only_items` is applied, then the plan runs with the digest of the new selection: the unconfirmed item is moved. The test replays `crates/mcp/src/lib.rs` `execute_plan` step by step (reviewers may not add tests to the mcp crate) |
| REL-5 | S1 | S3 | core `undo_refuses_a_different_file_at_the_stored_place` | clean an item, Finder empties the Trash, another item with the same name goes to the Trash, Undo | "the removed copy is gone" | Undo moves the unrelated file out of the Trash into the original folder (no size, inode or time check of the stored copy) |
| REL-6 | S2 | L4, P2, S9 | core `agent_is_not_stopped_when_its_file_is_not_moved` | Startup Items: select a user launch agent; its plist cannot move (permission denied; same for file in use, disk full) | the agent keeps running when its file stays, or the stop can be undone | `prepare()` runs `launchctl bootout` before the executor skips the file; the journal has only a Command line, which cannot be undone. Same code path for Threat Check |
| REL-7 | S2 | S1, J21 (every plan page) | ui `confirm_dialog_with_thousands_of_items_lays_out_only_visible_rows` | System Junk with 3000 selected items, Review and Clean | the confirmation list virtualised like the item list | `plan_page::confirm` builds one Grid row per selected item every frame (3000 rows); big plans (Large & Old on a full disk) stall the window while the dialog is open |
| REL-8 | S2 | S11, U3 | ui `damaged_settings_are_reported_and_the_ignore_list_survives` | settings.json truncated (crash, full disk) while it holds an ignore list; open the app, scan System Junk | a warning, and the ignore list is not silently dropped | `Settings::load` returns defaults with no notice; the ignored item is proposed and preselected again; the next save overwrites the old file (one wrong field also drops the whole file) |
| REL-9 | S2 | S2 | core `longest_name_already_in_the_trash_still_moves` | a 255-byte name (APFS limit) that already exists in the Trash | moved (shorter free name, or the quarantine) | `free_name` appends " 2", rename fails "File name too long (os error 36)", item skipped |
| REL-10 | S2 | S9 | ui `dead_cleanup_job_is_reported` | the cleanup worker thread dies (any panic in `run_plan` or a feature's `prepare`) | an error and the list back, like the scan, Quick Sweep and App Remover pages do | `plan_page::poll` ignores `is_dead()` for Cleaning: "Moving the selected items..." spins forever with no way back (the test injects the dead job into the page state, the only way to make a worker panic on demand) |

## Checked and found right (passing tests, keep them)

Core (`review_reliability_core.rs`):
- `file_deleted_between_scan_and_clean_is_reported_and_others_move`: "already gone", the rest moves, one journal line per move.
- `item_replaced_by_a_symlink_moves_only_the_link`: TOCTOU swap to a link; only the link moves, its target is untouched, freed counts the link only.
- `parent_replaced_by_a_symlink_is_refused`: a folder on the path becomes a link: refused, nothing moves.
- `grown_file_counts_its_size_at_move_time`: freed and the journal size are measured at move time.
- `running_a_plan_twice_moves_nothing_the_second_time`: double execution moves and logs nothing more.
- `trash_emptied_before_undo_is_explained_and_changes_nothing`, `undo_recreates_a_missing_parent_folder`, `undo_twice_is_refused_the_second_time`, `undo_when_the_place_is_taken_keeps_both`.
- `damaged_lines_do_not_hide_the_good_ones`: garbage and NUL lines are skipped by History and Undo.
- `journal_disk_full_moves_nothing_and_logs_nothing`, `startup_journal_unwritable_stops_no_agent`: log unwritable, nothing moves and no launchctl runs.
- `stale_digest_is_refused_before_anything_runs`: no bootout before the digest check.
- `damaged_settings_fall_back_to_safe_defaults`: a damaged file never lets agents clean (fail safe for S10).
- `damaged_or_forged_saved_plans_are_refused`: truncated plan file, `../` ids, unknown ids refused; a forged path into Documents is skipped by the fence.
- `non_utf8_name_is_never_lost`: such a name either moves and undoes, or stays in place.
- `many_files_in_one_tree_move_as_one_item_with_exact_size`: 10,000 files, exact size, one rename.

UI (`review_reliability_ui.rs`, real background threads where it matters):
- `item_list_of_thousands_lays_out_only_visible_rows` (3000 items, under 100 rows built, total matches disk).
- `switching_pages_while_scanning_keeps_the_result`.
- `move_to_trash_cannot_be_clicked_twice`: the button is gone the frame after the click; one journal line per item, no repeated ids.
- `scan_button_is_gone_while_scanning_and_undo_runs_once`.
- `history_undo_after_trash_emptied_says_why`.
- `ignore_and_history_survive_a_restart`: ignore list and History persist into a new window; Undo from History after restart works.

## Notes (not filed as defects)

- Plan pages have no Stop while scanning (only Disk Map has one; the scan's cancel flag is never set). FEATURES does not promise it; switching pages keeps the app usable.
- `RealSys::run` has no timeout (a hanging `lsof`, `codesign`, `osascript` blocks the job); fix1 announced a RealSys timeout, so not filed.
- `RealSys::open_files` ignores the `lsof` exit status. If `lsof` prints nothing (killed, sandboxed) the executor sees "nothing open" and moves files that may be open (S6). Cannot be tested without running the real tool; see hand checks.
- Undo marks the record only after the move back; if the journal rewrite then fails the item is back but History still offers Undo (later answer "something is already at ..."). Not reproducible with the current fault injection.
- `plans::save` writes in place (no temp file and rename like settings); a crash leaves a damaged plan, which `load` refuses cleanly.

## Real Mac only

- Volume Trash and quarantine across volumes: clean an item on an external disk with the quarantine method; `rename` across volumes fails with EXDEV ("Cross-device link"); expect it reported as skipped, nothing lost.
- `lsof -F n -w` with a file open in Preview: `lsof -F n -w | grep '^n/Users/<you>/Downloads/x.pdf'` must print the path, and Clean must skip it with "open in a running app".
- Permission denied mid-run for real: `sudo chown root ~/Library/Caches/<some app>/x; sudo chmod 700 ~/Library/Caches/<some app>`, clean that cache: expect "permission denied", nothing else affected; undo the chown afterwards.
- Trash emptied before Undo: clean, empty the Trash in Finder, Undo in History: expect "the removed copy is gone ... (was the Trash emptied?)".
- Admin password dialog cancelled during a Tune-up Task: expect "nothing changed", no journal line.
