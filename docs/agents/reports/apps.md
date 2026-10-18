# apps: App Remover, App Updates, Extensions (A1-A11)

Branch `agent/apps`. All tests below run on `FakeMac` and `FakeSys`; nothing touches real files or runs real system tools.

## What works (with tests)

| Row | What | Tests |
|---|---|---|
| A1 | App list from `/Applications`, `~/Applications`, one level of subfolders (`Utilities`, `Setapp`, vendor folders) and `/System/Applications`: name, bundle id, version, build, size (`fsutil::tree_size`), last opened (`mdls`), store (`_MASReceipt/receipt`, `/Applications/Setapp`, other), vendor from bundle id. XML and binary Info.plist. Search, sort (size, name, last opened), Enter opens the first match, Esc goes back | `apps_inventory::lists_every_kind_of_app`, `::find_by_name_id_or_path`, `::quick_listing_runs_no_commands`; UI `qa_apps_remover::list_groups_and_numbers_match_the_disk` (sizes and group totals recalculated from disk) |
| A2 | Uninstall: bundle plus leftovers in the A2 folders and their `/Library` equivalents. High confidence (named after the bundle id, incl. `.plist`, `.savedState`, `.binarycookies`, `TEAMID.`/`group.` prefixes, `id.helper`) preselected; medium (named like the app) and the maker's shared folder never preselected; another installed app's longer id is never matched. Confirmation lists the exact files; Done with Undo; errors per file | `apps_uninstall::plan_finds_leftovers_with_confidence`, `::uninstall_moves_bundle_and_selected_leftovers_and_undo_restores`, `::ignore_list_and_errors`; UI `qa_apps_remover::uninstall_moves_app_and_leftovers_then_undo`, `::file_errors_are_listed_per_file`, `::cancel_and_escape_move_nothing` |
| A3 | Apple apps (bundle id `com.apple.`, `/System/Applications`, or `codesign` authority `Software Signing`) listed with "Part of macOS", every item locked; `Uninstaller::prepare` refuses even a tampered plan. Running app: everything locked "Quit X first"; if it starts after the dialog, `prepare` refuses and nothing moves | `apps_uninstall::apple_apps_are_locked_and_refused`, `::running_app_is_locked_and_refused`, `apps_inventory::apple_signature_marks_app_as_apple`; UI `qa_apps_remover::apple_app_is_locked`, `::running_app_is_refused` |
| A4 | Reset: same leftovers, bundle listed locked "Kept: a reset keeps the app" | `apps_uninstall::reset_keeps_the_app`; UI `qa_apps_remover::reset_keeps_the_app` |
| A5 | Leftovers of removed apps (conservative: name is a bundle id, not Apple, no installed app with that id, a sub-id or the same maker prefix, and `mdfind` finds no copy anywhere; if Spotlight fails nothing is proposed and a note says so). LaunchAgents and Group Containers are not searched. Never preselected | `apps_leftovers::leftovers_of_removed_apps_are_conservative`, `::app_found_elsewhere_or_spotlight_failure_proposes_nothing`; UI `qa_apps_remover::leftovers_and_installers_groups` |
| A6 | `.pkg`, `.mpkg`, `.dmg` at the top of `~/Downloads`, never preselected | `apps_leftovers::installers_and_extensions_are_listed_and_removable`; UI `qa_apps_remover::leftovers_and_installers_groups` |
| A7 | Unused group: last opened more than 180 days ago; unknown dates are never "unused"; Apple apps excluded | `apps_inventory::lists_every_kind_of_app`; UI `qa_apps_remover::list_groups_and_numbers_match_the_disk` |
| A8 | Sparkle: `SUFeedURL`, appcast fetched with `curl -fsSL --max-time 15` via `env.sys`, own small XML parser (CDATA, entities, enclosure attributes, beta channels skipped), Sparkle-like comparison (build numbers decide, a lower marketing version is never offered), release notes from `<description>` or `sparkle:releaseNotesLink`, HTML stripped. Update = `open -b <bundle id>` (the app's own updater); Download opens the enclosure URL in the browser. Never downloads itself | `updater::tests::{version_table,appcast,html_notes}`, `apps_updates::sparkle_app_store_and_failures`, `::release_notes_link_is_fetched`; UI `qa_apps_updates::check_show_and_hand_off`, `::everything_offline` |
| A9 | App Store: lookup API, version compare, release notes; Update opens `macappstore://showUpdatesPage` | `updater::tests::lookup`, `apps_updates::sparkle_app_store_and_failures`; UI `qa_apps_updates::check_show_and_hand_off` |
| A10 | `softwareupdate --list` parsed (title, version, size, restart); Open Software Update opens the System Settings pane; failure shown, not fatal | `updater::tests::softwareupdate_list`, `apps_updates::up_to_date_and_bad_answers`; UI `qa_apps_updates::check_show_and_hand_off`, `::everything_offline` |
| A11 | Extensions group on the App Remover page: settings panes, internet plug-ins, Spotlight and Quick Look plug-ins, screen savers in `~/Library` and `/Library` (removable through the plan, `/Library` ones locked when not writable); Safari web extensions from `pluginkit -mAvvv` listed, locked "Part of <app>" | `extensions::tests::pluginkit_lines`, `apps_leftovers::installers_and_extensions_are_listed_and_removable`, `::pluginkit_failure_is_a_note`; UI `qa_apps_remover::leftovers_and_installers_groups` |

Network errors and timeouts are shown per app in plain words (`curl` exit codes 6, 7, 22, 28, 35/51/60 mapped), never fatal. Version comparison table: `updater::tests::version_table`.

MCP (`crates/mcp/src/apps.rs`): `list_apps`, `uninstall_preview(app, reset?)` (saves the plan, returns plan_id and digest; executed through the existing `execute_plan`, same "agents may clean" gate and digest check), `check_updates`. Test: `apps::tests::preview_then_execute_with_the_existing_gate`, `::updates_json`.

CLI (`apps/cleancraft/src/cli_apps.rs`): `cleancraft apps [--json]`, `cleancraft uninstall <app> [--reset] [--yes]` (without `--yes` prints the list, saves the plan, moves nothing), `cleancraft updates [--json]`. Test: `cli_apps::tests::apps_uninstall_updates`.

## Design notes

- `features::uninstaller::uninstall_plan(env, settings, app_path, reset) -> Plan`; `plan.feature = Uninstaller`, so `features::run_plan` works. The Uninstaller allow-list is `/Applications`, `~/Applications`, every leftover folder, `~/Downloads` and the extension folders. `Uninstaller::scan` (MCP `scan uninstaller`, CLI `scan uninstaller`) returns leftovers of removed apps, installers and extensions; the app list is `list_apps` / `cleancraft apps`.
- Everything moves through the executor (one `rename`, journal, undo). No delete calls added.

## PARTIAL / limits (honest)

- App bundles owned by root (installed by a `.pkg`) and leftovers in `/Library` need administrator rights. `/Library` leftovers are listed locked "Needs administrator rights"; a root-owned bundle is tried and reported "permission denied" in Done. There is no administrator-password path for uninstalling.
- Not searched for leftovers: `/private/var/folders`, package receipts (`pkgutil`), kernel and system extensions, audio plug-ins, Chrome/Firefox extensions.
- Safari extensions are listed only (they live inside their app).
- Updates: Sparkle and App Store only. No "custom updates" (Electron `latest-mac.yml`, Microsoft AutoUpdate, Homebrew). A Sparkle feed set only in user defaults (not in Info.plist) is not read. Signatures are not verified because CleanCraft never downloads updates.
- "Select All then Update" for many apps is not offered; each app has its own Update button.

## Hand checks on a real Mac

1. `mdls -name kMDItemLastUsedDate -raw /Applications/Safari.app` prints `2026-10-17 08:12:44 +0000` (or `(null)`). The parser expects exactly `%Y-%m-%d %H:%M:%S %z`.
2. `codesign -dv --verbose=2 /Applications/Safari.app 2>&1 | grep Authority` prints `Authority=Software Signing` first; an App Store app prints `Authority=Apple Mac OS Application Signing` (must NOT count as Apple).
3. `mdfind "kMDItemCFBundleIdentifier == 'com.apple.Safari'"` prints `/Applications/Safari.app`; for an unknown id it prints nothing and exits 0.
4. `pluginkit -mAvvv -p com.apple.Safari.web-extension`: header lines like `+    com.x.Ext(1.2)` followed by `Path = ...`, `Parent Bundle = ...`, `Display Name = ...`.
5. `curl -fsSL --max-time 15 "https://itunes.apple.com/lookup?bundleId=com.apple.dt.Xcode&entity=macSoftware"` returns JSON with `resultCount: 1` and `version`. If it returns 0 results for Mac apps, drop `&entity=macSoftware` in `updater::lookup_url`.
6. `softwareupdate --list` lines: `* Label: ...` then `Title: ..., Version: ..., Size: 123456KiB, Recommended: YES, Action: restart,`.
7. `open macappstore://showUpdatesPage` opens the App Store Updates page; `open "x-apple.systempreferences:com.apple.Software-Update-Settings.extension"` opens Software Update.
8. In the app: uninstall a drag-installed app with leftovers, check `~/.Trash` has the bundle and the files, then Undo puts everything back. Try a root-owned (pkg) app: expect "permission denied" in Done and nothing else lost.
9. macOS 14+: moving `~/Library/Containers/<id>` of another app may be refused by App Data protection; expect it listed under "Not moved" with the reason.
10. A Sparkle app: `defaults read /Applications/<App>.app/Contents/Info SUFeedURL`, then App Updates shows "installed -> newest" and What's New; Update opens the app.

## Shared-file changes (all small, announced on the board)

- `crates/core/src/lib.rs`: `pub mod apps; pub mod extensions; pub mod testkit_apps; pub mod updater;`
- `crates/core/src/features/mod.rs`: `pub mod leftovers; pub mod uninstaller;` and registry line `Box::new(uninstaller::Uninstaller)`.
- `crates/mcp/src/lib.rs`: `pub mod apps;` and `tool_router: Self::tool_router() + Self::apps_router()`.
- `apps/cleancraft/src/main.rs`: `mod cli_apps;`.
- `apps/cleancraft/src/cli.rs`: `#[command(flatten)] Apps(crate::cli_apps::AppsCmd)` and one match arm.
- No Cargo.toml changes, no new dependencies.

## For the coordinator

- Pre-existing clippy failure on main with rustc 1.97: `crates/core/src/policy.rs:195` and `:198` (`cloned_ref_to_slice_refs`, use `std::slice::from_ref(&caches)`). Left untouched (not my file); my code is clippy-clean with that patched locally.
