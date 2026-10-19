# designqa, round 2 report

Reviewer only: no app code changed.

## Delivered (branch agent2/designqa)

- `docs/design/REVIEW-A.md`: 69 defects on the v1 renders and the real-Mac v1 shots, each with page, element, measured value, target, severity and owner (S1 2, S2 41, S3 26). Top 10 posted on the board at 16:29.
- `docs/design/REVIEW-B.md`: verification of agent2/design 91ece24 (which contains agent2/motion and agent2/icons), rendered here with `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_render pages_render`. 41 fixed, 10 partly fixed, 18 open, 11 new (B1 to B11). Evidence crops in `docs/design/review-b/`.
- `crates/ui-egui/tests/review_design_a.rs`: ignored tests for REVIEW-A 1 (type scale), 2 (no text under 11 pt), 3 (text_faint contrast), 4 (label contrast on section gradients), 57 (headers not elided), plus a WCAG helper self-test. All five pass on agent2/design.
- `crates/ui-egui/tests/review_design_b.rs`: ignored tests for B1 (toolbar buttons overlap), B2 (Monitor figures elided), A31 ("Zero KB" wording). All three fail on agent2/design 91ece24.

## Remaining after round B

S1 1, S2 16, S3 21. Most important: B1 Privacy Traces toolbar overlap at 1060x680 (S1), B4 clippy fails on the integrated branch, B3 hero glow clipped into a visible box on every start screen, REVIEW-A 4 Danger pill white label 2.7:1, 53 and B2 Live Monitor (empty sparkline, elided memory figure), 45 vendor names and "Part of macOS" on Apple App Store apps, 66 count-up, stagger and dialog fade defined in motion.rs but unused.

## To check on a real Mac

Real app icons (only proven in tests with a fake `sips` output), hero smoothness and energy at 120 Hz, the 30 fps idle cap, Reduce Motion, the title bar (12) once changed.

## Shared files

None changed.
