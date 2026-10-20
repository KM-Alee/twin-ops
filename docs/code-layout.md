# Code layout and testing conventions

This document describes the **implemented** workspace structure in the current workspace. It is the source of truth for module boundaries, dependency injection, and how tests are organized. Slice plans (`docs/plan-slices.md`, `docs/imp-plan/`) and the full target layout in `docs/tech-document.md` may list crates not built yet.

---

## Workspace

Original foundation (additional implemented crates listed below), dependency direction inward:

```text
twin-core  →  (no internal twin deps)
twin-store →  twin-core (error types only at boundary today)
twin-app   →  twin-core, twin-store
twin-cli   →  twin-app  (+ lib target for output tests)
```

Human, JSON, DOT, and Mermaid rendering live in `twin-cli/src/output/`. The implemented workspace also contains twin-observation, twin-collectors, twin-ebpf, twin-container, twin-k8s, twin-config, twin-emulate, twin-test, twin-safety, twin-package, and twin-fixtures. The safety crate hosts cross-workspace source checks; it has no runtime business logic. Graph/rules/output logic remains in owning app/CLI modules; extract only for concrete coupling or reuse.

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
    mod.rs          # read, load, sync_default_template
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

`twin-fixtures` currently shares the RPM header fixture; proc/systemd/K8s fixtures still live in owning crate test support.

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

## Follow-up module locations

- `twin-app/src/commands/why.rs` and `model/why_result.rs`: stored evidence explanations.
- `twin-store/src/repo/graph_at.rs`, `history.rs`, `retention.rs`: retained history reconstruction, predecessor lookup, source-aware pruning.
- `twin-cli/src/output/graph_export.rs` and `why.rs`: graph export and explanation rendering.
- `twin-safety/tests/read_only.rs`: scans every production source crate.
- `twin-app/tests/config_commands.rs`: config validation and configured retention.

`clap_complete` generates completion scripts directly from the CLI definition. `chrono` with only std parses RFC3339 history timestamps. Neither dependency adds host mutation capabilities.

## Additional implemented adapters and fixtures

- `twin-k8s/src/live.rs`: bounded kube-rs get/list reads; `parse.rs` projects both fixture and live inputs to safe ClusterView fields.
- `twin-app/src/commands/ebpf_cgroup.rs`: read-only cgroup-v2 identity mapping and evidence-linked event attribution.
- `twin-app/src/commands/nginx_includes.rs`: bounded include/path expansion and config reads.
- `twin-app/src/commands/resolve_port.rs`: unambiguous TCP port shorthand.
- `twin-app/src/commands/evidence/conflicts.rs`: competing active ownership claims.
- `twin-package/src/rpm.rs`: immutable SQLite RPM header/file/version reads; `wal_snapshot.rs` handles private committed WAL replay and metadata stability; `ndb.rs` validates NDB slots/blobs; `bdb.rs` reads bounded standalone hash buckets/overflow chains; `error.rs` owns typed failures.
- `twin-fixtures/src/lib.rs` and `rpm_bdb.rs`: shared RPM header/NDB/BDB byte fixtures used by twin-package and twin-app tests.

Dependencies added for actual functionality: kube/k8s-openapi/Tokio (live Kubernetes), existing workspace rusqlite/thiserror (RPM reads). No kube/runtime/WS/OAuth features are enabled. The safety tests check mutation method calls while allowing inspection and rejection of credential-command configuration fields.

The process socket module exports protocol-neutral Inet* parser types; twin-core/transport_protocol.rs owns TCP/UDP transport identity.

`commands/operational_changes.rs` summarizes retained domain transitions; `model/operational_change.rs` carries sourced results. Core ChangeCategory is the typed category vocabulary.

`twin-fixtures/src/procfs.rs` owns the shared fake-proc writer, used by app/CLI/collector test-support wrappers. Store::get_edge_typed is the typed single-edge read boundary. Systemd file collection receives concrete runtime/proc instances through collect_with_instances; template fallback does not invent running instances. `tools/watch_soak.py` runs live read-only polling with isolated temporary XDG state and records RSS/timing statistics.

## Agent integration modules (slice 24)

- `twin-app/src/commands/preflight/{shell,intent,mod}.rs` and `model/preflight_result.rs`: command tokenizer, typed intents, graph-backed verdicts.
- `twin-app/src/commands/brief.rs` and `model/brief_result.rs`: host and target briefings composed from graph, emulation and history.
- `twin-cli/src/hook.rs`: agent hook input parsing and per-agent response dialects.
- `twin-cli/src/mcp/{mod,tools}.rs`: the MCP JSON-RPC handler (stdio and daemon modes) and tool implementations.
- `twin-cli/src/daemon/{mod,client}.rs`: `twin daemon run` socket server, its client and the brief/preflight/status calls; `twin-cli/src/output/daemon.rs` renders `daemon status`.
- `twin-app/src/daemon/{access,peer,unit}.rs`: socket peer credentials (`SO_PEERCRED`/`SO_PEERGROUPS`), `AccessPolicy` and the printed systemd unit.
- `twin-cli/src/agent_config.rs` and `assets/SKILL.md`: printed setup snippets and the embedded skill.
- Tests: `twin-app/tests/{preflight,brief}.rs` (shared `support::scan_postgres_django` fixture), `twin-cli/tests/{hook,agent_integration}.rs`.

## Slice 25 modules

- `twin-app/src/commands/preflight/script.rs`: script and Makefile expansion into operation lines.
- `twin-collectors/src/interfaces.rs`: `/proc/net` interface/address/default-route parser.
- `twin-cli/src/output/json.rs`: `JsonMode` and `cap_lists` output budgets.
- `twin-cli/src/agent_config.rs`: also `SetupSummary`/`render_setup` for `twin setup`.
- Release and packaging: `.github/workflows/{ci,release}.yml`, `install.sh`, `packaging/aur/PKGBUILD`, `LICENSE`.
- Tests: `twin-cli/tests/output_budget.rs`, `twin-collectors/tests/interfaces.rs`.

