# Final check of CleanCraft (checker)

Branch `agent/checker`, a copy of `main` at `8a88525`. I changed no app code. I added two test files, `crates/core/tests/check_safety.rs` and `crates/ui-egui/tests/check_shell.rs`, and this report.

## Summary

- All 415 workspace tests pass (0 failed, 0 ignored). Clippy with `-D warnings` and `cargo fmt --all --check` are clean. My 5 extra tests also pass.
- 95 rows. 1 test named in the contract does not exist (L4, renamed). Every other named test exists and passes.
- Of the 35 rows claiming WORKS, I confirm 28 and downgrade 4 (J2, J4, J8, M1). 3 more WORKS rows (S11, J15, U3) keep their status, but their text is misleading.
- 2 rows with other statuses have misleading text (S12, L4).
- I found no permanent delete in app code, no path the executor can move outside an allow-list, and no test that touches real files.
- Most important finding: **every UI test runs as root** (`uid=0` on this machine). So `Env::can_modify` is always true in the fake home, and the UI tests never show what a normal Mac user sees for root-owned system folders. On a real Mac, System logs (J4) and `/Library/Updates` (J15) are always locked, and System caches (J2) always need the password. The UI only ever tested the case where they are freely movable. `check_system_folders_owned_by_root_lock_j4_on_a_real_mac` proves this by giving the fake folders another owner.

## Suite results

| Command | Result |
|---|---|
| `cargo test --workspace --no-fail-fast` (build 10 min 32 s, 2 CPUs) | 415 passed, 0 failed, 0 ignored, exit 0. Slowest binary `qa_every_control` (23 tests, 100 s) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no warnings (my test files included) |
| `cargo fmt --all --check` | clean (my files formatted with rustfmt) |
| `#[ignore]` anywhere | none |
| My tests: `check_safety` (4), `check_shell` (1) | 5 passed |

How I checked the named tests: `refs.py` (scratch script) read every row's Test column, expanded `a::{b, c}`, `::x` and `*`, and matched each name against `--list` from every built test binary and against the run log.

## Row by row

Verdicts: CONFIRMED (tests support the claim), DOWNGRADE to X (tests do not support WORKS), MISLEADING (status may stand, text is wrong or stale).

### 0. Safety rules

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| S1 | WORKS | CONFIRMED | Digest refused on change (core). The UI confirmation lists exactly the selected files with the recalculated size; unselected and locked items stay. Note: through MCP the digest goes from `scan` straight to the agent, so "the user confirmed" rests on the agent showing the list. The app only enforces the setting. |
| S2 | PARTIAL | CONFIRMED | The text is accurate. The Trash refusing and the quarantine fallback (`executor.rs:634`) are never exercised. A quarantine move from another disk fails with EXDEV and is reported as skipped (reliability report). |
| S3 | WORKS | CONFIRMED | One journal line per move, Undo per item and per plan, and undo refused when the place is taken. Commands (Tune-up) cannot be undone by design. |
| S4 | WORKS | CONFIRMED | Forged items outside the allow-list are refused (core). My `check_symlinked_allow_root_is_never_followed` adds that a symlinked allow root (`~/Library/Caches` -> `~/Documents`) gets nothing proposed and nothing moved. Gap: `policy::check` checks links from the item up to the allow root, never above it (`~/Library` itself a link). That is unlikely on a Mac. |
| S5 | WORKS | CONFIRMED | Fence tests in core. |
| S6 | MAC-CHECK | CONFIRMED | Skipping is tested with FakeSys. An lsof failure skips the item (safe direction). |
| S7 | MAC-CHECK | CONFIRMED | |
| S8 | WORKS | CONFIRMED | `.cargo/config.toml` sets `CLEANCRAFT_FORBID_REAL=1`, and `Env::real()` is used only in `main.rs` and `cli.rs`. Two unit tests run real programs, both harmless: `/bin/sh -c printf` and `/bin/sleep` (`sys.rs:680-705`). |
| S9 | WORKS | CONFIRMED | Permission denied, disk full and in use come from `env.inject_fault`, checked before the rename (the standard allows this). The real io error mapping (`skip_for`) never runs with a real EACCES, because tests run as root. |
| S10 | WORKS | CONFIRMED | The gate is tested by calling `execute_plan` (refused without the setting, refused with a bad digest, moves on disk with both). The stdio server is not driven (see U6). |
| S11 | WORKS | MISLEADING | The text says REL-8 is open, but it was fixed. The app now warns and keeps a copy. After a damage, though, the ignore list is still not applied (defaults are used). The CLI and MCP use `Settings::load`, which drops a damaged file silently. Tests cover System Junk only. In code, the other scans honour the list through `ctx.ignored`/`walk_area`. Disk Map shows ignored items and refuses them only at removal (`features::run_plan`). |
| S12 | PARTIAL | MISLEADING | Says "open defect PAR-3", but PAR-3 is fixed (`review_parity::menu_bar_option_only_when_there_is_a_menu_bar_item` passes). A leftover remains: the Settings card is still titled "Agents and menu bar" (`pages/settings.rs:133`) with no menu bar option under it. |

### 1. Quick Sweep

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| Q1 | PARTIAL | CONFIRMED | |
| Q2 | WORKS | CONFIRMED (gaps) | The real modules are run by clicks, with disk and number checks and Undo per module (`qa_parity_flows`). The sweep's own result page covers only "file in use". A journal that cannot be written, permission denied and disk full are never shown on the sweep page. The failing-module case uses stand-ins. Adding the log-unwritable case would close it. |
| Q3 | WORKS | CONFIRMED | The skip is saved and left out of Run. The "45% opacity" dimming is stated but not asserted. |

### 2.1 System Junk

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| J1 | WORKS | CONFIRMED | Note: the sandboxed part (`~/Library/Containers/*/Data/Library/Caches`) can hit the macOS 14+ App Data prompt. Hand check H-C3. |
| J2 | WORKS | DOWNGRADE to MAC-CHECK | `/Library/Caches` is root-owned on a Mac, so every item takes the administrator path and none is preselected. That path is tested only in core (`fix_admin_move`), never by a UI click. The UI test moves system caches only because it runs as root. Also, `Env::can_modify` ignores group write (`env.rs:174`), so an admin user gets the password dialog even for folders the admin group may change. Proven by `check_system_folders_owned_by_root_lock_j4_on_a_real_mac`. |
| J3 | WORKS | CONFIRMED | |
| J4 | WORKS | DOWNGRADE to MAC-CHECK | `system_junk.rs:739-741` calls `area(..., admin=false)` for `/Library/Logs` and `/private/var/log`. Neither is user-changeable on a Mac, so every System log item is locked "Needs administrator rights". "Default: selected" never happens for a normal user. My check test proves every J4 item is locked and unselected when the folders belong to another user. The UI test cleans them only because it runs as root. |
| J5 | WORKS | CONFIRMED | |
| J6 | MAC-CHECK | CONFIRMED | |
| J7 | MAC-CHECK | CONFIRMED | |
| J8 | WORKS | DOWNGRADE to MAC-CHECK | `program_gone` (`system_junk.rs:1276`) treats *any* `symlink_metadata` error as "gone", including "Operation not permitted" from macOS privacy protection. An agent whose program sits in a protected folder (another app's container, Documents) would be called broken and **preselected** (Medium risk; undo exists). It should test `ErrorKind::NotFound` only. Root-run tests cannot show this (domain report item 5). |
| J9 | WORKS | CONFIRMED | |
| J10 | MAC-CHECK | CONFIRMED | |
| J11 | MAC-CHECK | CONFIRMED | |
| J12 | WORKS | CONFIRMED | |
| J13 | WORKS | CONFIRMED | |
| J14 | WORKS | CONFIRMED | |
| J15 | WORKS | MISLEADING | The Sparkle half works as claimed. The `/Library/Updates` half is locked on a real Mac (root-owned, `system_junk.rs:577-588`); my check test proves it. The text should say so. |
| J16 | WORKS | CONFIRMED | Not preselected, and the UI checks it is not moved. Moving a hand-selected disk image is never clicked, but it runs the same plan-page path. |
| J17 | WORKS | CONFIRMED | |
| J18 | WORKS | CONFIRMED | |
| J19 | WORKS | CONFIRMED | Same remark as J16. |
| J20 | MAC-CHECK | CONFIRMED | |
| J21 | WORKS | CONFIRMED | `click_nth` (used for Reveal, Ignore, Undo) does not check the control is inside the window, unlike `click`. |

### 2.2 Mail Downloads, 2.3 Trash Bins

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| M1 | WORKS | DOWNGRADE to MAC-CHECK | The whole folder is inside `~/Library/Containers/com.apple.mail`. On macOS 14+ reading or moving another app's container needs Full Disk Access or App Data consent (apps report item 9, cleanup report item 12). The "without Full Disk Access" UI test only blocks `~/Library/Mail`, never the container. |
| M2 | MAC-CHECK | CONFIRMED | |
| M3 | MAC-CHECK | CONFIRMED | |
| T1 | WORKS | CONFIRMED | On a Mac `~/.Trash` needs Full Disk Access. That case is handled and tested (`unreadable_trash_is_noted`), but on a Mac without access the numbers will mostly be a note. |
| T2 | MAC-CHECK | CONFIRMED | Only Finder empties. Nothing in CleanCraft deletes. |

### 3. Protection

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| P1 | MAC-CHECK | CONFIRMED | The status has no text saying what to check. See hand checks H-P1. |
| P2 | MAC-CHECK | CONFIRMED | |
| P3 | MAC-CHECK | CONFIRMED | |
| P4 | MAC-CHECK | CONFIRMED | |
| P5 | MAC-CHECK | CONFIRMED | |
| P6 | PARTIAL | CONFIRMED | |

### 4. Performance

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| K1 | MAC-CHECK | CONFIRMED | |
| K2 | MAC-CHECK | CONFIRMED | The named tests cover only Cancel. A successful `purge` is tested through Live Monitor (`free_up_ram_*`, R2), never from Tune-up Tasks. |
| K3 | MAC-CHECK | CONFIRMED | Thins local Time Machine snapshots, a permanent system action (stated as "cannot be undone"). |
| K4 | MAC-CHECK | CONFIRMED | Deletes local snapshots, permanent (stated). The UI assertion is an OR ("no snapshots" or "could not list"). No UI run. |
| K5 | MAC-CHECK | CONFIRMED | Core only, no UI run. |
| K6 | MAC-CHECK | CONFIRMED | Core only. |
| K7 | MAC-CHECK | CONFIRMED | Core only. |
| K8 | MAC-CHECK | CONFIRMED | Only the confirmation and Cancel are tested. The command is never run in any test. |
| K9 | MAC-CHECK | CONFIRMED | |
| K10 | MAC-CHECK | CONFIRMED | |
| L1 | MAC-CHECK | CONFIRMED | |
| L2 | MAC-CHECK | CONFIRMED | |
| L3 | MAC-CHECK | CONFIRMED | |
| L4 | MAC-CHECK | MISLEADING | Names `perf_startup::remove_agent_plist_through_the_plan_with_bootout_first_and_undo`, which does not exist. It is now `..._with_bootout_after_the_move_and_undo` (REL-6 changed the order). It passes. |
| L5 | PARTIAL | CONFIRMED | |

### 5. Applications

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| A1 | MAC-CHECK | CONFIRMED | |
| A2 | MAC-CHECK | CONFIRMED | |
| A3 | MAC-CHECK | CONFIRMED | |
| A4 | MAC-CHECK | CONFIRMED | |
| A5 | MAC-CHECK | CONFIRMED | |
| A6 | WORKS | CONFIRMED (gaps) | Clicks, disk check and numbers are verified. This group has no Undo or error case of its own. It shares the run and Done code (`pages/apps.rs:238`, `:994`) that the uninstall tests cover with errors and Undo. Only `.pkg` is used; `.dmg` only in core. |
| A7 | MAC-CHECK | CONFIRMED | |
| A8 | MAC-CHECK | CONFIRMED | |
| A9 | MAC-CHECK | CONFIRMED | |
| A10 | MAC-CHECK | CONFIRMED | |
| A11 | MAC-CHECK | CONFIRMED | |

### 6. My Clutter, 7. Disk Map

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| C1 | PARTIAL | CONFIRMED | |
| C2 | WORKS | CONFIRMED | Keep-one is enforced in the UI and again before the move (`check_copies_stay`). A keeper that vanished after the scan is tested in core. Permission and in-use errors are not in the duplicates UI test, but it uses the shared plan page tested in J. |
| C3 | WORKS (formats) | CONFIRMED | Same remark as C2. No Undo click in this test. |
| C4 | MAC-CHECK | CONFIRMED | |
| C5 | EXCLUDED | CONFIRMED | |
| D1 | PARTIAL | CONFIRMED | |
| D2 | MAC-CHECK | CONFIRMED | "Used" is now total minus free (DAT-5). The row text could say so. |
| D3 | WORKS | CONFIRMED | Full click flow, items moved on disk, sizes refreshed, Undo, permission denied and in use. |

### 8. Live Monitor, 9. App shell

| ID | Claimed | Verdict | Why |
|---|---|---|---|
| R1 | MAC-CHECK | CONFIRMED | |
| R2 | MAC-CHECK | CONFIRMED | The `free_up_ram_*` wildcards match 8 passing tests. |
| R3 | MAC-CHECK | CONFIRMED | |
| R4 | MAC-CHECK | CONFIRMED | |
| R5 | MAC-CHECK | CONFIRMED | |
| R6 | PARTIAL | CONFIRMED | |
| R7 | PARTIAL | CONFIRMED | |
| U1 | WORKS | CONFIRMED | My `check_sidebar_rows_positions` shows all 17 rows inside 1280 x 820 (Settings ends at y 718). Stale comments in `qa_cleanup_trash.rs:100` and `qa_perf_tasks.rs:104` still say History is below the window; they should be removed. |
| U2 | WORKS | CONFIRMED | |
| U3 | WORKS | MISLEADING | The method and ignore list are proven by clicks. Thresholds are proven saved; their effect on the next scan is proven only through saved settings in other tests. `review_reliability_ui::damaged_settings_are_reported_and_the_ignore_list_survives` passes on `warned \|\| !proposed`, so it never checks the "kept as a copy" claim, and the test name says the ignore list survives when it does not. |
| U4 | MAC-CHECK | CONFIRMED | |
| U5 | PARTIAL | CONFIRMED | `scan`, `clean`, `undo`, `history`, `features` and `access` indeed have no test. |
| U6 | PARTIAL | CONFIRMED | |
| U7 | PARTIAL | CONFIRMED | |

Counts:

| Claimed | Rows | Confirmed | Downgraded | Misleading |
|---|---|---|---|---|
| WORKS | 35 | 28 (2 with test gaps: Q2, A6) | 4 (J2, J4, J8, M1 to MAC-CHECK) | 3 (S11, J15, U3) |
| PARTIAL | 12 | 11 | 0 | 1 (S12) |
| MAC-CHECK | 47 | 46 | 0 | 1 (L4) |
| EXCLUDED | 1 | 1 | 0 | 0 |

## Safety audit

**Permanent deletion.** In `crates/*/src` and `apps/*/src` (test modules excluded) there is no `remove_file`, `remove_dir`, `remove_dir_all`, `unlink` or `rm`. `check_app_code_never_deletes_permanently` now guards this. Trash emptying goes only through `osascript ... Finder to empty trash` (`features/trash.rs:26`). Permanent system actions exist only as explicit, confirmed Tune-up commands: K3/K4 remove local Time Machine snapshots, K9 VACUUMs the Mail index, P6 runs `tccutil reset`.

**Executor fence.** Every move passes `policy::check_move` right before its single `rename`, plus the open-file and running-app checks. Destinations are checked with `env.contains`. Administrator moves are only `/bin/mv -n -- 'a' 'b'` lines, and FakeSys panics on a path outside the fake root. Remarks:
- The allow-list match is case-sensitive while protected places are compared case-insensitively. That is the safe direction.
- Links above the allow root are not checked (see S4).
- Undo of a `Toggled` record runs `/bin/sh -c <undo_command>` read from `journal.jsonl`. Anyone who can write that file can make Undo run a command as the user. The journal is in a protected data folder, so this is low risk, but worth knowing.
- `check_restore` only requires the original path to be inside the env root, which on a real Mac is "/". An edited journal could restore into Documents. Low risk, since the stored item came from there.
- The MCP digest is not a secret: an agent that can write the saved plan file could recompute it. The fence and `validate` still apply, so nothing outside the allow-lists can move.

**Tests and real files.** No test calls `Env::real()` except `real_is_refused_under_cargo` (expects an error). No test uses hard-coded `/Users` or `~` paths outside the fake root (they are fake-root strings or parser samples). The `remove_dir_all`/`remove_file` calls in tests (`review_reliability_core.rs`, `review_reliability_ui.rs:307`) act on paths inside the FakeMac temp folder. `executor::uid()` reads the real home folder's metadata (read only). No real network: `curl` goes through `env.sys`.

**Not copying the reference app.** No UI string contains the reference app, the reference vendor, Smart Scan, Space Lens or Smart Care (`check_texts_follow_the_rules_and_name_no_competitor`). The names appear only in docs and code comments. Two places users can see:
- `scripts/install-app.sh:68` puts "Not affiliated with the reference vendor." in the app's Info.plist copyright string, which Finder's Get Info and the About panel show.
- The internal ids `smart_care`, `space_lens`, `similar_photos`, `mail_attachments` are printed by `cleancraft features` (`cli.rs:107`) and MCP `list_features` (`mcp/src/lib.rs:130`). They echo the reference app module names.
- Only the app icon `assets/AppIcon.png` and the Inter fonts (OFL) are bundled. I did not compare the icon visually.

## UI text rules

No em dash (U+2014) anywhere in `crates`, `apps` or `scripts`. The whole word "nor" appears in no string literal. The only hits are a doc comment in `features/mod.rs:72` (not shown to users) and the Inter font licence. Both `check_texts_follow_the_rules_and_name_no_competitor` (all source literals) and the existing `qa_every_control::every_page_text_follows_the_rules` (rendered screens) pass.

## Hand checks on a real Mac

Merged from every report in `docs/agents/reports/` with duplicates removed, plus my own (marked *new*). Run as your normal admin user, not root.

### Access and safety
- **H-S1 Full Disk Access (S7, U4).** Start CleanCraft without Full Disk Access. The banner shows. Open Privacy Settings must open System Settings > Privacy & Security > Full Disk Access. Turn it on, click Check Again: the banner goes and "Full Disk Access is on." appears.
- **H-S2 Open files (S6).** Open `~/Downloads/Invoice (1).pdf` in Preview and run `/usr/sbin/lsof -F n -w | grep Invoice`. Expect `n/Users/<you>/Downloads/Invoice (1).pdf`. Repeat with `Café.pdf` and check lsof does not escape the bytes. Clean it with Large & Old Files: "open in a running app". Time `lsof -F n -w > /dev/null` on a busy Mac.
- **H-S3 Running apps.** `ps -axo pid=,comm= | grep Safari` prints `/Applications/Safari.app/Contents/MacOS/Safari`. An app started from a translocated copy (`/private/var/folders/.../AppTranslocation/`) is not seen as running from `/Applications`.
- **H-S4 Trash refused, quarantine fallback (S2).** `chmod 500 ~/.Trash`, clean System Junk. Items go to `~/Library/Application Support/CleanCraft/Quarantine`. Restore with `chmod 700 ~/.Trash`.
- **H-S5 Other disk.** Clean an item on an external disk with the quarantine method. Expect "skipped" (Cross-device link), nothing lost. With the Trash method it goes to `/Volumes/<disk>/.Trashes/<uid>` and Undo brings it back.
- **H-S6 Real permission denied.** `sudo chown root ~/Library/Caches/<app>/x; sudo chmod 700 ~/Library/Caches/<app>`, clean that cache. Expect "permission denied" and nothing else affected. Undo the chown afterwards.
- **H-S7 Trash emptied before Undo.** Clean, empty the Trash in Finder, trash another file with the same name, then Undo in History. Expect "the removed copy is gone from ...", and the other file stays.
- **H-S8 Journal.** After a clean, `tail -c 1 ~/Library/Application\ Support/CleanCraft/journal.jsonl | xxd` prints `0a`. Run `cleancraft clean <plan> --yes`, `cleancraft undo <plan>`, the same clean again, `cleancraft undo <plan>`. Both undos put the items back (ids end in `~2` the second time).
- **H-S9 Case-insensitive names.** `ls -d ~/documents` lists Documents. In Large & Old Files > Another folder type `~/documents`. Expect "... is protected (the contents of Documents)". Same in Disk Map > Folder to map.
- **H-S10 Photos library.** Disk Map > Map Home Folder: the Pictures row (holding "Photos Library.photoslibrary") has a disabled checkbox "protected: it holds a Photos library".
- **H-S11 Damaged settings (new).** Quit CleanCraft and cut the last bytes off `~/Library/Application Support/CleanCraft/settings.json` (`truncate -s -4 <file>`). Start: the notice names the `settings.damaged.json` copy, and the ignore list is not applied until you fix or re-enter it. `cleancraft scan system_junk` prints the same warning (fixed after this check).

### System Junk
- **H-J1 Root-owned folders (J2, J4, J15) (new).** `ls -ld /Library/Caches /Library/Logs /private/var/log /Library/Updates`. Scan System Junk. Expected from the code: System logs and macOS update downloads are all locked "Needs administrator rights". System caches in a group-writable `/Library/Caches` (`root:admin`, mode 1775) are movable without the password for admin users (fixed after this check: group write and `id -G` are now honoured); root-only entries need the password and are not preselected.
- **H-J2 Administrator move with an awkward name.** `sudo mkdir "/Library/Caches/it's a \$test \`x\`"`. Tick it by hand and clean: one password dialog, and the folder appears in the quarantine with the exact name.
- **H-J3 Broken agents and privacy protection (J8) (new).** Without Full Disk Access, create `~/Library/LaunchAgents/test.cleancraft.plist` with `Program` set to a file inside `~/Library/Containers/<another app>/Data/` that exists. Scan: it must NOT be listed as broken. Fixed after this check: only "not found" counts as broken. Also run a check on an agent pointing at a truly missing program.
- **H-J4 Languages (J6).** `defaults read -g AppleLanguages` prints `(` / `"en-US",` / `)`. J6 keeps those languages.
- **H-J5 Universal apps (J7).** `uname -m` prints `arm64`. `lipo -archs /Applications/<universal>.app/Contents/MacOS/<exe>` prints `x86_64 arm64`. `lipo -detailed_info <same>` has `architecture x86_64` blocks with `size N`. J7 shows that size.
- **H-J6 Broken preferences (J9).** `echo '<plist><dict>' > ~/Library/Preferences/test.cleancraft.plist`. `plutil -lint` exits non-zero and J9 lists it.
- **H-J7 iOS backups (J11).** The backup `Info.plist` has `Device Name`, `Target Identifier`, `Last Backup Date` (needs Full Disk Access).
- **H-J8 Sandboxed caches (J1, M1).** Without Full Disk Access, scanning `~/Library/Containers/*` may show the macOS 14+ "access data from other apps" prompt. Note what happens if you refuse it.
- **H-J9 Smallest window.** Resize to 1060 x 680 and click through System Junk results. Select None and the row buttons stay visible.

### Mail and Trash
- **H-M1 Mail Downloads container (M1) (new).** With and without Full Disk Access, open an attachment in Mail, then Scan Mail Downloads. Check that `~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads` is readable and that the move works.
- **H-M2 Accounts (M2).** With an IMAP account and a POP or On My Mac mailbox, IMAP attachments appear and POP ones do not. For UUID account folders, run `sqlite3 -separator '|' ~/Library/Accounts/Accounts4.sqlite "SELECT ZACCOUNT.ZIDENTIFIER, ZACCOUNTTYPE.ZIDENTIFIER FROM ZACCOUNT JOIN ZACCOUNTTYPE ON ZACCOUNT.ZACCOUNTTYPE = ZACCOUNTTYPE.Z_PK;"`. It prints `UUID|com.apple.account.IMAP` rows matching the folder names in `~/Library/Mail/V10`.
- **H-M3 Edited attachment (M3).** Edit an opened attachment in place (Preview, TextEdit) and scan: it must be unselected.
- **H-T1 Empty Trash (T2).** Use Empty Trash in Finder. macOS asks once for Automation ("CleanCraft wants to control Finder"). After Allow, Finder empties and History shows `osascript -e 'tell application "Finder" to empty trash'`. If Finder shows its own warning and nobody answers, after 60 s the page says "/usr/bin/osascript did not finish within 60 seconds and was stopped". Check whether 60 s is too short for the first consent prompt.
- **H-T2 Trash sizes (T1, R3).** Without Full Disk Access the Trash shows a note. With it, the size matches `du -sk ~/.Trash` times 1024, and `stat -f %b ~/.Trash` shows the folder's own blocks.

### Protection and privacy
- **H-P1 Threat Check (P1, P2).** `sudo /usr/bin/tee /Library/LaunchAgents/com.genieo.test.plist <<< '<?xml version="1.0"?><plist version="1.0"><dict><key>Label</key><string>com.genieo.test</string><key>ProgramArguments</key><array><string>/usr/bin/true</string></array></dict></plist>'`, then Threat Check > Scan. The row says "(needs the administrator password)". Move to Quarantine shows one password dialog, and the file is in the quarantine, owned by root. Undo in History asks again and puts it back. Repeat with Cancel: "the administrator password was not given, so it was not moved".
- **H-P2 Signatures.** `codesign -dv --verbose=2 /Applications/Safari.app/Contents/MacOS/Safari` prints `Authority=Software Signing` first. A Developer ID app prints `Authority=Developer ID Application: Name (TEAMID)`. An App Store app prints `Apple Mac OS Application Signing` and must not count as Apple. Ad hoc prints `Signature=adhoc`. Unsigned prints `code object is not signed at all`. `spctl --assess --type execute <unsigned app>` gives `rejected`, exit 3.
- **H-P3 Rule list.** Verify each label, app name and path in `malware_rules.rs` against public sources before release (written from memory).
- **H-P4 Browsers (P3).** Brave, Edge, Vivaldi and Arc cache folders (`~/Library/Caches/BraveSoftware/Brave-Browser/<Profile>` and the others) and their names in `ps -axo pid=,comm=`.
- **H-P5 Recent items (P4).** After cleaning, open Apple menu > Recent Items. If the lists come back, `sharedfilelistd` rewrote them.
- **H-P6 Wi-Fi (P5).** `networksetup -listallhardwareports` shows `Wi-Fi` / `en0`. `networksetup -listpreferredwirelessnetworks en0` lists the names (check they are not redacted). "Wi-Fi Settings" opens System Settings > Wi-Fi.
- **H-P7 App permissions (P6).** With Full Disk Access, `sqlite3 -readonly -separator '|' ~/Library/Application\ Support/com.apple.TCC/TCC.db "SELECT service, client, auth_value FROM access WHERE client_type = 0;"` gives rows like `kTCCServiceCamera|us.zoom.xos|2`. Reset one camera permission; the app asks again next time. Check `tccutil reset ScreenCapture <bundle id>` without sudo.

### Tune-up Tasks and Startup Items
- **H-K1 Commands.** `sudo dscacheutil -flushcache; sudo killall -HUP mDNSResponder` and `sudo purge`. `tmutil thinlocalsnapshots / 999999999999 4` runs without sudo and changes `df`. `tmutil listlocalsnapshots /` lines look like `com.apple.TimeMachine.<date>.local`. Also check `sudo tmutil deletelocalsnapshots <date>`, `sudo mdutil -E /`, the `lsregister` path and `-kill`, `sudo diskutil resetUserPermissions / $(id -u)`, `diskutil verifyVolume /` without sudo (within 20 min), Mail's `Envelope Index` VACUUM with Mail quit (check the V number), and `sudo periodic daily weekly monthly` on macOS 13. Cancel in a password dialog shows "Cancelled, nothing changed" with no journal line.
- **H-K2 Versions.** `id -u` and `sw_vers -productVersion` (like `14.6.1`).
- **H-L1 Login items (L1, L3).** Run the System Events script from perf report item 4. It prints `Name<TAB>/Applications/App.app`, and the first run shows the Automation prompt (denied gives -1743). Turn a login item off and on.
- **H-L2 Agents (L2, L3).** Run `launchctl print-disabled gui/$(id -u)` and `launchctl print-disabled system` (with and without sudo); the format is `"label" => disabled`. Turn a user agent off: History Undo runs `launchctl enable ... && launchctl bootstrap ...`. A global agent: check it can be disabled in the gui domain without the password. A daemon: the password dialog, and Cancel changes nothing.
- **H-L3 Remove (L4).** Remove a user agent: `launchctl print gui/$(id -u)/<label>` then fails. With `chmod a-w ~/Library/LaunchAgents` it is skipped and still running. Restore with `chmod u+w`.
- **H-L4 Heavy apps (L5).** Check the `ps -axo pid=,uid=,%cpu=,%mem=,comm=` columns (decimal comma in some locales). Quit TextEdit through the app.

### Applications
- **H-A1 Inventory.** `mdls -name kMDItemLastUsedDate -raw /Applications/Safari.app` prints `YYYY-MM-DD HH:MM:SS +0000` or `(null)`. `mdfind "kMDItemCFBundleIdentifier == 'com.apple.Safari'"` prints the path; an unknown id prints nothing and exits 0.
- **H-A2 Uninstall.** Uninstall a drag-installed app with leftovers: `~/.Trash` has the bundle and files, and Undo restores them. Uninstall a `.pkg` app (`ls -ld` shows root): one password dialog, the bundle goes to the quarantine. As a standard user macOS asks for an admin name. An App Store app owned by root:wheel: see what happens when it is moved.
- **H-A3 Extensions.** `pluginkit -mAvvv -p com.apple.Safari.web-extension` prints `+    com.x.Ext(1.2)` then `Path =`, `Parent Bundle =`, `Display Name =`.
- **H-A4 Updates.** `curl -fsSL --max-time 15 "https://itunes.apple.com/lookup?bundleId=com.apple.dt.Xcode&entity=macSoftware"` returns `resultCount: 1` (else drop `&entity=macSoftware`). Check `softwareupdate --list` (`Size: NNNNKiB` against System Settings). `open macappstore://showUpdatesPage` and the Software Update URL. For a Sparkle app (`SUFeedURL`), App Updates shows the newest version and What's New. Quick Sweep "N updates" equals App Updates minus macOS updates.
- **H-A5 Ignore an app.** In App Remover, Ignore leaves the list and the app appears in Settings > Ignore list.

### Files, Disk Map, Monitor
- **H-C1 Last opened.** `mdls -raw -name kMDItemLastUsedDate a b | od -c` gives values separated by `\0`. `stat -f "%Sa %Sm" file` changes atime after opening the file.
- **H-C2 Downloads source (C4).** `xattr -p com.apple.quarantine ~/Downloads/<from Safari>` gives `0083;<hex>;Safari;<UUID>`. Then "Downloads by source app" shows "Saved by Safari".
- **H-C3 HEIC (C3).** `sips -s format png -Z 256 IMG.HEIC --out /tmp/t.png` works and `sips -g pixelWidth -g pixelHeight` reports the size. Two HEIC shots of the same scene group together.
- **H-C4 Files and Folders prompts.** The first scan of Downloads, Pictures and Movies may prompt. If you deny, the scans say "folders could not be read".
- **H-D1 Storage bar (D2).** `df -Pk ~` columns 2 to 4. Used plus free equals the disk size in Finder's Get Info for Macintosh HD.
- **H-D2 Mount points (D1).** `hdiutil attach some.dmg -mountpoint ~/MountTest`, then Map Home Folder: MountTest shows 0 bytes and the note about other disks.
- **H-R1 Monitor parsers.** Compare with the samples in `testkit_monitor.rs` for `sysctl hw.memsize hw.ncpu kern.boottime vm.loadavg vm.swapusage kern.memorystatus_vm_pressure_level`, `top -l 2 -n 0 -s 1 | grep "CPU usage"`, `ps -axco pid=,%cpu=,rss=,comm=`, `vm_stat` (page size 16384 on Apple silicon), `pmset -g batt` (on battery, charging gives "Time to full", desktop), `ioreg -rn AppleSmartBattery` (health against System Settings, NominalChargeCapacity or AppleRawMaxCapacity) and `netstat -ib` (VPN counts traffic on both utun and en0). A 16 GB Mac reads "x GB of 16.0 GB" in binary units.
- **H-R2 Free Up RAM.** The password dialog appears. Cancel shows "Cancelled. Nothing was run." with no History entry. With the password, History shows `/usr/sbin/purge`.
- **H-R3 CLI and MCP.** `cleancraft status`, `cleancraft sweep`, and MCP `system_status` twice (the second call has a network speed) and `sweep_preview`.

### Install
- **H-U1 (U7).** Run `bash scripts/install-app.sh`, then open `/Applications/CleanCraft.app`. Check Get Info: the copyright line reads "MIT OR Apache-2.0. CleanCraft, an original app." (the reference vendor mention removed after this check).

## Recommended fixes (not made: app code is out of my scope)

Coordinator, after this check: 1 to 4 are done (commit after c06d61b, tests in crates/core/tests/fix_coordinator.rs and review_reliability_ui.rs); from 5, running UI tests as a non-root user is still open.

1. `system_junk::program_gone`: treat only `ErrorKind::NotFound` as gone (J8).
2. `Env::can_modify`: consider group write for the user's groups, or say in J2/J4 that admin users still need the password.
3. FEATURES: J2, J4, J8 and M1 to MAC-CHECK. Add the `/Library/Updates` lock to J15. Remove the stale REL-8 (S11) and PAR-3 (S12) notes. Fix the L4 test name. Fix the U3 damaged-settings wording.
4. Settings card title "Agents and menu bar" to "Agents". Remove "Not affiliated with the reference vendor." from the Info.plist. Consider renaming the CLI/MCP ids `smart_care` and `space_lens`.
5. Tests: run some UI tests as a non-root user (or with fake folder owners, as in my check test), add the log-unwritable case to the Quick Sweep page, make `click_nth` check the window bounds, and remove the stale "History below the window" comments.
