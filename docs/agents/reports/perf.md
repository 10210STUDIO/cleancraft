# perf report: Tune-up Tasks, Startup Items, Threat Check

Branch `agent/perf`. Rows K1-K10, L1-L5, P1-P2.

## What was built

| Area | Files |
|---|---|
| Tune-up Tasks engine | `crates/core/src/tasks.rs` (task list, `prepare` = availability + exact commands, `run`, journal, `last_runs`) |
| Startup Items engine | `crates/core/src/startup.rs` (login items, launch agents/daemons, codesign + print-disabled parsers, on/off with undo, `StartupFeature` plan for removing agent files, heavy apps + Quit) |
| Threat Check | `crates/core/src/features/malware.rs` (plan feature, quarantine override, agents stopped first), `crates/core/src/malware_rules.rs` (rule list with sources) |
| Test fixtures | `crates/core/src/testkit_perf.rs` (sample command outputs, a fake Mac with every kind of startup item, `FreeSpaceRises` Sys wrapper) |
| UI | `crates/ui-egui/src/pages/tasks.rs`, `crates/ui-egui/src/pages/startup.rs`; Threat Check uses the shared plan page |
| MCP | `crates/mcp/src/perf.rs`: `list_tasks`, `run_task`, `list_startup_items`, `toggle_startup_item` (changes gated by `agents_may_clean`) |
| CLI | `apps/cleancraft/src/cli_perf.rs`: `cleancraft tasks`, `cleancraft run-task <id> --yes`, `cleancraft startup` |

Every command goes through `env.sys` (`run` or `run_admin`); tests answer them with `FakeSys` and assert the exact command lines recorded. No `#[cfg(target_os)]` code was added.

## What works (with tests)

All tests below pass on the fake Mac. Command outputs used by the fakes are samples I wrote from knowledge of macOS (marked in `testkit_perf.rs`), so every row that depends on a macOS tool's output or behaviour is **MAC-CHECK**, not WORKS.

**Tune-up Tasks (K1-K10), MAC-CHECK.** Checklist with our own description, exact command, administrator badge, risk, availability reason and last run (from the journal). "Run Selected" re-checks each task and opens a confirmation listing the exact commands (and the tasks that cannot run now and why). Results per task: Done / Failed: reason / "Cancelled, nothing changed"; "Show log of <task>" shows the output; purgeable space shows "Free space before X, after Y" (checked against the fake disk's numbers); Speed up Mail shows the index size before and after. Each run (not a cancel) is one `ActionKind::Command` journal line (`plan_id = task-<id>-<ms>`), which feeds "Last run". Nothing runs when the journal cannot be written. Speed up Mail is refused while Mail is open, checked at confirmation and again right before running. Periodic scripts only on macOS 13 and earlier (`sw_vers -productVersion`).
- UI: `qa_perf_tasks::run_two_tasks_with_exact_commands_results_and_free_space`, `cancelled_password_dialog_says_nothing_changed`, `failing_command_is_shown_and_logged`, `tasks_that_cannot_run_explain_why_and_cancel_runs_nothing`.
- Core: `perf_tasks::*` (13 tests: admin path, cancel, failure, missing tool, free space, periodic gate, Mail gate and Full Disk Access, snapshot dates, user id, journal unwritable, non-macOS).

**Startup Items (L1-L4), MAC-CHECK.** Sections: login items, your launch agents, agents for all users, system daemons, heavy apps. Each agent shows label, program, signer (`codesign -dv --verbose=2` authority, ad hoc, unsigned, missing program) and On/Off (`launchctl print-disabled gui/<uid>` / `system`, else the plist `Disabled` key). Apple-signed items show "Part of macOS" with no buttons (and the engine refuses them).
- Turn Off / Turn On, user and global agents: `launchctl disable|enable gui/<uid>/<label>` then `bootout|bootstrap gui/<uid> <plist>` (stop/start failures tolerated). Daemons: same in the `system` domain through `run_admin`; cancel shows "Cancelled, nothing changed" and logs nothing. Each change is an `ActionKind::Toggled` with an `undo_command` (daemons: wrapped in `osascript ... with administrator privileges`, because History runs undo with `/bin/sh -c`). History Undo tested by clicks.
- Login items: Turn Off deletes through System Events; the item stays listed as Off with Turn On, which runs the journal undo (`make login item at end with properties {path:..., hidden:false}`). Items without a known path are refused (no way to add them back).
- Remove (L4): only offered where CleanCraft may write (your LaunchAgents). Opens a confirmation with the exact file, then the plan/executor path (`StartupFeature`, allow-list = the three folders, nothing preselected); `prepare()` runs `launchctl bootout` first and logs it; undo puts the file back.
- UI: `qa_perf_startup::lists_everything_with_signer_state_and_count` (count recalculated), `turn_agents_off_and_on_then_undo_from_history`, `cancelled_password_dialog_and_launchctl_failure`, `login_item_off_and_back_on`, `automation_refusal_is_explained`, `remove_agent_file_after_confirmation_and_undo`, `remove_agent_file_permission_denied`.
- Core: `perf_startup::*` (13 tests).

**Threat Check (P1), MAC-CHECK.** Findings with a reason each: agents/daemons whose program is unsigned or ad hoc, runs from `/tmp`, `/Users/Shared` or a hidden folder, uses a `com.apple.` label without Apple's signature, or has a hidden plist; rule matches (labels, app names, known paths); apps that `spctl --assess --type execute` rejects; legacy `/Library/StartupItems` entries. Only high-confidence rule matches on non-app items are preselected; apps never. A file inside a listed folder is not listed twice. Tests: `qa_malware::scan_shows_reasons_and_quarantines_the_confirmed_threats`, `perf_malware::findings_have_reasons_and_only_high_confidence_rules_are_preselected`, `apple_name_without_apple_signature_and_legacy_items`, `locked_daemon_finding_is_shown_but_not_selectable`.

**MCP and CLI.** `cleancraft-mcp` unit tests `perf::tests::*` (tools registered in the combined router, refusal without the setting, cancel reported). CLI unit tests in `cli_perf.rs` (`run-task` without `--yes` runs nothing).

## PARTIAL and why

- **L5 PARTIAL**: heavy apps (this user's processes from `ps -axo pid=,uid=,%cpu=,%mem=,comm=`, at least 30% CPU or 10% memory, never macOS parts or CleanCraft) and Quit after confirmation work (`osascript ... tell application id "<id>" to quit` for apps with a bundle id, else `kill -TERM <pid>`, pid re-checked first, logged). Detecting apps that are *not responding* is not built: it needs a responsiveness probe that no command-line tool offers reliably.
- **P2 PARTIAL**: user-writable findings move to the quarantine (always, whatever the Trash/quarantine setting; the plan page confirmation now says so) with launch agents booted out first; undo works; permission errors are shown per item. Findings in root-owned folders (`/Library/LaunchAgents`, `/Library/LaunchDaemons`, `/Library/StartupItems`) are listed but locked "Needs administrator rights": the executor only moves as the user. Quarantining them needs a privileged move, which is a change to `executor.rs` that I did not make (rule: do not weaken or bypass it). Proposal for the coordinator: an admin move step (`run_admin("mv ...")` into the quarantine plus a journal line) designed together with the safety owner.
- Sample outputs: every parser is unit-tested only on outputs I wrote; see hand checks.

## Hand checks on a real Mac

Run each, compare with the sample in `crates/core/src/testkit_perf.rs`, and fix the parser if the shape differs.

1. `id -u` prints the uid; `sw_vers -productVersion` prints like `14.6.1`.
2. `codesign -dv --verbose=2 /Applications/Safari.app/Contents/MacOS/Safari` (to stderr): first `Authority=Software Signing`. A Developer ID app: `Authority=Developer ID Application: Name (TEAMID)` and `TeamIdentifier=TEAMID`. An ad hoc binary (`codesign -s - x`): `Signature=adhoc`. Unsigned: `code object is not signed at all`, exit 1.
3. `launchctl print-disabled gui/$(id -u)` and `sudo launchctl print-disabled system`: a `disabled services = {` block with `"label" => disabled|enabled` (older: `true|false`). Also check that `print-disabled system` works without sudo.
4. Login items script: `osascript -e 'set out to ""' -e 'tell application "System Events"' -e 'repeat with i in (every login item)' -e 'set p to ""' -e 'try' -e 'set p to (path of i) as text' -e 'end try' -e 'set out to out & (name of i) & tab & p & linefeed' -e 'end repeat' -e 'end tell' -e 'return out'` prints `Name<TAB>/Applications/App.app` lines. First run must show the Automation prompt; denying it must give error -1743 (the app explains where to allow it). On macOS 13+, check whether System Events lists the items shown in System Settings > General > Login Items.
5. Turn off a test agent: `launchctl disable gui/$(id -u)/<label>` then `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/<label>.plist`; `launchctl print-disabled gui/$(id -u)` shows it disabled; Undo in History runs `launchctl enable ... && { launchctl bootstrap ... || true; }`. Check that a global agent (`/Library/LaunchAgents`) can be disabled in the gui domain without the password.
6. Daemon toggle: the password dialog appears; Cancel shows "Cancelled, nothing changed" (osascript error -128); the Undo command from History shows the dialog again.
7. Login item remove and re-add through System Events (Turn Off, then Turn On).
8. `ps -axo pid=,uid=,%cpu=,%mem=,comm=` columns (comm is a full path; decimal comma in some locales is handled). Quit an app: `osascript -e 'tell application id "com.apple.TextEdit" to quit'`.
9. `spctl --assess --type execute /Applications/<unsigned>.app` prints `...: rejected` and `source=...` on stderr with exit 3; an accepted app exits 0.
10. Tasks: `sudo dscacheutil -flushcache; sudo killall -HUP mDNSResponder`; `sudo purge`; `tmutil thinlocalsnapshots / 999999999999 4` (check it runs without sudo and that `df` free space changes); `tmutil listlocalsnapshots /` lines `com.apple.TimeMachine.<date>.local` and `sudo tmutil deletelocalsnapshots <date>`; `sudo mdutil -E /`; the `lsregister` path and whether `-kill` is still accepted on current macOS; `sudo diskutil resetUserPermissions / $(id -u)`; `diskutil verifyVolume /` without sudo; `sqlite3 ~/Library/Mail/V10/MailData/Envelope\ Index "VACUUM;"` with Mail quit (needs Full Disk Access; check the V number on current macOS); `sudo periodic daily weekly monthly` on macOS 13.
11. Rule list (`malware_rules.rs`): verify each label prefix, app name and path against the cited public sources before release; names were written from memory of public reporting.
12. `/Applications` is `root:admin 775`: check that quarantining an app as an admin user succeeds, and as a standard user is reported as permission denied.

## Shared-file changes

- `crates/core/src/lib.rs`: `pub mod malware_rules; pub mod startup; pub mod tasks; pub mod testkit_perf;`
- `crates/core/src/features/mod.rs`: `pub mod malware;` and two registry lines (`malware::Malware`, `crate::startup::StartupFeature`).
- `crates/ui-egui/src/pages/plan_page.rs`: the confirmation dialog uses the feature's `method_override()` for its title and button (Threat Check always says quarantine).
- `crates/mcp/src/lib.rs`: `pub mod perf;` and `tool_router: Self::tool_router() + Self::perf_router()` in `new()`.
- `apps/cleancraft/src/cli.rs`: `Tasks`, `RunTask { id, yes }`, `Startup` subcommands calling `cli_perf`; `apps/cleancraft/src/main.rs`: `mod cli_perf;`.
- `docs/FEATURES.md`: Test and Status of K1-K10, L1-L5, P1-P2 only.

## Notes for others

- `cargo clippy --workspace --all-targets -- -D warnings` fails on main before my changes: `crates/core/src/policy.rs:195,198` (`cloned_ref_to_slice_refs`, test code, not my file). With that lint allowed (`-A clippy::cloned_ref_to_slice_refs`) the workspace is clippy-clean. Full `cargo test --workspace` passes.

- The sidebar is taller than the 820 px test window: "History" sits at y=858, so `common::click("History")` fails; my tests scroll it into view first (board note posted).
- `Env::can_modify` ignores group write, so root:admin 775 folders count as not writable; Threat Check does not lock apps for that reason and lets the executor report the real result.
