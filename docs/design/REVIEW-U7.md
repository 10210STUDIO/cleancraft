# REVIEW-U7: task-based usability review of round 6

Reviewer: usab7. Base: main dc7e109 (round 6 merged by integ6). Method: a first-time Mac user's real tasks on the new features, driven with egui_kittest on FakeMac at 1280 x 820 by clicks and typing only, every step logged; steps outside the window (Finder, System Settings, the menu bar item, a notification) are counted and marked. Yardstick: Nielsen's ten heuristics (H1..H10), the Apple Human Interface Guidelines (HIG) and the user's two standing rules: (R1) every locked item has a way forward on the same screen; (R2) nothing is cleaned without a preview he confirms.

Evidence:
- Walkthroughs: `task_01..task_13` in `crates/ui-egui/tests/review_usability7.rs` (pass today; the step log prints with `cargo test -p cleancraft-ui-egui --test review_usability7 -- --ignored --nocapture task_`; add `CLEANCRAFT_REVIEW_U7_DUMP=1` to print every screen's texts and buttons).
- Proofs: `u7_01..u7_18` in the same file, `#[ignore]`d and failing today for the reason in their message. Drop the `#[ignore]` when the finding is fixed.
- Renders: `docs/design/review-u7/` (written by the tests with `CLEANCRAFT_REVIEW_U7_RENDERS=1`).

Severity: **S1** blocks the task. **S2** confusion or extra work. **S3** polish.

R2 held everywhere: every cleanup in these tasks (uninstall, leftovers, Developer Clutter, Quick Sweep, Disk Map) showed the exact list and waited for "Move to Trash"; the Mail erase asks first and says it cannot be undone; the menu bar, schedule and watchers never moved anything. R1 held for the new locks (running app has Quit Editor in its group, Mail trash has Erase in Mail, git-tracked and outside-the-code-folders build folders give exact steps, external top folders say "open it to choose what is inside"), with the gaps in U7-6 and U7-8.

## Tasks

| # | Task | Result | Steps | Doubts | Findings |
|---|---|---|---|---|---|
| 1 | First launch: welcome and AI step, Not Now | Pass, but there is no welcome: the first screen is the AI apps step with a JSON path and a command line; Full Disk Access comes after it as a banner | 1 | 4 | U7-5, U7-9 |
| 2 | Uninstall 3 apps at once, one of them running | Pass: tick 3, Review and Uninstall, Quit Editor in its group, Uninstall 3 Apps, Move to Trash; per-app Undo on Done | 9 | 1 | U7-18 |
| 3 | Remove a dragged app's leftovers from the notification | **Fail** from the notification: clicking it opens Script Editor and its text names App Remover, not the menu bar. Pass through the menu bar item's "Review: Viewer was moved to the Trash" (lands on the leftovers, 2 clicks). Needs two switches that are off by default | 6 (4 outside) | 1 | U7-2 |
| 4 | Clean old Rust target folders, keep the active project | Pass: the active project is listed unticked with "active project, a file changed today" | 5 | 2 | U7-10, U7-11 |
| 5 | Set code folders in Settings | Pass, the long way: the hint is under Details with no button; after adding, Developer Clutter still shows "Nothing to clean here" until Scan Again | 12 | 2 | U7-6 |
| 6 | Turn on the menu bar and a weekly scan | Pass (switch, Every week, Monday 10:00 by default, "Next scan" shown) | 4 | 2 | U7-15 |
| 7 | Read Mac Health and follow two recommendations | Pass, with detours: Mac Health is a segment inside Live Monitor; "Review in Threat Check" and "Review in System Junk" open start screens, so the found items need a new scan | 7 | 3 | U7-3, U7-4, U7-16 |
| 8 | Find what grew on the disk | **Fail**: the chart says 23.6 GB less free in 7 days; What grew needs two Disk Map scans days apart; "Open Disk Map" lands on the start screen; mapping now cannot answer | 5, dead end | 1 | U7-1 |
| 9 | Map an external drive | Pass (Where to look, My Backup, Map My Backup); its top folders cannot be removed as a whole | 6 | 1 | U7-8 |
| 10 | Group Large & Old by age | Pass (Group by: Kind > Age); groups not in age order | 5 | 1 | U7-17 |
| 11 | Erase Mail trash, without then with Accessibility | Pass: the refusal gives the reason, Open Accessibility Settings and Open Mail; coming back there is no Try Again (Erase in Mail... and the confirmation again) | 9 (1 outside) | 2 | U7-13, U7-14 |
| 12 | Clean Duplicate Downloads from Quick Sweep | Pass (Scan, Review and Clean, Move to Trash; Undo on Done); the confirmation never names the copy that stays | 3 | 2 | U7-11, U7-12 |
| 13 | Understand a process in Live Monitor | Pass for known names (WindowServer, mds_stores, Brave helpers); fails for an unknown one (com.docker.backend: no words, no way to learn); CPU 17% in all next to Brave 85% | 1 | 2 | U7-7 |

**11 of 13 tasks pass, 2 fail** (3 and 8).

## Findings

Owners per COMMON6: health6 (health.rs, pages/health.rs, app.rs, kit.rs, welcome, Live Monitor explanations), tray6 (settings.rs, schedule, notify, agent), watch6 (watch.rs), dev6 (dev_clutter), files6 (large_old, space, sweep.rs, disk_map.rs, plan_page.rs), apps6 (apps.rs).

### S1: blocks the task

**U7-1 S1 "What grew" cannot be answered for a first-time user** (health6: health.rs, pages/health.rs; files6: disk_map.rs; H1 visibility, H7 efficiency)
Evidence: `task_08`, `u7_01_a_baseline_for_what_grew_is_recorded_without_the_user`, `u7_01_open_disk_map_from_health_starts_measuring`, `review-u7/u7-01-what-grew.png`. Mac Health shows "Free space fell by 23.6 GB in 7 days" (Needs attention) and a chart going down; the card's answer is "Disk Map ... scan it twice to see what grew" and What grew says "Map a folder in Disk Map twice (days apart)". Both "Open Disk Map" buttons land on Disk Map's start screen (one more click), and a map made today has nothing to compare with. The user sees the problem and cannot act on it this week. Two stores exist for the same idea (`disk-map-history.json` for Health, `disk-map-snapshots.json` from files6, FEATURES D8).
Recommendation:
1. Record a baseline without the user: at launch (in the background, at most once a day) and on each scheduled scan of the menu bar agent, a light measure of the home folder two levels deep (the D8 walk, 300 biggest entries), written to one store (keep files6's `space_snapshots`, Health reads it).
2. What grew compares the newest measure with the one closest to the start of the chart's range (7, 30 or 90 days), so the card answers on day 2.
3. Until two measures exist: "First measure taken today. Come back tomorrow to see what grew." in place of "Map a folder twice".
4. "Open Disk Map" from Health opens Disk Map already measuring the home folder (or the startup disk when the card is about the startup disk).

**U7-2 S1 The drag-to-Trash notification is a dead end** (watch6: watch.rs `Finding` body; tray6: notify.rs, settings.rs; H1, H2 match the real world; HIG: a notification opens its app)
Evidence: `task_03` (steps outside the window), `core/src/notify.rs` header ("clicking it opens Script Editor (or nothing) on some macOS versions, never CleanCraft"), `watch.rs` body "N items it left behind use X. Review them in App Remover.", Settings `NOTIFY_NOTE`. The scheduled scan's body ends with "Open the menu bar item to review it." The trash offer's body does not. A first-time user clicks the notification, gets Script Editor, and has no clue that the menu bar item holds "Review: Viewer was moved to the Trash". Also the offer needs two switches that are off by default (menu bar, then "Offer to clean up after an app goes to the Trash") and Full Disk Access, which the switch's note does not say.
Recommendation:
1. Post notifications from the app bundle itself (UNUserNotificationCenter in the agent, which is CleanCraft.app run as an accessory) so a click opens CleanCraft on the route; keep osascript only as the fallback when that fails.
2. While osascript is used, every body that leads somewhere ends the same way as the scheduled scan: "Open the CleanCraft menu bar item to review them." (watch.rs trashed app and startup item bodies; schedule.rs disk alert "Quick Sweep can show what to clean." too).
3. Under the "Offer to clean up..." switch: "Needs Full Disk Access (macOS keeps the Trash private)." with the Open Privacy Settings button when it is missing.
4. Measurable once fixed: a core test that every `Notification` with a route has a body ending with the menu bar hint while the osascript sender is in use.

### S2: confusion or extra work

**U7-3 S2 Mac Health "Review in ..." buttons open start screens** (health6: pages/health.rs; app.rs router; H4 consistency, H7)
Evidence: `task_07`, `u7_03_review_buttons_show_what_the_card_talks_about`, `review-u7/u7-03-threat-landing.png`. "Threat Check found 1 item 2 days ago ... Review what it found" opens Threat Check's start screen with Scan; "2.30 GB of junk found 4 days ago" and "Review in System Junk" open System Junk's start screen. The finding is gone from view; the user scans again and looks for it.
Recommendation: a "Review in X" button opens X and starts its scan at once (the same as the `scan:` routes integ6 added for Quick Sweep), and for Threat Check selects the finding the card counted (as `AppRoute::ThreatReview` already does). When the scan runs, say so on the page ("Scanning again to show what Mac Health found 2 days ago"). Name a button "Scan X" only when it only scans.

**U7-4 S2 Mac Health is hard to find** (health6: app.rs sidebar, sweep.rs start screen with files6; H6 recognition)
Evidence: `task_07`, `u7_04_mac_health_is_visible_from_the_start_page`, `review-u7/u7-04-start-page.png`. Nothing on Quick Sweep, the sidebar or the menu bar says Mac Health exists; it is the second segment of Live Monitor. A user asked "how healthy is my Mac?" looks at the sidebar and does not find it.
Recommendation: one line on the Quick Sweep start screen under the Scan button: "Mac Health: Needs attention (6)" (or "Good") as a link to the page; the same line in the menu bar menu. Keep it in the Live Monitor switcher.

**U7-5 S2 First launch has no welcome and starts with AI apps** (health6: welcome.rs, app.rs `launch_checks`; H2, H10 help; HIG onboarding: brief, about the app's purpose, ask for permissions in context)
Evidence: `task_01`, `u7_05_first_launch_welcomes_and_asks_for_full_disk_access_first`, `review-u7/u7-05-first-launch.png`. The first screen is "Use CleanCraft from your AI apps?" with `claude_desktop_config.json` paths, `mcpServers` JSON and a `claude mcp add` command line; the most important first-run need (Full Disk Access, without which Mail, Safari and the Trash are invisible) comes later as a banner with its own Not Now.
Recommendation: a short Welcome with three steps and a progress dot row:
1. "CleanCraft looks first and moves nothing until you confirm. Everything goes to the Trash with Undo." with Continue.
2. Full Disk Access: why, Open Privacy Settings, Check Again (re-checked when the window becomes active), Skip.
3. AI apps (only when one is found): today's page with the JSON and command behind a "Show the exact change" disclosure.
Not Now on step 3 remembers the AI answer only.

**U7-6 S2 Developer Clutter: the way to add code folders is hidden, and adding one does not refresh** (dev6: pages/dev_clutter.rs; tray6: settings.rs; R1, H7)
Evidence: `task_05`, `u7_06_nothing_found_offers_to_add_a_code_folder`, `u7_06_a_new_code_folder_is_looked_in_at_once`, `review-u7/u7-06-dev-nothing-found.png`. A Mac with code in `~/Work/code`: "Nothing to clean here. Your Mac has nothing this module would clean right now." The default folders and "add your own in Settings" are a sentence under Details. In Settings the notice says "Developer Clutter looks there on its next scan", but back on Developer Clutter the old empty result stays until Scan Again (12 steps in all).
Recommendation: when nothing is found, say where it looked in the body ("Looked in ~/Developer, ~/Projects, ... No projects found.") with "Add Code Folder..." (the same folder dialog as Settings) and "Code Folders in Settings"; after a code folder is added or removed, Developer Clutter's result is marked stale and scans again when shown (or shows "Code folders changed. Scan Again").

**U7-7 S2 Live Monitor: CPU numbers on two scales, unknown processes unexplained** (health6: pages/monitor.rs, core proc_names.rs; H2, H10)
Evidence: `task_13`, `u7_07_process_cpu_is_on_the_same_scale_as_the_total`, `u7_07_unknown_process_names_its_app`, `review-u7/u7-07-monitor-cpu.png`. The CPU card shows 17% and right under it "Brave Browser (3 processes) 85%", "WindowServer 47%", "com.docker.backend 38%": the rows are percent of one core, the gauge is the whole CPU; nothing says so. com.docker.backend (and CleanCraft itself at 23%) get no line at all, while macOS names do.
Recommendation: show rows on the gauge's scale (85% of one core on 8 cores = 11%), or keep per-core and write it once under the list: "Per process: % of one core (Activity Monitor's way). 100% = one core busy." For an unknown name, derive the owner from the executable path (`ps -o comm` gives the path: inside `/Applications/Docker.app` gives "Part of Docker."), and for CleanCraft itself "CleanCraft (this app)". Always offer "Show in Activity Monitor" in the row tip.

**U7-8 S2 External drives: top folders cannot be removed** (files6: core space.rs lock rules, disk_map.rs; R1 met, H7)
Evidence: `task_09`, `u7_08_external_drive_top_folder_can_be_selected`, `review-u7/u7-08-external-top-folder.png`. On "My Backup" every top folder is "locked: a top folder of this disk (open it to choose what is inside)". The way forward exists, but removing an old "Archive 2019" means opening it, ticking every item inside and leaving an empty folder. On the startup disk this lock is right; on a user's own external drive it is the main task.
Recommendation: on external volumes (the D3 allow-list) let top folders be selected, except hidden ones and macOS's own (`.Spotlight-V100`, `.Trashes`, `.fseventsd`, `Backups.backupdb`, `.DocumentRevisions-V100`, `.TemporaryItems`), which stay locked with their reason; the confirmation says "Moves to the Trash of My Backup". Keep the top-folder lock on the startup disk.

### S3: polish

**U7-9 S3 AI step: two names for one setting, an unannounced title, two closing buttons** (health6: welcome.rs; tray6: settings.rs; H4, accessibility)
Evidence: `u7_09_ai_step_names_and_buttons`. The welcome says "Let AI agents clean after a preview", Settings "Let AI assistants clean after you confirm (MCP)", the guide link "How to connect an AI agent", the group "AI assistants". The title "Use CleanCraft from your AI apps?" is painted only, so VoiceOver announces nothing for a page that has no sidebar entry. With nothing connected, "Done" (primary) and "Not Now" both close the step.
Recommendation: one wording everywhere: "AI assistants", switch "Let AI assistants clean after you confirm"; give the title a label node on pages without a sidebar entry (ConnectAi, AgentGuide); show "Not Now" while nothing is connected and "Done" once something is.

**U7-10 S3 Developer Clutter rows are all named "target"** (dev6: core item names; H6)
Evidence: `task_04`, `u7_10_rows_lead_with_the_project`, `review-u7/u7-10-dev-rows.png`. Two rows read "target", the project only in the dim path and the reason.
Recommendation: title "alpha / target" (project, then folder), the path stays dim.

**U7-11 S3 Confirmation noise and the missing kept copy** (files6: plan_page.rs, sweep.rs; H8, H1)
Evidence: `task_04`, `task_12`, `u7_11_quick_sweep_confirmation_says_what_stays`, `review-u7/u7-11-sweep-confirm.png`. "2 items in all. Scroll the list to see every one." under two rows that are all in view; Quick Sweep's "Threats go to the CleanCraft quarantine." when no threat is included; Duplicate Downloads lists the two copies that go, "Same content as 2 other files", never the one that stays.
Recommendation: the scroll hint only when the list is taller than its box; the quarantine sentence only when a threat is in the plan; each duplicate row's tag "Copy of photo.jpg, which stays" (or a "Stays" line per set).

**U7-12 S3 Quick Sweep tiles** (files6: sweep.rs; H8, accessibility)
Evidence: `u7_12_quick_sweep_tiles_are_named_and_only_offer_what_applies`, `review-u7/u7-12-sweep-tiles.png`. The subtitle still reads "junk, mail downloads, the Trash, threats and your apps" (no duplicate downloads, no developer clutter); "All clean" tiles keep an Include box; five boxes are all named "Include" and eight buttons "Review" for VoiceOver.
Recommendation: subtitle "One scan across junk, downloads, the Trash, threats, code projects and your apps."; no Include on a tile with nothing to clean; accessible names "Include Duplicate Downloads", "Review Duplicate Downloads".

**U7-13 S3 Trash Bins: the bar adds the Mail trash next to "Empty Trash in Finder"** (owner of extras_cleanup.rs; H2)
Evidence: `task_11`, `u7_13_trash_bar_counts_what_finder_empties`, `review-u7/u7-13-trash-bar.png`. "1.59 MB in the Trash" beside Empty Trash in Finder, which frees 1.40 MB; the other 184 KB is Mail's and only Erase in Mail frees it.
Recommendation: "1.40 MB in the Trash" on the bar; the Mail size stays on its group header with Erase in Mail.

**U7-14 S3 Mail erase: no Try Again after Accessibility** (owner of extras_cleanup.rs; H9, H7)
Evidence: `task_11`, `u7_14_mail_erase_can_be_tried_again`, `review-u7/u7-14-accessibility.png`. After Open Accessibility Settings the notice says "Turn CleanCraft on, then use Erase in Mail again"; the user clicks Erase in Mail... and confirms again (2 steps).
Recommendation: a "Try Again" in the notice that runs the confirmed erase once more (the user confirmed it seconds ago), checking Accessibility first (`AXIsProcessTrusted`) when the window becomes active.

**U7-15 S3 Menu bar settings: an option shown On that cannot run, and an inverted label** (tray6: settings.rs; H1, H2)
Evidence: `task_06`, `u7_15_options_that_cannot_run_do_not_show_on`, `review-u7/u7-15-settings-menu-bar.png`. With the menu bar off, "Warn when the startup disk is almost full" shows a green (greyed) switch: nothing will warn. The label column says "Notifications" next to a "Pause notifications" switch, so On means silent.
Recommendation: while the menu bar is off, show the dependent switches Off (keep the saved choice and restore it when the menu bar comes on); label column "Pause" (or switch "Show notifications", On by default).

**U7-16 S3 Mac Health details** (health6: health.rs, pages/health.rs; H4, H7)
Evidence: `u7_16_health_buttons_go_where_the_text_says`, `review-u7/u7-01-what-grew.png`, render `docs/screens/v2/r6/mac-health-1280x820.png`. The battery card says "System Settings > Battery says if it needs service" and its button opens Live Monitor; the folders under What grew are text, not links; the chart's "Getting low" line sits at 15% (75 GB of 500 GB) while the disk alert warns at 10%; "Why:" is one sentence of six causes.
Recommendation: battery button "Open Battery Settings" (`x-apple.systempreferences:com.apple.Battery-Settings.extension`); each What grew folder opens Disk Map on it; one threshold for "Getting low" and the disk alert (the Settings value); "Why" as a short list or "6 things need attention" (the cards below say which).

**U7-17 S3 Large & Old age groups not in age order** (files6: core large_old.rs; H4)
Evidence: `task_10`, `u7_17_age_groups_in_age_order`, `review-u7/u7-17-age-groups.png`. Order: 3 to 6 months, 6 to 12 months, over a year, last 3 months.
Recommendation: oldest first: over a year, 6 to 12 months, 3 to 6 months, last 3 months, unknown.

**U7-18 S3 App Remover selection bar** (apps6: apps.rs; H1)
Evidence: `task_02`, `u7_18_selection_bar_numbers_and_hint`, `review-u7/u7-18-apps-bar.png`. "3 apps selected, 9.11 MB with their leftovers" under a header "9.09 MB in 4 apps": the selection is bigger than the whole (the header counts bundles only); the hint is cut to "Each app's file…".
Recommendation: header "9.09 MB in 4 apps (apps only)" or count leftovers there too; let the hint wrap to a second line or keep only "Shift-click selects a range." with the rest in a tooltip.

## Counts

S1: 2 (U7-1, U7-2). S2: 6 (U7-3 to U7-8). S3: 10 (U7-9 to U7-18). Proved by failing tests: every finding except U7-2 (notification click on a real Mac; the recommended core test is in its text).

## Checked and fine

- Multi-select uninstall with a running app: the lock is per app, with Quit Editor in that app's group and "Running apps stay until you quit them"; after the polite quit the button becomes "Uninstall 3 Apps" by itself; the confirmation groups files by app; Undo per app and Undo All on Done.
- Developer Clutter keeps the active project unticked and says why ("active project, a file changed today. Cargo build makes it again.").
- The trash-offer landing ("Viewer is already in the Trash. These are the files it left behind.") lists only the leftovers and leaves the app in the Trash.
- Weekly scan: one click on Every week gives Monday 10:00 with "Next scan" and "A scheduled scan only looks".
- Erase in Mail: the confirmation says Mail erases for good, on the server for mail accounts, and names the menu command; without Accessibility the notice gives both ways forward (Open Accessibility Settings, Open Mail).
- Disk Map location picker lists the external drive with size and free space; "Map My Backup" names the target.
- Large & Old grouping by Age works in one menu; each group has its own select box.
- Privacy cookies and Mail attachments are unticked by default (fix6), as asked.
