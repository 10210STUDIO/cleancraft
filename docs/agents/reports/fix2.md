# fix2 report: safety core (DOM-1..DOM-12) and executor/journal reliability (REL-1..5, REL-9)

Branch `agent/fix2`, merged with `main` and `agent/fix1` (fix1's FIX-*, DAT-*, REL-6 included).
Every defect below had a failing (ignored) test written by a reviewer; each ignore marker was
removed only after the test passed against the fix described.

## Defects

| Defect | Root cause | Fix commit | Test now passing |
|---|---|---|---|
| DOM-1 S1 | `policy` compared protected places byte for byte; on a default (case-insensitive) Mac `~/documents` is `~/Documents` | b85b568 | `review_domain_safety::dom1_case_variant_of_documents_is_still_documents`, ui `review_domain_disk_map::dom1_typed_lowercase_documents_can_be_emptied` |
| DOM-2 S1 | the fence looked only at the item path, never at protected places inside a folder item | b85b568 | `review_domain_safety::dom2_folder_holding_a_protected_place_is_not_protected`, ui `review_domain_disk_map::dom2_ticking_pictures_moves_the_photos_library` |
| DOM-3 S1 | execution trusted the saved plan: only allow-list + `policy::check` (Large & Old allow-list is the whole home) | 3efe47d | `review_domain_safety::dom3_large_old_allow_list_is_wider_than_the_row` |
| DOM-4 S1 | `check_keeps` trusted `Item.group`; an edited plan without groups lost every copy | 3efe47d | `review_domain_safety::dom4_duplicates_core_check_trusts_the_group_field` |
| DOM-5 S1 | MCP `execute_plan` applied `only_items` after the digest check, so it could add unconfirmed items | bc49b1a | `review_domain_safety::dom5_mcp_only_items_widens_the_confirmed_selection` |
| DOM-6 S1 | System Junk locks (newest iOS backup) and KEEP_CACHES were scan-time only | 3efe47d | `review_domain_safety::dom6_system_junk_locks_and_icloud_cache_are_not_fenced` |
| DOM-7 S2 | Apple-app refusal looked only at items of category `app`; Threat Check had none | 3efe47d | `review_domain_safety::dom7_apple_app_moves_when_the_category_is_edited` |
| DOM-8 S1 | `parse_lsof` cut names at the first `" ("` | 87f3eb4 | `review_domain_safety::dom8_lsof_names_with_parenthesis_are_cut`, `sys::tests::lsof_lines` |
| DOM-9 S1 | leftovers matched by bundle id ignored another installed copy with the same id | 21989f5 | `review_domain_safety::dom9_second_copy_of_an_app_takes_the_shared_data` |
| DOM-10 S2 | `uninstall_plan` applied the ignore list to leftovers only; App Remover had no Ignore control | 40273cd | `review_domain_safety::dom10_ignored_app_is_still_offered_for_removal`, ui `fix2_app_ignore::ignored_app_leaves_the_list_and_is_never_offered` |
| DOM-11 S2 | `Settings::is_ignored` compared case-sensitively | b85b568 | `review_domain_safety::dom11_ignore_entry_in_other_case_is_not_honoured` |
| DOM-12 S1 | an agent whose program is on an unmounted `/Volumes/<disk>` counted as broken | 8535922 | `review_domain_safety::dom12_agent_on_unplugged_disk_is_not_broken`, `system_junk::tests::program_on_an_unplugged_disk_is_not_gone` |
| REL-1 S1 | a record appended after a cut-short last line was glued to it and unreadable | 097c664 | `review_reliability_core::truncated_journal_line_does_not_swallow_the_next_move` |
| REL-2 S2 | Undo rewrote the journal from parsed records only, deleting unreadable lines | 097c664 | `review_reliability_core::undo_keeps_journal_lines_it_cannot_read` |
| REL-3 S2 | action ids `<plan>#<item>` repeated when a plan ran again; Undo found the old undone record | bd372a4 | `review_reliability_core::same_plan_run_again_after_undo_can_be_undone_again`, `executor::tests::fresh_ids_never_repeat` |
| REL-4 S1 | same root as DOM-5 | bc49b1a | `review_reliability_core::mcp_only_items_cannot_reach_beyond_the_confirmed_selection` |
| REL-5 S1 | Undo put back whatever had the stored name (no identity check) | a5649ed | `review_reliability_core::undo_refuses_a_different_file_at_the_stored_place` |
| REL-9 S2 | `free_name` appended " 2" past the 255-byte name limit | 6b36a06 | `review_reliability_core::longest_name_already_in_the_trash_still_moves`, `executor::tests::numbered_names_fit_the_limit` |

REL-6 was fixed by fix1 (merged); its un-ignore is kept here. REL-7, REL-8 and REL-10 (plan page,
settings loading) were not assigned to fix2 and are untouched.

## What changed, by place

- `policy.rs`: `fold_name`/`same_name`/`starts_with_ci`/`same_path_ci` (case and Unicode
  normalisation insensitive, like APFS on a default Mac; new dependency `unicode-normalization`
  0.1). `protected_reason` uses them and also protects `env.data_dir()`.
  `protected_inside` (a known protected folder below the item, or a Photos library anywhere below
  it, links not followed, depth 24) and `check_move` (= `check` + `protected_inside`), which the
  executor runs before every move. `check` itself stays cheap because scanners call it per entry.
- `features/mod.rs`: `Feature::validate` (per item, refused items become `SkipReason::Refused` and
  are listed first in the report) and `Feature::validate_set` (aborts the run). `run_plan_with`
  also refuses paths on the ignore list. `narrow_selection` for MCP `only_items`.
- Feature rules re-run at execution: Large & Old and Downloads by source (`large_old::validate_path`),
  Duplicates and Similar Images (files only; `check_copies_stay` re-hashes and requires a
  remaining unselected copy reached through look-alikes, not trusting `group`), System Junk
  (`validate_item`), App Remover (Apple apps abort via `validate_set`; running apps), Threat Check
  (Apple apps; Apple-looking names are allowed there because adware borrows them), Privacy
  (`trace_owner`: only paths the scan proposes; a running browser aborts by path).
- `executor.rs`: journal append/read/rewrite by bytes (REL-1/2), fresh action ids and
  `find_record` (REL-3), `ActionKind::Moved.stored_id` (serde default, older lines still undo) and
  the identity check in `check_restore` (REL-5), `numbered` names within `NAME_MAX` (REL-9),
  case-insensitive open-file check.
- `sys.rs`: `parse_lsof` keeps the whole name and also adds the name without a trailing `" (...)"`.
- `space.rs` / Disk Map: `Node.holds_library`, `node_lock_reason`, `lock_reason_deep`.
- `settings.rs`: case/normalisation-insensitive `is_ignored`.
- `uninstaller.rs`: same-bundle-id twin makes id-named leftovers "shared" and unselected; ignored
  apps fully locked. `apps.rs`: `apple_part`. App Remover page: ignored apps hidden, Ignore button.
- `mcp/src/lib.rs`: `execute_plan` narrows with `narrow_selection`.

## Shared-file and test-file changes

- Workspace `Cargo.toml`, `crates/core/Cargo.toml`, `Cargo.lock`: `unicode-normalization`.
- `crates/ui-egui/src/pages/disk_map.rs` (lock calls), `pages/apps.rs` (filter, Ignore button).
- `crates/ui-egui/tests/qa_fix_admin.rs` (fix1's): one exhaustive `ActionKind::Moved` pattern got `..`.
- Reviewer tests: the DOM-5 helper (`mcp_execute_plan`) and the REL-4 replay copied the old
  `mcp/lib.rs` steps line for line; they now replay the fixed code (`features::narrow_selection`),
  with a comment saying so. No assertion was changed.
- System Junk validation for a damaged preference runs `plutil -lint` once more at execution.

## Behaviour notes (honest limits)

- A plan item refused at execution is skipped with a reason; Duplicates/Similar, App Remover with
  an Apple app, and Privacy with a running browser stop the whole run (as before).
- Similar Images: a set is accepted when a kept picture is reachable through look-alikes, as the
  scan's transitive clustering does.
- App Remover leftover validation refuses Apple-id names and Apple apps; it does not re-derive
  that a leftover belongs to the removed app (PARTIAL: a forged plan can still name another
  non-Apple app's data inside the Library leftover folders).
- `protected_inside` reads the folder tree once more before a folder moves (the executor already
  walks it for its size).

## Verification

Run on the merged branch with a private target: `cargo test --workspace` (375 passed, 0 failed, 0 ignored; no
ignored reviewer test left in review_domain_* and review_reliability_core), `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo fmt --all --check`.

## Hand checks on a real Mac

1. Case-insensitive volume: `ls -d ~/documents` prints the Documents folder. In Large & Old Files
   type the folder `~/documents`: expected message "... is protected (the contents of Documents)".
2. Disk Map > Map Home Folder: the Pictures row (holding "Photos Library.photoslibrary") has a
   disabled checkbox with "protected: it holds a Photos library".
3. Open `~/Downloads/Invoice (1).pdf` in Preview, then `/usr/sbin/lsof -F n -w | grep 'Invoice'`:
   expected `n/Users/<you>/Downloads/Invoice (1).pdf`; a Large & Old clean of it reports
   "open in a running app".
4. Journal: after any clean, `tail -c 1 ~/Library/Application\ Support/CleanCraft/journal.jsonl | xxd`
   prints `0a`. Run `cleancraft clean <plan> --yes`, `cleancraft undo <plan>`, the same clean again,
   `cleancraft undo <plan>`: both undos put the items back (ids end in `~2` the second time).
5. Empty the Trash after a clean, trash another file with the same name, then Undo in History:
   expected "the removed copy is gone from ..." and the other file stays in the Trash.
6. App Remover: open an app, press Ignore: it leaves the list and appears in Settings > Ignore list.
