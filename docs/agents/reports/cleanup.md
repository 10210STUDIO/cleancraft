# cleanup report (System Junk, Mail Downloads, Trash Bins, Privacy Traces)

Branch `agent/cleanup`. Rows J1-J21, M1-M3, T1-T2, P3-P6. Every scan only looks; removing goes through the shared plan page, the digest check and `executor::execute` (move to Trash or quarantine, journal, undo). Every command my modules run goes through `env.sys` and is answered by `FakeSys` in tests.

## What works (with tests)

Core tests: `crates/core/tests/cleanup_{junk,mail,trash,privacy}.rs` (25 tests). UI end-to-end tests: `crates/ui-egui/tests/qa_cleanup_{junk,mail,trash,privacy}.rs` (16 tests). Fixtures: `crates/core/src/testkit_cleanup.rs` (`junk_mac`, `mail_mac`, `trash_mac`, `privacy_mac`, tool answers, `make_unlistable`, `age_dir`).

Every UI test clicks through the page on a FakeMac and checks the headline size, category sizes and the "N selected, X" line against `tree_size` recalculated from disk, the files that really moved and the freed number, and the error paths (permission denied, file open in an app, disk full, action log unwritable).

### System Junk (`features/system_junk.rs`)
- J1 user caches, one item per app (`group` = app); sandboxed `~/Library/Containers/*/Data/Library/Caches/*` (each container's Caches folder is its own allow root, links refused); iCloud and sign-in caches never.
- J2, J4 system caches and logs (`/Library/Caches`, `/Library/Logs`, `/private/var/log`): locked "Needs administrator rights" when the folder cannot be changed.
- J3 user logs, J5 crash reports grouped per app (`Example-2026-...ips` -> Example).
- J8 launch agents whose `Program` / `ProgramArguments[0]` is missing (plist crate). J9 preferences the plist crate cannot read, confirmed with `plutil -lint`; when plutil cannot run the item is listed locked ("could not double check").
- J11 iOS backups: per device (Info.plist `Target Identifier`), the newest is listed locked, older ones offered unselected.
- J12 `.ipsw`, J13 Xcode (DerivedData selected except `ModuleCache.noindex` which is never listed, device support and simulator caches selected, archives unselected), J14 unfinished downloads (locked while open in an app), J15 Sparkle downloads (`~/Library/Caches/<app>/org.sparkle-project.Sparkle`, the rest of that app's cache stays in J1) and `/Library/Updates` (locked without admin rights), J16 `.dmg` in Downloads not opened or changed for `settings.downloads_days`, unselected, J17 developer caches per tool (npm, Homebrew, pip, CocoaPods, SwiftPM, Cargo selected; Gradle unselected), J18 Claude/Cursor caches and logs, J19 local models unselected (LM Studio per model; Ollama as one item because its blobs are shared between models, the reason lists the model names).
- Report-only, locked and outside the allow-list (an edited saved plan cannot move them, `saved_plan_cannot_unlock_report_only_items`): J6 language files (keeps Base, English, `defaults read -g AppleLanguages` and `settings.keep_languages`), J7 universal binaries (`uname -m`, `lipo -archs`, size of the unused slices from `lipo -detailed_info`), J10 document versions, J20 deleted users.
- No item is inside another item; no path twice (`sizes_match_disk_and_the_fence_matches_the_locks`).
- J21: `review_controls_search_sort_select_reveal_ignore` proves search, sort by size and by name (row order checked), Select None / Select All on visible rows, a row checkbox, the category checkbox, Reveal recorded by FakeSys as `/usr/bin/open -R <path>`, Ignore saved to settings and absent after Scan Again. `locked_items_show_why_and_cannot_be_selected` proves locked rows stay unselected.

### Mail Downloads (`features/mail.rs`)
- M1 opened attachments (`.../Mail Downloads/**`), per file.
- M2 attachments under `~/Library/Mail/V*/<account>/**/Attachments/**`, only for accounts whose server keeps the original. Account type: folder prefix (`IMAP-`, `EWS-`, `Exchange-` server; `POP-` and `Mailboxes` local), then `V*/MailData/Accounts.plist` (`MailAccounts[].uniqueId/AccountType`), then `~/Library/Accounts/Accounts4.sqlite` via `sqlite3`. POP and On My Mac are left out with a note; unknown types are listed locked. Only server accounts' Attachments folders are in the allow-list. `Messages` folders are never entered.
- M3 a file whose mtime is more than 2 minutes after its creation time or its folder's time is not preselected.
- Mail running: every item locked "Quit Mail first"; Mail opened after the scan: `prepare` refuses the cleanup ("Mail is open. Quit Mail, then scan again."), nothing moves.

### Trash Bins (`features/trash.rs`)
- T1: `~/.Trash`, `/Volumes/*/.Trashes/<uid>` (uid of the home folder), Mail `Deleted Messages.mbox` / `Trash.mbox`; size = sum of each entry's tree size (Mail: the mailbox tree, count of `.emlx`).
- Honest and safe: these items are already in a Trash, so every item is locked and the feature's allow-list is empty (an edited plan is refused by the fence, `an_edited_plan_still_cannot_move_trash`). "Review and Clean" never appears.
- T2: "Empty Trash in Finder" (toolbar, only when a Finder Trash has something) opens a confirmation saying it cannot be undone and CleanCraft cannot put the items back; "Empty Trash" checks the journal is writable, runs `osascript -e 'tell application "Finder" to empty trash'` through `env.sys` (on a worker thread in the app, inline in tests), logs `ActionKind::Command` with the bytes freed, shows the result and scans again. Automation refusal (-1743) and cancel (-128) get plain explanations. Mail's trash is not emptied by Finder; its row says to use Mailbox > Erase Deleted Items.

### Privacy Traces (`features/privacy.rs`)
- P3 Safari, Chrome, Brave, Edge, Vivaldi, Arc (Chromium profiles = folders with `Preferences`, never System or Guest profile), Firefox profiles: history, cookies, cache, last session, download list (Safari `Downloads.plist`; in the history file for the others, said in the reason), form values. Never preselected: saved passwords (High), Chromium `Web Data` (also saved addresses, cards and search engines), Firefox `places.sqlite` (also bookmarks, High). Bookmarks files never listed. Safari passwords live in the Passwords app (note). A running browser locks its items "Quit <browser> first"; opened after the scan, `prepare` refuses the cleanup.
- P4 recent documents, apps, servers lists and per-app recent documents (`*.sfl2`, `*.sfl3` with "Recent"); Finder favorites and other lists are never offered.
- P5 Wi-Fi: device from `networksetup -listallhardwareports`, networks from `networksetup -listpreferredwirelessnetworks <dev>`, listed as locked rows (they are labels, not files, outside the allow-list); "Wi-Fi Settings" opens `x-apple.systempreferences:com.apple.wifi-settings-extension` via `env.sys.open_url`.
- P6 permissions: `sqlite3 -readonly -separator | <TCC.db> "SELECT service, client, auth_value FROM access WHERE client_type = 0;"` on the user and system databases (granted = auth_value 2 or 3), listed locked; without Full Disk Access a note. "Reset Permissions" opens a dialog with one checkbox per permission; "Reset Selected" runs `tccutil reset <Service> <bundle id>` per checked row (names checked to be plain identifiers; journal checked writable first), logs each as `ActionKind::Command`, reports successes and failures and scans again.

## PARTIAL and why
- P6: only apps recorded by bundle id are listed (TCC records command-line tools by path, and `tccutil reset` takes a bundle id); listing needs Full Disk Access; the TCC schema (`auth_value`, macOS 11 and later) and resetting system-level services are unverified on a Mac.

## Hand checks on a real Mac
Run the app, open each page, Scan, and compare. Commands to confirm the output formats my parsers expect:
1. `defaults read -g AppleLanguages` prints `(` / `"en-US",` / `)`. J6 must keep those languages.
2. `uname -m` prints `arm64`; `lipo -archs /Applications/<universal app>.app/Contents/MacOS/<exe>` prints `x86_64 arm64`; `lipo -detailed_info <same>` has `architecture x86_64` blocks with `size N`. J7 shows the x86_64 size.
3. `plutil -lint ~/Library/Preferences/<damaged>.plist` exits non-zero for a damaged file (make one with `echo '<plist><dict>' > ~/Library/Preferences/test.cleancraft.plist`). J9 lists it.
4. iOS backup `Info.plist` keys `Device Name`, `Target Identifier`, `Last Backup Date` (Full Disk Access needed).
5. Mail with an IMAP account and a POP or On My Mac mailbox: open an attachment, then Scan Mail Downloads. IMAP attachments appear, POP ones do not. With new-style UUID account folders, check `sqlite3 -separator '|' ~/Library/Accounts/Accounts4.sqlite "SELECT ZACCOUNT.ZIDENTIFIER, ZACCOUNTTYPE.ZIDENTIFIER FROM ZACCOUNT JOIN ZACCOUNTTYPE ON ZACCOUNT.ZACCOUNTTYPE = ZACCOUNTTYPE.Z_PK;"` prints `UUID|com.apple.account.IMAP` rows whose UUID matches the folder names in `~/Library/Mail/V10`. If they do not match, those attachments stay locked (safe) and M2 needs another source.
6. Edit an opened attachment in place (Preview, TextEdit), scan: it must be unselected (M3).
7. Trash Bins: Empty Trash in Finder. macOS asks once for Automation permission ("CleanCraft wants to control Finder"); after Allow, Finder empties the Trash and History shows `osascript -e 'tell application "Finder" to empty trash'`. Check whether Finder shows its own warning dialog (`osascript` blocks until it is answered; `Sys::run` has no timeout, the app runs it on a worker thread).
8. Browsers: Brave, Edge, Vivaldi, Arc cache folders (`~/Library/Caches/BraveSoftware/Brave-Browser/<Profile>`, `~/Library/Caches/Microsoft Edge/<Profile>`, `~/Library/Caches/Vivaldi/<Profile>`, `~/Library/Caches/Arc/User Data/<Profile>`) and process names in `ps -axo pid=,comm=` (`Brave Browser.app`, `Microsoft Edge.app`, `Vivaldi.app`, `Arc.app`, `Firefox.app`).
9. Recent items: after cleaning, open Apple menu > Recent Items; if lists come back, `sharedfilelistd` rewrote them from memory (would need a restart of that agent, which CleanCraft does not do).
10. `networksetup -listallhardwareports` shows `Hardware Port: Wi-Fi` / `Device: en0`; `networksetup -listpreferredwirelessnetworks en0` prints `Preferred networks on en0:` and tab-indented names (on recent macOS check that names are not redacted). "Wi-Fi Settings" must open System Settings > Wi-Fi.
11. With Full Disk Access: `sqlite3 -readonly -separator '|' ~/Library/Application\ Support/com.apple.TCC/TCC.db "SELECT service, client, auth_value FROM access WHERE client_type = 0;"` returns rows like `kTCCServiceCamera|us.zoom.xos|2`. Reset one camera permission from the dialog; the app asks again next time. Check `tccutil reset ScreenCapture <bundle id>` works without sudo for the system-level services.
12. Sandboxed caches: reading `~/Library/Containers/*` may trigger the macOS 14+ "access data from other apps" prompt without Full Disk Access.

## Shared-file changes
- `crates/core/src/lib.rs`: `pub mod testkit_cleanup;`.
- `crates/core/src/features/mod.rs`: `pub mod mail; pub mod privacy; pub mod trash;` and three registry lines (MailDownloads, TrashBins, Privacy).
- `crates/ui-egui/src/pages/plan_page.rs`, `confirm()`: the list's `ScrollArea::vertical()` became `ScrollArea::both().max_width(640.0)`. Long paths and reasons (Chrome profile files) made the dialog wider than the window and put "Move to Trash" off screen (x = -144).
- `crates/core/src/policy.rs`, tests only: two `&[caches.clone()]` became `std::slice::from_ref(&caches)` because clippy `-D warnings` failed on main (`cloned_ref_to_slice_refs`). No fence change.
- `docs/FEATURES.md`: my rows only.

## For the coordinator
- `item_row` in plan_page shows Reveal for every item; Wi-Fi and permission rows are labels, not files, so Reveal there is a silent no-op on a real Mac (`open -R` fails, the exit status is ignored). Suggest showing Reveal only when the path exists, and checking `open`'s status.
- The sidebar is taller than the 820 px test window: "History" sits at y = 858, so `click()` cannot reach it (`qa_cleanup_trash` navigates with `app.navigate("history")` for that one check).
- `Item.group` is used by my modules for per-app/per-tool/per-mailbox grouping; a keep-one-per-group guard must apply to Duplicates and Similar Images only.
- Shared `CARGO_TARGET_DIR` collides across worktrees (same crate metadata hash): my ui-egui once linked another worktree's core (E0432) until I touched my core sources.
- `Sys::run` has no timeout; a hung `osascript` (Finder dialog) would block its worker thread. A `run_timeout` in `sys.rs` would help.
