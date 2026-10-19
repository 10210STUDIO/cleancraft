# data (data-qa reviewer) report

Branch `agent/data`, from main with every module merged. I changed no app code. I added only:
- `crates/core/tests/review_data_numbers.rs`
- `crates/ui-egui/tests/review_data_screens.rs`

Each defect has a failing test marked `#[ignore = "DAT-n: ..."]`. I ran every one with `--ignored` and each fails for the reason given. Each expected value is worked out again, independently, from the fake disk (`tree_size`, `entry_size`, `fs::read_dir`) or from the sample tool output (`testkit_monitor`).

Run them with:
`cargo test -p cleancraft-core --test review_data_numbers -- --include-ignored`
`cargo test -p cleancraft-ui-egui --test review_data_screens -- --include-ignored`

## Defects

| ID | Sev | Row | Test | Steps | Expected | Actual |
|---|---|---|---|---|---|---|
| DAT-1 | S3 | all (fsutil::human_size) | core review_data_numbers::human_size_rolls_over_after_rounding | format 999,500 / 999,999 / 999,999,999 / 9,999,000 / 99,990,000 bytes | rounding moves to the next unit or precision band ("1.00 MB", "1.00 GB", "10.0 MB", "100 MB"); Finder shows 999,999 bytes as "1 MB" | "1000 KB", "1000 KB", "1000 MB", "10.00 MB", "100.0 MB" |
| DAT-2 | S3 | all, U5 | core review_data_numbers::singular_counts_and_sizes | human_size(1); Plan::summary of a 1-item plan (CLI scan/clean) | "1 byte", "1 item, 1 selected (5 KB)" | "1 bytes", "1 items, 1 selected (5 KB)" |
| DAT-3 | S1 | D1, D3 | core review_data_numbers::disk_map_freed_matches_the_map_for_hard_links | Disk Map: folder with a 400 KB file and a hard link to it, select the folder, remove | Done shows the same size the map and the confirmation showed (hard links once, D1) | map and confirmation 406 KB, Done "807 KB freed" (executor `tree_size` counts every link) |
| DAT-4 | S1 | D1 | core review_data_numbers::disk_map_refresh_counts_hard_links_once | Alpha/data.bin (400 KB) with a hard link in Beta; remove the folder Beta in the Disk Map | the patched tree equals a new scan | home shows 893 KB, a new scan 492 KB: `Tree::refresh` measures .Trash/Beta with a fresh Walker, so the link is counted a second time |
| DAT-5 | S2 | D2 | core review_data_numbers::disk_map_used_plus_free_is_the_disk | df sample (APFS Data volume) | used = total - free (Finder's Used, and the Live Monitor bar) | "410 GB used of 500 GB, 81.9 GB free": df's Used column counts only the Data volume, so the numbers do not add up and the bar differs from the Live Monitor's |
| DAT-6 | S2 | C1, A1 | core review_data_numbers::last_opened_uses_calendar_days, ::app_last_opened_uses_calendar_days | file / app last opened 1 minute before local midnight | "last opened yesterday" / "Yesterday" | "today" / "Today": days = elapsed seconds / 86400, not calendar days in the user's time zone |
| DAT-7 | S2 | R2, R6, U5 | core review_data_numbers::memory_total_reads_like_the_mac | Live Monitor sample with hw.memsize 17179869184 | "of 16 GB", as About This Mac and Activity Monitor show | "Memory: 8.36 GB of 17.2 GB used" (card, menu and CLI status). Swap "1.07 GB" for 1 GB (sysctl gives binary MB) |
| DAT-8 | S3 | R3, T1 | core review_data_numbers::monitor_trash_equals_trash_bins | the same ~/.Trash measured by Live Monitor and Trash Bins | the same number | 2,813,952 vs 2,809,856 bytes: the Monitor adds the Trash folder's own blocks (an empty Trash shows "4 KB" against "Zero KB" on ext4; APFS folder blocks to confirm) |
| DAT-9 | S3 | J21 and every plan page | ui review_data_screens::one_item_is_singular_on_every_step | System Junk with one cache folder: Scan, Review and Clean, Move to Trash | "found in 1 item", "Move 1 item", "1 item moved" | "1 items" on each step |
| DAT-10 | S2 | S2, every Done screen | ui review_data_screens::moving_to_the_trash_is_not_called_freed | clean one 1.2 MB cache to the Trash | the text says the items went to the Trash (space comes back when the Trash is emptied) | "1.20 MB freed", while ~/.Trash grew by exactly that size, so free space did not change |
| DAT-11 | S3 | R4 | ui review_data_screens::charging_estimate_is_time_until_full | pmset "87%; charging; 1:30 remaining" | a label such as "Time to full" | "Time left 1 h 30 min" |

## Checked and found right (passing tests)

Core `review_data_numbers`:
- `human_size_boundaries_that_are_right`: 0, 999, 1000, 999,499 bytes, 1 MB, 1 GB, and decimal units (1,024,000 bytes is "1.02 MB").
- `plan_totals_follow_selection_and_locks`: selected and total sizes, per-category sizes and counts; a locked item is never counted as selected, including after a category Select All.
- `system_junk_items_are_disk_sizes_and_never_nested`: each item's size is its `tree_size`, and no item sits inside another (Sparkle updates, Homebrew, Claude logs and crash reports are not counted twice).
- `freed_is_the_sum_of_really_moved_items_when_some_are_skipped`: with one item refused and one in use, `freed` is the sum of the moved items only, and the journal sizes match what is now in the Trash.
- `disk_map_sizes_match_a_recalculation_with_links`, `disk_map_refresh_after_removing_the_first_link`: the first scan counts a hard link once and does not follow symbolic links; removing the first link patches the tree to match a new scan.
- `old_files_need_the_full_number_of_days`: 365 days plus 1 hour is old, 365 days minus 1 hour is not; the size is the allocated size; nothing is preselected.
- `monitor_numbers_recalculated_from_the_samples`: memory app = anonymous - purgeable; used = app + wired + compressor; cached = file-backed + purgeable; binary swap; CPU = 100 - idle of the last `top` sample; uptime; battery health = AppleRawMaxCapacity / DesignCapacity, rounded; the top-level CycleCount (not the nested one); remaining minutes.
- `network_speed_divides_by_the_real_time_between_readings`: the counter difference is divided by the real 2.5 s between readings (not the nominal 2 s); loopback is left out.
- `sweep_tiles_and_text_match_each_module_plan`: the Quick Sweep tile equals the System Junk plan scanned directly and the sizes on disk; the CLI/MCP `describe` line uses the same numbers; the ready total leaves out the Trash and skipped modules.

UI `review_data_screens`:
- `done_and_history_sizes_match_the_disk`: the bottom bar count and size, the confirmation title and the Done "freed" figure all equal the recalculation from disk; each journal record's size equals what is stored in the Trash.

## Notes and things to check on a real Mac

- APFS: run `stat -f %b ~/.Trash` (folder blocks). If it prints 0, DAT-8 does not show on a Mac, but the two formulas still differ.
- `df -Pk ~` on an APFS Mac: check that Used + Available is less than 1024-blocks (DAT-5). Finder (Get Info on Macintosh HD) shows Used = Capacity - Available.
- Live Monitor top apps use `ps` RSS. Activity Monitor's "Memory" column is the physical footprint, so the numbers will differ. Compare `ps -axco pid=,rss=,comm=` with Activity Monitor for Safari.
- Network sums every non-loopback `<Link#>` row, including `utun*`. With a VPN on, the traffic is counted on both utun and en0. Compare against Activity Monitor > Network with a VPN connected.
- `softwareupdate --list` prints "Size: NNNNKiB". The parser multiplies by 1024. Check against the size shown in System Settings > Software Update, to see whether the number is really KiB or KB.
- Battery health: on Apple silicon, compare `ioreg -rn AppleSmartBattery` (AppleRawMaxCapacity, NominalChargeCapacity, DesignCapacity) with System Settings > Battery > Battery Health "Maximum Capacity". macOS may use NominalChargeCapacity.
- Hard links that live outside the removed folder: removing one link frees nothing, but Done still reports its full size. This has the same cause as DAT-3, so DAT-3's fix should count a file as freed only when its last link goes.
- Observation, not filed: plan headlines ("X found") include locked, report-only items (J6, J7, J10, J20, admin-locked caches), so System Junk's headline can be much bigger than anything any button can clean.

No shared files changed.
