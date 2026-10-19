# motion3, round 3: title bar, sidebar logo, glow, Privacy and App Remover heroes

Branch `agent3/motion3`, base `ec752a6`.

| Item | Status | Evidence |
|---|---|---|
| REVIEW-A 12 transparent title bar | Fixed on my side; needs design3's sidebar inset to look right | `titlebar::tests::*`, `main.rs tests::main_window_has_the_transparent_title_bar_and_the_icon` |
| REVIEW-A 16 sidebar logo from the 3D icon | API ready; design3 swaps it into the sidebar | `logo::tests::*` |
| B3 hero glow box | Fixed | `render3d::tests::soft_sprites_end_inside_the_rect`, `heroes::tests::nothing_reaches_the_outer_margin_of_the_slot` (failed before: Quick Sweep burst edge 25 levels off the backdrop at 96 pt), page renders |
| B11 Privacy and App Remover heroes | Fixed (my review at 96 pt; designqa to confirm) | `docs/screens/v2/heroes/privacy-*.png`, `uninstaller-*.png`, `sheet-*@2x.png` |
| Extra (lead): sidebar section icons | Done on my side; design3 integrates | `section_icons::tests::*`, `ux_heroes::section_icons_render_on_the_gpu`, `docs/screens/v2/heroes/section-icons.png` and `@2x` |
| B4 clippy on my files | Clean | `cargo clippy --workspace --all-targets -- -D warnings` |

## REVIEW-A 12: title bar

- New `crates/ui-egui/src/titlebar.rs`. `titlebar::viewport(builder)` sets the egui 0.36 flags (checked in the registry source, `egui-winit` maps them to winit's macOS attributes): `with_fullsize_content_view(true)`, `with_titlebar_shown(false)` (transparent title bar), `with_title_shown(false)`, `with_titlebar_buttons_shown(true)`, `with_decorations(true)`. `movable_by_window_background` stays off (it would fight sliders and drag to scroll).
- `titlebar::drag_strip(ui)`: a click-and-drag strip over the top `HEIGHT` = 28 pt of the window. Drag sends `ViewportCommand::StartDrag`, double click sends `Maximized(!current)` (zoom, like a native title bar). It is registered before the app's UI, so a control placed on the strip still gets its clicks (egui gives the click to the later widget).
- `apps/cleancraft/src/main.rs`: `viewport()` builds the window (size, min size, title, icon, title bar flags); `App::ui` calls `titlebar::drag_strip` first when `titlebar::ACTIVE` (macOS only; Linux and Windows unchanged).
- Layout contract posted to design3 on the board: traffic lights take x 0..76 pt (`LIGHTS_WIDTH`), y 0..28 pt; the first sidebar row starts at `SIDEBAR_TOP` = 40 pt; no controls or ScrollAreas in the top 28 pt (labels fine; page header buttons below y 28).
- Tests: flags on the builder, flags plus icon on the real main window, drag on the strip sends StartDrag, drag below it does not, double click zooms, a button on the strip still gets its click and no drag starts.
- Not verifiable here: the look of the traffic lights over the sidebar and the real drag on macOS. Check on the Mac: drag from the empty top of the sidebar and of the page, double click there zooms, traffic lights do not cover the logo.

## REVIEW-A 16: sidebar logo

- New `crates/ui-egui/src/logo.rs`. `logo::show(ui, size_pt) -> Response` (allocates the square, hover text "CleanCraft"), `logo::paint(ui, rect)`, `logo::SIDEBAR_PT` = 32.
- `assets/AppIcon.png` is embedded with `include_bytes!`, decoded once (`OnceLock`), cropped to its 824 px squircle (the Dock shadow is left out), premultiplied, then resampled with Lanczos into one texture per physical size (32 px at 1x, 64 px at 2x), cached in the egui context. The rect is snapped to physical pixels, so the texture maps 1:1. A soft shadow is painted under it.
- `image` moved from dev-dependencies to dependencies of `cleancraft-ui-egui` (already in the workspace and the lock file).
- Tests: decodes at 32 and 64 px with opaque centre and edges and transparent corners; premultiplied colour never exceeds alpha; `show` allocates its size and caches exactly the two textures at 1x and 2x.
- Open: design3 owns the sidebar; the swap of `gradient_tile` + `Icon::Logo` for `logo::show` is theirs.

## B3: glow box

- `render3d::fit_sprite`: every soft sprite ends `SPRITE_MARGIN` = 8 pt inside the render rect. Glows and floor shadows shrink their radius (the falloff still reaches zero) and dim with the square root of the shrink so they stay a halo rather than a spot; dots and glints fade over the last 6 pt. Continuous in position, so orbiting comets never pop.
- Glow profile softened (more stops, gentler tail).
- The done shock ring stops at 1.35 world units (was 1.8, outside the slot at 96 pt) and fades out as it grows.
- The backdrop halo sits behind the object along the camera's line of sight. Before, heroes seen from above (Disk Map, System Junk, Large & Old Files) had their halo lifted above the object, where it ran off the window top.
- Evidence: Disk Map idle 1060x680, x=640: y 18..26 was #79512C..#7E542E (the halo running into the top edge), now #342721 (the page backdrop). Over the top 60 pt of six idle pages, the largest 2 px step is 2 to 5 levels (only the smooth page gradient). Sheets at 1x and 2x checked by eye: round halos, no box in any state, including the burst.

## B11: Privacy and App Remover heroes

Both are original designs built from the renderer's primitives.
- **Privacy Traces**: a padlock (bevelled body, keyhole, silver shackle) in front of a small browser window whose three history lines are the traces. Scanning: the shackle lifts and turns (unlocked) and the lines flicker. Done: the lines clear and the shackle snaps shut with the done spring.
- **App Remover**: one app tile (squircle with a 2x2 glyph) with a round red minus badge on its corner. Scanning: the tile wiggles and four leftover chips orbit it. Done: the badge pops out, the tile shrinks and turns away, the chips fly in with it, and a dashed outline of the tile stays where the app was.
- Looked at at 96 pt (sheet downscaled) and in the 1x and 2x snapshots: the lock and the minus badge read at that size.
- Removed the old lens assets (housing, rim, iris, pupil, dome).

## Extra: sidebar section icons

- New `crates/ui-egui/src/section_icons.rs`: `show(ui, SectionIcon, size_pt, selected) -> Response`, `paint(ui, rect, icon, emphasis)`, `image(icon, px)`, `SectionIcon::of(Section)`, `SIDEBAR_PT` = 28.
- Nine original small 3D objects in the heroes' materials and light: Quick Sweep crystal with a spark, Clean soap bubbles, Protect shield with a check, Speed lightning bolt, Apps tile with a 2x2 glyph, Files folder, Live Monitor ring with a pulse line, History clock, Settings gear.
- Rendered once per icon and physical size on the CPU (4x4 supersampled) into a texture: 28 px at 1x, 56 px at 2x, pixel snapped. Hover lifts 0.75 pt with a faint halo, selected 1.5 pt with a stronger halo of the section colour, eased over `motion::tr::HOVER`, immediate with Reduce motion.
- Tests: each icon covers a sensible share of its square and never touches the outer pixel ring at 28 and 56 px, has a soft but crisp edge, the nine differ from each other, `show` caches exactly one texture per icon and size. Contact sheet at 1x and 2x checked by eye.
- The geometry helpers in heroes.rs (`facing`, `retint`, `ccw`, `rounded_rect`, `squircle`, `shield_outline`, `stroke`, `gear_outline`) are now `pub(crate)` for this.

## Renders

- `docs/screens/v2/heroes/`: every hero, idle, scanning, done-burst and done, at 1x and 2x, plus `sheet-*@2x.png`, all rendered again.
- `docs/screens/v2/*.png`: every page scene rendered again (the heroes on the pages changed).

## Checks

- `cargo test --workspace`: 502 passed, 0 failed. `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- Shared files touched: `crates/ui-egui/src/lib.rs` (three mod lines: logo, section_icons, titlebar), `crates/ui-egui/Cargo.toml` (`image` to dependencies).
