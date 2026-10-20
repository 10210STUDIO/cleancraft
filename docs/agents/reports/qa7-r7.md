# QA round 7 (qa7): error cases for the rows check7 downgraded

Branch agent7/qa7 (main dc7e109 plus agent7/check7). New test files only, no app code changed:

- `crates/ui-egui/tests/qa7_sweep.rs` (Q2, Q4)
- `crates/ui-egui/tests/qa7_dev_clutter.rs` (DV3, DV4, DV5, DV6, DV8)
- `crates/ui-egui/tests/qa7_apps.rs` (A6, A13, A14)
- `crates/ui-egui/tests/qa7_files.rs` (C1, C2, C3, D1, D3)

Every test runs by clicks and typing on the real pages on a FakeMac. Each test checks four things. The faulted items stay on disk with their reason shown next to the file name. Every other selected item moves, and nothing unselected moves. The freed size on screen equals a `tree_size` recalculation from disk, and so does the content of the Trash where checked. Undo puts back exactly what moved. Faults come from `env.inject_fault` (`PermissionDenied`, `DiskFull`) and `FakeSys::set_open` (file in use). Each row also gets a test that makes the action log unwritable (`DiskFull` on `executor::journal_path`). That test checks that nothing moves and that "could not write the action log" is shown.

Result: 24 new tests, 24 pass, 0 ignored. **No real app bug found**, so nothing was posted to fix7 or sfix7.

## Per row: what now closes it

| Row | check7 gap | Tests that close it |
|---|---|---|
| Q2 | permission denied, disk full on the Run | `qa7_sweep::qa7_q2_permission_denied_and_disk_full_are_shown_and_the_rest_moves` (a System Junk item denied and a Mail item on a full disk in one Run; per-module "N moved, 1 skipped, size" against disk; journal holds only what moved; Undo per module), `qa7_sweep::qa7_q2_disk_full_on_the_action_log_moves_nothing_and_says_so`. File in use and cancel were already covered by qa_sweep |
| Q4 | permission denied, disk full | `qa7_sweep::qa7_q4_disk_full_keeps_that_copy_and_says_so`, `qa7_sweep::qa7_q4_permission_denied_keeps_that_copy_and_says_so` (two recent sets; the kept copy of each set stays; Undo), `qa7_sweep::qa7_q4_disk_full_on_the_action_log_moves_nothing`. File in use: existing qa_files_r6::duplicate_downloads_copy_in_use_is_skipped |
| DV3, DV4 | disk full | `qa7_dev_clutter::qa7_dv3_dv4_disk_full_keeps_those_folders_and_says_so` (one `target` and one `node_modules` on a full disk, the next step "Empty the Trash, then Try Again." shown, Trash content equals the recalculation, Undo), `qa7_dev_clutter::qa7_dv3_dv4_disk_full_on_the_action_log_moves_nothing` |
| DV5 | core only | `qa7_dev_clutter::qa7_dv5_python_environment_listed_unticked_moves_only_when_ticked` (`.venv` listed, never preselected, an `env` folder without `pyvenv.cfg` not listed, a clean without ticking leaves it, ticked by hand it moves with its size, Undo) |
| DV6 | core only | `qa7_dev_clutter::qa7_dv6_other_build_output_moves_and_unlisted_dist_stays` (Next.js `.next/cache`, Gradle `build/`, a `.gitignore`d `dist` listed, preselected and moved; `.next/server`, a `build` not in `.gitignore` and an active project's `dist` untouched; total against disk; Undo) |
| DV8 | core only | `qa7_dev_clutter::qa7_dv8_code_folder_in_documents_is_locked_with_the_way_forward` (a code folder in Documents is listed locked with "(cannot be selected: Protected place (... the contents of Documents ...) ... Move the project out of Documents and Desktop ...)"; clicking it and Select All never tick it; a confirmed clean leaves it; a Photos library inside a code folder is never walked; the home folder as a code folder gives "Nothing to clean here" and Details says it "holds your whole home folder"). The code folders are written to Settings directly; the Settings flow by clicks is DV1's test |
| A13, A14 | disk full (single and batch) | Single: `qa7_apps::qa7_a13_a14_single_app_disk_full_keeps_that_file_and_says_so` ("Uninstalled. 3 items moved. 1 skipped.", size against disk, Undo "3 items put back."), `qa7_apps::qa7_a13_single_app_disk_full_on_the_action_log_moves_nothing`. Batch: `qa7_apps::qa7_a13_a14_batch_disk_full_per_file_and_per_app_then_undo` (a leftover of Viewer and Jotter's whole bundle on a full disk; per file "com.maker.Viewer: the disk is full", "Jotter.app: the disk is full"; "2 apps uninstalled. 6 items moved. 2 skipped."; Jotter "Kept: the app itself did not move"; Undo Viewer then Undo All), `qa7_apps::qa7_a13_batch_disk_full_on_the_action_log_moves_nothing` |
| A6 | no error case, no Undo | `qa7_apps::qa7_a6_installer_errors_are_shown_the_rest_moves_and_undo` (three installers ticked in the Installers group, one `.pkg` on a full disk, one open in an app, the `.dmg` moves; "1 item moved. 2 skipped."; Undo "1 item put back.") |
| C1 | disk full | `qa7_files::qa7_c1_disk_full_keeps_that_file_and_says_so`, `qa7_files::qa7_c1_disk_full_on_the_action_log_moves_nothing` |
| C2 | permission denied, file in use, disk full | `qa7_files::qa7_c2_permission_denied_in_use_and_disk_full_then_undo` (three sets, four selected copies: one denied, one open, one on a full disk, one moves; every kept copy stays; "1 item moved. 3 skipped."; Undo), `qa7_files::qa7_c2_disk_full_on_the_action_log_moves_nothing` |
| C3 | the three errors and Undo | `qa7_files::qa7_c3_permission_denied_in_use_and_disk_full_then_undo` (two picture sets, the 800 x 600 one of each kept; same checks as C2), `qa7_files::qa7_c3_undo_puts_back_every_moved_picture`, `qa7_files::qa7_c3_disk_full_on_the_action_log_moves_nothing` |
| D1, D3 | disk full | `qa7_files::qa7_d1_d3_disk_full_keeps_that_file_says_so_and_sizes_follow` (typed folder, three files selected, one on a full disk; the map's tree size equals `tree_size` after the move and after Undo), `qa7_files::qa7_d1_d3_disk_full_on_the_action_log_moves_nothing` |

## For sfix7 and the lead (FEATURES.md)

Once these tests are added to the Test column, the error-case gap check7 named is closed for Q2, Q4, DV3, DV4, DV5, DV6, DV8, A6, A13, A14, C1, C2, C3, D1 and D3. Each of those rows can go back to WORKS, with the existing MAC-CHECK notes kept (A13: the real quit and password dialogs; C1: `mdls`; C3: HEIC through `sips`; D3: real external drives).

Two rows still have a separate safety finding from check7, so they should stay PARTIAL until it is fixed. That decision belongs to the lead, not to these tests:
- D3: CHK7-2 (a folder that holds an ignored item can be selected and moves it).
- DV1, J17, J19, S4, S5: CHK7-1. These rows are outside this task.

## Notes (not defects)

- In Quick Sweep, the result card lists each skipped file with its reason but without the next step. The plan pages show the next step ("Empty the Trash, then Try Again."). This is a small UX difference; I did not file it as a bug.
- Test runs: `cargo test -p cleancraft-ui-egui --test qa7_sweep --test qa7_dev_clutter --test qa7_apps --test qa7_files`: 24 passed. Clippy with `-D warnings` and rustfmt are clean on these files.
