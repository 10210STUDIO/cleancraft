# icons, round 2: real app icons, file-type icons, "Opened: Unknown"

Branch `agent2/icons`. Renders: `docs/screens/v2/icons-app-remover@1x.png`, `@2x.png`, `icons-file-kinds@1x.png`, `@2x.png` (`CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test icons_ui icons_render`).

## What is built

### Core: `crates/core/src/app_icons.rs` (new)
- `icon_source(app)`: reads Info.plist. `CFBundleIconFile` ("AppIcon" or "AppIcon.icns") in `Contents/Resources` gives `Icns`; else `CFBundleIconName` or `Assets.car` gives `AssetCatalog`; else `Resources/AppIcon.icns` if present; else `Unknown`.
- `app_icon_png(env, app, px) -> Result<(png, Method)>`, blocking, sizes rounded up to 64 or 128 px:
  1. `.icns`: `/usr/bin/sips -s format png -Z <px> <icns> --out <png>`
  2. else or on failure: `/usr/bin/qlmanage -t -s <px> -o ~/Library/Caches/CleanCraft/icons/ql <app>`, then `<App>.app.png` is renamed to the cache name (one qlmanage at a time; a stale file from an earlier run is moved aside first, so it can never pass for a new one)
  3. else: `/usr/bin/osascript -l JavaScript -e WORKSPACE_SCRIPT <app> <png> <px>` (NSWorkspace iconForFile drawn into a bitmap, written as PNG; must print "ok")
  4. else the placeholder stays.
  Every tool goes through `env.sys.run_timeout` with a 20 s limit; success means the output file starts with the PNG signature (qlmanage exits 0 even when it writes nothing).
- Cache: `~/Library/Caches/CleanCraft/icons/<blake3(path, bundle mtime+len, Info.plist mtime+len, px)>.png`. An app update rewrites Info.plist (or replaces the bundle), so its key changes and the icon is made again. When every tool ran and failed, an empty `<key>.none` marker stops retries for that version; a missing tool or a time out leaves no marker (tried again next launch). A half-written PNG counts as a miss.
- File previews: `preview_png(env, file, px)`: PNG/JPEG/GIF/BMP/TIFF/WebP decoded and scaled in process (no tool), HEIC/HEIF/RAW through `sips` then Quick Look, MOV/MP4/M4V/PDF through Quick Look; at most 64 MB files; cached as `p-<key>.png`.
- `load_png` decodes for the UI (at most 256 px).

### UI: `crates/ui-egui/src/images.rs` and `file_icons.rs` (new)
- `images::app_icon(ui, app_path, size_pt) -> Response`, `images::paint_app_icon(ui, rect, app_path) -> Shown`
- `images::file_icon(ui, path, kind, size_pt) -> Response`, `images::paint_file_icon(ui, rect, path, kind) -> Shown`
- `images::dialog_head_app(ui, t, app_path, headline, sub)`: `kit::dialog_head` with the app icon.
- `file_icons::FileKind::of(path)` (Folder, Document, Archive, Image, Video, Audio, DiskImage, Package, Log, Cache, Plist, App, Other; folders inside Caches/Logs are caches/logs) and `file_icons::paint(painter, rect, kind)`: drawn vector icons, own design.
- Loader in the egui context: `Mode::Threads(2)` in the app (two workers, so at most two icon tools at once), newest request first, queue capped at 96 (stale requests from a fast scroll are dropped and asked again when visible), textures capped at 768 (least recently painted released). Only painted rows ask (`show_rows` lists and `is_rect_visible`). The frame never runs a tool. Placeholder = `kit::initial_tile` for apps, the drawn kind for files; the image fades in over 0.18 s with a slight grow (cross-fade with the placeholder); an image ready in the same frame (cache, sync) shows without fade.
- `images::ensure(ctx, env, sync)` (one line in `app.rs::logic`) installs the loader only on a real Mac: UI tests and fake Macs get none unless a test calls `images::install(ctx, env, Mode::Sync | Manual)`, so the older tests run no icon tools and write no cache files.

### Pages (one-call edits, announced on the board to design)
- `pages/apps.rs`: list tile, detail tile, App Updates row tile -> `images::paint_app_icon`; Uninstall/Reset confirmation head -> `images::dialog_head_app`; detail file rows -> `images::paint_file_icon` (20 pt, was a 16 pt mono icon); "Opened" text -> `apps::opened_text`.
- `pages/startup.rs`: login items and heavy apps show the app's icon (macOS path resolved with `apps::resolve`); launch agents/daemons get a 28 pt icon of the app that contains the program (`sys::app_bundle_of`), else a grey gear tile.
- Not done (design's call): `plan_page.rs` item rows still use `kit::kind_icon`; swap to `images::paint_file_icon(ui, rect, &item.path, FileKind::of(&item.path))`.

### "Opened: Unknown" for iMovie, GarageBand, Live Home 3D
Cause: not a CleanCraft bug as far as can be told from here. `mdls -name kMDItemLastUsedDate -raw` prints `(null)` when Launch Services never recorded the app being opened: typical for preinstalled or App Store apps the owner has never launched, and for apps only started by other means. "Unknown" was just an unhelpful word. Now (core `apps.rs`, additive):
1. `kMDItemLastUsedDate`; when null, `mdls -name kMDItemLastUsedDate_Ranking -raw` (Spotlight's day-rounded copy): shown as "Opened: X".
2. Still nothing: `AppInfo::last_active` = newest modification time of the app's own files: `~/Library/Preferences/<id>.plist`, `~/Library/Saved Application State/<id>.savedState`, and, only with Full Disk Access (else macOS would show an "access data from other apps" prompt), the same two inside `~/Library/Containers/<id>/Data/Library`. Shown as "Last active: X (app data)". Never used for the Unused group.
3. Neither: "Opened: No record".
`kMDItemUseCount` was considered and not used: it does not give a date. The second `mdls` runs only for apps whose first answer is null.

## Tests
- `crates/core/tests/app_icons.rs` (15): Info.plist variants; sips path + cache hit (one sips) + 64/128 buckets; Info.plist or bundle mtime change invalidates; asset-catalog app via qlmanage and rename; fallback order sips -> qlmanage -> osascript; stale Quick Look file never reused; all-fail marker, no retry, retried after update; missing tool or time out leaves no marker, per-tool time limit; corrupt cache file remade; removed app runs nothing; in-process image previews (aspect kept, broken image, log, folder); HEIC via sips, MOV via Quick Look; Ranking date; app-data fallback with and without Full Disk Access, labels; non-sandboxed preferences; odd bundle ids build no paths.
- `crates/ui-egui/tests/icons_ui.rs` (10): App Remover placeholder before (rendered pixel is not the icon colour, requests queued, no tool ran in a frame) and the real icon after (rendered pixel is the icon colour, 2 textures), next launch from cache with no tool; failed icon keeps the placeholder and is not retried per frame; detail, Uninstall confirmation and "Opened: 2 days ago" / "Opened: No record"; App Updates rows; Startup Items login item and agent owner; file previews vs drawn kinds; 300 rows keep the queue at or under 96; real background threads load without blocking; no loader means no tools; renders at 1x and 2x.
- Unit tests in `file_icons.rs` (kinds by name and by place) and `app_icons.rs` (buckets, preview methods).
- FakeSys gained `answer_creating(key, stdout, file, bytes)`: a scripted tool writes its output file (inside the fake root only).

## Shared-file changes
- `crates/core/src/lib.rs`: `pub mod app_icons; pub mod testkit_icons;`
- `crates/core/src/sys.rs`: `FakeState::creates` + `FakeSys::answer_creating` (additive, test only behaviour).
- `crates/core/src/apps.rs`: `AppInfo::last_active` (`#[serde(default)]`), `spotlight_date`, `app_data_paths`, `last_active_from_data`, `opened_text`; `inventory` asks the Ranking date and the app data only when the first date is null. `last_used_text` unchanged (CLI and MCP still use it).
- `crates/ui-egui/src/lib.rs`: `pub mod file_icons; pub mod images;`
- `crates/ui-egui/src/app.rs`: one line in `logic()`.
- `crates/ui-egui/src/pages/apps.rs`, `pages/startup.rs`: as listed above.

## MAC-CHECK (hand checks on the owner's Mac)
Run in Terminal, then look at the app:
1. sips on an icns: `sips -s format png -Z 128 /Applications/Safari.app/Contents/Resources/AppIcon.icns --out /tmp/safari.png && open /tmp/safari.png`: a 128 px Safari icon. (Some apps name another file: `defaults read /Applications/Safari.app/Contents/Info CFBundleIconFile`.)
2. Quick Look on an app: `mkdir -p /tmp/ql && qlmanage -t -s 128 -o /tmp/ql /System/Applications/Calculator.app; ls /tmp/ql; open /tmp/ql/Calculator.app.png`: expect `Calculator.app.png` with the app icon (not a generic page). MAC-CHECK: the output file name and that the icon has no document decoration.
3. AppKit fallback: copy `WORKSPACE_SCRIPT` from `crates/core/src/app_icons.rs` into `/tmp/icon.js`, then `osascript -l JavaScript /tmp/icon.js /Applications/Safari.app /tmp/ws.png 128` prints `ok` and `/tmp/ws.png` is the icon. MAC-CHECK: the bitmap init call with `null` and the PNG constant names in JXA; if it errors, sips/Quick Look still cover nearly all apps.
4. In CleanCraft: App Remover shows real icons for every app within a second or two of opening (placeholder tiles first, then fade in), Retina sharp (128 px used for the 38 pt rows). Second launch: icons appear at once (`ls ~/Library/Caches/CleanCraft/icons | head`). Apps that only have Assets.car (newer Apple apps, Tahoe "Icon Composer" apps) also show icons.
5. App Updates rows, the app detail page, the Uninstall confirmation, Startup Items (login items, agents of apps such as Docker or Dropbox) show icons.
6. Scroll a long list quickly (App Remover with 130+ apps): no stutter; Activity Monitor shows at most two `sips`/`qlmanage` at a time.
7. Opened column: for iMovie/GarageBand run `mdls -name kMDItemLastUsedDate -name kMDItemLastUsedDate_Ranking -name kMDItemUseCount /Applications/iMovie.app` and compare with the row: dates shown as "Opened", no date with existing `~/Library/Containers/com.apple.iMovieApp/Data/Library/Preferences/com.apple.iMovieApp.plist` (needs Full Disk Access) shows "Last active: ... (app data)", else "Opened: No record". Open iMovie once, quit, Look Again: it should switch to "Opened: Today". MAC-CHECK: whether Ranking is ever set when the exact date is null (if never, step 1 just costs one extra mdls per such app).
8. Without Full Disk Access, the App Remover scan must not trigger any "CleanCraft would like to access data from other apps" prompt.

## Known limits
- Cache files of older app versions stay in `~/Library/Caches/CleanCraft/icons` (small; System Junk may list it as a cache, which is fine: it is rebuilt on demand).
- Mipmapped texture filtering only works on the glow backend; on wgpu, 128 px icons drawn at 76 px on Retina use linear filtering (looked clean in the 2x render).
- `FileKind::App` rows in `paint_file_icon` show the app icon pipeline (placeholder initial letter until loaded).
