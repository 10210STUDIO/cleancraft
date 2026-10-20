# apps4 round 4: "Quit X first" flow for App Remover

Branch agent4/apps4 (base main 4e23d58). The user could not uninstall PhotoStudio_5: every row
locked with "Quit PhotoStudio_5 first" although he had closed its window. Cause: the lock
(`uninstaller::app_lock` / `is_running`) counts every process inside the bundle, and the app
keeps a menu bar helper, a health monitor and agents running from
`Contents/Library/LoginItems` and `Contents/Library/LaunchServices`. Nothing in the app could
stop those, so the user had no way forward.

The lock itself is unchanged (safety stays: nothing inside a running app moves; `prepare` and
`validate` still refuse at run time). What is new is a way to see and stop what runs.

## What was done

| Item | Status | Evidence |
|---|---|---|
| 1. List what runs for an app: each process inside the bundle (name, pid, owner, program, GUI app with bundle id and type from `lsappinfo`, login item, launchd job with label and domain), plus loaded launchd jobs whose plist points into the bundle or whose label is the app's id or starts with it (also helper apps' ids, and `SMAppService` plists in `Contents/Library/LaunchAgents|LaunchDaemons`). LaunchServices' own `application.<id>.N.N` labels are not treated as jobs. Processes in folders named after the id (`/Library/PrivilegedHelperTools/<id>.Agent`) are listed but do not lock. | WORKS (core, FakeMac) | `apps_quit::every_helper_inside_the_bundle_is_listed`, `launchctl_failure_is_a_note_and_the_lock_still_holds`, unit tests `app_quit::tests::*` (ps, lsappinfo, launchctl print parsers) |
| 2. Quit: `launchctl bootout gui/<uid>/<label>` for the app's agents and login items first (so launchd does not restart them), then `osascript` "if application id X is running then tell application id X to quit" (never launches it, 10 s answer limit) for the helper apps and the app last; system services (`launchctl bootout system/<label>`) through `env.sys.run_admin` (one password dialog) only with `allow_admin` (the user's yes). Waits up to 5 s, re-checking every 250 ms, then reports what still runs. | WORKS (core) | `quit_sends_the_right_commands_and_unlocks_the_plan`, `a_launch_daemon_needs_a_confirmed_administrator_password` (no yes: no dialog; cancelled: said; yes: one admin command) |
| Force Quit: `kill -TERM`, then `kill -KILL` after a 3 s grace, only for processes inside the bundle, pid re-checked before each signal; root processes inside the bundle only with `allow_admin` (TERM in the one admin command). Never the app's helpers outside the bundle, never other apps, never macOS apps or CleanCraft. | WORKS (core) | `still_running_and_relaunched_are_reported_then_force_quit_stops_them`, `macos_apps_and_cleancraft_are_never_quit` |
| Journal: every bootout is a `Toggled` line with the command that loads it again (`launchctl bootstrap gui/<uid> '<plist>'` when the plist is outside the bundle, else `open '<helper app>'`, which re-registers it; system daemons get `admin_wrap(bootstrap system ...)`, or no undo plus "starts again at the next restart" when their plist is inside the bundle). Every quit, failed bootout, admin command and kill is a `Command` line. Feature = Uninstaller. | WORKS | same tests check `executor::history` |
| 3. Login items / SMAppService: login-item helpers run as launchd jobs in `gui/<uid>`, so the bootout keeps them from starting again right after the quit. If something still starts it again, the report says so honestly ("HealthMonitor started again by itself right after it stopped.") and Force Quit is offered. | PARTLY | `still_running_and_relaunched_...` (relaunch reported). Legacy System Events login items are not read on every check (it would show the Automation prompt); BTM (`sfltool dumpbtm`) needs root and is not used. |
| 4. Plan unlocks on the next check: `uninstall_plan` after a successful quit has no note, the app and the id-named files selected. | WORKS (core) | `quit_sends_the_right_commands_and_unlocks_the_plan` |
| Nothing moves while something runs | WORKS | `still_running_...`: a plan force-unlocked by hand is refused by `run_plan` while a helper runs; after Force Quit the same uninstall moves the app to the Trash |
| 5. Leftover sizes: the formula was right (shared `tree_size`, allocated blocks). The "0 KB" rows are folders whose contents could not be read (permission or macOS privacy: the folder's own entry is 0 blocks on APFS) or really empty folders; before, an unreadable folder silently showed its readable part. Now `uninstaller::leftover_size` counts unreadable parts and the row's reason ends with ". Some of it could not be read, so it may be bigger than shown" (`uninstaller::SIZE_PARTIAL`, `size_is_partial(item)`). No plan note is added (a note blocks the App Remover page). | WORKS (core), UI wording with design4 | `unreadable_leftover_folders_say_their_size_may_be_bigger` |

## API for the UI (design4)

```rust
use cleancraft_core::app_quit::{self, QuitOptions, ForceOptions};
let st = app_quit::running_parts(&env, &app_path);      // RunningState, never fails (notes say what could not be checked)
st.locked(); st.headline(); st.needs_admin(); st.admin_question(); st.parts; // part.name, part.role(), part.pid
let rep = app_quit::quit_app(&env, &app_path, QuitOptions { allow_admin, ..Default::default() })?;   // Err(String) = refused
let rep = app_quit::force_quit(&env, &app_path, ForceOptions { allow_admin, ..Default::default() })?;
rep.done(); rep.can_force(); rep.message(); rep.relaunched; rep.admin_needed; rep.after (RunningState)
uninstaller::size_is_partial(&item)
```
Both quit calls block up to about 5 s (force about 5 s): run them in a `Job`. Recommended UI is
on the board (10:32 apps4 -> design4).

## Shared-file changes

- `crates/core/src/lib.rs`: `pub mod app_quit;`, `pub mod testkit_quit;`.
- `crates/core/src/sys.rs` (my additions): `FakeState::effects` and `FakeEffect` (a scripted
  command can stop or start fake processes and replace answers), `FakeSys::on_run`; effects also
  apply to `admin:<command>` answers. Nothing changed for existing tests.
- `crates/core/src/features/uninstaller.rs`: `leftover_size`, `SIZE_PARTIAL`, `size_is_partial`.
- New: `crates/core/src/app_quit.rs`, `crates/core/src/testkit_quit.rs`, `crates/core/tests/apps_quit.rs`.

## Needs a check on the real Mac

1. `ps -axo pid=,uid=,comm=` prints the full program path for helpers in
   `Contents/Library/LoginItems/*.app` (as `ps -axo pid=,comm=` already does for the lock).
2. `lsappinfo list` block format: `N) "Name" ASN:...:`, `bundleID="..."`,
   `pid = N type="Foreground|UIElement|BackgroundOnly"`. If it differs, the bundle id still comes
   from the helper's Info.plist; only the "menu bar helper" wording is lost.
3. `launchctl print gui/$(id -u)` and `launchctl print system` without root: a `services = {`
   block with "pid  status  label" lines. Check that the reference app's menu helper and health monitor
   appear there with their pids, and what labels they use.
4. `osascript -e 'with timeout of 10 seconds' -e 'if application id "com.example.photostudio5" is running then tell application id "com.example.photostudio5" to quit' -e 'end timeout'`
   quits it without asking for Automation permission each time (it may ask once).
5. After `launchctl bootout gui/<uid>/<menu helper label>`, does the menu helper stay stopped,
   or does BTM / the main app start it again? The report says it honestly either way.
6. The 0 KB caches: `ls -la ~/Library/Caches/com.example.photostudio5` and `du -sk` on it, with
   and without Full Disk Access for CleanCraft, to see whether it is empty or unreadable.
7. Administrator path: one password dialog for `launchctl bootout system/<label>`; cancel shows
   the cancelled message.
