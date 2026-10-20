# safety6 report, round 6

Reviewer only: no app code changed (merge commits on my branch only union module lists,
settings fields and two logic lines so the merged tree builds).

Threat model, defects and verification log: `docs/review/SAFETY-R6.md`.

## Tests

- `crates/core/tests/review_safety6.rs`: static guards (processes only in sys.rs, no permanent
  developer cleanup arguments, background code never reaches removal), tray6, watch6, health6,
  dev6, files6 checks. Run: `cargo test -p cleancraft-core --test review_safety6 -- --include-ignored`.
- `crates/ui-egui/tests/review_safety6_ui.rs`: the lead's way-forward rule (every all-locked page
  state offers an action or exact steps: Mail open, Trash Bins, browsers open, Developer Clutter
  in Documents) and Mac Health buttons only look.
- `crates/core/tests/review_safety6_apps.rs` on branch `agent6/safety6-apps` (built from
  agent6/apps6, which still conflicts with watch6 in uninstaller.rs): batch uninstall checks.

## Defects

| ID | Sev | Owner | Status |
|---|---|---|---|
| SAF6-1 permanent tool deletes | S1 | dev6 | fixed 43facd5 |
| SAF6-2 git pathspec globs | S1 | dev6 | fixed 43facd5 |
| SAF6-3 venvs preselected | S2 | dev6 | fixed 43facd5 |
| SAF6-5 symlinked AI settings file replaced | S2 | health6 | fixed 439520a |
| SAF6-6 notification text read as osascript -e | S1 (MAC-CHECK) | tray6 | open |
| SAF6-7 Erase in Mail scope wider than the dialog | S1 | files6 | fixed 55dab45 |
| SAF6-8 edited uninstall plan moves another app | S2 | apps6 | open |

Counts: S1 4 (3 fixed, 1 open), S2 3 (2 fixed, 1 open), S3 0.

## Real-Mac checks

See the last section of `docs/review/SAFETY-R6.md`.
