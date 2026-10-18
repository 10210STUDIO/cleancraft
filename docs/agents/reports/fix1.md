# fix1 report (software-engineer, branch agent/fix1)

Defects FIX-1 to FIX-5 from the builder reports. One commit per defect:

| Fix | Commit | Rows |
|---|---|---|
| FIX-1 administrator move | 9278ad9 | P2, A2, J2 |
| FIX-2 Reveal only for files | ae1956b | (P5, P6 rows on the plan page) |
| FIX-3 command time limit | 89f9201 | all rows that run system tools |
| FIX-4 Quick Sweep counts | 2197b0c | Q1 |
| FIX-5 hung apps | docs only (this report, L5 row) | L5 |

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` pass on the final tree and were run before the FIX-2, FIX-3 and FIX-4 commits (shared target `/home/claude/cargo-target`). The FIX-1 commit alone was checked with the core tests, fmt and clippy. The full UI suite ran on the FIX-1 + FIX-3 tree before FIX-3 was split into its own commit.

## FIX-1: items only an administrator can change (WORKS on the fake Mac, MAC-CHECK for the real dialog)

Design (safety model unchanged: same fence, same checks, quarantine only, journal and undo):
- `plan::Item.needs_admin: bool` (`#[serde(default)]`). It is part of the plan digest, so a list confirmed without the administrator note is refused. `Plan::mark_admin`, `Plan::selected_admin_count`.
- `Feature::allows_admin()` defaults to false. True only for Threat Check (`features/malware.rs`), App Remover (`features/uninstaller.rs`, which also covers its leftovers and extensions) and System Junk (`features/system_junk.rs`, where only system caches are marked). `features::run_plan_with` passes it to `executor::execute_with`. `executor::execute` (used by every other caller) never runs an administrator command.
- Scanners mark instead of locking:
  - Threat Check: a finding whose folder this user cannot change (`/Library/LaunchAgents`, `/Library/LaunchDaemons`, `/Library/StartupItems`, rule paths). For apps, the bundle itself is checked, because `/Applications` is `root:admin 775`.
  - App Remover: a bundle this user does not own (installed by a `.pkg`), leftovers in `/Library/...`, extensions in system folders.
  - System Junk: `/Library/Caches/*` entries when the folder is not writable. They are marked, not locked, and never preselected, so a default clean or Quick Sweep never asks for the password. System logs and `/Library/Updates` stay locked.
- `executor::execute_with`: every selected item passes the same `check_item` (policy::check, still there, not open per `lsof`, not inside a running app, injected faults). For an administrator item the "permission denied" fault is skipped, because that is the reason it needs the administrator. All administrator items of the plan then go into the plan's quarantine folder with ONE `env.sys.run_admin(script)`. The script is made only of `/bin/mv -n -- 'from' 'to'` lines (`sys::admin_move_script` with `sys::shell_quote`). Destinations are unique even within the batch (`free_name_except`). Afterwards each item is checked on disk (source gone, destination present) and journaled as `Method::AdminQuarantine`. If the log line cannot be written, the item is moved back with another administrator command and reported. A cancelled dialog (`sys::admin_cancelled`, osascript -128) gives `SkipReason::AdminCancelled` ("the administrator password was not given, so it was not moved") for every item, and nothing is journaled. An item macOS refuses even to root is reported with the `mv` error.
- Undo: `undo` and `undo_plan` put `AdminQuarantine` records back with one `run_admin` of `mv -n` lines per plan, after the same checks as a normal undo (inside the root, place free, stored copy present; the original folder must still exist and is not created). Each record is marked undone only when it is back on disk. A cancelled dialog gives `UndoError::AdminCancelled`.
- `RealSys::run_admin` now also escapes line breaks for the AppleScript string literal, so a quoted name with a newline survives.
- `FakeSys::run_admin` carries out only scripts that `sys::parse_admin_moves` reads back exactly (`/bin/mv -n -- 'a' 'b'` lines). It asserts that both paths are inside the fake root (`FakeState.root`, set by `Env::fake`), and anything else starting with `/bin/mv` is refused. `FakeSys::fail_admin_move(src)` simulates macOS refusing one item.
- UI: rows say "(needs the administrator password)". The confirmations on the plan page, App Remover and Quick Sweep add "N items are in folders only an administrator can change. macOS will ask for the administrator password, and they go to the CleanCraft quarantine." (`plan_page::admin_note`, `features::admin_count`).

Tests:
- core `sys::tests::{shell_quote_keeps_every_character_literal, admin_move_script_round_trips_awkward_names, the_shell_reads_a_quoted_word_back_unchanged, fake_time_out_and_admin_moves, fake_admin_move_outside_the_root_panics}`. They cover spaces, quotes, `$`, backticks, newlines, backslashes and refused scripts.
- core `plan::tests::digest_covers_the_administrator_mark`.
- core `fix_admin_move::{threats_in_root_folders_are_marked_not_locked_and_move_with_one_admin_command, cancelled_password_dialog_moves_nothing_and_says_so, checks_run_before_the_admin_command_and_failures_are_found_on_disk, awkward_names_survive_the_shell, without_the_feature_allowing_it_no_admin_command_runs, the_fence_still_applies_to_admin_items, undo_with_cancelled_dialog_keeps_the_item_in_quarantine, package_installed_app_and_system_leftovers_uninstall_with_the_password, system_caches_need_the_password_and_are_not_preselected}`.
- ui `qa_fix_admin::{threat_in_a_root_folder_moves_with_the_password_and_comes_back, cancelled_password_dialog_moves_nothing_and_is_reported, package_installed_app_uninstalls_with_the_password_or_stays_when_cancelled}`. These are click flows that check the row text, the confirmation note, the numbers against `tree_size`, the files on disk, the journal method, Undo, and Cancel.
- Changed expectations in other owners' tests (behaviour changed on purpose): `cleanup_junk::admin_folders_are_locked_and_unreadable_folders_noted`, `safety::locked_system_folders_are_listed_not_selected`, and `perf_malware::locked_daemon_finding_is_shown_but_not_selectable`, renamed to `root_folder_daemon_finding_needs_the_administrator_password`.

Limits:
- Launch daemons in `/Library/LaunchDaemons` are not booted out before the move, because `launchctl bootout system/...` needs root and would be a second password dialog. They stop at the next restart. Global agents are booted out in the user's GUI domain as before.
- Startup Items' own "Remove" (StartupFeature, `startup.rs`) still locks root-owned property lists. It was not part of FIX-1, so `allows_admin` stays false there.
- An administrator command killed at its time limit (FIX-3) stops `osascript`, but a root shell already running may finish. What moved is still checked on disk.

## FIX-2: Reveal (WORKS)

`plan_page::item_row` shows Reveal only when `symlink_metadata(path)` succeeds. Wi-Fi networks and permissions are labels and get no button. A failing `open -R` is now shown ("Could not open Finder: ...") instead of ignored.

Test: ui `qa_fix_reveal::reveal_only_for_paths_that_exist`. It fails before the fix, with 2 Reveal buttons on the Wi-Fi rows.

## FIX-3: time limit for system commands (WORKS on Linux and the fake Mac, MAC-CHECK on macOS)

- `RealSys::run` uses `sys::run_with_limit`: spawn with piped output read on two threads, poll `try_wait` every 10 ms until the deadline. On time out it kills and reaps the child it started and returns `io::ErrorKind::TimedOut` with "<program> did not finish within N and was stopped". Output is awaited only until the deadline, in case a helper keeps the pipes open.
- `sys::DEFAULT_TIMEOUT` is 60 s. `Sys::run_timeout` takes a per-call limit (the trait default calls `run`). `run_admin` uses `ADMIN_TIMEOUT` of 15 minutes, which includes the password dialog. Tune-up task steps use `tasks::TASK_TIMEOUT` of 20 minutes and report "Failed: <program> did not finish within 20 minutes and was stopped".
- `FakeSys::time_out(key)` (same keys as `answer`) simulates it. `FakeState.limits` records the limits asked for.

Tests:
- core `sys::tests::a_command_past_its_limit_is_stopped`. It runs the harmless `/bin/sleep 30` (killed after 300 ms) and `/bin/sh -c 'echo ...'`. These are the only real processes a test starts; no macOS tool is run.
- core `sys::tests::fake_time_out_and_admin_moves`.
- ui `qa_fix_timeout::a_hung_task_command_is_stopped_and_explained`. It checks the message on screen, that the task's 20-minute limit was used, and the failed journal line.

## FIX-4: Quick Sweep counts (Q1, still PARTIAL only for duplicate downloads)

- `tasks::ROUTINE`: verify disk every 30 days, Launch Services every 90, Mail index every 30, periodic scripts every 7. `tasks::suggestion(id, last_runs, ready, now)` is a pure rule. It suggests a task when the task can run on this Mac and CleanCraft never ran it, its last run failed, or its last good run is older than the interval. `tasks::suggestions`, `tasks::suggestion_count`. Spotlight reindex, snapshots, permissions and DNS are never suggested: they are for problems, not routine.
- The Tune-up Tasks page shows the same "Suggested: ..." line per task, so the tile number can be checked on screen.
- `updater::available_count`: the apps with a newer version, using the same `check_app` as App Updates. macOS updates are not counted. If every app check failed, it returns an error and the tile shows "Could not scan" instead of 0.
- `sweep::count_sources()` lists both.

Tests:
- core `tasks::tests::routine_suggestions`.
- ui `qa_fix_sweep::{tune_up_suggestions_are_counted_and_match_the_tasks_page, app_updates_are_counted_and_match_the_updates_page, update_check_failing_everywhere_is_not_a_zero}`.

## FIX-5: hung apps (L5 stays PARTIAL, nothing faked)

No reliable way exists without private APIs:
- "Not responding" in Activity Monitor and the Force Quit window comes from the WindowServer: the app has not taken its events for a few seconds. The calls behind it are private CoreGraphics (CGS) functions. No public API or command-line tool reports it.
- `ps` state letters do not show it. A hung app spinning in a loop shows `R`, and one stuck on a lock shows `S`, the same as healthy apps.
- The only public probe is an Apple event with a timeout, for example `osascript -e 'with timeout of 5 seconds' -e 'tell application id "X" to count windows' -e 'end timeout'`, where error -1712 means no answer. It needs the Automation permission for every target app (one macOS prompt per app, -1743 when refused), and it can report an app busy with a long task or a modal dialog as hung. A list built on it would be wrong often enough to mislead, so it is not built.
- Proposal if the owner wants it anyway: an explicit "Check whether apps respond" action on one chosen app, labelled as an Apple-event test, never a background list.

## Hand checks on a real Mac

1. Threat Check administrator move. Run `sudo /usr/bin/tee /Library/LaunchAgents/com.genieo.test.plist <<< '<?xml version="1.0"?><plist version="1.0"><dict><key>Label</key><string>com.genieo.test</string><key>ProgramArguments</key><array><string>/usr/bin/true</string></array></dict></plist>'`. Then Threat Check, then Scan. Expected: the row ends with "(needs the administrator password)". The confirmation says "1 item is in a folder only an administrator can change...". Move to Quarantine shows the macOS password dialog once. After it, `ls -l ~/Library/Application\ Support/CleanCraft/Quarantine/*/com.genieo.test.plist` shows the file owned by root. In History, Undo asks for the password again, and `ls -l /Library/LaunchAgents/com.genieo.test.plist` shows it back, owned by root. Repeat with Cancel in the dialog. Expected: Done lists it with "the administrator password was not given, so it was not moved", and the file is still in `/Library/LaunchAgents`.
2. Awkward name. Run `sudo mkdir "/Library/Caches/it's a \$test \`x\`"`. Then System Junk: tick that system cache by hand and clean. Expected: one password dialog, and the folder is in the quarantine with the exact name.
3. Package app. Install any small app from a `.pkg` (`ls -ld /Applications/<App>.app` shows `root`). In App Remover, Uninstall. Expected: the app row says it needs the password, one dialog, and the bundle is in the quarantine. As a standard (non-admin) user, macOS asks for an administrator's name and password.
4. Time limit. Open Trash Bins with files in the Trash and use Empty Trash. If Finder shows a dialog nobody answers, after 60 s the page shows "/usr/bin/osascript did not finish within 60 seconds and was stopped". Check whether 60 s is too short for the Automation consent prompt on first use: answering it later only needs a second click.
5. Tune-up limit. Run Reindex Spotlight. It finishes within the 15-minute administrator limit (`mdutil -E /` returns at once). Verify startup disk (`diskutil verifyVolume /`) must finish within 20 minutes.
6. Reveal. In Privacy Traces, the Wi-Fi networks and App permissions rows have no Reveal button. File rows open Finder at the file.
7. Quick Sweep. Expected: "N suggestions", where N equals the "Suggested:" lines on Tune-up Tasks, and "N updates", equal to App Updates' "N updates available" minus macOS updates.

## Shared-file changes (announced on the board)

- core `executor.rs`: `Method::AdminQuarantine`, `SkipReason::AdminCancelled`, `UndoError::AdminCancelled` (now `Clone`), `execute_with`, `check_item`, administrator batch, administrator undo, and `free_name_except`. `execute` keeps its signature.
- core `plan.rs`: `Item.needs_admin`, `mark_admin`, `selected_admin_count`, and the digest.
- core `features/mod.rs`: `Feature::allows_admin` and `admin_count`. `run_plan_with` calls `execute_with`.
- core `sys.rs`: quoting and the script parser, `admin_cancelled`, time limits, `run_timeout`, `run_with_limit`, `FakeSys` administrator moves, time outs and limits.
- core `env.rs`: `Env::fake` stores the fake root in `FakeSys`.
- core `sweep.rs`: `count_sources`. Core `tasks.rs`: `TASK_TIMEOUT`, `ROUTINE`, suggestions. Core `updater.rs`: `available_count`.
- Scanners: `features/malware.rs`, `features/uninstaller.rs`, `extensions.rs`, `features/system_junk.rs`.
- UI: `pages/plan_page.rs` (admin note, reason text, Reveal), `pages/apps.rs` (confirmation note, row text), `pages/sweep.rs` (confirmation note), `pages/tasks.rs` (Suggested line). In `pages/disk_map.rs` and `pages/startup.rs`, `Method::AdminQuarantine` joined the quarantine match arms.
- Tests of other owners adjusted: `core/tests/cleanup_junk.rs`, `core/tests/safety.rs`, `core/tests/perf_malware.rs`.
- `docs/FEATURES.md`: rows P2, A2, J2, Q1 and L5.
