# Design review B (designqa, round 2)

What was checked: `agent2/design` at 91ece24, which already contains `agent2/motion` (36c5559) and `agent2/icons` (36d57fb); a local merge of the three branches gave the same tree. On that tree I ran:

- `cargo test -p cleancraft-ui-egui --test review_design_a -- --include-ignored`: all 6 pass (REVIEW-A 1, 2, 3, 4, 57 are fixed; design may drop those `#[ignore]`s on its branch).
- `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_render pages_render`: 35 scenes at 1060x680 and 1280x820, 1x and 2x, plus the hero sheet, all reviewed by eye and with sampled pixels.
- `cargo test -p cleancraft-ui-egui --test review_design_b -- --include-ignored` (new file, this commit): 3 tests, all fail today (B1, B2, A31).
- `cargo clippy -p cleancraft-ui-egui --lib -- -D warnings`: **fails** (B4).

Evidence crops are in `docs/design/review-b/`. Contrast is the WCAG ratio from sampled render pixels.

## Status of the REVIEW-A defects

69 defects: **41 fixed, 10 partly fixed, 18 open.**

| # | Sev | Owner | Status | Evidence |
|---|---|---|---|---|
| 1 | S2 | design | Fixed | A1 test: 9 sizes on screen (was 20). |
| 2 | S2 | design | Fixed | A2 test: nothing under 11 pt (was 74 texts). |
| 3 | S2 | design | Fixed | A3 test: text_faint #939DAA, >= 4.5:1 on bg, card, glass. |
| 4 | S2 | design | Partly | A4 test passes for every section gradient (ink labels). The Danger pill still paints white: "Move to Trash" 2.69:1 at the light end (#FD726D), "Uninstall" 2.68:1 (#FD736D), 3.25:1 at the dark end. Target 4.5:1. |
| 5 | S1 | design | Partly | kit radios replace egui's (settings, Large & Old Files). Unselected ring #5F6876 on card #212632 = 2.69:1; target 3:1 (now S3). |
| 6 | S2 | design | Fixed | @2x "Review and Clean" edge is smooth; hero meshes are smooth. |
| 7 | S2 | design | Fixed | Toolbars 32 pt (search, sort, buttons); bars and dialogs use one height. |
| 8 | S2 | design | Partly | "Empty Trash in Finder", "Wi-Fi Settings", "Reset Permissions" are pills now; Duplicates "Select Copies" is still egui's rectangular button (duplicates-results). |
| 9 | S2 | design | Fixed | Search field with magnifier, 32 pt. |
| 10 | S3 | design | Fixed | Sort pop-up uses an up/down chevron. |
| 11 | S2 | design | Fixed | "Back to Apps" and Disk Map "Back" use a chevron. |
| 12 | S2 | design | Open | Title bar unchanged (main.rs out of design's scope; needs the lead). |
| 13 | S2 | design | Fixed | Section headers 11.5 pt semibold, letter-spaced. See B7 for spacing at 680 pt. |
| 14 | S3 | design | Fixed | Active row keeps its weight. |
| 15 | S3 | design | Open | Not addressed. |
| 16 | S3 | motion | Open | Sidebar logo "C" still closes at 1x (gap under 1 px); the new 3D AppIcon.png is not used in the sidebar. |
| 17 | S3 | design | Open | Not addressed. |
| 18 | S2 | design + motion | Open | Idle still has no header; scanning, results and done have it. The hero morphs between phases, but the page title still jumps. |
| 19 | S2 | design | Fixed | Footnotes at #939DAA. |
| 20 | S2 | design | Fixed | "ready to clean" / "found in N items" share the number's baseline. |
| 21 | S2 | design | Fixed | Quick Sweep tiles in 3 columns, no scrollbar over cards. |
| 22 | S2 | design | Fixed | Neutral title with a warning glyph. |
| 23 | S3 | design | Open | Still "Run". |
| 24 | S3 | design | Open | Lists and the category column still end in a hard cut (system-junk-results: 7th category half visible). |
| 25 | S2 | design | Fixed | Show History and Done share one height. |
| 26 | S3 | design | Open | Quick Sweep Done is top-aligned with cards; module Done is centred. |
| 27 | S2 | design | Partly | Duplicates now shows full paths (copies can be told apart). System Junk still cuts names and paths at 1060: "iPhone_17...Restore.ipsw", "/Users...tore.ipsw", tag "iPhone system update: iPhone_17.0_Restor..."; Privacy tags end in "..." on every row. |
| 28 | S3 | design | Fixed | Reveal and Ignore appear on hover only. |
| 29 | S2 | design | Open | Locked Disk Map rows keep an enabled-looking checkbox. |
| 30 | S2 | design | Partly | One row on System Junk, Duplicates, Trash Bins. Privacy Traces at 1060: buttons overlap, see B1 (S1). |
| 31 | S3 | design | Partly | Bottom bar no longer repeats the size; "Zero KB" remains in Trash Bins' bottom bar, group headers ("Zero KB selected"), Disk Map "Selected: Zero KB", Monitor "Zero KB/s", the empty state. Test `review_a31_no_zero_kb_wording`. |
| 32 | S2 | design | Fixed | No slab under the bottom bar. |
| 33 | S3 | design | Fixed | Mixed (dash) state on categories. |
| 34 | S2 | design | Open | Trash item still named "0". |
| 35 | S2 | design | Fixed | Reason column starts at one x. |
| 36 | S3 | design | Fixed | List box shrinks to content. |
| 37 | S3 | design | Partly | Esc closes; no close button. |
| 38 | S3 | design | Fixed | Count line first, explanation after. |
| 39 | S3 | design | Fixed | Hero descriptions balanced on two lines. |
| 40 | S2 | design + motion | Fixed | Hero 96 to 240 pt by available height. Large & Old Files still gets the minimum 96 pt because of its options card (S3 polish). |
| 41 | S2 | design | Fixed | Options card left-aligned with kit radios. |
| 42 | S3 | design | Open | Disk Map idle still crowded; "Map Folder" ghost is 26 pt next to a 32 pt field. |
| 43 | S2 | design + motion | Open | Scanning shows the hero, one status line and m:ss; no per-module or found-so-far progress. |
| 44 | S1 | icons | Fixed in tests | `icons_ui.rs` proves the real icon replaces the placeholder (pixel check) in App Remover, App Updates, Startup Items, dialogs. Renders still show letters because FakeMac has no icons. Placeholder is still the letter tile (target was neutral). Confirm on the real Mac. |
| 45 | S2 | icons | Partly | "Opened" now falls back to Spotlight ranking date and app data. Vendor still comes from the bundle id (`apps::vendor_of`: "Freecad", "Jetbrains", "Co"), and every Apple app still gets "Part of macOS" (`pages/apps.rs:717`, `if a.apple`), so iMovie and GarageBand stay mislabelled. |
| 46 | S2 | design | Fixed | Reasons in text_dim. |
| 47 | S2 | design | Fixed | Label centred; one height. Ignore and Check Again still sit in the bar (S3). |
| 48 | S3 | design | Open | Same as 24 (needs a real Mac shot to confirm). |
| 49 | S3 | design | Fixed | Zero rows dimmed. |
| 50 | S2 | design | Open | "On" tag plus "Turn Off" button. |
| 51 | S3 | design | Open | Launch agent paths still cut at the end. |
| 52 | S2 | design | Fixed | Ink labels on light blocks, no mid-word breaks. |
| 53 | S2 | design | Open | Claimed fixed, but live-monitor-1060x680 and -1280x820 still show the empty 214x56 pt box with one flat segment (`review-b/a53-cpu-sparkline-empty.png`). |
| 54 | S2 | design | Fixed | Three columns filled. |
| 55 | S3 | design | Open | Gauge still about 70 pt. |
| 56 | S3 | design | Open | "375 d 6 h", "discharging", "Zero KB/s". |
| 57 | S2 | design | Fixed | A57 test; header wraps to two lines. |
| 58 | S3 | design | Fixed | Descriptions capped at about 630 pt. |
| 59 | S3 | design | Partly | Label lighter; still egui's monospace. |
| 60 | S2 | design | Fixed | Name as title, relative date, module tile. |
| 61 | S2 | design | Fixed | Labels leading and right-aligned, values with units. |
| 62 | S2 | design | Fixed | 32 pt field. |
| 63 | S2 | motion | Fixed | `heroes::draw`: full rate only while scanning or bursting, `motion::ambient_repaint` otherwise. |
| 64 | S2 | motion | Fixed | `motion::detect_reduce_motion` reads `defaults read com.apple.universalaccess reduceMotion` through env.sys, with a test. |
| 65 | S3 | motion | Fixed | Pill text no longer re-laid out during the press. |
| 66 | S2 | motion | Partly | Hero phase blend, settle spring and done burst exist. `motion::tr::COUNT`, `APPEAR`, `STAGGER`, `OVERLAY` are defined but no page uses them: totals do not count up, tiles and rows do not stagger in, dialogs pop in without a fade. |
| 67 | S3 | design | Fixed | Claimed; no first-frame jump seen. |
| 68 | S2 | motion | Fixed | 15 distinct lit 3D heroes, idle, scanning, done and burst states, 1x and 2x (`docs/screens/v2/heroes`). See B3 and B11. |
| 69 | S3 | design | Fixed | Empty, error, no-match, leftovers, history-empty, disk-map-scanning, app-updates-results scenes added. |

## New defects found in round B

| # | Sev | Owner | Page / element | What is wrong | Measured | Target |
|---|---|---|---|---|---|---|
| B1 | S1 | design | Privacy Traces results, toolbar, 1060x680 | "Reset Permissions" is drawn over "Sort by size"; both texts collide and the sort pop-up is half covered. | Sort [492..615], Reset Permissions [540..687] on the same row (`review-b/b1-privacy-toolbar-overlap@2x.png`). Test `review_b1_results_toolbar_buttons_never_overlap`. | No overlap: move Wi-Fi Settings and Reset Permissions into a "..." menu or a second row at narrow widths. |
| B2 | S2 | design | Live Monitor, Memory card, 1060x680 | The card's key figure is elided: "7.78 GB of 1..." | `review-b/b2-memory-elided.png`. Test `review_b2_monitor_figures_are_not_elided`. | Never elide figures: shrink to NUMBER_S (24 pt) or put "of 16.0 GB" on the line below. |
| B3 | S2 | motion (design for the slot) | Every hero (idle, scanning, done), both sizes | The hero glow fills its square to the edges, and the slot is clipped: a lit rectangle with a hard top edge shows behind every hero. | Disk Map idle, x=640: y 22 #342721, y 24 #7E542E (jump of 74 levels in 2 px); same on Quick Sweep, System Junk, Threat Check, Large & Old Files (`review-b/b3-hero-glow-clipped.png`). | Glow reaches zero alpha at least 8 pt inside the rect (radial falloff), or the slot is not clipped; no visible box. |
| B4 | S2 | motion, design | Build | `cargo clippy -p cleancraft-ui-egui --lib -- -D warnings` fails on the integrated branch. | heroes.rs:963, heroes.rs:1044, render3d.rs:523, 1245, 1251 (needless_range_loop, unnecessary cast, let_and_return), kit.rs:1628. | Clean clippy, as COMMON2 requires. |
| B5 | S2 | design | System Junk empty state | Empty result shows "Zero KB" at 52 pt with "found in 0 items" and two diagnostic lines ("system languages could not be read", "processor type could not be read"), then a flat 2D check tile, then 300 pt of nothing. | system-junk-empty-1060x680 (`review-b/b5-empty-state.png`). | Done-style 3D hero with "Nothing to clean", the two notes behind a small "Details" disclosure at text_dim, no 0 total. |
| B6 | S3 | design | System Junk error banner | Raw OS text in red: "Could not read ~/Library/Caches: permission denied (os error 13)". | system-junk-error-1060x680. | Plain sentence ("CleanCraft is not allowed to read ~/Library/Caches.") with an "Open Privacy Settings" action. |
| B7 | S3 | design | Sidebar at 1060x680 | Section headers sit about 4 pt under the row above and about 15 pt above their first item, so "CLEAN" reads as part of Quick Sweep's group. | sidebar crop of quick-sweep-idle-1060x680. Fine at 1280x820. | At least 8 pt above the header, 4 pt below, compress rows instead. |
| B8 | S3 | design | Quick Sweep results, Trash Bins tile | Sub-line cut: "1 item in the Trash. Emptied fro..." | quick-sweep-results-1060x680. | Wrap to two lines or shorten the copy. |
| B9 | S3 | design | App Updates results, macOS card | "macOS Sequoia 15.1  15.1 · 2.05 GB · needs a / restart": version twice, one-word wrap. | app-updates-results-1060x680. | Version once; "Restart required" as a tag. |
| B10 | S3 | design | Trash Bins rows | "Trash of USB: 1 items" and "2 items (cannot b..." | trash-bins-results-1060x680. | "1 item"; no truncated reason. |
| B11 | S3 | motion | Privacy Traces and App Remover heroes | The Privacy hero (orange disc with dots) reads as a speaker or a peephole, and App Remover's stacked rounded slabs read as layers, not removal. Others are clear (shield, trash can, envelope, cube of cubes, gears). | heroes/privacy-idle, heroes/uninstaller-idle. | Shapes that read as the module at 96 pt. |

## Remaining by severity

Open or partly fixed REVIEW-A items plus the new ones:

- **S1: 1.** B1 (Privacy toolbar overlap; regression of 30).
- **S2: 16.** 4 (Danger pill white 2.7:1), 8 (Select Copies), 12 (title bar), 18 (header jump), 27 (System Junk and Privacy truncation), 29 (locked rows), 34 (Trash "0"), 43 (scan progress), 45 (vendor names, Part of macOS), 50 (startup switch), 53 (empty sparkline), 66 (count-up, stagger, dialog fade unused), B2, B3, B4, B5.
- **S3: 21.** 5, 15, 16, 17, 23, 24, 26, 31, 37, 42, 48, 51, 55, 56, 59, B6, B7, B8, B9, B10, B11.

Top remaining, in order: B1 (overlap), B4 (clippy blocks the merge), B3 (glow box on every start screen), 4 (Danger pill contrast on the one button that moves files), 53 and B2 (Monitor looks broken at the smallest window), 45 (wrong "Part of macOS" and vendor names on the real Mac), 66 (no count-up or stagger: the results screens still appear all at once).

## Tests

- `crates/ui-egui/tests/review_design_a.rs`: all 5 checks now pass on agent2/design; they stay `#[ignore]` on agent2/designqa only because that branch has the old code.
- `crates/ui-egui/tests/review_design_b.rs`: `review_b1_results_toolbar_buttons_never_overlap`, `review_b2_monitor_figures_are_not_elided`, `review_a31_no_zero_kb_wording`, all `#[ignore]` and failing on agent2/design 91ece24. Remove the `#[ignore]` when fixed.
