# files: report (C1-C4, D1-D3)

Branch `agent/files`. Everything below was run in this worktree: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` pass (final run in a private target dir, see "Build note").

## What works, with tests

Core tests are in `crates/core/tests/files_*.rs`, UI tests (egui_kittest, clicks and typing on a FakeMac) in `crates/ui-egui/tests/qa_files_*.rs` and `qa_disk_map.rs`.

### C1 Large & Old Files (`features/large_old.rs`): PARTIAL
- Walks the chosen location; leaves out `~/Library`, Documents, Desktop, `~/Applications`, hidden items (so the Trash), iCloud and cloud folders, Photos, Music, TV, Photo Booth libraries, app bundles; document packages (`.pages`, `.sparsebundle`...) are one item. Large = `settings.large_min_bytes` or more on disk; old = not opened for `settings.old_days` days, for files of 1 MB or more. Categories by kind; nothing preselected.
- Last opened = later of mtime and (Spotlight `kMDItemLastUsedDate`, asked with one `mdls -raw` call per 100 files, else atime).
- Location picker in the idle page: Home folder, Downloads, Movies, Music, Pictures, Downloads by source app, Another folder (typed path; Enter applies, Esc cancels; protected places, Library, outside home, missing, files and links refused with a message). The choice is remembered in `<data>/files.json` (`ScanOptions`), which the scan reads.
- Tests: `qa_files_large_old::{downloads_scan_select_clean_undo, typed_folder_checked_with_enter_and_esc, permission_denied_and_file_in_use}`, `files_large_old::{thresholds_kinds_and_exclusions, locations_limit_the_walk, typed_folders_are_checked, spotlight_last_used_date_wins_over_access_time, chosen_items_move_and_come_back}`.
- PARTIAL because: no size or age groups (an item has one category, used for kind). Size and age are in each item's reason; sort by size and search ("Not opened") work.

### C2 Duplicates (`features/duplicates.rs`): WORKS
- Same areas as C1 with its own remembered location. Length > 100 KB, then BLAKE3 of the first 64 KiB, then full BLAKE3. Hard links (same dev+inode) count once. Each set has a `group`; a set stays in one category (kind of its first file).
- Smart keep: shortest path outside Downloads, then oldest modified. Select Copies menu: Keep Suggested, Keep Newest, Keep Oldest (Similar adds Keep Largest File). The plan page's Select None clears.
- Safety: the item checkbox refuses to select the last kept copy (notice shown); Select All and category checkboxes put the suggested copy back (notice); `Feature::prepare` (`check_keeps`) refuses any run where a set has no unselected copy that still exists, nothing moves.
- Tests: `qa_files_duplicates::{duplicate_sets_keep_one_copy, every_copy_selected_is_refused_by_core}`, `files_duplicates::{exact_sets, smart_keeps_one_per_set, keep_newest_and_oldest, every_copy_selected_is_refused, smart_selection_moves_only_extra_copies, moved_files_are_matched_by_content_only}`. I checked the UI test fails when the guard is disabled.
- Not done: APFS clones are not detected (they look like copies; removing one frees little).

### C3 Similar Images (`features/similar.rs`): WORKS (HEIC is MAC-CHECK)
- Pictures > 100 KB in `~/Pictures` and `~/Downloads` (libraries never opened). Decoded with `image` (JPEG, PNG, GIF, BMP, TIFF, WebP); HEIC/HEIF via `sips -s format png -Z 256 <file> --out <data>/Cache/Similar/<hash>.png` (cached, reused while newer than the picture). 64-bit difference hash on 9x8 grey; sets = Hamming distance <= 10, transitive. Keep = most pixels (header read; HEIC via `sips -g pixelWidth -g pixelHeight`), then biggest file. Reason shows the size in pixels.
- Tests: `qa_files_duplicates::similar_pictures_keep_the_biggest`, `files_similar::{hashes_tell_alike_from_different, groups_keep_most_pixels, heic_goes_through_sips}` (generated scenes with small changes in size, brightness, noise and format; a different scene stays out).
- No thumbnails in rows.

### C4 Downloads by source (`features/downloads.rs`): MAC-CHECK
- A Large & Old Files location (no new FeatureId): every top-level item of `~/Downloads`, one category per app from `xattr -p com.apple.quarantine` (third field), "No source recorded" otherwise. Nothing preselected. `kMDItemWhereFroms` is not used.
- Tests: `qa_files_large_old::downloads_by_source_app`, `files_large_old::downloads_grouped_by_source_app`, unit `downloads::tests::quarantine_values`.

### D1-D3 Disk Map (`space.rs`, `features/space_lens.rs`, `pages/disk_map.rs`)
- D1 PARTIAL: maps the home folder or a typed folder inside it (protected folders may be viewed, items stay locked). Size tree: shared size formula, hard links once, symlinks not followed, does not cross into another device (`st_dev`). Measuring runs in a background Job with a file counter and Stop. Squarified treemap (40 biggest blocks plus "Other items"), list sorted by size, drill down by block or folder name, Back, breadcrumbs, Map Again, Other Folder. Trees are kept per mapped folder (a folder inside a mapped one opens without a scan); after a removal or undo only the changed folders and CleanCraft's data folder are measured again (`Tree::refresh_after`). PARTIAL because the whole disk and other volumes are not offered, and the mount-point stop is not covered by a test.
- D2 WORKS: used, total, free from `df -Pk` (free only if df fails), selected size in text and on the bar.
- D3 WORKS: checkboxes; locked: protected places, `~/Library`, hidden items and the Trash, apps, anything `policy::check` refuses. Review and Remove opens a dialog with the exact list; removal by `features::run_plan` with feature `space_lens` (allow-list = home folder; `prepare` refuses locked items); nested selections collapse to the outer folder. Done screen with freed size, skipped items with reasons, Undo, Show History.
- Tests: `qa_disk_map::{map_drill_select_remove_undo, cancel_keeps_everything, typed_folder_and_errors}`, `files_space::{sizes_match_a_recalculation, hard_links_once_and_links_not_followed, refresh_after_move_and_back, locked_places, removal_plan_refuses_locked_items, disk_use_from_df}`, unit `space::tests::{treemap_areas_add_up, df_columns, outermost_drops_nested}`.

## Hand checks on a real Mac

1. Spotlight last used, batch form: `mdls -raw -name kMDItemLastUsedDate ~/Downloads/a.dmg ~/Downloads/b.zip | od -c | head`. Expected: two values separated by `\0`, each `YYYY-MM-DD HH:MM:SS +0000` or `(null)`. If the separator or count differs, CleanCraft silently falls back to atime/mtime (old files may then be missed or extra).
2. Quarantine source: `xattr -p com.apple.quarantine ~/Downloads/<something downloaded with Safari>`. Expected `0083;<hex>;Safari;<UUID>`; a file made locally exits 1 with "No such xattr". Then Large & Old Files > "Downloads by source app" > Scan shows "Saved by Safari".
3. HEIC: `sips -s format png -Z 256 ~/Pictures/IMG_1234.HEIC --out /tmp/t.png && sips -g pixelWidth -g pixelHeight ~/Pictures/IMG_1234.HEIC`. Expected a PNG of at most 256 px and lines `  pixelWidth: 4032` / `  pixelHeight: 3024`. Similar Images should group two HEIC shots of the same scene.
4. Storage bar: `df -Pk ~` second line columns 2-4 (1024-blocks, Used, Available). Note: on APFS "Used" is the volume's own use, not the container's, so Used + Free may be less than the total.
5. Mount points: `hdiutil attach some.dmg -mountpoint ~/MountTest`, then Disk Map > Map Home Folder: MountTest shows 0 bytes and the note "folders on other disks are shown empty".
6. Privacy prompts: the first scan of Downloads, Pictures, Movies may trigger macOS "Files and Folders" prompts; if denied, scans list the folder as unreadable (Large & Old: "folders could not be read"; Disk Map: count of unreadable folders).
7. Atime: `stat -f "%Sa %Sm" file` after opening a file in Preview: atime should change; if the volume does not update atime, "old" relies on Spotlight (check 1).

## Shared-file changes (all small, announced on the board)

- `crates/core/src/lib.rs`: `pub mod space;`, `pub mod testkit_files;` (my test helper for generated pictures, re-exports `image::ImageFormat`).
- `crates/core/src/features/mod.rs`: `pub mod downloads; duplicates; large_old; similar; space_lens;` and registry lines LargeOldFiles, Duplicates, SimilarImages, SpaceLens.
- `crates/ui-egui/src/pages/plan_page.rs` (I own the group display): `Row` and `rows()` (set headers when items have `group`), `group_row()`, `keep_a_copy()` after Select All and category select, keep-one-copy guard on the item checkbox. Confirm dialog untouched. `qa_system_junk` stays green.
- `crates/core/src/policy.rs`: test-only clippy fix (`std::slice::from_ref`, lines 195/198), identical to monitor's.
- No `Cargo.toml` change (`blake3` and `image` were already core dependencies).

Files CleanCraft now writes in its own data folder: `files.json` (chosen locations) and `Cache/Similar/*.png` (HEIC thumbnails, about 100 KB each, never removed by CleanCraft).

## Known limits

- The plan page has no Stop button, so Large & Old, Duplicates and Similar scans cannot be cancelled from the UI (the scans honour `ScanCtx::cancel`). Disk Map has Stop.
- Similar Images compares every pair (fine for thousands of pictures, slow for hundreds of thousands).
- Disk Map keeps the whole tree in memory.

## Build note

Workspace crates built in the shared `CARGO_TARGET_DIR` can be overwritten by another worktree's build (I saw ui-egui doctests compile against another branch's `cleancraft_core`). The final full test and clippy run used `CARGO_TARGET_DIR=/home/claude/wt/files/target` (ignored by git through the worktree's info/exclude).
