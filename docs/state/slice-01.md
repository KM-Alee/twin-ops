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
