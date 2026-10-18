# CleanCraft team

Coordinator: the main session. Merges branches into `main`, owns shared files, runs the final click-through.

All worktrees live in `/home/claude/wt/<name>` on branch `agent/<name>`, created from the same `main` commit. Shared cargo target: `/home/claude/cargo-target` (set `CARGO_TARGET_DIR`), with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. The machine has 2 CPUs: build only what you need (`-p <crate>`, `--test <file>`), full suite before commits.

Message board: `/home/claude/wt/BOARD.md`, one line per message: `HH:MM FROM -> TO: text`.

| Name | Role | Owns (files) | FEATURES.md rows |
|---|---|---|---|
| cleanup | systems-engineer | core `features/system_junk.rs`, `features/mail.rs`, `features/trash.rs`, `features/privacy.rs`, their tests; UI `pages/extras_cleanup.rs`, `tests/qa_cleanup_*.rs` | J1-J21, M1-M3, T1-T2, P3-P6 |
| perf | systems-engineer | core `features/malware.rs`, `tasks.rs` (maintenance), `startup.rs` (login items, agents, apps), tests; UI `pages/tasks.rs`, `pages/startup.rs`, `tests/qa_perf_*.rs`, `tests/qa_malware*.rs` | K1-K10, L1-L5, P1-P2 |
| apps | product-engineer | core `apps.rs` (inventory), `features/uninstaller.rs`, `features/leftovers.rs`, `updater.rs`, `extensions.rs`, tests; UI `pages/apps.rs`, `tests/qa_apps_*.rs` | A1-A11 |
| files | product-engineer | core `features/large_old.rs`, `features/duplicates.rs`, `features/similar.rs`, `features/downloads.rs`, `space.rs`, tests; UI `pages/extras_files.rs`, `pages/disk_map.rs`, group display in `pages/plan_page.rs` (coordinate), `tests/qa_files_*.rs`, `tests/qa_disk_map*.rs` | C1-C4, D1-D3 |
| monitor | systems-engineer | core `monitor.rs`, `sweep.rs`, tests; UI `pages/monitor.rs`, `pages/sweep.rs`, `tests/qa_monitor*.rs`, `tests/qa_sweep*.rs`; menu bar code in `apps/cleancraft/src/menubar.rs` | Q1-Q3, R1-R7 |
| parity | parity-lead (later) | `tests/qa_every_control.rs`, `docs/FEATURES.md` statuses | S12, U1-U7 audit |
| ux | ux-designer (later) | `theme.rs`, `kit.rs`, `assets/`, snapshot example | look and layout |
| reviewers | domain-qa, data-qa, reliability-engineer (later) | only `tests/review_*.rs` with ignored failing tests | |

Shared files (coordinator owns, builders make small announced additions): `Cargo.toml` files, `crates/core/src/lib.rs`, `features/mod.rs` (registry line), `testkit.rs`, `app.rs`, `pages/mod.rs`, `pages/plan_page.rs`, `pages/extras.rs`, `crates/mcp/src/lib.rs` (new tools: put them in your own `crates/mcp/src/<name>.rs` with `#[tool_router(router = <name>_router)]` and tell the coordinator to combine routers), `apps/cleancraft/src/cli.rs` (new subcommands: put the logic in `apps/cleancraft/src/cli_<name>.rs`).
