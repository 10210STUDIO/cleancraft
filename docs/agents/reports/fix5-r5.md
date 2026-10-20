# fix5 round 5 report

Branch `agent5/fix5` (from main d50de53 with REVIEW-C round D and REVIEW-U final verification
merged). Owner of `crates/ui-egui/src`; core changed only where a fix needed it, each with
FakeMac tests. Nothing is ever deleted: the new quarantine action renames into the Trash and is
journaled with Undo. The Privacy cookies default (core `COOKIES.selected`) is untouched, waiting
for the user's decision.

Evidence:
- `crates/ui-egui/tests/fix5_review.rs`: 27 tests, one or more per item, each failing before its
  fix (run `cargo test -p cleancraft-ui-egui --test fix5_review`).
- `crates/core/tests/quarantine_trash.rs` (6 tests), `sweep_run::scan_one_brings_a_single_tile_up_to_date`,
  `safety::a_file_open_in_an_app_names_the_app`, `sys::tests::lsof_lines_name_the_holder`,
  `section_icons::tests::sidebar_icons_weigh_alike_and_sit_centred` (unit).
- `review_usability::u05_quarantine_can_be_seen_and_emptied`: `#[ignore]` removed, passes.
- Renders: `docs/screens/v2/*` (35 scenes) and `docs/screens/v2/ux4/*` regenerated at
  1060 x 680, 1280 x 820, 1600 x 1000, 1x and 2x, and looked at.

## Status

| Item | Status | Evidence |
|---|---|---|
| D1 S1 Trash Bins bar overlap at 1060 | fixed | The bar shows what the Trash holds ("3.38 MB in the Trash"), never "0 items selected"; the hint gets only the room left and ends in an ellipsis with a tooltip. `d1_trash_bins_bar_has_no_overlap_at_the_smallest_window` (no two bar texts intersect at 1060 x 680); render `trash-bins-results-1060x680` |
| N2 S2 Quick Sweep Trash tile stale | fixed | Core `sweep::scan_one` scans one tile again; `files_moved` asks for the Trash tile (and marks the Trash Bins page stale); Empty Trash refreshes it too. `n2_sweep_trash_tile_follows_moves_into_the_trash` |
| U4 S2 no space freed (Quick Sweep part) | fixed | Quick Sweep Done has "Empty Trash..." first, the note says where the space comes back by method; the Trash tile reads "Waiting in the Trash. Empty it to get the space back." with "Empty Trash..." in place of "Emptied from its own page"; Live Monitor's Storage card has "Empty Trash..." beside Free Up Space. `u04_quick_sweep_done_offers_empty_trash`, `u22_live_monitor_offers_empty_trash`, usability `u04` still passes; renders `quick-sweep-results`, `quick-sweep-done`, `live-monitor` |
| D2 S2 hovered Privacy row splits the tag | fixed | While a row's actions show (hover or keyboard), its tag ends before them. `d2_hovered_privacy_row_keeps_its_tag_before_the_buttons`; render `privacy-traces-results-1060x680` |
| C17 S2 Privacy tags cut | fixed | A tag that does not fit drops its asides in brackets first ("Google Chrome browsing history and download list"), the tooltip keeps every word; tags at 1060 are whole. Same test |
| U5 S2 quarantine cannot be emptied | fixed | Core `executor::move_quarantine_to_trash` (rename into the Trash, one journal run `quarantine-to-trash-<time>`, the cleanup records follow the item so their own Undo still works, Undo All puts the run back; nothing deleted; a full-disk journal moves nothing), `quarantine_items`, `quarantine_size`. Settings: size line, "Show Quarantine in Finder", "Move Quarantine to Trash..." with a confirmation, a notice with Undo; History names the run "Quarantine moved to the Trash". Core `quarantine_trash.rs` (6), `u05` un-ignored (assertion label changed to the lead's name "Move Quarantine to Trash", and it now also checks the move and its Undo), `u05_quarantine_to_trash_is_listed_in_history_with_undo_all` |
| U9 S2 app holding a file not named | fixed | `Sys::open_files_by` (lsof `+c 0 -F pcn`, helpers named after their app), `SkipReason::InUseBy(app)`: "open in Safari", next step "Quit Safari, then Try Again." `u09_skipped_item_names_the_app_holding_it`, core `a_file_open_in_an_app_names_the_app`, `lsof_lines_name_the_holder` |
| C10 S2 first results frame | fixed | Rows fade in from 30 % (present on frame 1); bottom bars get their height from the first frame (no 38 pt jump). `c10_first_results_frame_has_rows_and_the_bar_in_place`; `review_c10` passes |
| C14 S2 monospace commands | fixed | JetBrains Mono Regular, subset to Latin (16 KB), `assets/fonts/JetBrainsMono-Regular.ttf` with `assets/fonts/OFL-JetBrainsMono.txt`; `theme::mono`; Tune-up cards and the confirmation. `c14_commands_are_monospace`; render `ux4/tune-up-tasks` |
| C15 S2 chips per phase | fixed | The app uses one module set in every phase; the extra chip was the scanning render scene using other sources (scene fixed). `c15_quick_sweep_lists_the_same_modules_in_every_phase` |
| C19 S2 one bottom bar | fixed | `kit::bar_figure` (24 pt figure, 13 pt count) and `kit::bar_hint` on plan pages, App Remover files and detail, Disk Map. `c19_c20_app_remover_bar_and_headline`; renders `app-remover-detail`, `disk-map` |
| C20 S2 App Remover list headline | fixed | "9.58 MB in 4 apps" with Scan Again, like results pages. Same test; render `app-remover` |
| C21 S2 Tab order | fixed | The results bar is laid out after the toolbar and lists (its place kept); a category with a checkbox is one Tab stop (Space ticks, Return shows it). `c21_tab_follows_reading_order_with_one_stop_per_category` |
| History per-file Undo | fixed | With Undo All on a card, the items wait behind "Show N items" (announced "Show N items of <action>"). `history_hides_per_file_undo_behind_a_disclosure`; render `history` |
| N1 S3 cleaned tile shows 0 KB with Include | fixed | "All clean / Cleaned just now." with "Scan Again" for that tile only, no Include; a fresh empty module "Nothing to clean right now." `n1_cleaned_tile_says_so_and_offers_a_scan` |
| N3 S3 Ignore notice without Undo | fixed | Notices can carry an Undo (`CleanApp::tell_undo`, announced "Undo Ignore"). `n3_ignore_notice_has_undo` |
| N4 S3 banned word | fixed | a whole-word, case-blind search of crates, apps and scripts for the banned conjunction finds nothing (two comments reworded; the checks build the word with `concat!`) |
| D3 S3 App Remover Done order | fixed (verified) | Already on d50de53 through `done_view`; the review crop is blank. `d3_app_remover_done_buttons_in_the_module_order` proves Empty Trash..., Undo, Show History, Done |
| D4 S3 raw tmutil text | fixed | `tasks::plain_reason`: first clause as a sentence, raw text in the tooltip. `d4_task_errors_are_plain_sentences` |
| D5 S3 History empty twice | fixed | One `kit::empty_state` "No actions yet" plus the help line. `d5_empty_history_says_it_once`; render `history-empty` |
| D6 S3 row names cut at 1060 | fixed | Names stay whole while the path keeps a 64 pt stub; display names end with an ellipsis, file names in the middle. `d6_trash_row_names_are_whole_at_1060` |
| C23 S3 start screens at 1600 | partly | The object is about 380 pt at 1600 x 1000 (design4's sizing); the block is about 45 % of the height, under the 55 % target. Render `quick-sweep-idle-1600x1000` |
| C24 S3 sidebar icon weights | fixed | Per-icon optical fit (scale and shift): ink between 157 and 194 pt² (1.24 x, was 1.63 x by this measure), painted box centred within 0.25 pt. `section_icons::tests::sidebar_icons_weigh_alike_and_sit_centred` and design4's `ux_design4::section_icons_have_even_weight_and_sit_centred` both pass |
| C25 S3 Live Monitor columns | fixed | The last card of each column grows so the three end level. Render `live-monitor-1280x820` (all three end at 717); the Storage buttons wrap in the narrow 1060 column |
| C27 S3 Remove always on agent rows | fixed | Remove shows on row hover or keyboard focus (always present for VoiceOver), tooltip "Move its file to the Trash (asks first). Turn Off keeps it." `u21_c27_startup_rows` |
| C28 S3 Disk Map glyphs | fixed | 32 pt kind tiles like the result lists. Render `disk-map` |
| C29 S3 Disk Map field and ghost | fixed | Text fields have pill ends like the search field; Map Folder is a live secondary button that says what to do when the field is empty. `c29_map_folder_is_live_and_explains_an_empty_field` |
| C30 S3 empty states | fixed | One `kit::empty_state` (App Remover, Disk Map folder, History), headlines without a full stop ("Nothing to clean here"). `c30_empty_states_have_no_full_stop` |
| C33 S3 VoiceOver roles | fixed | Switches announce as `Role::Switch`; result checkboxes carry "5.01 MB, Derived data" as description. `c33_switch_role_and_row_descriptions` |
| C35 S3 scroll bars at rest | partly | Dormant opacity was already 0 (the renders park the pointer over the list); now hovering a list shows only a faint handle with no track. Bars do not appear on scroll alone (egui gives no scroll-only state) |
| C36 S3 App Updates card heights | fixed | Both cards take the taller one's height. Render `app-updates-results-1060x680` |
| C37 S3 "Measuring ..." | fixed | "Measuring..." when no folder name. `c37_disk_map_measuring_has_no_space_before_the_dots` |
| C38 S3 Large & Old start screen | partly | "Where to look" is one row with a pop-up, so the object shows at 1060 x 680 (about 70 pt, under the 96 pt target) and at 1280 x 820 at full size. Renders `large-old-files-idle-*` |
| C39 S3 Duplicates headline | fixed | "606 KB in 2 duplicate copies" (kept copy excluded). `c39_duplicates_headline_counts_the_copies_that_can_go` |
| U15 S3 action names | fixed | App Remover "Reset App..." and "Hide from List"; App Updates "From the developer" (core label) beside the existing "Download Only". `c19_c20_app_remover_bar_and_headline` |
| U21 S3 startup rows | fixed | Owner app name leads (since round 4); "Not signed by a known developer" (core label); Remove as in C27. `u21_c27_startup_rows` |
| U22 S3 Live Monitor duplicates | partly | "Empty Trash..." added on Storage (U4). "Free Up RAM" keeps its own dialog (its tooltip now says it is the Tune-up task); merging the two flows would rewrite four qa_monitor tests and is left open |
| U24 S3 Disk Map links | fixed | The Library row reads "Use System Junk or App Remover" with two links. `u24_disk_map_library_row_links_to_the_tools`; render `disk-map-1060x680` |
| U25 S3 Disk Map "0 skipped" | fixed | `kit::moved_summary` on Disk Map Done (source) |
| U26 S3 Settings wording | fixed | "AI assistants" group with a switch "Let AI assistants clean after you confirm (MCP)"; the quarantine path is a dim line under its choice. `u26_settings_assistants_switch_and_quarantine_line` |
| U20 cookies default | not touched | Waiting for the user's decision (as asked) |

Totals: fixed 36, partly 4 (C23, C35, C38, U22), open 0, one left by request (U20 cookies).

## Changes to existing tests (no assertion weakened)

- Labels renamed by these fixes: "Reset" to "Reset App..." (qa_apps_remover, review_usability
  task_08), app "Ignore" to "Hide from List" (fix2_app_ignore), "Ad hoc signature (no developer
  identity)" to "Not signed by a known developer" (qa_perf_startup), the Settings agents
  checkbox and quarantine radio labels (qa_shell), "Nothing to clean here." without the full
  stop (ux_design3, qa_cleanup_trash).
- Flows: History tests open the action's disclosure before per-file Undo (qa_shell,
  review_reliability_ui, review_usability u14, ux4_flows); Large & Old tests open the "Where to
  look" pop-up (qa_files_large_old); qa_shell scrolls to the taller Settings switch.
- qa_files_duplicates "total": the headline now shows what can be freed (C39 asks for it).
- Role lists: `Role::Switch` added beside `Role::CheckBox` where tests collect clickable
  controls (qa_every_control, ux_render, review_design_c), and ux_design3 looks for switches.
- review_usability u05: label "Move Quarantine to Trash" (the lead's name), plus new checks.

## Final gate (commit 6feb371 plus this report line)

`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `cargo test --workspace`: 655 passed, 0 failed, 18 ignored (the usability walkthroughs
`task_*`, ignored by design). target/ deleted afterwards.
