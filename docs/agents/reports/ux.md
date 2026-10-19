# UX report (ux-designer)

Branch `agent/ux`. Goal: a premium Mac look with CleanCraft's own identity (nothing taken from the reference app: own colours, icons, layout details, texts and motion).

## Before (docs/screens/real-mac-v0)

- Most sidebar and page icons were empty boxes: the Unicode glyphs in `kit::feature_look` are not in Inter, and egui's fallback fonts miss most of them on macOS.
- Pages were flat dark rectangles with a small title and a lot of empty space.
- Scanning was a tiny spinner and one path line, with no sense of progress.
- Results were plain lists with stock checkboxes; dialogs and Done screens were plain text.
- Live Monitor had no room to scroll at small window sizes.

## After (docs/screens/v1, 57 PNGs)

- **Icons** (`crates/ui-egui/src/icons.rs`): 31 original vector icons painted with egui shapes on a 24-unit grid. They cover the 15 modules, History, Settings, the logo and the small UI icons (check, warning, folder, file, app, undo, reveal, search, stop, close, chevron, picture, gear). Lines have round ends and joins, and the stroke grows with size, so they stay crisp at 1x and 2x. The same file has the mesh helpers: vertical gradients, radial glows, gradient discs and tiles, and the start-screen illustration (glowing orb, orbit rings, drifting satellites and sparkles).
- **Tokens** (`theme.rs`): spacing, radii, a type scale with a 46 pt display size, card and float shadows, glass surfaces, and the motion module (durations, easing, loops, `REDUCE_MOTION`). There is one colour pair per section: Overview mint into sea green, Clean glacier blue, Protect coral into ember, Speed lime into teal, Apps periwinkle into azure, Files amber into tangerine, System sky into spring green, and Neutral for History and Settings. The shell sets the pair every frame (`Tokens::for_section`). Sliders get a trailing fill.
- **Kit** (`kit.rs`):
  - pill buttons (Primary, Secondary, Danger, Ghost) with animated hover glow and press scale
  - an animated checkbox and a checkbox with text
  - glass cards, wells, the bottom action bar, a dialog frame and a modal helper
  - the section backdrop (vertical wash plus two soft glows)
  - the compact page header and the hero start screen (illustration, title, description, optional controls, big round action button with a two-ring breathing halo, which turns into a wide pill for long labels)
  - the progress screen (sweeping, stretching gradient ring with a glowing head; icon and elapsed m:ss inside; place shortened in the middle; Stop)
  - the Done check that grows in and draws itself
  - tags, gradient bars, the semicircle gauge and a filled sparkline
  - initial tiles for apps, middle truncation, and `text_at`, a label that shows short text but announces the full text to VoiceOver and tests
  - `place`, which lays out over a row without taking layout space (no drift in virtual lists)
- **Shell** (`app.rs`):
  - the content backdrop changes colour per section
  - pages fade in and slide 10 pt (180 ms ease-out, applied as a visual transform, so input stays exact)
  - the sidebar has the gradient logo tile, vector icons, an animated active bar and hover; row heights adapt so every entry fits at 680 pt
  - restyled Full Disk Access banner and notices
- **Pages**:
  - Plan modules (`plan_page.rs`): hero idle; progress with Stop where the scan honours `ScanCtx::cancel`; results with the big total, category cards with share bars, item rows (kind icon, name, reason tag, dimmed path shortened in the middle, size, Reveal and Ignore), and a floating bottom bar with the selected size and Review and Clean; a confirm dialog with icon head, admin strip and an icon/path/reason/size list; a shared Done view (`done_view`) with the check, Undo, Show History, Done and the Not moved card.
  - Quick Sweep: hero, progress with Stop, a responsive tile grid (1 to 4 columns; icon tile, module glow, value, Review and Include), a grouped confirm list, and Done with the check and one card per module with Undo.
  - App Remover: group cards with bars, app rows with coloured initial tiles and a chevron, a detail view with a large tile and total, a bottom bar, the confirm dialog and Done.
  - App Updates: hero, then cards per update.
  - Disk Map: hero with the disk bar and folder field, progress with Stop, a treemap with gradient rounded blocks in the Files family and a selection check, list rows with check, icon and share bar, the bottom bar, confirm and Done.
  - Live Monitor: scrollable, with gauges (CPU, battery), a filled sparkline, coloured bars that turn warm near the limit, and a restyled purge dialog.
  - Startup Items (long paths are now cut short instead of running under the buttons; On/Off tags; app tiles), Tune-up Tasks (checkbox titles, tags, command wells), History (icon tiles, Undo pill, empty state) and Settings (cards with icons).
- **Stop for plan scans**: `PlanState.cancel` and `sweep::State.cancel` (`Arc<AtomicBool>`) are passed to the scan. Stop sets the flag, drops the job and returns to the start screen with "... stopped. Nothing changed." No core API changed. Stop shows only for System Junk, Mail Downloads, Large & Old Files, Duplicates, Similar Images, Privacy Traces, Threat Check, Quick Sweep and Disk Map (which already had it).
- `jobs.rs`: `Job.started` and `elapsed_secs()` for the elapsed time, and `Job::waiting(status)`, a job that never finishes, for screenshots and Stop tests.

## Tests

- `tests/ux_render.rs`
  - `no_page_uses_a_glyph_inter_cannot_draw_and_nothing_leaves_the_window`: 28 scenes (idle, scanning, results, confirm, done for the plan flow and Quick Sweep, plus every other page and the banner) at 1280x820 and 1060x680. It fails on any character missing from Inter's own character map (egui's fallback fonts excluded), on visible text cut by the window edge or cut sideways by its clip rectangle, and on any sidebar entry below the window.
  - `module_icons_are_shapes_not_text`
  - `pages_render`: wgpu rendering (llvmpipe through EGL works headless here) with a check that the content is not a flat fill. With `CLEANCRAFT_SNAPSHOTS=1` it writes every scene as a PNG to `docs/screens/v1` (plus Quick Sweep at 2x).
- `tests/ux_flows.rs`: the System Junk and Quick Sweep flows at 1060x680 (every click inside the window, files moved and put back by Undo); Stop on a plan scan and on Quick Sweep; no Stop for Trash Bins.
- Every existing test passes unchanged; no visible label used by tests was renamed. Painted names in rows (where a checkbox or row button already carries the name) are painted, not labelled, so labels stay unique. `cargo test --workspace` (431 tests), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` are clean.

Regenerate the screenshots with:

```
CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_render pages_render -- --nocapture
```

## Shared-file changes

- `app.rs`: shell restyle (backdrop, sidebar, transitions, banners), `CleanApp::section()`, and private fields `shown_page`, `page_since`, `row_h`.
- `lib.rs`: `pub mod icons`.
- Pages owned by other builders were restyled without behaviour changes: `apps.rs`, `disk_map.rs`, `monitor.rs`, `startup.rs`, `tasks.rs`, `extras_cleanup.rs`, `history.rs`, `settings.rs`, `sweep.rs`, `plan_page.rs`.
- New public items: `plan_page::{can_stop, stop_scan, done_view, skipped_card, category_row}` and the `cancel` fields.
- No core, policy or executor change.

## To check by hand on a real Mac

- Motion smoothness at 60/120 Hz. The hero and progress screens repaint continuously while visible (set `theme::motion::REDUCE_MOTION` to stop the loops). It could be wired to the macOS "Reduce motion" setting later.
- How the glass and glow read on a Retina display, and in a window much larger than 1280x820.
- The headless renders use llvmpipe; Metal anti-aliasing may differ slightly.
