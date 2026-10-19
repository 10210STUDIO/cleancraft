# CleanCraft design system (round 2)

The single source of these numbers is `crates/ui-egui/src/theme.rs` (`space`, `radius`,
`size`, `icon`, `hit`, `Tokens`). Components that use them live in `kit.rs`. Pages never
write their own font sizes, icon sizes or colours; when a page needs a new role, the role is
added to `theme.rs` first and to this file.

Units are points (pt). On a Retina Mac one point is two pixels; snapshots are rendered at
`pixels_per_point` 1 and 2 (`docs/screens/v2/*.png` and `*@2x.png`).

## Audit of v1 (what the owner saw on his Mac)

From `docs/screens/real-mac-v1` and `docs/screens/v1`:

- Sidebar group labels were 9.5 pt upper case in a grey with 4.1:1 contrast; on Retina they
  read as noise.
- Paths, counts, tags and app meta lines were 10.5 to 11.5 pt in `text_faint` (#6E7883), which
  measures 3.4:1 on a glass card and 3.0:1 on a selected row: below WCAG AA for small text.
- Sidebar icons were 16 pt with 13 pt labels in 28 pt rows: a dense, small sidebar for a
  1280 pt window.
- Page header tile 40 pt with a 21 pt title: weak hierarchy against the 46 pt result total.
- Quick Sweep start screen: the picture, title and button sat in the middle of a large empty
  field; the results grid used 170 pt tiles and left the lower half of the page empty.
- Live Monitor: two rows of three columns with an empty sixth slot.
- Done screens: small check, small summary text, the "space is freed" note above the summary.

## Type scale (Inter, SIL OFL)

Mapped to the macOS Human Interface Guidelines roles for a Mac utility, one step larger
than the HIG defaults for body and secondary text, because CleanCraft's own reading
material is file names and paths.

| Token (`theme::size`) | Size | Weight | Line height | HIG role | Used for |
|---|---|---|---|---|---|
| `DISPLAY` | 52 | Bold | 1.1 | (none) | the total found on a results page |
| `NUMBER` | 30 | Bold | 1.15 | (none) | values in Quick Sweep tiles and Live Monitor cards, App Remover detail total |
| `LARGE_TITLE` | 30 | Bold | 1.2 | Large Title (26) | start-screen title, Done headline |
| `NUMBER_S` | 24 | Bold | 1.2 | (none) | selected size in the bottom bar (same size as `TITLE`) |
| `TITLE` | 24 | Bold | 1.2 | Title 1 (22) | page title in the header, app name in App Remover detail |
| `TITLE2` | 19 | SemiBold / Bold | 1.25 | Title 2 (17) | dialog headline, progress headline, app name in the sidebar logo, empty states |
| `TITLE3` | 16 | SemiBold | 1.3 | Title 3 (15) | card titles, "found in" and "ready to clean" next to the total |
| `BODY_L` | 16 | Regular | 1.4 | (none) | lead text under a start-screen title, Done summary (same size as `TITLE3`) |
| `BODY` | 14 | Regular / Medium / SemiBold | 1.35 | Body (13) | row names (SemiBold), sidebar entries (Medium, SemiBold when active), sizes in rows, primary buttons |
| `CALLOUT` | 13 | Regular / Medium | 1.35 | Callout (12) | page subtitles, secondary lines, secondary buttons, banners, strips, confirmation lines |
| `FOOTNOTE` | 12 | Regular / Medium | 1.3 | Footnote (10) | paths, tags, counts, app meta lines, row actions (Reveal, Ignore) |
| `CAPTION` | 11.5 | SemiBold, upper case, +0.6 to 0.8 letter spacing | 1.2 | Caption (10) | sidebar group labels and eyebrows only |

Nine sizes in all (`theme::SCALE`: 52, 30, 24, 19, 16, 14, 13, 12, 11.5); `theme::snap`
puts text sized from a shape (the initial in an app tile) on the nearest one.

Rules:
- Nothing a person must read is under 12 pt. 11.5 pt exists only as spaced upper case.
- Weights: Bold for numbers and titles, SemiBold for names and card titles, Medium for
  controls and sidebar entries, Regular for running text.
- One `DISPLAY` number per screen at most.
- Running text is at most `space::MEASURE` (560 pt) wide.

## Icon and picture sizes (`theme::icon`)

| Context | Token | Size |
|---|---|---|
| Inline icons (buttons, strips, banners) | `INLINE` | 16 |
| Sidebar entries | `SIDEBAR` | 18 (stroke 1.6) |
| File-kind icon in a results row | `ROW` in `ROW_TILE` | 20 in a 36 tile |
| App icon in list rows (App Remover, Startup Items) | `APP_ROW` | 32 (36 in App Remover rows, 40 in App Updates cards, where the macOS icon's own margin makes it read as 32) |
| App icon in App Remover detail | `APP_LARGE` | 64 |
| Module tile in a card head | `CARD` | 36 (28 in Live Monitor card heads) |
| Module tile in the page header | `HEADER` | 48 |
| Dialog head tile | `DIALOG` | 48 |
| Hero object, start screen (`heroes::show`, Idle) | `HERO_MIN` to `HERO_MAX` | 96 to 240 by window height (square of side x 1.12) |
| Hero object, scanning (Scanning) | | 170 or 220 by window height; elapsed time under the status line |
| Hero object, Done (Done, with a check badge at 30% of its size on its lower right) | | 150 or 200 (Quick Sweep 120 or 160, it has cards under it) |

App icons go through `kit::app_icon(ui, rect, name, bundle_path)`: the real icon from the
icons agent's `images::app_icon` when loaded, the coloured initial tile as placeholder.

## Spacing (`theme::space`), 4 pt grid

`XXS 2`, `XS 4`, `S 8`, `M 12`, `L 16`, `XL 24`, `XXL 32`, `XXXL 48`.

- Content margin: 32 left/right, 24 top/bottom (`PAGE_X`, `PAGE_Y`).
- Sidebar: 232 wide, inner margin 12 x 16.
- Gutter between cards and between the two columns of a results page: 16 (`GUTTER`).
- Header to content: 16. Card inner margin: 16 to 18. Dialog inner margin: 24.
- Results left column: 272 wide (236 under 760 pt of content width).

## Corner radii (`theme::radius`)

`XS 6` tags and checkboxes, `S 8` sidebar rows and small tiles, `M 12` strips, wells and list
rows, `L 16` cards, `XL 22` dialogs and the bottom bar, `PILL` buttons. Icon tiles use 30% of
their side (a squircle-like tile), app initial tiles 26%.

## Elevation

| Level | Where | Shadow |
|---|---|---|
| 0 | backdrop, sidebar | none |
| 1 | glass cards, tiles | `card_shadow`: y 6, blur 22, black 27% |
| 2 | bottom action bar, dialogs, popups | `float_shadow`: y 10, blur 36, spread 2, black 47% |
| Modal scrim | behind dialogs | #04060A at 67% |

Glass cards are `glass` (#2A3140 at 59%) over the section backdrop, with a 1 pt hairline
(`glass_stroke`, white 8%) and a top edge highlight (`glass_edge`, white 13%).

## Colour roles and contrast

Surfaces (as composited): `bg` #10131A, `sidebar` #0B0E14, glass card #1F2530, well
(black 24% on glass), selected row (section colour 9% on glass), dialog #1A1F2A, bottom bar
#1C222E.

WCAG 2.1 contrast ratios, measured with `docs/design/contrast.py`:

| Text | Colour | bg | sidebar | glass | well | selected row | dialog | bar |
|---|---|---|---|---|---|---|---|---|
| `text` | #F0F2F5 | 16.5 | 17.1 | 13.6 | 15.1 | 11.8 | 14.6 | 14.3 |
| `text_dim` | #BEC6D0 | 10.8 | 11.2 | 8.9 | 9.9 | 7.7 | 9.6 | 9.4 |
| `text_faint` | #939DAA | 6.8 | 7.0 | 5.6 | 6.2 | 4.9 | 6.0 | 5.9 |
| `accent` | #2EC4A6 | 8.5 | 8.8 | 7.0 | 7.8 | 6.1 | 7.5 | 7.4 |
| `danger` | #FF7A74 | 7.3 | 7.6 | 6.1 | 6.7 | 5.3 | 6.5 | 6.4 |
| `warning` | #F2B13C | 9.9 | 10.2 | 8.2 | 9.0 | 7.1 | 8.7 | 8.6 |
| `success` | #5AD68A | 10.1 | 10.5 | 8.4 | 9.3 | 7.3 | 9.0 | 8.8 |
| Clean blue (section) | #4EA8FF | 7.4 | 7.7 | 6.1 | 6.8 | 5.3 | 6.6 | 6.4 |

Every text/background pair is at least 4.5:1 (AA for small text). v1's `text_faint`
(#6E7883) was 3.0 to 4.3 and is gone.

Tags draw their text in the full role colour on a 16% tint of it; the tint lowers the
background by under 1 point of contrast, so tag text stays above 5:1.

Text on gradient fills (primary buttons, the round action, checkbox ticks) uses
`theme::on_color`: dark ink #08161C on light section colours, white on deep ones.

Each section gradient is chosen so its label colour reaches 4.5:1 at both ends (Overview
end #2690B4: ink 5.0; Neutral end #7A8AA6: ink 5.3; the others 5.5 to 11).

Section colour pairs (start, end) give each sidebar section its own field: Overview mint,
Clean glacier blue, Protect coral, Speed lime, Apps periwinkle, Files amber, System sky,
Neutral slate (History and Settings).

## Hit targets and control heights (`theme::hit`)

- Minimum hit target 28 pt (HIG). Ghost row actions (Reveal, Ignore) are 28 tall.
- One control height per row: primary, secondary and danger buttons, text fields, search
  fields and pop-up menus are all 32 (`hit::PRIMARY`, `SECONDARY`, `FIELD`); the primary
  stands out by its fill. Only the start screen's round action (112) is bigger.
- Checkbox: 18 pt box in a 22 pt hit area inside a 28 pt line.
- Sidebar row 28 to 36 (adapts to the window height so all 17 entries fit at 680 pt, never
  under 28).
- Results item row 60, category row 56, app row 58, confirmation line 30.

## Components (kit.rs)

`pill` (Primary, Secondary, Danger, Ghost), `switch`, `check`, `check_mixed`, `check_text`, `radio`,
`search_field`, `text_field`, `sort_menu`, `quiet_label`, `card`, `well`,
`bar_frame`, `bottom_bar`, `dialog_frame`, `modal`, `dialog_head`, `strip`, `header`,
`card_head`, `hero`, `round_action`, `chips`, `progress`, `progress_ring`, `done_check`,
`done_hero`, `tag`,
`bar`, `gauge`, `sparkline`, `badge`, `icon_tile`, `app_icon`, `initial_tile`, `confirm_line`,
`text_at`, `place`.

### Added in round 3

- `switch`: a 38 x 22 pt macOS switch (green when on) for states that are on or off
  (Startup Items). Its accessible name is the action a click takes ("Turn Off Dropbox"). It
  replaces the old "On" tag plus "Turn Off" button pair.
- `modal` returns a `Dialog`: it fades in with its scrim and rises 8 pt (`motion::tr::OVERLAY`,
  0.2 s; instant with Reduce motion), has a close button (X, 28 pt hit area, "Close dialog")
  at its top right, and `should_close()` is true after the X or Esc (both mean Cancel). A
  click on the scrim does nothing: the dialogs confirm moves.
- `display_count`: the results total counts up from 0 over 0.6 s (`motion::tr::COUNT`);
  VoiceOver hears the final figure from the first frame.
- `appear`: rows of a results list and Quick Sweep tiles fade in 35 ms apart
  (`motion::appear`).
- `chips_with_state`: Quick Sweep's scanning screen lists every module as a chip that is done
  (green check), scanning (spinning arc, accent border) or waiting (dimmed).
- `progress_with`: the scanning screen with a known share done and extra content.
- `scroll_fade` (lists in cards) and `scroll_fade_page` (lists on the backdrop): a 24 pt fade
  at the bottom of a scroll area while more content is below, and at its top once scrolled.
- `dither`: one or two levels of white noise (a 64 px tile, one texel per physical pixel) over
  the backdrop, so its wide dark gradients do not band on Retina displays.
- `theme::DANGER_A` / `DANGER_B` and `danger_label()`: the Danger pill's label is ink, at
  least 4.5:1 on both ends of its gradient (white was 2.7:1).

## Layout

- Supported window sizes: 1060 x 680 (minimum) to 1920 x 1200; default 1280 x 820, which
  fits a 14 inch MacBook Pro at its default scaling (1512 x 982) with room for the Dock.
- Nothing overlaps, clips or scrolls sideways at any of those sizes (checked by
  `ux_render::no_page_uses_a_glyph_inter_cannot_draw_and_nothing_leaves_the_window`).
- Every module page shows its header (badge, title, one line) in every phase: start screen,
  scanning, results, Done. Clicking Scan never moves the title. The start screen under it is
  the module's object, optional controls, the round action and one note; the title and
  explanation are not repeated in it.
- Start screens centre their block vertically (85% of the free space above, so the block
  sits slightly high), with the picture sized by what is left.
- Done screens, Quick Sweep's included, are one centred composition: object with check badge,
  headline, buttons, then any per-module cards.
- The sidebar's top 28 pt are the title band: on macOS the traffic lights sit at its left
  (transparent title bar), the logo and "CleanCraft" right of them. Group labels get 8 pt
  above and 4 pt under them (22 pt at the 680 pt minimum, 26 to 30 in taller windows); rows
  never go under 28 pt.
- Results rows: line 1 is the name, the path (shortened in the middle) and the size; line 2
  is the reason tag, which runs under the size to the row's end. A reason that repeats the
  row's name drops it ("Trash of USB: 1 item" on the row "Trash of USB" reads "1 item").
  Locked rows show a padlock instead of the checkbox; why they are locked follows the tag
  when it fits whole, and is always in the tooltip.
- Grids (Quick Sweep tiles) share the page height between rows so they fill the page.
