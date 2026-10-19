# Design review A (designqa, round 2)

Sources: `docs/screens/v1/*.png` (57 headless renders, 1060x680 and 1280x820, one @2x) and `docs/screens/real-mac-v1/*.jpg` (real Retina captures, downscaled to 1280 px wide, so 1 px is about 1 pt). Reference: `shellycraft-reference-add-device.jpg`.

Bar: Apple HIG for macOS plus what a premium commercial Mac utility ships. Sizes are in points (pt). Contrast is the WCAG 2.x ratio, computed from `theme.rs` colours and, where gradients or alpha are involved, from pixels sampled in the renders. AA needs 4.5:1 for text under 18 pt (14 pt bold) and 3:1 for large text and for the boundaries of controls (WCAG 1.4.11).

Severity: **S1** broken or unreadable, **S2** clearly off, **S3** polish. Owners follow COMMON2: **design** (theme, kit, shell, page layouts), **motion** (motion.rs, render3d, heroes, app icon), **icons** (real app and file icons, app metadata).

Reference measurements used below:

| Token / colour | Value | On | Ratio |
|---|---|---|---|
| text | #ECEFF1 | bg #10131A | 16.09 |
| text_dim | #A9B1BA | bg / glass #1F2530 / sweep card #212B3B | 8.57 / 7.09 / 6.57 |
| text_faint | #6E7883 | bg #10131A | 4.14 |
| text_faint | #6E7883 | sidebar #0B0E14 | 4.30 |
| text_faint | #6E7883 | glass card (render #212B3B) | 3.17 |
| text_faint | #6E7883 | selected row (render #223041) | 2.98 |
| text_faint | #6E7883 | dialog (render #141820) | 3.96 |
| white | #FFFFFF | Protect hero button (render #FC776E / #EB8542) | 2.62 / 2.64 |
| white | #FFFFFF | Apps hero pill (render #7290FD / #48A2F4) | 2.95 / 2.71 |
| white | #FFFFFF | Danger pill (render #FA7775 / #E4455C) | 2.64 / 3.96 |
| white | #FFFFFF | treemap block on amber (render #ECA749) | 2.06 |
| ink #08161C | | the same five fills | 6.24 to 8.92 |
| radio ring (unselected) | render #232935 | card #202531 | 1.05 |
| proposed text_faint | #9099A5 | bg / glass / selected row | 6.45 / 4.94 / 4.65 |

## Defects

### Global: type, colour, controls

1. **All pages / type scale.** `theme::size` has 7 sizes, but the code uses 21 literal sizes (9.5, 10.5, 11, 11.5, 12, 12.5, 13, 13.5, 14, 14.5, 15, 16, 17, 18, 19, 20, 22, 24, 26, 28, 30): 12.5 appears 16 times, 13.0 14 times, 14.0 8 times, all outside the scale. Result: neighbouring texts differ by 0.5 pt, which reads as error, not hierarchy. Target: 8 roles only (Display 44, Title1 26, Title2 20, Headline 15 semibold, Body 13, Callout 12, Caption 11, plus a numeric role), every call through `size::*`. S2, design.
2. **All pages / minimum size.** Text at 9.5 pt (sidebar section headers, app.rs:286), 10.5 pt (row reason tags plan_page.rs:939, treemap labels disk_map.rs:911, `size::TINY` tags and eyebrows), 11 pt (row paths plan_page.rs:950, monitor eyebrows). Target: nothing under 11 pt; HIG macOS minimum for legible secondary text is 11 pt, sidebar section headers 11 pt semibold. S2, design.
3. **All pages / text_faint contrast.** #6E7883 is used for captions, paths, footnotes ("Scanning only looks..."), "N items" lines, sweep tile sub-lines: 4.14 on bg, 3.17 on glass, 2.98 on selected rows, 3.96 in dialogs (table above). Fails AA at 10.5 to 12.5 pt. Target: #9099A5 or lighter (4.65 on the selected row). S2, design.
4. **Hero action and pills / white on light gradients.** `theme::on_color` switches to white below luminance 150, which picks white for Protect (#FF6B6B, 2.78), Apps (#7D8CFF, 2.97) and the Danger pill always paints white (2.64 at the light end). Measured: Threat Check and Privacy Traces "Scan" 2.62:1, App Updates "Check for Updates" 2.71 to 2.95:1, "Move to Trash" and "Uninstall" 2.64 to 3.96:1. Target 4.5:1: ink #08161C gives 6.2 to 7.0 on all of them, or darken the gradient ends (for white, the fill must be at least #D9363E-ish). Use a WCAG luminance test in `on_color`, not a perceptual sum. S2, design.
5. **Settings, Large & Old Files / radio buttons.** egui default radios: 16 pt, unselected ring #232935 on #202531 = 1.05:1, effectively invisible (settings-1060x680 "CleanCraft quarantine", large-old-files-idle "Downloads", "Movies"...). Selected state is a 6 pt white dot on the same fill. Target: kit radio styled like `kit::check` (18 pt, ring 1.5 pt at >= 3:1, e.g. #707A8A = 3.53:1, selected = section gradient with ink dot). S1, design.
6. **All gradient shapes / anti-aliasing.** `icons::fan`, `gradient_disc`, `gradient_tile` (and so every Primary/Danger pill, icon tile, badge, hero disc, sparkle) build raw meshes with no feathered edge. The @2x render shows hard pixel stairs on the hero disc rim and on every sparkle; the 1x bottom bar shows stairs on "Review and Clean". egui's own shapes are feathered, so our gradient shapes look rougher than the flat ones next to them. Target: add an outer ring of vertices 1 physical px wide fading to transparent (same technique as egui's tessellator). S2, design (kit/icons mesh helpers); heroes in motion's renderer must do the same.
7. **All toolbars / control heights.** In one row: App Remover search 20 pt (egui default TextEdit, no border, no icon), "Sort by size" 28 pt, "Look Again" 32 pt; plan results: search 28, sort 30, Select All 32. Primary 38, Secondary 32, Ghost 26 pt in the same dialogs and bars (Cancel 32 next to Move to Trash 38; Show History 26/32 next to Done 38; Ignore 26, Check Again 32, Uninstall 38 in App Remover detail). Target: HIG uses one control size per row. Toolbars 28 pt, dialog and bar buttons 32 pt (primary distinguished by fill, not height), large hero action is the only exception. S2, design.
8. **Results toolbars / mixed button styles.** "Select Copies" (Duplicates), "Wi-Fi Settings", "Reset Permissions" (Privacy Traces), "Empty Trash in Finder" (Trash Bins) are egui default rectangular buttons (radius 8, no stroke) beside kit pills. Target: all kit pills. S2, design.
9. **Search fields.** No magnifier glyph, no clear button, placeholder "Search" in text_faint 3.4:1 on glass; App Remover's field has no visible frame at all. Target: 28 pt rounded field, magnifier 12 pt at 8 pt inset, clear (x) when not empty, placeholder >= 4.5:1. S2, design.
10. **Sort popup.** "Sort by size" with a solid ▼ triangle glyph. macOS pop-up buttons use an up/down chevron pair (or one chevron) drawn as an icon, 9 pt. S3, design.
11. **Back buttons use the Undo icon.** "Back to Apps" (App Remover detail) and "Back" (Disk Map) show the curved Undo arrow. Undo is a real action in this app (History, Done), so the same glyph for navigation is misleading. Target: chevron.left. S2, design.
12. **Window chrome (real Mac).** Standard title bar 28 pt with the title "CleanCraft", which repeats the sidebar brand right under it; the sidebar starts below the title bar. Premium utilities use a full-size content view with a transparent title bar, no title text, traffic lights over the sidebar, sidebar running full height. Target: `with_fullsize_content_view(true)`, `with_titlebar_shown(false)`, `with_title_shown(false)` in main.rs, sidebar top inset 28 pt for the traffic lights, logo moved under them. S2, design.
13. **Sidebar section headers.** 9.5 pt semibold text_faint (4.30:1), 21 pt row. Target 11 pt semibold, #9099A5 or text_dim, 24 pt row with 8 pt above. S2, design.
14. **Sidebar active row changes weight.** Medium to SemiBold on selection (app.rs:374): the label widens about 3% and visibly jumps when you click. Target: keep Medium, mark the active row by fill, colour and the bar only. S3, design.
15. **Sidebar icons, weight balance.** 16 pt line icons at 1.45 pt stroke; Tune-up Tasks (filled gauge) and Quick Sweep (filled sparkles) read about twice as heavy as System Junk or Mail Downloads; Trash Bins sits about 1 pt lower optically than the cap height of its label. Target: one stroke weight, filled variants only for the active row, optical centring to the x-height. S3, design (vector icons in icons.rs; icons owns only real app and file icons).
16. **Logo at 1x.** The "C" in the logo tile closes into an "O" at 1x (quick-sweep-idle-1060x680: gap under 1 px); fine at 2x. Target: wider gap (>= 2 pt) or the new 3D app icon. S3, motion (app icon).
17. **Backdrop banding.** The large dark gradient washes show visible 8-bit steps in the @2x render (quick-sweep-idle-1280x820@2x, right half). Target: add 1 to 2 LSB ordered dither or a faint noise texture. S3, design.

### Quick Sweep

18. **Idle / layout jump.** Idle has no page header; scanning, results and done have the 40 pt header. Clicking Scan moves the page title from the centre to the top-left and changes the whole composition at once. Target: keep the header (or keep the hero object and morph it into the progress ring). S2, design + motion.
19. **Idle / footnote.** "Scanning only looks..." 11.5 pt text_faint, 4.14:1. See 3. S2, design.
20. **Results / total label alignment.** "ready to clean" (and "found in N items" on every plan page) is top-aligned to the 46 pt number: its baseline sits about 20 pt above the number's baseline. Target: share the baseline (or sit centred on the number's x-height). S2, design.
21. **Results / scrollbar over cards.** The tile grid's scrollbar is drawn at x=1030 on top of the right column's border (cards end at 1031). Target: 12 pt gutter or overlay scrollbar inset 4 pt inside the content margin. S2, design.
22. **Results / error tile.** Threat Check "Could not scan" in 22 pt bold danger red with a lowercase sub-line "rules could not be loaded" in faint 11.5 pt (3.17:1). Error is louder than every number on the page. Target: neutral title, a warning glyph, sentence case sub-line at text_dim, plus a Retry action. S2, design.
23. **Results / primary label.** "Run" while the dialog that follows says "Move to Trash". Target: "Clean 14.0 MB" or "Review and Clean", like plan pages. S3, design.
24. **Results / last tile cut.** At 1060x680 the 5th tile is cut mid-card by the window bottom with no fade. Target: 24 pt bottom fade on the scroll area (same on every list). S3, design.
25. **Done / button heights.** Show History 26 pt ghost next to Done 38 pt. See 7. S2, design.
26. **Done / composition.** Done content is top-aligned (QS) but centred (System Junk done): two Done layouts for one concept. Bottom 230 pt empty at 1060x680. S3, design.

### Plan pages (System Junk, Mail, Trash, Large & Old, Duplicates, Similar, Privacy, Threat)

27. **Results rows / truncation at 1060.** Tag max = 50% of a ~200 pt text column (plan_page.rs:940), path gets the rest at 11 pt: "Derive...emo-abc", "Cache...e.Browser", "Same c...ther files", "Trash ...for good)", "/Users/t...Demo-abc", "/Users/...oads/a.bin". In Duplicates the three "a.bin" copies cannot be told apart, which defeats the page. Target: Reveal and Ignore only on hover or in a context menu (frees about 120 pt), reason on line 2 untruncated up to 240 pt, path on line 2 after it or in the tooltip, middle truncation keeping the last 2 path components. S2, design.
28. **Results rows / repeated buttons.** Every row has Reveal and Ignore pills (26 pt, white 14 alpha): 12 identical buttons on one screen. Target: show on hover and keep a context menu; HIG lists keep secondary actions out of every row. S3, design.
29. **Results rows / locked items.** Disk Map "locked: app data in Lib... System Junk or App Remover)" is truncated mid-sentence with an unmatched parenthesis; locked rows keep an enabled-looking checkbox. Target: lock glyph in place of the checkbox, reason in full on line 2 or tooltip. S2, design.
30. **Results / toolbar wraps.** At 1060x680 "Select None" (System Junk, Trash), "Select Copies" (Duplicates), "Wi-Fi Settings"/"Reset Permissions" (Privacy) drop to a second row, so the list starts 40 pt lower on some pages than others. Target: one row; Select All/None as one toggle or a menu. S2, design.
31. **Results / group header copy.** "Tester's iPhone: 2 items, Zero KB selected", bottom bar "Zero KB  0 selected, Zero KB", Disk Map "Selected: Zero KB", Monitor "Zero KB/s". Target: "0 KB" or "Nothing selected"; and do not repeat the size twice in the bottom bar ("32.0 MB  27 selected, 32.0 MB"). S3, design.
32. **Results / bottom bar slab.** Under the rounded bottom bar a square dark rectangle (#1B202B, 264..1031 x 650..660 at 1060x680) shows below the rounded corners, on every page with the bar. S2, design.
33. **Results / category list.** "All items" uses a dot while the others use checkboxes; partial categories show a full check (no mixed state). Target: mixed (dash) state; "All items" as a plain selectable row. S3, design.
34. **Results / Trash Bins naming.** The first item is called "0" (the folder name in /Volumes/.../.Trashes/0). Target: "Trash on <volume name>". S2, design.
35. **Confirm dialog / ragged reason column.** Reason x varies 573 to 586 pt row to row (system-junk-confirm), and reasons are themselves middle-truncated ("Downloaded u...m.example.Updater"). Target: fixed column start, end truncation of the reason, full text in tooltip. S2, design.
36. **Confirm dialog / list box height.** The list box keeps a fixed height and shows about 60 pt of empty space under 12 items (system-junk-confirm). Target: shrink to content up to the max. S3, design.
37. **Confirm dialog / no close affordance.** No close button, no Esc hint; Cancel is 32 pt next to a 38 pt destructive button (see 7). ShellyCraft's reference dialog has a close X at top-right. Target: close X 28 pt hit area, Esc and Return mapped. S3, design.
38. **Done / hierarchy.** The faint 11.5 pt line "The space is freed when..." sits above the 13 pt dim line "27 items moved. 0 skipped.": smaller and fainter first. Target: count line first in text_dim, explanation after. S3, design.
39. **Idle / description orphans.** Hero description max 540 pt leaves one-word lines: "install them." (App Updates), "unwanted." (Threat Check), "you." (Large & Old), "it to the Trash." (Disk Map). Target: balance lines (wrap at the width that gives even lines) or max 460 pt with shorter copy. S3, design.
40. **Idle / hero size inconsistency.** Hero disc 120 pt on most pages, 72 pt on Large & Old Files (large-old-files-idle), where the orb sits 40 pt from the top and the footnote 27 pt from the bottom. S2, design + motion.
41. **Large & Old Files idle / options card.** Centred "Where to look" heading over left-aligned radios that wrap into 2 rows, then three centred faint lines (3.17:1 on glass). Target: segmented control or a pop-up for the location, one info line at text_dim. S2, design.
42. **Disk Map idle / crowding.** Hero + disk card + folder field + Map Folder + big pill + footnote in 680 pt: orb 40 pt from the top. "Map Folder" ghost 26 pt next to a 30 pt field. Target: drop the hero object to 96 pt here or move the folder field into the results header. S3, design.
43. **Scanning / no real progress.** The ring is an endless spinner with "0:00" elapsed; nothing says how far along it is. Quick Sweep shows one line "Looking at System Junk". Target: Quick Sweep lists its 5 modules with a check each as they finish; plan scans show items found and size so far, counting up. S2, design + motion.

### App Remover, App Updates, Startup Items

44. **Letter tiles instead of app icons.** App Remover list (36 pt), detail (56 pt), Startup Items (28 pt), App Updates rows. The owner asked for real icons. S1, icons.
45. **App metadata (real Mac).** Vendor from bundle id: "Freecad", "Jetbrains", "Openai", "Co" (Screaming Frog), "Blenderfoundation", "Belightsoft". iMovie and GarageBand show "Part of macOS" and "App Store" at once. "Opened: Unknown" for iMovie, GarageBand, Live Home 3D (both). Target: real developer name, "Part of macOS" only for /System/Applications (and Safari), last opened from Spotlight. S2, icons.
46. **App Remover detail / red reasons.** Reasons like "Editor 2.0", "Cache: named after com.example.Editor" are painted in danger red (#EF5B5B) at 11 pt; red tells the user these are dangerous. Target: text_dim, risk colour only for real risk. S2, design.
47. **App Remover detail / bottom bar.** "4 selected, 4.33 MB" sits 12 pt above the button centre; 4 actions of 3 heights (Ignore 26, Check Again 32, Reset 32, Uninstall 38). Target: centred label; Ignore and Check Again into a "..." menu; Reset as secondary; one height. S2, design.
48. **App Remover list / clipped last row.** The last visible row ("Visual Studio Code", "918 MB") is cut by the card's rounded corner with no fade (real Mac). S3, design.
49. **App Remover / empty categories.** "From Setapp 0 apps" with an empty bar track; "Leftovers 0 items" etc. Target: dim zero rows or hide them, no empty track. S3, design.
50. **Startup Items / On tag + Turn Off button.** Each row shows a green "On" tag and a "Turn Off" button: two controls for one state. Target: a macOS switch (38x22 pt) with the accessible name kept. "Remove" has the same weight as "Turn Off" although it is destructive. S2, design.
51. **Startup Items / path truncation.** "Runs /Applications/Example.app/Contents/Library/LoginItems/ExampleHelper.app/Contents/MacOS..." end-truncated; other pages truncate in the middle. Target: middle truncation keeping the executable name. S3, design.

### Disk Map, Live Monitor, Tune-up, History, Settings

52. **Disk Map / treemap labels.** White 10.5 to 12 pt on amber blocks = 2.06:1; "Docum ents" breaks mid-word over two lines; "Projec..." clipped by its block. Target: ink labels on light blocks (>= 4.5:1), no mid-word breaks (hide label when it does not fit one word), 11 pt minimum. S2, design.
53. **Live Monitor / empty sparkline.** On first frame the CPU card shows a 214x44 pt dark box with a single flat segment at the right edge: looks broken. Target: draw nothing until 2 samples, or a dashed baseline with "Collecting...". S2, design.
54. **Live Monitor / unbalanced grid.** Card heights 386 / 410 / 214 pt in row one; row two starts at 516 for battery and network while storage's column is empty below 300. Target: equal-height rows or a true masonry that fills the third column. S2, design.
55. **Live Monitor / gauge scale.** 60 pt semicircle gauge next to a 24 pt number; the gauge reads as a small loading indicator. Target: 88 to 100 pt gauge with the number inside. S3, design.
56. **Live Monitor / formats.** "375 d 2 h", "Zero KB/s", "discharging" lowercase. Target: "375 days", "0 KB/s", "Discharging". S3, design.
57. **Tune-up Tasks / header truncated.** Subtitle "...commands cannot be u..." truncated by `kit::header`'s `.truncate()` at 1060 pt. A page explanation must not be cut. Target: wrap to 2 lines, or shorten copy. S2, design.
58. **Tune-up Tasks / line length.** Card descriptions run the full card width (about 700 pt, 110 characters at 13.5 pt) with one-word orphans ("load."). Target: max 560 pt (about 80 characters). S3, design.
59. **Tune-up Tasks / command wells.** egui default monospace (Hack via fallback) at 12 pt, 3.9:1 text_faint label "Command:". Target: SF Mono-like look via Inter tabular or a bundled mono, 11.5 pt, label at text_dim. S3, design.
60. **History / row hierarchy.** Title is the full path in 13.5 semibold; the item name is not shown on its own; every row has the same blue document tile regardless of module or kind; dates are ISO "2026-10-19 13:26". Target: name as title, path dim below, module icon tile in module colour, relative dates ("Today, 13:26"), group by action. S2, design (file kind icons: icons).
61. **Settings / form layout.** Threshold rows are slider, number, then label ("Large file minimum (MB)") after the control; the 100 pt slider has a 22 pt flat white knob. Target: HIG form: label leading (right-aligned column), control trailing, unit after the number; knob 18 pt with shadow. S2, design.
62. **Settings / text field.** "Path to ignore" is an egui default 20 pt field next to a 32 pt button. See 7 and 9. S2, design.

### Motion (code review; the renders are stills)

63. **Hero repaint.** `kit::hero` calls `request_repaint()` every frame forever (kit.rs:560): idle pages redraw at the display rate (120 Hz on ProMotion), which a cleaner app on a laptop should not do. Target: cap ambient loops at 30 fps (`request_repaint_after(33 ms)`), stop when the window is unfocused or hidden, honour macOS Reduce Motion. S2, motion.
64. **Reduce Motion is a const.** `theme::motion::REDUCE_MOTION = false` is compile-time. Target: read `defaults read com.apple.universalaccess reduceMotion` once through env.sys (FakeSys in tests) into a runtime flag. S2, motion.
65. **Press scale re-lays out text.** `kit::pill` lays the label out again at `fs * scale` during the press (kit.rs:143): glyphs re-snap to the pixel grid each frame and shimmer. Target: scale the painted shapes with a transform, or keep text size and only scale the fill. S3, motion.
66. **Missing motion where it matters.** No animation on: results arriving (the total should count up over ~600 ms ease-out, tiles stagger in 40 ms apart), dialog open/close (egui Modal pops in: target 160 ms fade + 0.97 to 1.0 scale), rows leaving after a clean, Done arriving from the progress ring (ring closes into the check). The page transition (180 ms fade + 10 pt slide) is the only spatial motion. S2, motion.
67. **centered_block first frame.** `kit::centered_block` uses the previous frame's height, so the first frame of a page draws the block at the top and the second frame centres it (a 1-frame jump partly hidden by the page fade). Target: measure with a sizing pass (`ui.scope` + discard) or keep the height per page id from the start. S3, design.
68. **Hero objects are flat.** The owner wants 3D objects like ShellyCraft; today each module is the same gradient disc + glyph + orbit rings + sparkles at 120 pt, only the colour and glyph change, and the sparkles are aliased (6). Target: one distinct 3D object per module, lit, 150 to 200 pt depending on window height, idle drift under 4 pt, scanning state that visibly works, done state that resolves. S2, motion.

### States not covered by renders (please add scenes)

69. **Missing state renders.** No snapshots of: a plan scan that finds nothing (empty results), a plan scan error (e.g. permission denied), App Remover search with no match, History empty, Disk Map in a folder with one item, a confirm dialog with the admin strip, Startup Items with nothing. These are the screens most often seen by a clean Mac. Target: add them to ux_render so they can be reviewed. S3, design.

## Counts

69 defects. S1 = 2 (5, 44). S2 = 41 (1, 2, 3, 4, 6, 7, 8, 9, 11, 12, 13, 18, 19, 20, 21, 22, 25, 27, 29, 30, 32, 34, 35, 40, 41, 43, 45, 46, 47, 50, 52, 53, 54, 57, 60, 61, 62, 63, 64, 66, 68). S3 = 26 (10, 14, 15, 16, 17, 23, 24, 26, 28, 31, 33, 36, 37, 38, 39, 42, 48, 49, 51, 55, 56, 58, 59, 65, 67, 69).

By owner: design 58, motion 6 (16, 63, 64, 65, 66, 68), design and motion together 3 (18, 40, 43), icons 2 (44, 45).

Board top 10 (posted 16:29) map to: D1 = 44, D2 = 45, D3 = 5, D4 = 6, D5 = 4 and 52, D6 = 3, D7 = 27, D8 = 7, 8 and 30, D9 = 7, 32 and 47, D10 = 63 and 68.
