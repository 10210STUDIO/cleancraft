# lists7, round 7: search, filter and sort on every list

User request: "in all apps and maybe other sections, it would be nice to have filter, search,
sort based on what makes sense". Lead addition: apps that are part of macOS hidden in App
Remover until asked for.

Branch `agent7/lists7` (base main dc7e109, agent7/fix7 merged up to 21a49f0).

## The shared toolbar: `crates/ui-egui/src/kit_list.rs` (new)

- `search`: the kit search field with an accessible name; Cmd+F focuses it (when no dialog is
  open), Esc clears it.
- `filter_menu`: a "Filter" pill with a funnel ("Filter (2)" and tinted while filters are on;
  announced "Filter" with the filters that are on as its value). Its pop-up lists the filters
  by group with a check mark and the count each matches ("App Store (12)", a checkbox for
  VoiceOver), stays open while filters are switched, ends with Clear Filters, and is as wide
  as its longest entry.
- Filters combine like Finder rules: choices of one group marked `any` match when any of them
  does (App Store or Setapp), every other filter must match too (Unused and Large).
- `sort_menu`: "Sort by size" pop-up with an arrow for the direction; the orders, then the two
  directions of the chosen one ("Largest first" / "Smallest first", "A to Z" / "Z to A",
  "Newest first" / "Oldest first"). Announced "Sort" with the value "Sort by size" or
  "Sort by size, smallest first" (existing tests that read "Sort by size" still pass).
- `status`: while a search or a filter is on, "Showing 12 of 135 apps" (in the warning colour
  at 0), one chip per filter (click removes it, "Remove filter App Store") and Clear Filters
  (clears the search too). Its controls wait (disabled) while a pop-up covers them.
- `hidden_note` / `selected_text`: "3 selected (1 hidden by filters)".
- Remembered per list in `Settings::lists` (core `settings.rs`: new `ListPrefs { filters,
  sort, reverse }`, `#[serde(default)]`, so old settings files load). Searches are not kept.
  A list back at its defaults leaves no entry.
- Pointing hand on every control (kit `feel`/`pointer`), focus rings, Tab order: search,
  Filter, Sort, then the line.

## Where it is applied

| Page | Search | Filters (with counts) | Sorts |
|---|---|---|---|
| App Remover (apps) | name, developer, bundle id | Source: App Store, Setapp, Other sources; Show only: Unused, Running, Large (1 GB or more), Has files in Library, Selected | size, name, last opened, developer, install date (bundle birth time, else change time) |
| App Remover (Leftovers, Installers, Extensions) | path, reason | Selected, Not selected, Locked | size, name |
| App Updates | name, bundle id | App Store, From the developer | name, size |
| Startup Items | label, program, app, signer | Kind: Login items, Agents, Daemons; State: On, Off; Not signed by a known developer | name, signer |
| Every results page (System Junk, Mail Downloads, Trash Bins, Developer Clutter, Privacy, Threat Check, Large & Old, Duplicates, Similar Images) | path, reason | Selected, Not selected, Locked (only where something can be ticked; Trash Bins has none) | size, name, date where items have one ("Sort by last opened" on Large & Old) |
| Large & Old, Duplicates, Similar Images | as above | plus Kind (Archives, Movies...) and Location (~/Downloads, ~/Movies, a disk name, Elsewhere) | as above |
| Disk Map list | names in the folder | Folders, Files | size, name |
| History | module, file paths, commands | Module (those that acted), Date: Today, Last 7 days, Last 30 days | date (newest or oldest first), size |
| Live Monitor CPU and Memory top lists | | | CPU, memory, name (each card its own, remembered) |

App Remover, lead request: apps that are part of macOS are hidden by default. The toolbar
row says "2 macOS apps hidden" with "Show apps that are part of macOS" (on its own line when
the row is too narrow); the headline says "in 135 apps (apps only), 12 from macOS hidden" and
the group counts leave them out. Shown, they keep the padlock and the "Part of macOS" tag;
they are never selectable, and Select all in a group never includes them. Remembered.

## Safety: filters never decide what moves

- Bottom bars count every selected item and say how many the search and filters hide: App
  Remover apps and files, every results page, Disk Map ("1 item selected (1 hidden by
  filters)"). The group or category column only navigates, so items in another group are not
  called hidden.
- Confirmations list `plan.selected()` / every ticked app, whatever the filters (unchanged
  code paths, checked by tests).
- Select All / Select None and a group's select-all act on the items shown only.

## Tests (egui_kittest on FakeMac)

- `crates/ui-egui/tests/lists7_apps.rs` (6): macOS apps hidden by default, shown locked, never
  selectable, remembered; search narrows list and counts, Cmd+F and Esc; every App Remover
  filter with its count, any/all combining, chips, remembered after relaunch (search not),
  Clear Filters; every sort and direction (size, name, last opened with unknown last,
  developer, install date) remembered; ticked apps hidden by filters counted in the bar,
  listed in the review and the confirmation, moved only after it; App Updates search, filter,
  sort, remembered.
- `crates/ui-egui/tests/lists7_plans.rs` (5): System Junk search, Selected / Not selected /
  Locked with counts, sorts both ways, remembered after relaunch and rescan; hidden selected
  counted, Select All / None act on what is shown, confirmation lists the hidden items, they
  move after it; Large & Old kind, location, kind AND location, last opened both ways,
  remembered; Duplicates by location; Mail Downloads, Privacy, System Junk all offer the
  toolbar; Trash Bins has no empty Filter.
- `crates/ui-egui/tests/lists7_other.rs` (6): Startup Items kind, state, unsigned, search,
  signer sort, remembered; Disk Map Files / Folders, name sort both ways, search, hidden tick
  counted and moved after the confirmation, remembered; History module, Today, 7 and 30 days,
  oldest first, size, search, remembered; Live Monitor CPU card sorted by name, remembered;
  Clear Filters while the pop-up is open; pop-up width.
- `kit_list` unit tests (4): filter logic, sort fallback, texts, stored prefs round trip.
- Updated for the new default and the hidden note: qa_apps_batch, qa_apps_remover, icons_ui,
  ux4_render, fix5_review c19_c20, qa_cleanup_junk.
- qa_every_control (the click-everything explorer) passes on every page with the new controls.

clippy `--workspace --all-targets -D warnings` and fmt clean.

## Renders (1280 x 820, looked at): `docs/screens/v2/r7/`

app-remover-toolbar, app-remover-filter-menu, app-remover-filtered-hidden-selected,
app-remover-sort-menu, app-remover-macos-shown, app-updates-toolbar, startup-items-filtered,
system-junk-filtered, large-old-filter-menu, disk-map-list-toolbar, history-filtered,
monitor-sort-menu (`crates/ui-egui/tests/lists7_render.rs`, CLEANCRAFT_SNAPSHOTS=1).

## Not from lists7: failing on the merged tree (posted to fix7 on the board)

Quick Sweep tile "Review" renamed "Review <tile>" and other fix7 changes: qa_sweep (7),
qa_fix_sweep (2), fix6_defaults quick_sweep..., fix5_review n1/n2/u04, review_usability u17,
ux_render (quick-sweep scenes), qa_files_r6 (2: Review tile; Disk Map top folder now ticks its
contents), health6_ui (6: routes Scan vs Feature, first-launch steps, monitor tests),
qa_apps_batch scan_select_three (bar text now "leftovers included").

## Notes

- Install date uses the bundle's birth time from the file system (else its change time); on
  macOS that is when the app was copied to this Mac. MAC-CHECK on a real Mac.
- "Running" is read with one process listing when the list loads and again every 5 s while
  App Remover shows; "Has files in Library" uses the uninstaller's own leftover finder in the
  background after the list loads.
