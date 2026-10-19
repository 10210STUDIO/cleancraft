# motion, round 2: easing, 3D renderer, heroes, app icon

Branch `agent2/motion`. New files only, plus the small shared changes listed at the end.

## What is there

### `crates/ui-egui/src/motion.rs` (central motion)
- `Easing`: `Linear`, `CubicBezier(x1, y1, x2, y2)` solved like browsers (Newton, then bisection), `CubicOut`, `SineInOut`, `Spring(Spring)`. Named curves `GLIDE`, `ENTER`, `EXIT`, `SWAP`, `POP`.
- `Spring { stiffness, damping }`: closed form for under, critical and over damping, `settle_time()`.
- `Motion { duration, easing, delay }` with `progress`, `delayed`, `Motion::spring`. Named transitions in `motion::tr` (PAGE, HOVER, PRESS, CHECK, COUNT, APPEAR + STAGGER, OVERLAY, HERO_PHASE, HERO_SETTLE, HERO_BURST, HERO_ENTER) and loops in `motion::lp` (PULSE, SPIN, DRIFT, hero turn, float, orbit, glint, breath).
- The flat names `theme::motion` had (`REDUCE_MOTION`, `PAGE`, `HOVER`, `PRESS`, `PRESS_SCALE`, `PULSE`, `SPIN`, `CHECK`, `DRIFT`, `ease_out`, `ease_in_out`, `progress`, `looped`) with the same values, so the theme can become `pub use crate::motion::*;`.
- Drivers keyed by egui `Id`: `animate`, `animate_bool` (restart from the value on screen), `count_to` (numbers counting up, exact end value), `appear` (staggered entrance), `since` (seconds since a state began), `ambient_repaint` (about 30 fps, none when the window is unfocused or motion is reduced).
- Reduced motion at runtime: `reduced()`, `set_reduced()`, `detect_reduce_motion(sys)` runs `/usr/bin/defaults read com.apple.universalaccess reduceMotion` through `env.sys` (REVIEW-A 64).
- Tests (`motion::tests`): browser reference values for `cubic-bezier`, monotonic curves, POP overshoot, springs (overshoot by ratio, settle, critical closed form), named timings, loops, animate restart, count_to, appear stagger, since, the macOS setting via FakeSys.

### `crates/ui-egui/src/render3d.rs` (software 3D into an egui Mesh)
- Geometry `Mesh3`: icosphere, rounded box, lathe (crease on repeated points), puck, torus and `tube` (arc with round ends), bevelled extrusion of any simple 2D outline (ear clipping), faceted gem; `transformed`, `faceted`, `finish` (winding fixed from normals, welded silhouette edges, gradient coordinate, bounding centre).
- Perspective camera, studio rig (warm key, cool fill, back light, sky/ground ambient), Blinn-Phong specular, Fresnel rim, emissive, filmic tone mapping, materials gloss / satin / metal / glass.
- Visibility: back faces culled; opaque objects sorted as a whole by centre (with an optional `bias_last`), their triangles sorted inside; glass sorted per triangle so it wraps what is inside. No depth buffer, so no z-fighting by construction; the painter order is stable frame to frame.
- Edges: every silhouette edge gets a strip one physical pixel wide fading to transparent (egui's own feathering technique), so outlines are smooth on the GPU without multisampling.
- Sprites sorted with the triangles: additive glow, soft dot, four-point glint, contact shadow.
- `rasterize`: CPU rasteriser of any egui mesh (supersampled, banded memory), for the icon and for tests.
- Tests (`render3d::tests`): every generator is closed with outward normals, concave triangulation, transform order and normal matrix, back-face culling plus one silhouette loop, key-light side brighter, near object drawn on top whatever the add order, supersampled soft edges.

### `crates/ui-egui/src/heroes.rs` (15 original heroes)
Quick Sweep faceted crystal with orbiting shards; System Junk glass cube of 27 tiles (tiles collapse when done); Mail an envelope whose flap opens and a letter rises; Trash a ribbed bin whose lid lifts; Threat Check a bevelled shield with a scan beam and a check that pops in; Privacy a lens with an iris, bead ring and pupil that dilates; Tune-up two meshing gears (correct tooth ratio and phase); Startup a power puck that presses and lights; App Remover stacked app tiles (top one lifts away when done); App Updates a refresh ring with arrowheads; Large & Old stacked slabs that rise in sequence; Duplicates two mirrored cards that merge; Similar Images fanned photo frames; Disk Map a sliced platter whose slices rise; Live Monitor a gauge ring with a heartbeat line.
- States: Idle (slow turn or sway, float under 4 pt at 200 pt, occasional glint, twinkles), Scanning (faster, comets on two tilted orbits that pass in front and behind, glow driven by progress), Done (spring into the rest pose, burst of particles and stars, shock ring, flash, then calm). Colours from `theme::Section::colors`.
- API: `heroes::show(ui, feature, HeroPhase, progress, size) -> Response` (allocates and tracks the time since the phase changed) or `heroes::draw(ui, feature, HeroState { phase, progress, t, since }, rect)`; `heroes::mesh(...)` for offscreen use; `heroes::app_icon(px, ppp)`. Repaint: full rate only while scanning or bursting, otherwise `ambient_repaint` (REVIEW-A 63). With reduced motion the heroes hold a still pose.
- Tests (`heroes::tests`): every hero in every state renders inside its rect with silhouettes; scanning and done add particles; done settles to a still pose; frame-to-frame motion is continuous (no jumps).

### App icon (REVIEW-A 16)
`assets/AppIcon.png` 1024 x 1024, rendered by `cargo run -p cleancraft-ui-egui --example app_icon` (4 x 4 supersampled on the CPU): a macOS-grid squircle (824 pt, superellipse corners, drop shadow, top sheen) in deep teal ink with a glossy 3D "C" tube around a faceted crystal and two glints. Test `ux_heroes::app_icon_is_a_full_squircle_with_transparent_corners`.

### Snapshots and budget
- `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_heroes` writes every hero in idle, scanning, done-burst and done at 1x and 2x (wgpu) to `docs/screens/v2/heroes/`, plus `sheet-<state>@2x.png` contact sheets. Reviewed: edges smooth at 1x and 2x, no z-fighting, parts layered correctly.
- `cargo run -p cleancraft-ui-egui --example hero_sheet -- <idle|scanning|burst|done> out.png 2` renders a sheet on the CPU (no GPU) for quick iteration.
- `ux_heroes::hero_frame_fits_the_budget`: one scanning frame at 2x for a 260 pt hero. Unoptimised test build on this shared 2-core box: 1 to 9 ms per frame (worst System Junk, 2400 triangles); bound 40 ms in debug, 4 ms in optimised builds.

## For design (requested on the board, their files)
1. `theme.rs`: `pub mod motion { pub use crate::motion::*; }`.
2. `kit::anim` through `motion::animate_bool` with `GLIDE`.
3. `motion::REDUCE_MOTION` uses -> `motion::reduced()`.
4. Ambient `request_repaint()` -> `motion::ambient_repaint(ctx)`.
5. `CleanApp::new`: `crate::motion::detect_reduce_motion(env.sys.as_ref());`.
6. REVIEW-A 65 (press scale re-lays out text): design says done.
7. Use `heroes::show` in `kit::hero`, the scanning screen and the done screen; `count_to`, `appear`, `tr::OVERLAY` for REVIEW-A 66.
Until design merges, the heroes are not on any page; the app looks as before.

## Check on a real Mac
- `defaults read com.apple.universalaccess reduceMotion` prints `1` when Reduce motion is on (missing key = never changed = motion on).
- Hero smoothness at 120 Hz and CPU use while idle (30 fps cap), plus `scripts/install-app.sh` with the new icon.

## Shared-file changes
- `crates/ui-egui/src/lib.rs`: `pub mod heroes; pub mod motion; pub mod render3d;`.
- `crates/ui-egui/Cargo.toml`: dev-dependency `image` (workspace) for the icon and sheet examples; `Cargo.lock` accordingly.
- No change to theme.rs, kit.rs, app.rs, pages, core, policy or executor.
