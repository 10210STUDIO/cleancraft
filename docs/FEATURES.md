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
| A1 | App list | Every app in `/Applications` and `~/Applications` with size, version, last opened, store (App Store, Setapp, other), vendor | `Info.plist`, `_MASReceipt`, `mdls -name kMDItemLastUsedDate` | Low | | PLANNED |
| A2 | Uninstall with leftovers | App plus its files found by bundle id and name: Application Support, Caches, Preferences, Containers, Group Containers, Saved Application State, HTTPStorages, WebKit, Cookies, Logs, LaunchAgents | listed paths | High | | PLANNED |
| A3 | Apple apps protected | Apps signed by Apple and system apps are never removable | `/System/Applications`, `codesign -dv` authority | | | PLANNED |
| A4 | Reset an app | Remove its data, keep the app | same leftovers without the bundle | High | | PLANNED |
| A5 | Leftovers of removed apps | Files whose app is gone | leftover folders whose bundle id matches no installed app | Medium | | PLANNED |
| A6 | Installer packages | `.pkg` and `.dmg` installers in Downloads | `~/Downloads` | Medium | | PLANNED |
| A7 | Unused apps group | Not opened in six months | `kMDItemLastUsedDate` | | | PLANNED |
| A8 | App updates (Sparkle) | Compare installed version with the app's appcast; show release notes; Update opens the app's own updater or download page | `SUFeedURL` in Info.plist, HTTP GET of the appcast | Low | | PLANNED |
| A9 | App updates (App Store) | Look up App Store version, open the App Store updates page | `https://itunes.apple.com/lookup?bundleId=`, `macappstore://showUpdatesPage` | Low | | PLANNED |
| A10 | macOS updates | List from `softwareupdate --list`, open Software Update | | Low | | PLANNED |
| A11 | Extensions | Preference panes, internet plug-ins, Spotlight and Quick Look plug-ins, screen savers, Safari extensions list | `~/Library/{PreferencePanes,Internet Plug-Ins,Spotlight,QuickLook,Screen Savers}`, `/Library/...`; `pluginkit -mA` | Medium | | PLANNED |

## 6. My Clutter

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| C1 | Large & old files | Files over the size threshold (default 50 MB) or not opened for N days, grouped by kind, size and age; nothing preselected | home folder except Library, Documents, Desktop, iCloud, Photos and Music libraries, app bundles | High | ui::qa_files_large_old::{downloads_scan_select_clean_undo, typed_folder_checked_with_enter_and_esc, permission_denied_and_file_in_use}; core::files_large_old::{thresholds_kinds_and_exclusions, locations_limit_the_walk, typed_folders_are_checked, spotlight_last_used_date_wins_over_access_time} | PARTIAL: grouped by kind (Archives, Movies, Music, Pictures, Documents, Disk images, Other); size and last-opened age are shown per item (sort by size, search "Not opened"), but there are no size or age groups. Location picker (home, Downloads, Movies, Music, Pictures, typed folder; protected refused). Last opened uses `mdls kMDItemLastUsedDate` in batches (MAC-CHECK), else atime/mtime |
| C2 | Duplicates | Identical content (size, then partial hash, then full BLAKE3), files over 100 KB; hard links ignored; select newest/oldest/smart | same areas as C1 | High | ui::qa_files_duplicates::{duplicate_sets_keep_one_copy, every_copy_selected_is_refused_by_core}; core::files_duplicates::{exact_sets, smart_keeps_one_per_set, keep_newest_and_oldest, every_copy_selected_is_refused, smart_selection_moves_only_extra_copies} | WORKS: size, first 64 KiB BLAKE3, full BLAKE3; >100 KB; hard links ignored; set headers; Select Copies menu (Keep Suggested, Newest, Oldest); one copy per set always kept (UI guard and core check before any move). APFS clones are not detected (they would show as copies) |
| C3 | Similar images | Perceptual hash (difference hash) groups, keep suggestion = largest resolution | `~/Pictures` (outside Photos library), `~/Downloads`; HEIC through `sips` | High | ui::qa_files_duplicates::similar_pictures_keep_the_biggest; core::files_similar::{hashes_tell_alike_from_different, groups_keep_most_pixels, heic_goes_through_sips} | WORKS for JPEG, PNG, GIF, BMP, TIFF, WebP (64-bit dHash, Hamming <= 10, keep most pixels then biggest file). HEIC/HEIF through `sips` is MAC-CHECK. No thumbnails in rows |
| C4 | Downloads by source | Files in Downloads grouped by the app that saved them | `~/Downloads`, `com.apple.quarantine` / `kMDItemWhereFroms` via `xattr` / `mdls` | High | ui::qa_files_large_old::downloads_by_source_app; core::files_large_old::downloads_grouped_by_source_app | MAC-CHECK: Large & Old Files location "Downloads by source app", one category per app from `xattr -p com.apple.quarantine` (agent field); `kMDItemWhereFroms` not used; nothing preselected |
| C5 | Shredder | Permanent secure delete | | | | EXCLUDED: the safety rules forbid permanent deletion; not reliable on SSDs either |

## 7. Disk Map (CleanCraft: Space Lens)

| ID | Feature | What | Paths / APIs | Risk | Test | Status |
|---|---|---|---|---|---|---|
| D1 | Map of a folder or disk | Size tree, proportional blocks, list sorted by size, drill down, back, breadcrumbs | directory walk, hard links counted once, not crossing mount points | Low | ui::qa_disk_map::{map_drill_select_remove_undo, typed_folder_and_errors}; core::files_space::{sizes_match_a_recalculation, hard_links_once_and_links_not_followed, refresh_after_move_and_back} | PARTIAL: home folder or a typed folder inside it (whole disk and other volumes not offered: removal is limited to the home folder); squarified block map, size list, drill down by block or list, Back, breadcrumbs, background measuring with Stop, tree kept per mapped folder and patched after removal and undo. Not crossing mount points is implemented (st_dev) but untested (no mounts in tests) |
| D2 | Storage bar | Used, free, selected | `df` | Low | ui::qa_disk_map::map_drill_select_remove_undo; core::files_space::disk_use_from_df | WORKS: used, total, free from `df -Pk` (free space only when df fails) and the selected size; purgeable space not shown; `df` columns are MAC-CHECK |
| D3 | Select and remove | Checkboxes, review list, move to Trash; system and protected items not selectable | allow-list = home folder minus protected | High | ui::qa_disk_map::{map_drill_select_remove_undo, cancel_keeps_everything, typed_folder_and_errors}; core::files_space::{locked_places, removal_plan_refuses_locked_items} | WORKS: checkboxes (protected places, Library, hidden items and apps locked), Review and Remove lists the exact items, moved to Trash or quarantine by the executor (feature space_lens, allow-list = home folder), Undo, permission denied and file in use reported |

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
