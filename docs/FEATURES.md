# CleanCraft features (the contract)

Every row is one user-facing capability. CleanCraft aims for feature parity with the reference app (the reference vendor) as described in its public documentation (research notes with sources: [research/the reference app-public.md](research/the reference app-public.md)). Names, texts, layout and icons are CleanCraft's own.

**Status** values:
- **WORKS**: an end-to-end test drives the UI by clicks and typing on a fake home folder, checks the files that actually moved and the numbers shown against a recalculation, and covers the error paths that apply.
- **PARTIAL**: some of the row works and is tested; the row says what is missing.
- **PLANNED**: not built yet.
- **EXCLUDED**: deliberately left out; the row says why.
- **MAC-CHECK**: logic tested on the fake home, but the real behaviour depends on macOS and must be confirmed by hand (see the hand-check list at the end).

**Risk**: Low = rebuilt automatically. Medium = something is lost but not personal (history, re-downloadable attachments). High = personal files or app parts, never preselected.

**Test** names refer to test functions (`crate::file::function`). Safety rules that apply to every row are in section 0.

## 0. Safety rules (apply to every feature)

| ID | Rule | How | Test | Status |
|---|---|---|---|---|
| S1 | Preview first: exact files, sizes and reason; nothing removed without confirming that list | Scans return a `Plan`; the executor needs the plan digest the user confirmed; a changed list is refused | | PLANNED |
| S2 | Removing = move to the Trash of the item's volume, or CleanCraft quarantine; never a permanent delete | `executor::move_one` uses one `rename`; falls back to quarantine when the Trash refuses | | PLANNED |
| S3 | Every action logged with an undo | `journal.jsonl` line per move; History screen puts items back | | PLANNED |
| S4 | Hard allow-list per feature; nothing outside it | `policy::check` before scan proposal and again before each move | | PLANNED |
| S5 | Never SIP-protected system files, iCloud Drive, Photos libraries, contents of Documents and Desktop | `policy::PROTECTED_*` | | PLANNED |
| S6 | Never files open in running apps, never inside running apps | `lsof -F n` and `ps` before moving; open items skipped and reported | | PLANNED |
| S7 | Full Disk Access requested the proper macOS way; features work without it where possible | Detect by reading a protected file; deep link to System Settings > Privacy & Security > Full Disk Access; unreadable places listed as notes | | PLANNED |
| S8 | Tests only on a fake home in a temp folder | `Env::fake`, `.cargo/config.toml` sets `CLEANCRAFT_FORBID_REAL=1` so `Env::real()` fails in any cargo process; executor refuses paths outside the env root | | PLANNED |
| S9 | Errors handled: permission denied, file in use, disk full, action log unwritable | Skipped items listed with the reason; nothing moves when the log cannot be written | | PLANNED |
| S10 | AI agents (MCP) can scan and preview; cleaning needs the setting "Let agents clean" and the plan digest | MCP `execute_plan` | | PLANNED |
| S11 | Ignore list honoured by every scan | `Settings::ignore` | | PLANNED |
| S12 | Every control on every screen responds; no dead clicks, no "not available" | `qa_every_control` test | | PLANNED |

## 1. Quick Sweep

| ID | Feature | What it does | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| Q1 | One-button sweep | Runs System Junk, Trash Bins, Mail Downloads, Threat Check (quick), tune-up suggestions, app updates, duplicate downloads; one tile per module with size or count | the module scanners | Low/Med | | PLANNED |
| Q2 | Tiles with Review and Run | Each tile opens that module's review, or runs it with its safe preselection after one confirmation listing everything | | Low/Med | | PLANNED |
| Q3 | Skip a tile | Toggle per tile; skipped modules shown dimmed | Settings | Low | | PLANNED |

## 2. Cleanup

### 2.1 System Junk

| ID | Category | What | Paths / APIs | Risk | Default | Test | Status |
|---|---|---|---|---|---|---|---|
| J1 | User cache files | One entry per app cache folder | `~/Library/Caches/*`, `~/Library/Containers/*/Data/Library/Caches/*`; never iCloud and sign-in caches (`com.apple.bird`, `CloudKit`, ...) | Low | selected | | PLANNED |
| J2 | System cache files | Shared caches | `/Library/Caches/*`; locked "needs administrator rights" when not writable | Low | selected | | PLANNED |
| J3 | User log files | | `~/Library/Logs/*` | Low | selected | | PLANNED |
| J4 | System log files | | `/Library/Logs/*`, `/private/var/log/*` (never `/private/var/db/diagnostics`) | Low | selected | | PLANNED |
| J5 | Crash reports | | `~/Library/Logs/DiagnosticReports/*` | Low | selected | | PLANNED |
| J6 | Language files | `*.lproj` in apps for languages the user does not use | `/Applications/*.app/Contents/Resources/*.lproj`; keeps `Base`, `en` and the system languages (`defaults read -g AppleLanguages`) | High | locked | | PLANNED |
| J7 | Universal binaries | Apps carrying code for both Intel and Apple silicon | `lipo -archs` | High | locked | | PLANNED |
| J8 | Broken startup items | Launch agent plists whose program no longer exists | `~/Library/LaunchAgents/*.plist` (`Program`, `ProgramArguments[0]`) | Medium | selected | | PLANNED |
| J9 | Broken preferences | Preference files that cannot be read | `~/Library/Preferences/*.plist` failing to parse | Medium | selected | | PLANNED |
| J10 | Document versions | Old document versions | `/.DocumentRevisions-V100` (root only) | Medium | locked | | PLANNED |
| J11 | iOS device backups | All but the newest | `~/Library/Application Support/MobileSync/Backup/*` (`Info.plist` for name and date; needs Full Disk Access) | Medium | not selected | | PLANNED |
| J12 | iOS software updates | Old `.ipsw` files | `~/Library/iTunes/*Software Updates/*.ipsw` | Low | selected | | PLANNED |
| J13 | Xcode junk | Derived data, device support, simulator caches, archives (archives never preselected), module caches kept | `~/Library/Developer/Xcode/{DerivedData,Archives,iOS DeviceSupport,watchOS DeviceSupport}`, `~/Library/Developer/CoreSimulator/Caches` | Low/Med | partly | | PLANNED |
| J14 | Unfinished downloads | Broken browser downloads | `~/Downloads/*.{download,crdownload,part}` | Low | selected | | PLANNED |
| J15 | Old app updates | Downloaded update packages | `~/Library/Caches/*/org.sparkle-project.Sparkle`, `/Library/Updates` | Low | selected | | PLANNED |
| J16 | Unused disk images | `.dmg` files not opened for N days | `~/Downloads/**/*.dmg` | Medium | not selected | | PLANNED |
| J17 | Developer caches | Package manager caches | `~/.npm/_cacache`, `~/Library/Caches/{Homebrew,pip,CocoaPods,org.swift.swiftpm}`, `~/.cargo/registry/cache`, `~/.gradle/caches` (never preselected) | Low | partly | | PLANNED |
| J18 | AI tool clutter | Caches and logs of AI apps and CLIs | `~/Library/Application Support/{Claude,Cursor}/{Cache,Code Cache,GPUCache,logs}`, `~/Library/Logs/Claude` | Low | selected | | PLANNED |
| J19 | Local AI models | Downloaded models, never preselected | `~/.ollama/models`, `~/.lmstudio/models`, `~/.cache/lm-studio/models` | Medium | not selected | | PLANNED |
| J20 | Deleted users | Archived home folders of deleted accounts | `/Users/Deleted Users/*.dmg` (root) | Medium | locked | | PLANNED |
| J21 | Review and choose | Category list, per-item checkboxes, select all/none per category, search, sort by size or name, reveal in Finder, add to ignore list | | | | | PLANNED |

### 2.2 Mail Downloads

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| M1 | Opened attachments | Copies Mail saved when attachments were opened | `~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads/*` | Low | | PLANNED |
| M2 | Downloaded attachments | Attachments stored with messages, IMAP/Exchange accounts only (POP and On My Mac excluded: could be the only copy) | `~/Library/Mail/V*/<account>/**/Attachments/*` (needs Full Disk Access); Mail must be quit | Medium | | PLANNED |
| M3 | Modified attachments kept | Files changed after download are not preselected | mtime vs folder time | Medium | | PLANNED |

### 2.3 Trash Bins

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| T1 | Size of every Trash | Startup disk, external disks, Mail trash | `~/.Trash`, `/Volumes/*/.Trashes/<uid>`, Mail `Deleted Messages.mbox` | Medium | | PLANNED |
| T2 | Empty | CleanCraft never deletes permanently. "Empty" asks Finder to empty the Trash (Finder's own confirmation) | `osascript -e 'tell application "Finder" to empty trash'` | Medium | | PLANNED |

## 3. Protection

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| P1 | Threat Check scan | Checks startup locations and installed apps: unsigned or ad hoc signed programs in launch agents and daemons, items matching CleanCraft's own rule list (known adware names and paths), apps failing Gatekeeper assessment | `~/Library/LaunchAgents`, `/Library/LaunchAgents`, `/Library/LaunchDaemons`, `/Library/StartupItems`, `/Applications`; `codesign --verify`, `spctl --assess` | High | | PLANNED |
| P2 | Quarantine threats | Selected findings move to the quarantine (never deleted); launch agents unloaded first | `launchctl bootout` | High | | PLANNED |
| P3 | Privacy: browsers | Per browser: history, cookies, cache, sessions, download list, form values (passwords never preselected); browser must be quit | Safari `~/Library/Safari/{History.db,LastSession.plist,Downloads.plist,Form Values}`, Chrome/Brave/Edge/Vivaldi/Arc `~/Library/Application Support/<vendor>/<Profile>/{History,Cookies,Web Data,Current Session,...}`, Firefox `Profiles/*/{places.sqlite,cookies.sqlite,formhistory.sqlite,sessionstore*}` | Medium | | PLANNED |
| P4 | Privacy: recent items | Recent documents, apps, servers lists | `~/Library/Application Support/com.apple.sharedfilelist/*.sfl*` | Low | | PLANNED |
| P5 | Privacy: Wi-Fi networks | List known networks; removal handed to System Settings | `networksetup -listpreferredwirelessnetworks` | Medium | | PLANNED |
| P6 | App permissions | List apps with camera, microphone, screen recording, files access; revoke through macOS | `tccutil reset <service> <bundle id>` | Low | | PLANNED |

## 4. Performance

### 4.1 Tune-up Tasks

Each task shows what it does, the exact command, whether it needs the administrator password, and asks before running. Commands cannot be undone; the log records them.

| ID | Task | Command | Admin | Risk | Test | Status |
|---|---|---|---|---|---|---|
| K1 | Flush DNS cache | `dscacheutil -flushcache; killall -HUP mDNSResponder` | yes | Low | | PLANNED |
| K2 | Free up RAM | `purge` | yes | Low | | PLANNED |
| K3 | Free up purgeable space | `tmutil thinlocalsnapshots / 999999999999 4` | no | Low | | PLANNED |
| K4 | Thin Time Machine snapshots | `tmutil listlocalsnapshots /`, `tmutil deletelocalsnapshots <date>` | yes | Medium | | PLANNED |
| K5 | Reindex Spotlight | `mdutil -E /` | yes | Low | | PLANNED |
| K6 | Rebuild Launch Services | `lsregister -kill -r -domain local -domain system -domain user` | no | Low | | PLANNED |
| K7 | Repair home folder permissions | `diskutil resetUserPermissions / <uid>` | yes | Medium | | PLANNED |
| K8 | Verify startup disk | `diskutil verifyVolume /` | no | Low | | PLANNED |
| K9 | Speed up Mail | Mail quit, then `sqlite3 <Envelope Index> "VACUUM;"` | no | Medium | | PLANNED |
| K10 | Run periodic scripts | `periodic daily weekly monthly` (macOS 13 and earlier) | yes | Low | | PLANNED |

### 4.2 Startup Items

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| L1 | List login items | Apps opened at login | `osascript` System Events `login items` | Low | | PLANNED |
| L2 | List launch agents and daemons | With program, signature, enabled state | `~/Library/LaunchAgents`, `/Library/LaunchAgents`, `/Library/LaunchDaemons`; `launchctl print-disabled gui/<uid>` | Low | | PLANNED |
| L3 | Disable / enable (undoable) | User agents: `launchctl disable gui/<uid>/<label>` + `bootout`; login items removed via System Events, undo adds them back | Medium | | PLANNED |
| L4 | Remove | Agent plist moved to Trash after confirmation (undo moves it back) | Medium | | PLANNED |
| L5 | Hung and heavy apps | Apps not responding or using a lot of CPU; Quit | `ps -axo pid,%cpu,comm` | Medium | | PLANNED |

## 5. Applications

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| A1 | App list | Every app in `/Applications` and `~/Applications` with size, version, last opened, store (App Store, Setapp, other), vendor | `Info.plist`, `_MASReceipt`, `mdls -name kMDItemLastUsedDate` | Low | core apps_inventory::lists_every_kind_of_app, apps_inventory::find_by_name_id_or_path; ui qa_apps_remover::list_groups_and_numbers_match_the_disk | MAC-CHECK |
| A2 | Uninstall with leftovers | App plus its files found by bundle id and name: Application Support, Caches, Preferences, Containers, Group Containers, Saved Application State, HTTPStorages, WebKit, Cookies, Logs, LaunchAgents | listed paths | High | core apps_uninstall::plan_finds_leftovers_with_confidence, ::uninstall_moves_bundle_and_selected_leftovers_and_undo_restores, ::ignore_list_and_errors; ui qa_apps_remover::uninstall_moves_app_and_leftovers_then_undo, ::file_errors_are_listed_per_file, ::cancel_and_escape_move_nothing | MAC-CHECK |
| A3 | Apple apps protected | Apps signed by Apple and system apps are never removable | `/System/Applications`, `codesign -dv` authority | | core apps_uninstall::apple_apps_are_locked_and_refused, apps_inventory::apple_signature_marks_app_as_apple; ui qa_apps_remover::apple_app_is_locked, ::running_app_is_refused | MAC-CHECK |
| A4 | Reset an app | Remove its data, keep the app | same leftovers without the bundle | High | core apps_uninstall::reset_keeps_the_app; ui qa_apps_remover::reset_keeps_the_app | MAC-CHECK |
| A5 | Leftovers of removed apps | Files whose app is gone | leftover folders whose bundle id matches no installed app | Medium | core apps_leftovers::leftovers_of_removed_apps_are_conservative, ::app_found_elsewhere_or_spotlight_failure_proposes_nothing; ui qa_apps_remover::leftovers_and_installers_groups | MAC-CHECK |
| A6 | Installer packages | `.pkg` and `.dmg` installers in Downloads | `~/Downloads` | Medium | core apps_leftovers::installers_and_extensions_are_listed_and_removable; ui qa_apps_remover::leftovers_and_installers_groups | WORKS |
| A7 | Unused apps group | Not opened in six months | `kMDItemLastUsedDate` | | core apps_inventory::lists_every_kind_of_app; ui qa_apps_remover::list_groups_and_numbers_match_the_disk | MAC-CHECK |
| A8 | App updates (Sparkle) | Compare installed version with the app's appcast; show release notes; Update opens the app's own updater or download page | `SUFeedURL` in Info.plist, HTTP GET of the appcast | Low | core updater::tests::{version_table,appcast,html_notes}, apps_updates::sparkle_app_store_and_failures, ::release_notes_link_is_fetched; ui qa_apps_updates::check_show_and_hand_off, ::everything_offline | MAC-CHECK |
| A9 | App updates (App Store) | Look up App Store version, open the App Store updates page | `https://itunes.apple.com/lookup?bundleId=`, `macappstore://showUpdatesPage` | Low | core updater::tests::lookup, apps_updates::sparkle_app_store_and_failures; ui qa_apps_updates::check_show_and_hand_off | MAC-CHECK |
| A10 | macOS updates | List from `softwareupdate --list`, open Software Update | | Low | core updater::tests::softwareupdate_list, apps_updates::up_to_date_and_bad_answers; ui qa_apps_updates::check_show_and_hand_off, ::everything_offline | MAC-CHECK |
| A11 | Extensions | Preference panes, internet plug-ins, Spotlight and Quick Look plug-ins, screen savers, Safari extensions list | `~/Library/{PreferencePanes,Internet Plug-Ins,Spotlight,QuickLook,Screen Savers}`, `/Library/...`; `pluginkit -mA` | Medium | core extensions::tests::pluginkit_lines, apps_leftovers::installers_and_extensions_are_listed_and_removable, ::pluginkit_failure_is_a_note; ui qa_apps_remover::leftovers_and_installers_groups | MAC-CHECK |

## 6. My Clutter

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| C1 | Large & old files | Files over the size threshold (default 50 MB) or not opened for N days, grouped by kind, size and age; nothing preselected | home folder except Library, Documents, Desktop, iCloud, Photos and Music libraries, app bundles | High | | PLANNED |
| C2 | Duplicates | Identical content (size, then partial hash, then full BLAKE3), files over 100 KB; hard links ignored; select newest/oldest/smart | same areas as C1 | High | | PLANNED |
| C3 | Similar images | Perceptual hash (difference hash) groups, keep suggestion = largest resolution | `~/Pictures` (outside Photos library), `~/Downloads`; HEIC through `sips` | High | | PLANNED |
| C4 | Downloads by source | Files in Downloads grouped by the app that saved them | `~/Downloads`, `com.apple.quarantine` / `kMDItemWhereFroms` via `xattr` / `mdls` | High | | PLANNED |
| C5 | Shredder | Permanent secure delete | | | | EXCLUDED: the safety rules forbid permanent deletion; not reliable on SSDs either |

## 7. Disk Map (CleanCraft: Space Lens)

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| D1 | Map of a folder or disk | Size tree, proportional blocks, list sorted by size, drill down, back, breadcrumbs | directory walk, hard links counted once, not crossing mount points | Low | | PLANNED |
| D2 | Storage bar | Used, free, selected | `df` | Low | | PLANNED |
| D3 | Select and remove | Checkboxes, review list, move to Trash; system and protected items not selectable | allow-list = home folder minus protected | High | | PLANNED |

## 8. Live Monitor

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| R1 | CPU | Load, top apps, uptime | `top -l 1`, `ps -axo pid,%cpu,comm`, `sysctl kern.boottime` | | | PLANNED |
| R2 | Memory | Used, available, pressure, swap, top apps | `vm_stat`, `sysctl vm.swapusage`, `memory_pressure` | | | PLANNED |
| R3 | Storage | Free space, Trash size, Free Up (opens Quick Sweep) | `df` | | | PLANNED |
| R4 | Battery | Charge, state, health, cycles | `pmset -g batt`, `ioreg -rn AppleSmartBattery` | | | PLANNED |
| R5 | Network | Upload and download speed | `netstat -ib` byte counters | | | PLANNED |
| R6 | Menu bar presence | Status item with live CPU/RAM, opens monitor and app | tray icon | | | PLANNED |
| R7 | Alerts | Low free space, heavy memory, large Trash | thresholds in Settings | | | PLANNED |

## 9. App shell

| ID | Feature | What | Test | Status |
|---|---|---|---|---|
| U1 | Sidebar navigation | Every module reachable | | PLANNED |
| U2 | History | Every action with time, size, where it went; Undo per item and per cleanup | | PLANNED |
| U3 | Settings | Removal method, ignore list (add/remove), thresholds, agents may clean, menu bar | | PLANNED |
| U4 | Full Disk Access banner | Shows when missing, button opens the right System Settings pane, re-checks | | PLANNED |
| U5 | CLI | `cleancraft scan|clean|undo|history|tasks|apps|monitor` with the same safety | | PLANNED |
| U6 | MCP server | `cleancraft serve`: scan, preview, execute (gated), undo, history, monitor | | PLANNED |
| U7 | Install | `bash scripts/install-app.sh` builds and installs `/Applications/CleanCraft.app` | | PLANNED |

## Out of scope for v1

| Feature in the reference app | Why |
|---|---|
| Cloud Cleanup (iCloud, Google Drive, OneDrive, Dropbox) | Deletes propagate to the cloud; needs OAuth apps per provider; touches iCloud, which the safety rules forbid |
| Email cleanup (newsletters, unsubscribe) | Needs mail account access; not requested |
| Real-time malware monitoring | Needs an Endpoint Security system extension and an Apple entitlement |
| Privileged helper | CleanCraft uses the macOS administrator password dialog per task instead |
| Smart Insights (on-device AI explanations) | Optional; can come later |

## Hand-check list (on your Mac)

Filled in at delivery.
