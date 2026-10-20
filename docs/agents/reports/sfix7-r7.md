# sfix7, round 7: fixes of release check 7's safety findings

Branch agent7/sfix7 (main dc7e109 + agent7/check7). Owner of policy.rs, executor.rs, fsutil.rs and the confirmations of Empty Trash, Erase in Mail and Move Quarantine to Trash.

## Fixed

### CHK7-1 (High): links above an allow-list folder
- `policy::check` now refuses an item when ANY folder on the way to it is a symbolic link: from the env root (the disk root on a Mac) down to the item's parent, the allow-list folder itself included (`policy::link_above`). The item itself may still be a link (moving a link never moves its target). macOS's own `/var`, `/tmp`, `/etc` links into `/private` are not counted; a path through them is checked again on its real place.
- The refusal names the real place: `protected: the contents of Documents, reached through the link /Users/x/.npm` (Documents, Desktop, iCloud Drive, cloud storage, Photos libraries, the home folder, or a place outside the env root count as protected); a link to a safe place is `reached through a symbolic link (...)`.
- At scan time (`features::scan_with`, every feature): each allow-list folder that is a link or lies under one is listed as ONE locked row (category `linked`, "Reached through a link") on the link, with a plain reason and a next step (`policy::link_lock_reason`: "~/.npm is a link into the contents of Documents (~/Documents/store). CleanCraft never cleans through a link, so nothing in it is moved. Clear it with the app or tool that made it, or make ~/.npm a real folder again, then scan again"). Not added when the scan already lists something under that link with its own lock.
- At move time the executor's `check_move` runs the same fence: a folder swapped for a link after the scan is refused, nothing moves.

### CHK7-2 (Medium): a folder holding an ignored item
- Shared run check (`features::validated`, used by every front end through `run_plan`): an item that holds an ignore-list entry is refused, "it holds <path>, which is on your ignore list" (`policy::ignored_inside`).
- Scan time (`features::scan_with`, every feature): such a folder is narrowed to what is in it minus the ignored item (folders on the way to it opened too, up to 8 levels), when the feature accepts those parts (`Feature::validate` per part, `validate_set` on the result). Otherwise it stays listed, locked: "It holds <path>, which is on your ignore list, so it never moves as a whole. Take it off the ignore list in Settings, or remove the other items yourself in Finder". Disk Map narrows; System Junk cache folders and app bundles lock.

### CHK7-3: Empty Trash and Erase in Mail confirmations list what is erased
- Empty Trash: every entry of every Finder Trash with its size and its Trash ("Trash of USB"), in a scrollable list, with "Mail and Finder erase these for good; CleanCraft cannot undo it." If the Trash changed after the list was shown, Finder is NOT asked; the new list is shown with a note, and the person confirms again.
- Erase in Mail: each mailbox of the account ("Deleted Messages of iCloud"), its message count and size, the same warning.

### CHK7-5 and CHK7-6
- Move Quarantine to Trash lists every item with its size (snapshot taken when the dialog opens) and moves exactly that list (`executor::quarantine_list`, `executor::move_quarantine_items_to_trash`); an item that came in afterwards stays in the quarantine.
- Trash and quarantine moves are collision-proof: `fsutil::rename_no_replace` (macOS `renamex_np(RENAME_EXCL)`, Linux `renameat2(RENAME_NOREPLACE)` through `rustix`, so no unsafe code of ours; a check right before a plain rename on volumes without the flag) and `fsutil::move_unique` (a taken name is retried with a fresh one, up to 64 times). Used by `move_one`, the quarantine-to-Trash move, every put-back after a log failure, and Undo (a place taken in between is `OriginalTaken`, never replaced).

### DV1
`dev_clutter::root_lock`: a code folder reached through a link into Documents says "Your code folder ~/Work/code is reached through the link ~/Work, which leads into the contents of Documents (~/Documents/Work/code). CleanCraft never moves anything from there. Move the project out of Documents and Desktop (for example to ~/Projects) to clean it here"; a link to a safe place says "Add the real folder <real> under Developer Clutter in Settings instead, then scan again" (tested to work).

### CHK7-4
Unchanged by request; FEATURES.md K3, K4, K9, P6 now say they are confirmed with the exact command, logged and cannot be undone.

## Tests
- check7 un-ignored and passing: core `check7` (5), ui `check7_ui::chk7_3_empty_trash_confirmation_lists_the_exact_items`.
- New core `tests/sfix7.rs`: chk7_1_dot_folders_linked_anywhere_are_never_cleaned (`.npm`, `.cargo`, `.gradle`, `.lmstudio`, `.cache` each linked into Documents, Desktop, iCloud Drive, a Photos library and a safe place outside the allow-list: fence, locked row with reason and next step, a run with everything selected moves nothing), chk7_1_a_link_made_after_the_scan_is_refused_at_move_time, chk7_1_an_allow_list_folder_that_is_a_link_is_refused, dv1_code_folder_through_a_link_says_why_and_what_to_do, chk7_2_disk_map_offers_the_other_items_never_the_ignored_one, chk7_2_a_cache_folder_holding_an_ignored_file_is_locked_or_narrowed_never_moved, chk7_2_disk_map_plan_of_a_parent_is_refused_with_the_reason, chk7_5_quarantine_moves_only_the_confirmed_list, chk7_6_trash_move_never_replaces_and_undo_works.
- fsutil unit tests: rename_never_replaces, move_unique_retries_when_the_name_is_taken_in_between (the collision path).
- New ui `tests/sfix7_ui.rs`: empty_trash_lists_items_and_refuses_a_changed_trash, erase_in_mail_lists_mailboxes_with_counts, quarantine_confirmation_lists_and_moves_only_those_items.
- Full core crate: 515 passed, 0 failed. clippy -D warnings and fmt clean (workspace).

## FEATURES.md
Applied all 22 check7 downgrades with the checker's reason, then re-upgraded only rows whose gaps are closed by tests that pass on this branch (main with qa7 merged): S4, S5, S11, J17, J19 by sfix7 (CHK7-1, CHK7-2); Q2, Q4, DV3, DV4, DV5, DV6, DV8, A6, A13, A14, C1, C2, C3, D1 by qa7's error-case UI tests (docs/agents/reports/qa7-r7.md); D3 by both (qa7 disk full, sfix7 CHK7-2). Existing MAC-CHECK notes kept (A13, C1, C3, D3). W2 and W5 stay MAC-CHECK. Notes added to S2, DV1, T2, T3, and to K3, K4, K9, P6 (confirmed, logged, cannot be undone: CHK7-4 kept as is). Totals: 62 WORKS, 77 MAC-CHECK, 11 PARTIAL, 2 EXCLUDED. qa7's 24 tests, check7_ui and sfix7_ui all pass here (28 tests).

## Shared-file changes
- `Cargo.toml` (workspace dependency `rustix` 1.1, features std+fs, already in Cargo.lock through tempfile) and `crates/core/Cargo.toml` (`rustix`).
- `crates/core/src/features/mod.rs`: `scan_with` narrows/locks folders holding ignored items and lists linked allow-list folders (`CAT_LINKED`, `ignored_inside_reason`); `validated` refuses a folder holding an ignored item.
- `crates/core/src/features/dev_clutter.rs`: `root_lock` (DV1), used for each code folder.
- `crates/ui-egui/src/pages/extras_cleanup.rs` (confirmations, `finder_trash_entries`, `erase_list`, `ERASE_NOTE`, `CHANGED_NOTE`) and `pages/settings.rs` (`confirm_quarantine` only).
- `crates/core/tests/check_safety.rs::check_symlinked_allow_root_is_never_followed`: the assertion now allows the one locked row on the link itself (still nothing proposed through it).

## Hand checks on a real Mac
1. `renamex_np(RENAME_EXCL)` on APFS and on an exFAT/FAT USB drive (falls back to check-then-rename when unsupported): move an item to the Trash when the Trash already holds one with the same name.
2. A real `~/.npm` or `~/.cache` link into Documents: System Junk shows the locked "Reached through a link" row and offers nothing in it.
3. `/var` paths (`/private/var/log` is the allow-list folder; nothing goes through `/var`) and the `/Volumes/Macintosh HD` link to `/` in Disk Map: refused as a link.
4. Empty Trash with a full Trash (thousands of entries): the list scrolls and opens quickly.
