# CleanCraft

Original macOS cleaner and maintenance app, aiming at feature parity with the leading commercial Mac cleaners (studied from public documentation only). Read `docs/agents/STANDARD.md` (rules, safety model, quality bar), `docs/agents/TEAM.md` (who owns what) and `docs/FEATURES.md` (the contract) before any change.

- Rust 2024, egui 0.36. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.
- Tests run only on `testkit::FakeMac` (temp folder). `Env::real()` refuses to run under cargo.
- Nothing is ever deleted: plans are confirmed by digest, items move to the Trash or quarantine, every action is journaled with undo.
- Texts: English, no em dashes, never the word "nor".
