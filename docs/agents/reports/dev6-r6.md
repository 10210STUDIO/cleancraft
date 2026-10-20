# dev6 round 6: Developer Clutter

Branch agent6/dev6. Rows: docs/features-r6/dev6.md (DV1 to DV13).

## What it is

A new tool in the Clean section, after Trash Bins: FeatureId::DevClutter (`dev_clutter`, "Developer Clutter").

- Core: `crates/core/src/features/dev_clutter.rs`, a plan Feature like System Junk.
  - Code folders: ~/Developer, ~/Developers, ~/Projects, ~/Code, ~/src, ~/dev, ~/work, ~/GitHub plus `Settings::dev_roots`. Links are never followed. The home folder (or a folder holding it) is refused with a note.
  - One item per project folder: Rust `target/` next to `Cargo.toml`; `node_modules` next to `package.json` (never inside another `node_modules`); Python venvs (`.venv`, `venv`, `env` holding `pyvenv.cfg`); `.next/cache`; Gradle `build/`; `dist`/`build` only when the project's own `.gitignore` lists them and the project is inactive.
  - Activity rule: an item is selected only when no project file changed for `Settings::dev_inactive_days` (default 30). Build folders, packages and `.git` internals do not count; `.git/HEAD` and `.git/index` do. Otherwise the item is listed unselected with "active project, a file changed N days ago". Python venvs are never selected (safety6 SAF6-3).
  - Git: `git --literal-pathspecs -C <repo> ls-files -z -- <folders>`, one call per work tree (SAF6-2). A tracked folder is locked. When git cannot be asked, only folders a `.gitignore` lists are offered. A folder holding its own `.git` is locked. All of this is checked again in `validate` right before the move. Git is Homebrew's, or /usr/bin/git only after `xcode-select -p` succeeds, so macOS never shows the "install developer tools" offer.
  - Protected places: a code folder inside Documents or Desktop has its projects listed locked, with the reason and the way forward. Photos libraries are skipped.
  - Every lock text gives a way forward (COMMON6 rule at 17:25).
  - Unavailable Xcode simulators: the device folders `~/Library/Developer/CoreSimulator/Devices/<UDID>` of the devices `xcrun simctl list devices unavailable -j` lists. They are never preselected, a running device is locked, and the UDID is rechecked with simctl at the move. The executor moves them to the Trash with undo.
  - Docker and Homebrew are report only (`scan_tools`): `docker system df` and `brew cleanup -n`, with sizes, the exact command (`docker image prune`, `docker builder prune`, `brew cleanup`) and the plain reason why CleanCraft does not run it. Absent, not running and failing tools are each reported in words.
  - `dev_clutter::plan(&env, &settings)` is the plan for Quick Sweep (files6 adds the tile by id).
- UI: `crates/ui-egui/src/pages/dev_clutter.rs` wraps the shared `plan_page` flow (Scan, Review, Confirm, Done, Undo). After a scan, a banner (when Docker or Homebrew is installed) opens the "Docker and Homebrew" dialog: one card per tool with its report, the command in the code font, a "Copy ... Command" button (it copies and says so), Check Again and Done. Every button calls app.act(), and the kit widgets give the pointer and states.

## Design change during the round (approved by the lead at 17:46)

The brief asked for `xcrun simctl delete unavailable`, `docker image prune`, `docker builder prune` and `brew cleanup` as confirmed commands. safety6 (SAF6-1) pointed out that these delete for good, and that they act on whatever is true at run time (TOCTOU). COMMON6 forbids permanent deletes. The lead confirmed: no run buttons. So simulators became plan items that move to the Trash with undo, and Docker and Homebrew are reported with the command to copy. The brief's tests "commands only after confirmation with exact strings" became tests that no deleting command is ever run (`no_permanent_commands` in the core tests, and the UI test checks the FakeSys call list).

## Go build cache, pip cache

`~/Library/Caches/go-build` and `~/Library/Caches/pip` are already user caches in System Junk (J1/J17), so they are not duplicated here (DV12).

## Tests

- core `crates/core/tests/dev_clutter.rs` (12): discovery_per_category, activity_rule_and_its_setting, git_tracked_folder_is_never_offered, links_are_never_followed, protected_places_are_locked_and_never_walked, own_code_folder_moves_to_the_trash_and_undo_restores, permission_denied_and_file_in_use_are_skipped, unavailable_simulators_move_to_the_trash_never_preselected, simulators_without_xcode_are_a_note, docker_absent_stopped_failing_and_running_is_reported_only, homebrew_absent_failing_and_dry_run_only, a_build_folder_holding_its_own_repository_is_locked; unit tests in the module (gitignore patterns, sizes, docker table, brew lines, runtimes).
- ui `crates/ui-egui/tests/qa_dev_clutter.rs` (6): scan_review_confirm_moves_selected_and_numbers_match (total and moved size recalculated from disk, the confirmation lists only the selected folders, Trash content equals the moved size, Undo), permission_denied_and_file_in_use_are_shown, git_tracked_folder_is_shown_locked_and_never_moves, unavailable_simulator_is_listed_unticked_and_moves_only_when_ticked, docker_and_brew_are_reported_with_the_command_never_run, nothing_found_says_where_it_looked.

## Shared-file changes (small additions)

- core `plan.rs`: FeatureId::DevClutter (ALL grows to 16, as_str, title).
- core `settings.rs`: `dev_roots: Vec<PathBuf>`, `dev_inactive_days: u64` (default 30; serde default keeps old files loading).
- core `features/mod.rs`: `pub mod dev_clutter;` and the registry line.
- ui `app.rs`: GROUPS Clean gets DevClutter after TrashBins (health6 agreed to the line).
- ui `kit.rs` feature_look arm (Icon::Junk for now), `theme.rs` Section::of (Clean), `heroes.rs` two arms mapping to System Junk's hero, `pages/mod.rs` module, route arm and covers lines.

## Asked of others (board)

- tray6: Settings group "Developer Clutter" (code folders list with Add/Remove, inactive days).
- files6: Quick Sweep tile (by id), plan_page `blurb()` text and `can_stop()` for DevClutter.
- health6: a better icon if one exists. GROUPS line already in.

## Check on a real Mac (MAC-CHECK)

- `xcrun simctl list devices unavailable -j` keys (isAvailable, availabilityError, state). Also how CoreSimulator behaves when a device folder is moved to the Trash: it should drop the device, and Undo should bring it back.
- `docker system df` table and Docker Desktop's "not running" text; docker found at /usr/local/bin or inside Docker.app when CleanCraft runs as an app (no shell PATH).
- `brew cleanup -n` line format, run from an app.
- git ls-files speed in a big repository; Documents access prompt when a code folder lives in Documents.

## Gate

Every workspace test target passed: 82 targets, run one at a time with each test binary deleted after it ran, because the shared disk dropped under 2 GB. That covers the core lib and all core test files, the mcp lib, the ui-egui lib and all 45 ui test files (qa_every_control included, with the new page), the app crate and the doc tests. `cargo clippy --workspace --all-targets -- -D warnings` is clean and `cargo fmt --all --check` is clean. target/ is deleted.
