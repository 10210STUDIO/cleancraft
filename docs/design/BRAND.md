# CleanCraft brand: Deep Ocean

A clean drop of water with a sweep inside: "fresh and clean" at a glance. Trustworthy
electric blue on navy, cyan as the highlight, coral only for warnings and actions that move
files. The UI numbers (type, spacing, contrast tables) are in `docs/design/SYSTEM.md`; the
tokens are in `crates/ui-egui/src/theme.rs`.

## The mark

A white water drop with a blue sweep curve inside and a small cyan dot, on a 100 unit grid
(viewBox `0 0 100 100`):

| Part | Geometry | Colour |
|---|---|---|
| Drop | `M50 10 C50 10 22 42 22 60 A28 28 0 0 0 78 60 C78 42 50 10 50 10 Z` | white #FFFFFF |
| Sweep | `M36 62 Q46 74 62 60`, stroke 7, round caps | blue #2F7BFF |
| Dot | circle at (66, 44), radius 4 | cyan #39D5FF |

The drop's box is 22 to 78 across and 10 to 88 down: 56 x 78 units.

### Files

| File | What | Made by |
|---|---|---|
| `assets/brand/AppIcon.svg` | the app icon, source | by hand |
| `assets/AppIcon.png` | the app icon, 1024 x 1024 (Dock, `.icns`, sidebar logo, window icon) | `scripts/render-icon.sh` |
| `assets/brand/MenuBarTemplate.svg` | the menu bar icon, source | by hand |
| `assets/MenuBarTemplate.png`, `assets/MenuBarTemplate@2x.png` | the menu bar icon, 18 x 18 and 36 x 36 | `scripts/render-icon.sh` |

`scripts/render-icon.sh` renders every PNG from its SVG with resvg (the `resvg` command, or
`pip install resvg-py`), so the PNGs are reproducible. Change the SVG, run the script,
commit both. `scripts/install-app.sh` builds the `.icns` from `assets/AppIcon.png`;
`logo.rs` crops the 824 px tile out of it for the sidebar.

## App icon

Apple's macOS icon grid: a 1024 canvas with an 824 x 824 tile, 100 px margin on each side.

- Tile: continuous-corner squircle (corner 180 px, Apple's smooth curve), gradient #2F7BFF
  at the top to #1546B8 at the bottom, at 160 degrees. A soft white light from the top
  (16%), a slightly deeper blue along the bottom edge, and a 5 px white hairline on the top
  edge that fades out down the sides.
- Shadow: the tile casts a soft black shadow (32%, 10 px down, 14 px blur) into the margin,
  like the system icons.
- Mark: the drop centred on the tile at 5.9 px per unit (330 x 460 px, 56% of the tile
  height), its box a little above the centre so the drop with its shadow sits optically
  centred. Depth is gentle: a cool shade (#9DB9EC) towards the drop's lower right edge, a
  faint inner rim, a soft cast shadow and a contact shadow under it, a small specular streak
  on the upper left, a tiny lift under the sweep and a highlight on the cyan dot.
- No sparkles, glints or extra objects.

Check it at 1024, 64 and 32 px: at 32 px the drop, the sweep and the dot must still read.

## Menu bar icon

A template image: black on transparent, macOS tints it for light and dark menu bars and for
the highlighted state. An outline drop (stroke 7.5 units, round joins) with the sweep
(stroke 8 units, round caps), on a square viewBox `6 4 88 88`, 18 pt. No dot: it would be a
speck at 18 pt. Load the PNG pair as one image and set it as a template
(`NSImage.isTemplate`), never draw it in colour.

## Clear space and sizes

- Clear space around the mark: at least the width of the sweep's stroke times three
  (21 units on the 100 grid) on every side. On the tile, the tile itself is the clear space.
- Smallest sizes: the app icon 16 px (macOS draws it at 16 in lists), the mark alone 20 px,
  the menu bar icon 18 pt.
- The sidebar logo is the app icon tile at 32 pt (`logo::SIDEBAR_PT`), with the wordmark
  beside it.

## Wordmark

"Clean" in text white #EDF2FA and "Craft" in wordmark blue #5FB8FF, one word, no space,
Inter Bold, letter spacing -0.3 pt. In the sidebar it is 19 pt (`size::TITLE2`).

## Palette (dark UI)

| Role | Hex | Token | Use |
|---|---|---|---|
| Navy | #0A1424 | `bg` | the window backdrop |
| Sidebar | #081020 | `sidebar` | the sidebar, a little darker than the backdrop |
| Card | #142238 | `card`, `glass` over `bg` | cards, tiles, wells' surroundings |
| Card hover | #1B2C47 | `card_hover`, `glass_hover` over `bg` | hovered cards and rows |
| Dialog | #122036 | `kit::DIALOG_FILL` | dialogs |
| Blue | #2F7BFF | `BRAND_BLUE` | the brand: icon tile, progress bar start; never text |
| Deep blue | #1546B8 | `BRAND_DEEP` | the icon tile's lower end |
| Cyan | #39D5FF | `accent`, `BRAND_CYAN` | highlight, primary buttons, focus, links in egui widgets |
| Cyan light | #6AE0FF | `BRAND_CYAN_LIGHT` | the top of the primary button gradient |
| Ink on cyan | #04203A | `on_accent` | labels on primary buttons |
| Navy ink | #061A2E | `INK` | labels on other colour fills (section gradients, danger) |
| Wordmark blue | #5FB8FF | `WORDMARK_BLUE` | "Craft", blue text on navy |
| Coral | #FF7A59 | `danger` | danger text and icons, the Protect section |
| Coral pill | #FF8A6B to #F2694E | `DANGER_A`, `DANGER_B` | the button that moves files |
| Amber | #FFB547 | `warning` | warnings that are not dangerous |
| Green | #3DDC97 | `success` | done, freed, healthy |
| Text | #EDF2FA | `text` | primary text |
| Dim | #B5C1D3 | `text_dim` | secondary text |
| Faint | #9BA9BE | `text_faint` | paths, captions, meta lines |

Every text colour reaches WCAG AA (4.5:1) on every surface it sits on; the table with the
measured ratios is in SYSTEM.md.

Each sidebar section keeps its own colour field (SYSTEM.md); Quick Sweep's is the brand
pair, cyan into blue.

## Do

- Use the mark exactly as drawn: white drop, blue sweep, cyan dot.
- Keep the app icon on its blue tile; on navy the mark may stand alone in white.
- Use cyan for the one main action on a screen, blue for brand graphics, coral only for
  danger.
- Render the PNGs with `scripts/render-icon.sh` after any change to an SVG.

## Do not

- Do not recolour the drop, rotate it, stretch it or add outlines, glows or sparkles.
- Do not put the mark on busy pictures or on light blue, where the white drop loses its edge.
- Do not use brand blue #2F7BFF for text on navy (4.1:1); use #5FB8FF.
- Do not use coral for anything that is not a warning or an action that moves files.
- Do not draw the menu bar icon in colour; it is a template image.
- Do not use another typeface for the wordmark.
