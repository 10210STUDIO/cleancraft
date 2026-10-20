# Release check 7 (independent checker)

Base: main dc7e109, branch agent7/check7. I read the code and the tests; I changed no app code. Everything below was checked against the user's brief: a preview with exact files, sizes and reason before anything is removed; Trash or restorable quarantine only; every action logged with undo; a hard allow-list per feature; never SIP files, iCloud, Photos libraries, Documents or Desktop contents, or files open in running apps; WORKS only with an end-to-end UI test (clicks and typing) that checks the files moved, the numbers against a recalculation, and the errors permission denied, file in use and disk full.

New tests: `crates/core/tests/check7.rs` and `crates/ui-egui/tests/check7_ui.rs`. Each defect has an `#[ignore]` test that fails today:

```
cargo test -p cleancraft-core --test check7 -- --ignored
cargo test -p cleancraft-ui-egui --test check7_ui -- --ignored
```

## Summary

- 64 rows say WORKS. I agree with 42 and downgrade 22 (20 to PARTIAL, 2 to MAC-CHECK).
- Safety findings: 1 High (CHK7-1), 2 Medium (CHK7-2, CHK7-3), 3 Low or decisions for the user (CHK7-4 to CHK7-6). No `remove_file`, `remove_dir_all`, `rm` or `unlink` in app code; every removal goes through `features::run_plan` and `executor::execute_with`; administrator moves and every undo command quote file names (`shell_quote`, `as_quote`, argv for osascript); no background code moves or changes anything.
- Test runs: see "Tests run" at the end.

## Most common reason for a downgrade

The brief names three error cases. Disk full is driven by a UI test only on System Junk, Mail Downloads, Privacy and Trash Bins (`inject_fault(.., Fault::DiskFull)`). Quick Sweep, Duplicate Downloads, Developer Clutter, App Remover (batch and installers), Large & Old, Duplicates, Similar Images and Disk Map never drive it, and Duplicates and Similar Images drive no error case at all. The executor code is shared, so the fix is mostly one more test per page, but by the contract these rows are PARTIAL until then.

## Verdict per WORKS row

| ID | Verdict | Why |
|---|---|---|
| S1 | agree | `plan_page::confirm` lists every selected file with its size and reason; digest checked in `run_plan_with`; qa_cleanup_junk and qa_dev_clutter check the list. Empty Trash is outside this rule today, see CHK7-3 |
| S3 | agree | Journal before every move, move put back when the line cannot be written; History undo per item and per cleanup driven by clicks; undo after the Trash was emptied explained (review_reliability_ui::history_undo_after_trash_emptied_says_why) |
| S4 | downgrade to PARTIAL | CHK7-1: `policy::check` checks links only from the item up to the allow-list folder, never above it, so an allow-list folder reached through a linked parent (a linked `~/.npm`, `~/.cargo`, `~/.gradle`, `~/.lmstudio`, `~/.cache`) lets items that live elsewhere move |
| S5 | downgrade to PARTIAL | Same hole: the Documents contents behind such a link are moved (proved for System Junk). `protected_reason` compares the path as written, not the real place on disk |
| S8 | agree | `.cargo/config.toml` sets CLEANCRAFT_FORBID_REAL; all commands through `env.sys`; tests use FakeMac |
| S9 | agree | qa_cleanup_junk::errors_are_shown_per_item_and_log_failure_moves_nothing drives all three errors and the unwritable log |
| S10 | agree (note) | The gate is the setting plus the digest; the digest is returned by `scan` to the same agent, so it proves only that the list was not changed, not that a person saw it. Documented; the stdio transport is untested (U6) |
| S11 | downgrade to PARTIAL | CHK7-2: `Settings::is_ignored` refuses an item on the list or inside an entry, never a folder that holds an entry, so an ignored file moves with its parent folder |
| Q1 | agree | Tiles against each plan recalculated with `tree_size`, a failing module shown |
| Q2 | downgrade to PARTIAL | qa_sweep drives cancel and file in use only; permission denied and disk full on the Quick Sweep result screen are not driven; most Run checks use stand-in modules (said in the row) |
| Q3 | agree | Saved, skipped module untouched on disk |
| Q4 | downgrade to PARTIAL | qa_files_r6 drives file in use only; permission denied and disk full not driven |
| Q6 | agree | Route only scans, journal checked empty |
| J1, J3, J5, J9, J12, J13, J14, J16, J18, J21 | agree | qa_cleanup_junk: rich fixture with every category, totals and category sizes recalculated, every selected path gone and every unselected one kept, Undo, all three errors and the unwritable log |
| J17 | downgrade to PARTIAL | CHK7-1 proved on this row: with `~/.npm` linked into `~/Documents`, a default System Junk clean moves Documents files (core check7::chk7_1_system_junk_never_moves_documents_contents_through_a_linked_dot_folder). Same for `~/.cargo` and `~/.gradle` |
| J19 | downgrade to PARTIAL | Same hole for `~/.lmstudio/models` and `~/.cache/lm-studio/models` (model folders are often links to other places); `~/.ollama` is safe because the allow-list folder is `~/.ollama` itself |
| T1 | agree | Display rows; sizes against disk in qa_cleanup_trash and qa_trash_mail |
| DV1 | agree (note) | Settings flow driven by clicks. A code folder under a linked parent pointing into Documents is safe: its build folders are listed locked and never move (guard core check7::chk7_1_developer_clutter_never_moves_documents_contents_through_a_linked_parent), but the lock reason says "Not inside one of your code folders. Add its folder...", which is wrong and gives no working way forward (UX rule): it should say the folder is reached through a link into Documents |
| DV2 | agree | Active project listed unselected and not moved, checked on disk in qa_dev_clutter |
| DV3, DV4 | downgrade to PARTIAL | qa_dev_clutter drives permission denied and file in use; disk full not driven |
| DV5 | downgrade to PARTIAL | Python environments are only in core tests; no UI test scans, lists or moves one |
| DV6 | downgrade to PARTIAL | Next.js, Gradle and `.gitignore`d dist/build only in core tests |
| DV8 | downgrade to PARTIAL | Core only; the locked rows and their way forward are not driven in the UI |
| A6 | downgrade to PARTIAL | qa_apps_remover::leftovers_and_installers_groups moves and recalculates, but drives no error case and no Undo |
| A12 | agree | Selection only; ticking, ranges, macOS apps locked, the bar's numbers, all by clicks |
| A13, A14 | downgrade to PARTIAL | Permission denied and file in use per file are driven (qa_apps_batch::permission_denied_and_file_in_use_are_shown_per_file); disk full is not |
| A15 | agree | Opens on the leftovers only, no move without the normal confirmation |
| C1 | downgrade to PARTIAL | Permission denied and file in use driven; disk full not driven |
| C2 | downgrade to PARTIAL | qa_files_duplicates drives no permission denied, file in use or disk full; the keep-one rule is well tested |
| C3 | downgrade to PARTIAL | Same as C2, and no Undo driven |
| D1 | downgrade to PARTIAL | Numbers against `tree_size` very well checked; permission denied and file in use driven; disk full not driven |
| D3 | downgrade to PARTIAL | As D1, plus CHK7-2 (a folder holding an ignored item is selectable and moves it) |
| D7 | agree | Unreadable folders said in words, never zero |
| D8 | agree | Data only; shown and driven by H8 |
| U1, U2, U3, U8 | agree | Driven by clicks; U8 routes checked to never clean |
| M8 | agree | Safety property: agent code calls only scan, save, notify, open (checked by reading `agent.rs`, `schedule.rs`) |
| W2, W5 | downgrade to MAC-CHECK | Logic tested in core on inotify only; FSEvents delivery, coalescing on a real Mac and the notification on screen are unverified, same as W1 |
| W6 | agree | Safety property: `watch.rs` never calls the executor, tests check the journal |
| H1, H2, H3, H5, H6, H7, H8, H9, H11, H14, H16 | agree | Facts and cards only, opened by clicks, journal checked empty; nothing is removed from these pages |

Rows already PARTIAL, MAC-CHECK or EXCLUDED: I read them and found them honest. Two notes: S2 stays PARTIAL (the quarantine fallback is untested); T2, T3 and the tune-up tasks are honest about running commands that cannot be undone, see CHK7-3 and CHK7-4.

## Safety findings

### CHK7-1 (High): the fence does not look for links above an allow-list folder

`policy::check` walks from the item's parent up to the allow-list folder and refuses a link on that stretch. It never looks above the allow-list folder, and `protected_reason` compares the path as written. So when an allow-list folder is reached through a linked parent, items that really live in Documents, Desktop, iCloud Drive or a Photos library pass the fence and are moved (to the Trash, restorable, but the brief says never).

Real cases: `~/.npm`, `~/.cargo`, `~/.gradle`, `~/.lmstudio`, `~/.cache` linked to another folder (common for big caches and models). Proved: with `~/.npm` linked to `~/Documents/npm-store`, a default System Junk clean moved `~/Documents/npm-store/_cacache/content-v2` to the Trash. Developer Clutter was tried the same way (code folder under a linked parent) and is safe: the item is locked, with a misleading reason (see DV1).

Tests (fail today):
- core check7::chk7_1_fence_refuses_an_allow_root_reached_through_a_link_into_documents
- core check7::chk7_1_system_junk_never_moves_documents_contents_through_a_linked_dot_folder

Suggested fix (owner of policy.rs, with the lead's approval because STANDARD forbids weakening it, this strengthens it): in `check`, refuse when any folder between the env root and the item is a link, or canonicalize the item's parent and run `protected_reason` on the real path as well.

### CHK7-2 (Medium): a folder that holds an ignored item is not refused

`Settings::is_ignored(p)` is true when `p` is on the list or inside an entry. `run_plan` checks only that, so selecting a folder that contains an ignored entry (Disk Map, Large & Old folders at the top of Downloads, a System Junk cache folder, a Developer Clutter build folder) moves the ignored entry with it. The brief's user expects an ignored item never to be touched.

Test: core check7::chk7_2_a_folder_holding_an_ignored_item_is_not_moved (fails today).

Suggested fix: in `features::validated`, also refuse an item when an ignore entry lies inside it (`starts_with_ci(entry, item)`), with the reason "it holds an item on your ignore list".

### CHK7-3 (Medium, decision for the user): Empty Trash removes for good without the exact list

"Empty Trash in Finder" asks Finder to remove everything in every Trash for good. CleanCraft's confirmation says so plainly and the command is journaled, but it shows only the total, not the exact files, and it cannot be undone. Erase in Mail (T3) is the same kind of action. This contradicts two rules of the brief (preview with exact files, never a permanent delete); the team documented it as a design choice, so the user should decide: keep it (with the exact list), or remove the button and leave emptying to Finder.

Test: ui check7_ui::chk7_3_empty_trash_confirmation_lists_the_exact_items (fails today: no file is listed).

### CHK7-4 (Low, decision for the user): commands that cannot be undone

Tune-up tasks K3 and K4 delete local Time Machine snapshots for good (`tmutil thinlocalsnapshots`, `tmutil deletelocalsnapshots`), K9 rewrites Mail's index in place (`sqlite3 VACUUM`, no copy kept), P6 resets permissions (`tccutil reset`). All are confirmed with the exact command and journaled, none can be undone. The brief asks for undo on every action; the user should accept these explicitly or drop K3/K4. No test: it is a scope decision, not a code defect.

### CHK7-5 (Low): Move Quarantine to Trash confirms a total only

Settings > Move Quarantine to Trash shows "N items, size", not the list. Nothing is lost (Trash, History Undo), so Low.

### CHK7-6 (Low): name chosen, then rename

`executor::move_one` picks a free name with `free_name` and then calls `rename`, which on macOS replaces anything that appears at that name in between (another app writing into the Trash at the same moment). Rare and only in the Trash or quarantine. A no-replace rename needs `renamex_np(RENAME_EXCL)` (a system call, so outside the no-unsafe rule) or a check after the move. No test (needs a race).

### Checked and found right

- Grep of app code: no `remove_file`, `remove_dir_all`, `rm`, `unlink`, `truncate` of files, `shred`. Docker prune and `brew cleanup` exist only as text to copy; `simctl delete` is never run.
- Administrator moves: one `/bin/mv -n -- 'from' 'to'` per item, `shell_quote` then AppleScript escaping; names with quotes, `$`, backticks and newlines stay literal. Launch item, login item, uninstall stop and MCP setup undo commands quote every path and label. Notifications pass texts as argv. Snapshot names are filtered to digits and dashes before they reach the administrator shell.
- One way in: every removal (UI pages, Quick Sweep, Disk Map, App Remover, CLI, MCP) goes through `features::run_plan` (digest, feature `validate`/`validate_set`, ignore list, allow-list, `check_move`, open files via lsof, running apps). An lsof failure moves nothing.
- Background: the menu bar agent, schedules, watchers, Mac Health and launch checks only scan, save plans, write CleanCraft's own data, notify or open a page.

## Hand checks on the real Mac (most important first)

1. CHK7-1 on your own setup: `ls -la ~ | grep -- '->'` and `ls -la ~/.cache ~/.cargo ~/.npm ~/.gradle ~/.lmstudio 2>/dev/null`. If any of these is a link, do not run System Junk's developer caches or local models until CHK7-1 is fixed.
2. A file open in Preview is skipped (H-S2), with the real `lsof` speed on a full Mac.
3. Empty Trash: check what Finder really does from the script (does it ask?) and decide on CHK7-3.
4. Undo from History after a System Junk clean, then after emptying the Trash (should say why it cannot).
5. Disk Map on a Pictures folder holding the Photos library: it must stay locked.
6. Threat Check administrator move and its Undo with the password dialog.
7. Menu bar agent: build and look at `tray.rs` (never compiled), then turn it off and check the plist went to the Trash.

## Tests run

- `cargo test -p cleancraft-core -p cleancraft-ui-egui` (debug info off, 2 threads): 102 test binaries, 888 passed, 0 failed, 22 ignored (the 4 ignored check7 tests included). Every test named in a WORKS row exists (checked by script against the sources) and passes.
- `cargo test -p cleancraft-core --test check7 -- --ignored`: chk7_1_fence..., chk7_1_system_junk... and chk7_2... fail as described.
- `cargo test -p cleancraft-ui-egui --test check7_ui -- --ignored`: chk7_3 fails ("old.zip is not listed in the confirmation").
- mcp and apps/cleancraft were not run here (the lead runs the full suite); my verdicts on S10, U5, U6 and M-rows come from reading their tests.
