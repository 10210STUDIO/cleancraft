# CleanCraft brand: Graphite Lime

A "CC" monogram, two nested arcs that read as CleanCraft's own initials at a glance.
Graphite surfaces, lime as the brand highlight, sky as the second brand colour, coral only
for warnings and actions that move files. The UI numbers (type, spacing, contrast tables)
are in `docs/design/SYSTEM.md`; the tokens are in `crates/ui-egui/src/theme.rs`.

## The mark

Two nested "C" arcs (a CC monogram) and a small sky dot, on a 100 unit grid
(viewBox `0 0 100 100`):

| Part | Geometry | Colour |
|---|---|---|
| Outer arc | `M74 24 A36 36 0 1 0 74 76`, stroke 13, round cap | lime #C6F432 |
| Inner arc | `M64 36 A19 19 0 1 0 64 64`, stroke 13, round cap | white #F1F3F4 |
| Dot | circle at (80, 50), radius 7 | sky #5AC8FA |

The mark's box is about 7.5 to 87 across and 8.5 to 91.5 down (both arcs plus the dot).

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

- Tile: continuous-corner squircle (corner 180 px, Apple's smooth curve), gradient #26292D
  at the top to #141618 at the bottom, at 160 degrees. A soft white light from the top
  (10%), a slightly deeper shade along the bottom edge, and a 5 px white hairline on the
  top edge that fades out down the sides.
- Shadow: the tile casts a soft black shadow (34%, 10 px down, 14 px blur) into the margin,
  like the system icons.
- Mark: the CC monogram centred on the tile at about 4.57 px per unit (46% of the tile
  side). Depth is gentle: a soft cast shadow under both arcs, a thin top-edge highlight
  along each arc, and a small lift and specular highlight on the sky dot.
- No sparkles, glints or extra objects.

Check it at 1024, 64, 32 and 16 px: the CC monogram must still read at every size.

## Menu bar icon

A template image: black on transparent, macOS tints it for light and dark menu bars and for
the highlighted state. The same two nested arcs (stroke 12 units, round caps), on the mark's
own `0 0 100 100` viewBox, 18 pt. No dot: it would be a speck at 18 pt. Load the PNG pair as
one image and set it as a template (`NSImage.isTemplate`), never draw it in colour.

## Clear space and sizes

- Clear space around the mark: at least the width of an arc's stroke times two on every
  side. On the tile, the tile itself is the clear space.
- Smallest sizes: the app icon 16 px (macOS draws it at 16 in lists), the mark alone 20 px,
  the menu bar icon 18 pt.
- The sidebar logo is the app icon tile at 32 pt (`logo::SIDEBAR_PT`), with the wordmark
  beside it.

## Wordmark

"Clean" in text white #F1F3F4 and "Craft" in wordmark lime #C6F432, one word, no space,
Inter Bold, letter spacing -0.3 pt. In the sidebar it is 19 pt (`size::TITLE2`).

## Palette (dark UI)

| Role | Hex | Token | Use |
|---|---|---|---|
| Graphite | #111315 | `bg` | the window backdrop |
| Sidebar | #0C0E10 | `sidebar` | the sidebar, a little darker than the backdrop |
| Card | #1D2023 | `card`, `glass` over `bg` | cards, tiles, wells' surroundings |
| Card hover | #262A2E | `card_hover`, `glass_hover` over `bg` | hovered cards and rows |
| Dialog | #171A1C | `kit::DIALOG_FILL` | dialogs |
| Lime | #8FBF1E | `BRAND_LIME_DEEP` | the progress bar start, graphics; never text |
| Graphite deep | #141618 | `BRAND_GRAPHITE_DEEP` | the icon tile's lower end (docs only) |
| Bright lime | #C6F432 | `accent`, `ACCENT_LIME` | highlight, primary buttons, focus, links in egui widgets, the outer arc |
| Lime light | #DFFF66 | `ACCENT_LIME_LIGHT` | the top of the primary button gradient |
| Sky | #5AC8FA | `ACCENT_SKY`, the Overview section's second colour | the logo's dot |
| Ink on lime | #101406 | `on_accent` | labels on primary buttons |
| Near-black ink | #10130E | `INK` | labels on other colour fills (section gradients, danger) |
| Wordmark lime | #C6F432 | `WORDMARK_LIME` | "Craft", lime text on graphite |
| Coral | #FF8A65 | `danger` | danger text and icons, the Protect section |
| Coral pill | #FF8A6B to #F2694E | `DANGER_A`, `DANGER_B` | the button that moves files |
| Amber | #FFB547 | `warning` | warnings that are not dangerous |
| Green | #34D399 | `success` | done, freed, healthy |
| Text | #F1F3F4 | `text` | primary text |
| Dim | #BFC5CA | `text_dim` | secondary text |
| Faint | #A1A7AD | `text_faint` | paths, captions, meta lines |

Every text colour reaches WCAG AA (4.5:1) on every surface it sits on; the table with the
measured ratios is in SYSTEM.md.

Each sidebar section keeps its own colour field (SYSTEM.md); Quick Sweep's is the brand
pair, lime into sky, the logo's own two colours.

## Do

- Use the mark exactly as drawn: lime outer arc, white inner arc, sky dot.
- Keep the app icon on its graphite tile; on graphite the mark may stand alone without it.
- Use lime for the one main action on a screen, sky as the second brand colour, coral only
  for danger.
- Render the PNGs with `scripts/render-icon.sh` after any change to an SVG.

## Do not

- Do not recolour the arcs, rotate them, stretch them or add outlines, glows or sparkles.
- Do not put the mark on busy pictures or on light colours, where it loses its edge.
- Do not use the deeper lime #8FBF1E for text; use bright lime #C6F432 or white.
- Do not use coral for anything that is not a warning or an action that moves files.
- Do not draw the menu bar icon in colour; it is a template image.
- Do not use another typeface for the wordmark.

## Sound

The interface sounds are part of the brand: water, never alarms. All are synthesized by
`crates/ui-egui/examples/make_sounds.rs` (no samples) into `assets/sounds/*.wav`, 44.1 kHz,
16-bit mono, peaks at -6 to -8 dBFS (the tap at -18), with fades at both ends.

| Cue | Sound |
|---|---|
| Scan start | a soft underwater swell: filtered noise rising like a slow wave over a deep glide |
| Scan done | two soft sonar pings a fifth apart (A5, E6) with a gentle tail |
| Clean done | a wave washes in and draws back, then a bright water drop plink |
| Trash | a short bubble gulp |
| Error | a low, muted double tone (G3, E3), calm |
| Tap | a tiny water drop tick |
