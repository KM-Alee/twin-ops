# Code layout and testing conventions

This document describes the **implemented** workspace structure after slice 1. It is the source of truth for module boundaries, dependency injection, and how tests are organized. Slice plans (`docs/plan-slices.md`, `docs/imp-plan/`) and the full target layout in `docs/tech-document.md` may list crates not built yet.

---

## Workspace (slice 1)

Four crates, dependency direction inward:

```text
twin-core  →  (no internal twin deps)
twin-store →  twin-core (error types only at boundary today)
twin-app   →  twin-core, twin-store
twin-cli   →  twin-app  (+ lib target for output tests)
```

No `twin-output` in slice 1. Human and JSON rendering live in `twin-cli/src/output/`. Extract `twin-output` when multiple commands need DOT/Mermaid and shared formatters.

---

## Module trees

### `twin-core`

```text
src/
  lib.rs
  error.rs
  config/
    mod.rs
    retention.rs    # TwinConfig, RetentionConfig
    template.rs     # DEFAULT_CONFIG_TOML
tests/
  config.rs         # serde roundtrip, template vs defaults
```

### `twin-store`

Store receives a **path**; it does not resolve XDG directories.

```text
src/
  lib.rs
  error.rs
  migration/
    mod.rs          # MIGRATION_001, LATEST_VERSION
  store/
    mod.rs          # Store { conn }
    open.rs         # open, open_in_memory, pragmas
    migrate.rs      # initialize, schema_version, is_initialized
    health.rs       # health_check, journal_mode_wal
tests/
  store.rs
```

### `twin-app`

Commands take an explicit **`TwinLayout`** (see below). The CLI uses `TwinLayout::from_xdg()` once at the boundary.

```text
src/
  lib.rs            # pub API: init, doctor, InitRequest, TwinLayout
  error.rs
  paths/
    mod.rs
    layout.rs       # TwinLayout::isolated, ensure_dirs, config_file, …
    xdg.rs          # TwinLayout::from_xdg
  config_io/
    mod.rs          # read, sync_default_template
  commands/
    mod.rs
    init.rs
    doctor/
      mod.rs
      core.rs
      database.rs
      permissions.rs
  model/
    mod.rs
    init_result.rs
    doctor_result.rs
tests/
  support/
    mod.rs          # IsolatedHome fixture
  init.rs
  doctor.rs
```

### `twin-cli`

Binary is `twin`; crate name is `twin-cli`. Library target `twin_cli` exists so `tests/` can exercise renderers without spawning the binary.

```text
src/
  lib.rs
  main.rs           # parse → twin_app → output
  cli/
    mod.rs
    args.rs
  output/
    mod.rs
    init.rs
    doctor.rs
    json.rs
tests/
  output.rs           # renderer unit tests (no binary spawn)
  cli_integration.rs  # end-to-end: spawns `twin` with isolated HOME
  support/mod.rs      # TwinHome fixture, fake /proc writers
```

**CLI integration tests** set `HOME` to a temp dir (real XDG layout under it) and `TWIN_PROC_ROOT` to a fake `/proc` tree so `twin scan` never reads the host. `TWIN_PROC_ROOT` is read only in `twin-cli` `main` when set (test harness); production scans use `/proc`.

---

## `TwinLayout` and command API

Path resolution is an **application** concern. The store only opens paths it is given.

```rust
// Production (CLI)
twin_app::init(InitRequest { force, config_override })?;
// Resolves TwinLayout::from_xdg() inside init.

// Tests — inject an isolated layout (no env vars, no global mutex)
let home = IsolatedHome::new();  // tempfile + TwinLayout::isolated
twin_app::init_in(&home.layout, InitRequest::default())?;
twin_app::doctor_in(&home.layout, None)?;
```

`TwinLayout` fields: `data_dir`, `config_dir`, `state_dir`. Helpers: `config_file(override)`, `db_file()`, `log_file()`, `ensure_dirs()`.

XDG defaults on Linux:

```text
~/.config/twin/config.toml
~/.local/share/twin/twin.db
~/.local/state/twin/twin.log
```

Command results (`InitResult`, `DoctorResult`) live in `twin-app/src/model/` and derive `Serialize` for `--json`. Rendering stays in `twin-cli`.

---

## Testing conventions

| Rule | Rationale |
|------|-----------|
| Integration tests in `crates/<crate>/tests/` | Keeps production modules free of `#[cfg(test)]` blocks for I/O and commands. |
| Shared fixtures in `tests/support/` | e.g. `IsolatedHome` for `twin-app`; one place per crate. |
| Inject `TwinLayout` | Tests must not set global state or hidden env vars in production code. |
| Unit tests in-source only for pure logic | Rare in slice 1; prefer crate integration tests. |
| No host mutation in tests | Use temp dirs and in-memory SQLite; never write under real `~/.config/twin` in CI. |
| CLI E2E via `CARGO_BIN_EXE_twin` | `twin-cli/tests/cli_integration.rs` exercises init, doctor, scan, graph (human + `--json`). |

Example (`twin-app/tests/init.rs`):

```rust
mod support;

#[test]
fn second_run_is_idempotent() {
    let home = support::IsolatedHome::new();
    let first = twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let second = twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    assert!(first.config_created);
    assert!(!second.config_created);
}
```

Later slices add `twin-fixtures` for fake `/proc`, systemd, K8s, etc. Slice 1 does not include that crate.

---

## Config template refresh

`config_io::sync_default_template` writes `DEFAULT_CONFIG_TOML` when:

- the file is missing;
- `init --force`;
- the file parses to `TwinConfig::default()` but bytes differ from the shipped template (comment/header refresh without touching customized retention values).

---

## Adding modules later

When a command or subsystem grows past a single file:

1. Add a directory under `commands/` or a new top-level module (`collectors/`, etc.).
2. Keep one primary type per file where practical (see `AGENTS.md`).
3. Add `tests/<feature>.rs` beside `tests/support/` helpers.
4. Pass dependencies (`TwinLayout`, adapters, graph handles) into functions — do not add test-only branches in production paths.

Target workspace (all crates) remains documented in `docs/tech-document.md` §4.1; this file tracks what is **shipped** until more slices land.
