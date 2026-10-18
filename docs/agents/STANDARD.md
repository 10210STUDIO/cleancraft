# CleanCraft team brief (read fully before working)

CleanCraft is a macOS cleaning and maintenance app with feature parity with the reference app, built as an original product: own name, design, texts and icons. Rust 2024, egui 0.36, workspace like the owner's ShellyCraft project.

## Hard rules

1. **Never copy the reference app.** No code, binaries, images, sounds, UI layout or wording. Use only `docs/research/the reference app-public.md` (public docs) and general macOS knowledge. Our feature names are in `FeatureId::title()`; write our own texts.
2. **Safety model, never bypass it.**
   - Scans only look. They return a `Plan` (exact items, sizes, reasons). Nothing is removed outside `executor::execute`, which needs the plan digest the user confirmed.
   - Removing = moving to the Trash or the CleanCraft quarantine with one `rename`, logged in the journal with an undo. Never `remove_file`, `remove_dir_all`, `rm`, or any permanent delete in app code. (`grep -rn "remove_file\|remove_dir" crates apps` must stay empty outside tests and tempdir cleanup.)
   - Every feature has a hard allow-list (`Feature::allow_roots`); `policy::check` is the fence. Never touch SIP-protected system files, iCloud Drive, Photos libraries, the contents of Documents and Desktop, or files open in running apps. Do not weaken `policy.rs` or `executor.rs`; if you think they need a change, write it in your report and stop.
   - Commands that change the system (maintenance, launchctl, osascript) go through `env.sys` (`Sys` trait) so tests use `FakeSys`, and are logged with `executor::append_journal` (`ActionKind::Command` or `Toggled` with an undo command when one exists).
3. **Tests only on a fake home.** Use `cleancraft_core::testkit::FakeMac` (a temp folder). `Env::real()` fails in any cargo process (`CLEANCRAFT_FORBID_REAL=1`). Never read or write real files outside the temp folder in tests. Never run the real `osascript`, `launchctl`, `tmutil`, `purge` etc. in tests: answer them with `FakeSys::answer`.
4. **No unsafe code** (workspace forbids it). macOS facts come from system command-line tools through `env.sys.run(...)` with parsers that are unit-tested on recorded sample output. We cannot compile for macOS here, so keep `#[cfg(target_os = "macos")]` code to zero in your files.
5. **Quality standard.** A feature WORKS only when an end-to-end UI test (egui_kittest, see `crates/ui-egui/tests/qa_system_junk.rs` and `tests/common/mod.rs`) drives it by clicks and typing, checks the files actually moved and the numbers on screen against a recalculation from disk, and covers the error paths that apply (permission denied, file in use, disk full: `env.inject_fault`, `FakeSys::set_open`). Anything less is PARTIAL. A false WORKS is the worst failure; an honest PARTIAL is fine.
6. **No dead controls.** Every button you add must do something visible and call `app.act()`. Never show "not available", "coming soon" or similar. If something cannot work on this platform, do not show a button for it.
7. **Texts:** English, plain words. Never use em dashes. Never use the word "nor".
8. **Your files only.** You own the files listed in your assignment. Shared files (`Cargo.toml`, `crates/core/src/lib.rs`, `features/mod.rs`, `app.rs`, `pages/mod.rs`, `mcp/src/lib.rs`, `apps/cleancraft/src/cli.rs`) get only small additions (a `pub mod`, a registry line, a match arm, a new subcommand or MCP tool). Mention every shared-file change in your report.
9. **Processes and disk.** Never stop processes you did not start (no `pkill`/`killall` by name). Check `df -h /home/claude` before big builds; your worktree has its own `target/` (set `CARGO_TARGET_DIR` to your worktree's `target`). Run `cargo build`/`cargo test` with `-j 1` if the machine is busy.
10. **Commit often** in your worktree branch with clear messages, ending with:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01Gp8iScvZqjUdHvM1N9P7rg
    ```

## How the code fits together

- `crates/core`: `env.rs` (fake/real Mac paths: `env.home_path("Library/Caches")`, `env.sys_path("/Library/Caches")`), `sys.rs` (`Sys`, `FakeSys`), `policy.rs`, `plan.rs` (`Plan`, `Item`, `Risk`, `FeatureId`), `executor.rs` (execute, journal, undo), `features/` (one file per feature implementing `Feature`; register in `features::registry()`), `fsutil.rs` (`tree_size`, `human_size`: use these so numbers match everywhere), `settings.rs`, `testkit.rs` (`FakeMac`; you may add helpers in your own `testkit_*.rs` file instead of editing the shared one).
- `crates/ui-egui`: `app.rs` (shell, sidebar, `CleanApp`), `pages/plan_page.rs` (the shared Scan, Review, Confirm, Done flow for plan features), custom pages in `pages/*.rs`, `jobs.rs` (background work; `app.sync` runs it inline for tests).
- `crates/mcp`: MCP tools. `apps/cleancraft`: binary (`main.rs`, `cli.rs`).

## Definition of done for your assignment

- Every row you own in `docs/FEATURES.md` has a status and a test name (fill in the Test and Status columns of your rows only).
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass in your worktree.
- A short report `docs/agents/reports/<your-name>.md`: what works (with test names), what is PARTIAL and why, what must be checked by hand on a real Mac (commands whose output format you guessed, permissions), and every shared-file change.
