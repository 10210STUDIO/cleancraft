# icons3, round 3: developer names, "Part of macOS", neutral loading tile

Branch `agent3/icons3` (base ec752a6). Items: REVIEW-A 45 (vendor and "Part of macOS"), REVIEW-A 44 (placeholder).

## 1. Real developer names (REVIEW-A 45, vendor part): fixed in tests, MAC-CHECK below

Core `crates/core/src/apps.rs`:
- `AppInfo::vendor` = `developer_name(bundle, signer, part_of_macos)`, best source first:
  1. part of macOS, or signature "Software Signing": "Apple";
  2. `codesign -dv --verbose=2` first `Authority=` line: "Developer ID Application: NAME (TEAMID)" (also Apple Distribution, 3rd Party Mac Developer Application, Apple Development, Mac Developer) gives NAME, team id dropped, e-mail names refused;
  3. "Apple Mac OS Application Signing" (App Store: Apple signs every app) or no signature: `NSHumanReadableCopyright` holder (`copyright_holder`: drops the copyright sign, "(c)", "Copyright", years and ranges incl. en dash, "All rights reserved", leading "by", keeps the first sentence, keeps "Inc." / "Ltd." / "s.r.o.", refuses license or trademark lines and anything over 60 chars);
  4. the copyright part of `CFBundleGetInfoString`;
  5. last resort `vendor_of(bundle_id)`: a known maker table (JetBrains, OpenAI, FreeCAD, Blender Foundation, BeLight Software, Google, Mozilla, ...), a mixed-case part as written ("SweetScape"), or a plain word of 4+ letters capitalised ("Example"); fragments ("co", "io", "app", 3 letters or fewer) give "Unknown developer".
- `signers(env, paths)`: codesign through `env.sys.run_timeout` with `CODESIGN_LIMIT` 10 s, up to `CODESIGN_THREADS` 8 at once (`std::thread::scope`, results kept in order). Asked only in the deep listing and only for apps not already known to be part of macOS. The quick listing (leftovers, CLI) runs no command and uses the Info.plist sources.
- No cache: there was no existing inventory cache, so none was added (codesign -d only reads the signature; 130 apps over 8 threads is well under a second on a Mac).
- `vendor_segment(bundle_id)`: the lower-case maker part, used by the uninstaller for the maker's shared folder, so leftover matching is unchanged now that the shown name is "JetBrains s.r.o.".

Tests (`crates/core/tests/apps_developer.rs`, all 6 fail on ec752a6 and pass now):
- `developer_names_come_from_the_signature_then_the_copyright`: the six apps of the review: GoLand "JetBrains s.r.o.", Screaming Frog "Screaming Frog Ltd" (bundle id "co.uk..." used to give "Co"), FreeCAD ad hoc signed -> copyright holder, Blender Developer ID, ChatGPT (App Store) -> "OpenAI" from the copyright, Live Home 3D (App Store) -> "BeLight Software, Ltd." from CFBundleGetInfoString, a bare app -> "Unknown developer"; none of "Co", "Jetbrains", "Freecad", "Openai", "Blenderfoundation", "Belightsoft"; every codesign had the 10 s limit.
- `quick_listing_reads_the_copyright_without_commands`, `a_slow_codesign_falls_back_to_the_copyright` (FakeSys time out), `a_long_list_keeps_each_signature_with_its_app` (120 apps, each its own name).
- Unit tests in apps.rs: `vendors`, `signers_from_codesign`, `copyright_holders`, `developer_name_order`. The old `vendors` unit test asserted `vendor_of("io.x") == "Io"` and `"foo" == "Unknown"`: those encoded the fragment behaviour the review rejects, now `"Unknown developer"`.

## 2. "Part of macOS" (REVIEW-A 45, origin part): fixed in tests, MAC-CHECK below

`AppInfo::apple` meant "com.apple. bundle id", so every Apple app was locked and tagged. It now means part of macOS: under `/System` (`apps::in_system`: /System/Applications, /System/Library/CoreServices), or `apps::macos_app` (a `com.apple.` id and no App Store receipt: Safari), or signed "Software Signing". iMovie, GarageBand, Pages, Numbers, Keynote carry `Contents/_MASReceipt/receipt` (the real Mac already showed "App Store" for them, which comes from that receipt), so they are ordinary removable App Store apps.

No UI change needed: `pages/apps.rs:717 if a.apple` now tags only real system apps, and the Unused group includes the App Store Apple apps. Posted on the board for design3.

Safety (core `features/uninstaller.rs`, announced on the board, owner lead/apps):
- `apps::apple_part` (the `validate_set` gate) uses `macos_app`, so an App Store Apple app may be selected; a macOS app still stops the whole run.
- `prepare()` refuses `macos_app` or `/System` bundles (was: any `com.apple.` id).
- Leftovers that `apple_part` would refuse (named after an Apple id, e.g. `~/Library/Containers/com.apple.iMovieApp`) are not proposed, so an iMovie plan never fails the gate: they stay on disk (conservative).
- The maker shared-folder match uses `vendor_segment` and never the "apple" folder.

Tests (`apps_developer.rs`): `apple_app_store_apps_are_not_part_of_macos` (five App Store Apple apps: not apple, store App Store, vendor "Apple Inc.", `app_lock` none; Safari and Calculator apple and locked; quick and deep listings agree; CoreServices path is system), `imovie_can_be_uninstalled_and_its_apple_named_files_stay` (plan selects iMovie.app, run moves it to the Trash, the com.apple container stays, Safari and Calculator plans select nothing). UI: `icons_ui::app_remover_shows_real_developers_and_part_of_macos_only_for_system_apps` (the tag sits only in the Safari and Calculator rows; developer names in each row's info line). Existing `apps_uninstall::apple_apps_are_locked_and_refused` and `qa_apps_remover` stay green.

Renders: `docs/screens/v2/icons3-app-remover-meta-1060@1x.png`, `@2x`, `icons3-app-remover-meta-1280@1x.png`, `@2x` (`CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test icons_ui metadata_render`). Checked: GarageBand and iMovie "10.4 · Apple Inc. · App Store", no tag; Calculator and Safari tagged; GoLand "JetBrains s.r.o."; Screaming Frog "Screaming Frog Ltd" (at 1060 the info line is middle-truncated to "Screaming Frog...d": the info width is design3's layout).

## 3. Neutral loading tile (REVIEW-A 44): fixed

`crates/ui-egui/src/images.rs`: while an app icon is on its way the slot shows `images::loading_tile`: a grey rounded square (macOS icon proportions, 8% inset, 22.5% corners, white 6% fill, 9% edge), no letter and no colour; the real icon cross-fades from it. The coloured initial tile remains only as the fallback when no icon can be made (every tool failed) or no loader runs (fake Macs and the older UI tests), so apps stay distinguishable. File rows are unchanged (their drawn kind icon is final).

Tests: `icons_ui::the_loading_placeholder_is_a_neutral_tile_not_a_coloured_letter` (pixel inside the loading tile is as grey as the row and lighter than it; fails before: the letter tile is saturated), `an_app_without_an_icon_keeps_its_initial_tile`. Render: the meta renders above are taken with the loader installed and not yet run, so all six tiles are the loading state.

## Checks
`cargo test --workspace` green (incl. check_safety text rules), `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all --check` clean; target/ deleted afterwards.

## MAC-CHECK (owner's Mac)
1. `codesign -dv --verbose=2 /Applications/<JetBrains app>.app 2>&1 | grep Authority` shows "Developer ID Application: JetBrains s.r.o. (...)"; the App Remover row shows the same name. Same for FreeCAD, Blender, Screaming Frog.
2. `codesign -dv --verbose=2 /Applications/iMovie.app 2>&1 | grep Authority` shows "Apple Mac OS Application Signing"; `ls /Applications/iMovie.app/Contents/_MASReceipt/receipt` exists. Row: "Apple Inc. · App Store", no "Part of macOS"; Uninstall works and asks nothing about macOS. Same for GarageBand, Pages, Numbers, Keynote.
3. Safari and the /System/Applications apps keep "Part of macOS" and cannot be selected.
4. Opening App Remover with 130+ apps: the deep listing should not be noticeably slower than before (codesign now runs for every non-macOS app, 8 at once).
5. On first launch, rows show grey tiles that fade into the icons; an app whose icon cannot be made shows its coloured initial.

## Open
- The App Store seller name is not read (the receipt is a signed PKCS7 blob); the Info.plist copyright stands in for it, which for Apple's apps gives "Apple Inc.".
- iMovie and GarageBand leftovers named after their Apple ids are left on disk when uninstalling (the safety gate refuses Apple-named Library entries by design); GarageBand's sound library in /Library/Application Support/GarageBand is only proposed as a name match (unselected).
- Ownership note: the small `uninstaller.rs` edits are outside my listed files; announced on the board for the lead.
