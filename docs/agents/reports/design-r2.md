# Design round 2 report (design, ux-designer, visual lead)

Branch `agent2/design`. The design system is written down in `docs/design/SYSTEM.md` (type
scale, icon sizes, spacing, radii, elevation, colour roles with WCAG ratios, hit targets) and
lives in code in `crates/ui-egui/src/theme.rs` (`space`, `radius`, `size`, `SCALE`, `snap`,
`icon`, `hit`, `Tokens`, `on_color`, `contrast`) with the components in `kit.rs`. Every page
now takes its sizes from there. Renders: `docs/screens/v2/*.png` (1280x820 and 1060x680, each
at 1x and `@2x`), 36 scenes.

Merged into this branch: `agent2/motion` (3D heroes, motion.rs), `agent2/icons` (real app
icons, file icons) and `agent2/designqa` (REVIEW-A and its checks).

## What changed, everywhere

- **Type**: nine sizes (52, 30, 24, 19, 16, 14, 13, 12, 11.5), body 14, secondary 13, paths
  and meta 12, 11.5 only for spaced upper-case group labels. v1 used 21 literal sizes from
  9.5 to 46.
- **Colour**: `text_faint` #6E7883 (3.0 to 4.3:1) became #939DAA (5.6:1 on glass, 4.9 on a
  selected row); `text_dim` #BEC6D0; `danger` #FF7A74. Labels on colour fills use
  `on_color`, which now picks white or ink by WCAG contrast; the Overview and Neutral
  gradient ends were moved so the label reaches 4.5:1 at both ends. Danger pill text is ink.
- **Controls**: one height per row. Primary, secondary and danger buttons, search fields, text
  fields and pop-up menus are 32 pt; row actions 28 pt; the round start action 112 pt. New kit
  parts: `radio` (ring at 3.5:1, replaces egui's invisible radios), `search_field`
  (magnifier), `text_field`, `sort_menu` (pill with up/down chevrons), `check_mixed` (dash
  for partly selected categories), `chips`, `card_head`, `quiet_label`, `truncate_end`,
  `balanced_width`, `baseline_offset`, `app_icon`, `done_hero`. Egui default buttons in the
  Privacy toolbar and Large & Old Files were replaced by kit pills.
- **Edges**: every gradient fill (pills, tiles, discs) has a one-pixel feathered edge like
  egui's own shapes (`icons::feathered_fan`), so no stair steps at 1x or 2x. The Quick Sweep
  tile glow is a mesh clipped to the rounded tile (`icons::tile_glow`), no corner bleed.
- **Icons**: sidebar 18 pt (stroke 1.6), header tile 48, dialog tile 48, card heads 36 or 28,
  file kinds 24 in a 36 tile, app icons 36 to 40 in lists and 64 in the detail. App icons go
  through `kit::app_icon` -> `images::paint_app_icon` (real icon, initial tile while it
  loads). New vector icons: `Back` (chevron left, for Back buttons instead of the Undo
  arrow), `UpDown`, `Lock`.
- **Motion**: `theme::motion` re-exports `crate::motion`; `kit::anim` uses its eased
  `animate_bool`; Reduce motion is read at runtime (`motion::reduced()`, detected once in
  `CleanApp::logic` outside tests); ambient loops repaint at 30 fps and stop unfocused; pill
  labels keep their size while pressed.
- **Heroes**: `kit::hero` (start screens) shows `heroes::show(.., Idle)` at 96 to 240 pt by
  window height; `kit::progress` (every scanning and working screen) shows the Scanning
  object at 170 or 220 pt with the elapsed time under the status; `kit::done_hero` (every
  Done screen) shows the Done object at 150 to 200 pt with the check as a badge on it.
- **Layout fixes**: `centered_block` discards the first frame of a new block instead of
  drawing it one frame too high; confirmation lists have a height known on the first frame
  (the dialog no longer jumps for three frames, which also made clicks land beside the
  button); the bottom bar's shadow is no longer cut into a square slab and the panels' own
  separator lines are gone; page subtitles wrap instead of ending in an ellipsis; hero
  descriptions wrap to even lines.

## Before and after, per page

| Page | Before (v1, real Mac v1) | After (v2) |
|---|---|---|
| Sidebar | 16 pt icons, 13 pt labels, 9.5 pt group labels at 4.3:1, 28 pt rows, label weight jumped when selected | 18 pt icons, 14 pt Medium labels in both states, 11.5 pt spaced group labels, rows 28 to 36 pt by window height (never under 28 at 680 pt), active row with fill, hairline and bar |
| Page header | 40 pt tile, 21 pt title, 12.5 pt subtitle cut with an ellipsis | 48 pt tile, 24 pt title, 13 pt subtitle that wraps |
| Start screens | flat orb 72 to 120 pt, 28 pt title, one-word last lines | the module's 3D object 96 to 240 pt, 30 pt title, 16 pt lead text in even lines, 12 pt note at 5.6:1 |
| Quick Sweep idle | object, title and button in a large empty field | 3D gem, the five modules it covers as chips (3 and 2 per line when narrow), button, note: one balanced block |
| Quick Sweep results | 170 pt tiles, lower half of the page empty, error tile in loud red, scroll bar over the cards | tiles share the page height (184 to 260 pt), 3 per row from 1060 pt, value 30 pt, a selected/found bar, neutral error tile with a warning glyph, scroll gutter only when it scrolls |
| Scanning | spinner ring with an icon | the module's object at work, headline 19 pt, place 13 pt, elapsed time, Stop |
| Results (plan pages) | toolbar wrapped to two lines at 1060, egui buttons mixed with pills, reason and path squeezed to "Derive...emo-abc", Reveal and Ignore on every row, 10.5 and 11 pt text, total label 20 pt above the number's baseline | one toolbar row over both columns (search with magnifier, pop-up sort, Select All, Select None, module buttons), name and path on line 1, reason tag on line 2 at full width, Reveal and Ignore appear on hover (always there for the keyboard and VoiceOver), padlock on locked rows, mixed checkbox on partly selected categories, "found in" on the number's baseline, bottom bar shows "27 items selected" next to the size |
| Confirm dialog | ragged reason column, middle-cut reasons, dialog moved for three frames | fixed columns, reasons cut at their end, list sized to its rows on the first frame, Esc cancels, 48 pt head tile, 19 pt headline |
| Done screens | small check, explanation before the count | the module's Done object with a check badge, 30 pt headline, count line then explanation, one button height |
| App Remover | letter tiles, 20 pt search field without a frame, groups cut by the bottom bar, empty groups at full strength, red reasons in the detail, detail bar label 12 pt above the buttons | real icons (initial tile while loading), kit search and sort, groups scroll, empty groups dimmed with no empty bar, reasons in text_dim, centred bar label, one button height, Back with a chevron, empty states with a picture |
| App Updates | plain text lists under the cards | real icons, "Up to date" and "macOS" as two cards side by side |
| Startup Items | 28 pt letter tiles, 34 pt rows | real icons (from the icons agent), 44 pt rows, 14/12 pt text |
| Tune-up Tasks | header cut, 110-character lines, faint 11.5 pt monospace | header wraps, descriptions at most about 80 characters, command well in 12 pt text_dim |
| Live Monitor | 2 x 3 grid with an empty slot, 60 pt gauges, empty sparkline box | three stacking columns (CPU; Memory and Network; Storage and Battery), card heads with tiles, 96 pt gauges with the value inside, "Collecting" baseline before the first two samples |
| Disk Map | white labels on amber blocks at 2.1:1, names broken mid-word, 44 pt list rows, locked reason cut mid-sentence | ink or white by contrast, one-line names cut at the end, 52 pt rows, reasons cut at the end with the full text on hover, kit text field |
| History | full path as the title, same tile for every row, ISO dates | file name as the title, module tile in the module colour, "Today, 16:46", path line under it (where it is now on hover) |
| Settings | invisible radios, slider-number-label order, 20 pt text field | kit radios, form rows with the label leading and the unit after the value, kit text field, kit checkbox |
| Banners and strips | 12.5 pt | 13 pt, 14 x 10 pt padding, 18 pt icon |

## Window size

`apps/cleancraft/src/main.rs` keeps 1280 x 820 by default and 1060 x 680 as the minimum: the
default fits a 14 inch MacBook Pro at its default scaling (1512 x 982 pt) with room for the
Dock and menu bar, and the minimum is what every layout test checks. No edit was needed.

## Tests

- `ux_render`: 36 scenes (added App Updates results, App Remover leftovers and no match,
  Disk Map scanning, System Junk empty and error, History empty) checked at 1280 x 820 and
  1060 x 680 for missing glyphs, text outside the window or cut sideways and sidebar rows
  below the window. `CLEANCRAFT_SNAPSHOTS=1` writes every scene at 1x and 2x to
  `docs/screens/v2/`; `CLEANCRAFT_SNAPSHOT_ONLY=<part of a name>` and
  `CLEANCRAFT_SNAPSHOT_1X=1` narrow it while iterating.
- `review_design_a -- --ignored` (designqa): all five measurable REVIEW-A checks pass (A1 type
  scale, A2 nothing under 11 pt, A3 faint text contrast, A4 label contrast on every gradient
  end, A57 headers not cut). designqa can drop those `#[ignore]`s.
- Accessible names kept. Where a visible text got shorter, the full text is still
  announced through `kit::text_at` (bottom bar "27 items selected" announces "27 selected,
  32.0 MB"; History rows announce "... · now in ..."; paths shortened in the middle announce
  the whole path).
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all`: see the last commit.

## REVIEW-A status (design items)

Fixed: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 19, 20, 21, 22, 25, 27, 28, 29, 30, 31 (bottom
bar), 32, 33, 35, 36, 37 (Esc), 38, 39, 40, 41, 46, 47, 49, 52, 53, 54, 55, 57, 58, 59, 60, 61,
62, 63, 65, 67, 69 (scenes); 18, 40, 43 partly through the heroes (the scanning screen now
shows the module at work; it still has no per-module list).

Open:
- 12 Title bar: a full-size content view with a transparent title bar needs viewport flags
  in `main.rs` (`with_fullsize_content_view`, `with_titlebar_shown(false)`,
  `with_title_shown(false)`) plus a 28 pt top inset in the sidebar for the traffic lights. My
  brief allows only size edits there; proposed to the lead.
- 17 backdrop dithering, 23 "Run" label (tests click "Run"), 24 bottom fade on lists, 26 one
  Done layout for Quick Sweep (it keeps its cards under the headline), 34 Trash Bins item
  naming ("0" is the folder name core reports), 42 Disk Map start screen density, 43 a
  per-module progress list, 50 switches for Startup Items (tests click "Turn Off <name>"), 51
  middle truncation in Startup Items, 56 core formatters ("375 d 5 h", "Zero KB/s",
  "discharging" are asserted by core and UI tests).

## Shared-file changes

- `theme.rs`, `kit.rs`, `app.rs`, `icons.rs` (mine): as above.
- Pages restyled without behaviour changes: `plan_page.rs`, `sweep.rs`, `apps.rs`,
  `disk_map.rs`, `monitor.rs`, `startup.rs`, `tasks.rs`, `history.rs`, `settings.rs`,
  `extras_files.rs`, `extras_cleanup.rs`.
- Public API changes: `kit::hero` and `kit::progress` take a `FeatureId` instead of an
  `Icon`; `plan_page::done_view` takes the `FeatureId`; `plan_page::category_row`'s check
  tuple gained a `mixed` flag; `theme::size` gained roles (old names kept as aliases).
- `CleanApp::logic` reads Reduce motion once (not in `sync` test mode).
- `tests/ux_render.rs`: v2 output folder, 1x and 2x, filters, seven new scenes.
- No core, policy or executor change.

## To check on a real Mac

- The 3D heroes at 120 Hz (scanning repaints at full rate, idle at 30 fps).
- Real app icons in App Remover and Startup Items (icons agent's extraction).
- The hover-revealed row actions with a trackpad.
- Reduce motion: `defaults read com.apple.universalaccess reduceMotion`.
