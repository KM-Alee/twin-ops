# Slice 1: CLI, Config, Init, and Doctor Foundation

## Status: done

## Implemented

Documented in **`docs/code-layout.md`** (module trees, `TwinLayout`, testing rules).

- Workspace: `twin-core`, `twin-store`, `twin-app`, `twin-cli` (binary `twin`, lib `twin_cli`)
- `twin-core`: `config/` module, `tests/config.rs`
- `twin-store`: `store/{open,migrate,health}.rs`, `migration/`, `tests/store.rs`
- `twin-app`: `paths/`, `commands/`, `model/`, `config_io/`; `init` / `doctor` via `InitRequest` + `TwinLayout`; `tests/support/IsolatedHome`
- `twin-cli`: `cli/`, `output/`; `tests/output.rs`

## Acceptance Criteria

- [x] `twin init` creates config and state dirs
- [x] Running `twin init` twice does not overwrite config
- [x] `twin doctor` reports config, DB, OS, permission mode
- [x] `--json` works
- [x] Normal CLI errors do not panic

## Decisions

- Sync first: no tokio for init/doctor commands
- rusqlite with `bundled` feature for self-contained binary
- `dirs` crate for XDG-compliant platform paths
- TOML config format, serde + toml crate
- Error types use thiserror derive, no anyhow in library crates
- No `twin-output` crate; rendering in `twin-cli` until more formats exist
- Path resolution in `twin-app`, not `twin-store`
- Initial DB schema: `schema_migrations` only (full schema in slice 2)
- `TwinLayout` + `tests/support/IsolatedHome` for isolated integration tests (no global test state)
- Modular dirs: `commands/`, `model/`, `paths/`, `config_io/` (app); `store/` submodules (store); `config/` (core); `cli/` + `output/` lib (cli)

## Deviations from Plan

- `plan-slices.md` / `tech-document.md` Phase 0 mention `twin-output`; slice 1 renders in `twin-cli` until later extraction
- Human output for `init` / `doctor` uses shared `twin-cli/src/output/format.rs` helpers (polished tree layout; documented in `AGENTS.md`)
- `RetentionConfig` uses manual `Default` impl (serde field defaults do not apply to `derive(Default)`)
- Config template refresh on `init` when values are still defaults but file bytes differ (not in original imp-plan)

## 2026-10-08 follow-up: Configuration, completion, and workspace safety

All read commands validate the selected settings file. An explicit missing config is an error; an absent implicit config uses defaults. CLI help states that settings do not relocate the XDG database. Scan validates config before collecting or writing. `twin completions bash|zsh|fish|powershell|elvish` generates scripts without opening the store. `clap_complete` is used to keep scripts consistent with clap arguments. `twin-safety` scans production source across every crate for host mutation and shell execution paths. Doctor reads effective UID from proc instead of running `id`. Tests: config_commands, cli_integration, and twin-safety/read_only.

Shared fixture follow-up: twin-fixtures is a std-only dev dependency of app/CLI/collectors/package. It owns the shared fake-proc file writer and RPM header fixture; test-support wrappers retain caller assertions. No runtime crate depends on fixture writers in production.
