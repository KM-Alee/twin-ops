# Slice 1 Implementation Plan: CLI, Config, Init, and Doctor Foundation

**Implemented layout and tests:** see `docs/code-layout.md` (canonical module trees, `TwinLayout`, integration-test conventions).

## 1. Overview

This slice produces a working `twin` binary that does two things: `twin init` and `twin doctor`. Every type, table, file, and module exists because one of those commands needs it. Nothing else.

```bash
twin --help
twin --version
twin init
twin init --force
twin doctor
twin doctor --json
```

---

## 2. Design Problems with the Previous Plan

The previous plan had these structural issues:

| Problem | Why It Matters |
|---------|---------------|
| NodeId, EdgeId, ObservationId, etc. defined but unused | Dead code. AGENTS.md says: "Dead code = bug." |
| Full SQL schema (observations, nodes, edges, etc.) in slice 1 | Those tables are slice 2. Slice 1 init only needs schema_migrations. |
| Separate `twin-output` crate for 2 report types | YAGNI. Two renderers for two structs don't justify a crate. |
| Path resolution inside `twin-store` | Store should receive a path, not decide where data lives. That's an app concern. |
| `AppError::Config(String)` | String-typed error variants are unstructured. Use proper error types. |
| RetentionConfig, EbpfConfig, CollectorConfig | Not used by init or doctor. Pure speculation. |
| One file per enum (kinds.rs, state.rs, evidence.rs) | Tiny files with no callers. Group until growth justifies splitting. |
| Report types in twin-output, business logic in twin-app, rendering split across both | The "report" is a command result. It belongs in twin-app. Rendering is CLI presentation. |

This plan fixes all of them.

---

## 3. Crate Design

### 3.1 Dependency DAG

```
twin-core      (leaf — no deps outside std)
twin-store     (depends on twin-core)
twin-app       (depends on twin-core, twin-store)
twin-cli       (depends on twin-app)
```

Four crates. No `twin-output`. Rendering logic lives in `twin-cli` until it grows large enough to justify extraction (it won't in this slice).

### 3.2 Why No `twin-output`

Two struct renderers don't justify a crate. The AGENTS.md rule is: "No trait until needed. No config field until needed. No enum variant until needed." A separate output crate is "until needed" failing — it's not needed yet. When we add DOT, Mermaid, and multiple report types in later slices, we extract. For now, `impl Display` and a JSON serialize in `twin-cli` is enough.

### 3.3 Why No Domain Types Beyond Slice 1

`NodeId`, `EdgeKind`, `EvidenceStrength`, `ObservationSource` — none of these are used by `init` or `doctor`. They belong in slice 3 when the observation pipeline and graph model arrive. Defining them now means maintaining dead code for 2 slices.

Every type in slice 1 must be reachable from either `twin init` or `twin doctor`. If it isn't, it doesn't ship.

---

## 4. `twin-core`

### 4.1 Purpose

The absolute minimum domain types shared across crates. No OS-specific imports. No eBPF, K8s, or container code. No types that aren't used in this slice.

Only what init and doctor need:

- Error types (the error enum pattern for the project)
- Config types (what `twin init` writes and `twin doctor` reads)
- The version constant

### 4.2 Files (implemented)

```
crates/twin-core/src/
  lib.rs
  error.rs
  config/
    mod.rs
    retention.rs
    template.rs
crates/twin-core/tests/
  config.rs
```

No `id.rs`, no `kinds.rs`, no `evidence.rs` — those arrive in slice 3. Config tests live in `tests/`, not inline `#[cfg(test)]`.

### 4.3 Types

**`config.rs`** — Config that init writes and doctor reads:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinConfig {
    pub retention: RetentionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_observations_days")]
    pub raw_observations_days: u32,
    #[serde(default = "default_graph_days")]
    pub graph_history_days: u32,
    #[serde(default = "default_ebpf_days")]
    pub ebpf_events_days: u32,
}

fn default_observations_days() -> u32 { 7 }
fn default_graph_days() -> u32 { 30 }
fn default_ebpf_days() -> u32 { 3 }

impl Default for TwinConfig {
    fn default() -> Self {
        Self {
            retention: RetentionConfig::default(),
        }
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            raw_observations_days: default_observations_days(),
            graph_history_days: default_graph_days(),
            ebpf_events_days: default_ebpf_days(),
        }
    }
}
```

Why only `RetentionConfig`? Because init writes defaults and doctor reads them. No `EbpfConfig` or `CollectorConfig` until slices that use them. The config file is additive — fields appear when features that need them appear.

The serialized default config becomes:

```toml
[retention]
raw_observations_days = 7
graph_history_days = 30
ebpf_events_days = 3
```

**`error.rs`** — Core error types:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config read error: {path}: {source}")]
    Read { path: String, source: std::io::Error },
    #[error("config parse error: {source}")]
    Parse { source: toml::de::Error },
    #[error("config write error: {path}: {source}")]
    Write { path: String, source: std::io::Error },
}
```

Only `ConfigError` for now. Other error types arrive in their slices. `CoreError` as a general enum would be premature — errors are crate-scoped.

**`lib.rs`**:

```rust
pub mod config;
pub mod error;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

### 4.4 What's Deliberately Not Here

| Type | Arrives In | Reason |
|------|-----------|--------|
| `NodeId`, `EdgeId`, `ObservationId` | Slice 3 | No graph until slice 3-4 |
| `NodeKind`, `EdgeKind`, `EdgeClass` | Slice 3 | No graph nodes until slice 3 |
| `RiskLevel`, `EvidenceStrength` | Slice 8 | No impact/emulation until slice 8 |
| `ObservationSource`, `ConfidenceHint` | Slice 3 | No observation pipeline until slice 3 |
| `NodeState`, `EdgeState` | Slice 3 | No observations until slice 3 |
| `CollectorName` | Slice 4 | No collectors until slice 4 |

### 4.5 Tests

- `TwinConfig::default()` serializes to valid TOML
- `TwinConfig` deserializes from TOML string
- Roundtrip: default → serialize → deserialize → equal
- Missing fields get defaults on deserialize

---

## 5. `twin-store`

### 5.1 Purpose

SQLite bootstrap: create DB, run migrations, health check. That's it for slice 1. The store receives its path; it does not discover it.

### 5.2 Dependencies

- `twin-core`
- `rusqlite` (features: `bundled`)

### 5.3 Design Decision: Store Receives Path, Not Discovers It

The previous plan had path resolution (`data_dir()`, `config_dir()`, etc.) inside `twin-store`. This violated separation: the store is a library that operates on a database path given to it. The application decides where data lives. Path resolution moves to `twin-app`.

This means `twin-store` has no `dirs` dependency. The store's constructor is `Store::open(path)` and nothing else.

### 5.4 Files (implemented)

```
crates/twin-store/src/
  lib.rs
  error.rs
  migration/mod.rs
  store/
    mod.rs
    open.rs
    migrate.rs
    health.rs
crates/twin-store/tests/
  store.rs
```

No `paths.rs` in the store crate. Store submodules split open / migrate / health concerns.

### 5.5 `migration.rs`

Only `schema_migrations`. The full schema (observations, nodes, edges) is slice 2. Including it now means maintaining tables that no code reads or writes — dead schema.

```sql
-- Migration 001
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at_ns INTEGER NOT NULL
);
```

That's the entire initial migration. One table. It proves the store can create and version a database. Everything else comes when code needs it.

### 5.6 `store.rs`

```rust
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, StoreOpenError> { ... }
    pub fn open_in_memory() -> Result<Store, StoreOpenError> { ... }
    pub fn initialize(&self) -> Result<(), StoreError> { ... }
    pub fn schema_version(&self) -> Result<i64, StoreError> { ... }
    pub fn is_initialized(&self) -> bool { ... }
    pub fn health_check(&self) -> Result<(), StoreError> { ... }
}
```

Key decisions:
- `open_in_memory()` for tests. No `:memory:` path hackery in tests.
- `open()` takes a path. It does not resolve XDG dirs.
- `initialize()` runs pending migrations. Idempotent.
- `is_initialized()` checks if schema_migrations table exists.
- `health_check()` runs `SELECT 1`. Simple, fast, proves connection is alive.

### 5.7 SQLite Pragmas

Applied on every connection in `open()`:

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
PRAGMA synchronous=NORMAL;
```

### 5.8 Error Model

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreOpenError {
    #[error("cannot create parent directory for {path}: {source}")]
    CreateDir { path: String, source: std::io::Error },
    #[error("cannot open database at {path}: {source}")]
    Open { path: String, source: rusqlite::Error },
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("migration v{version} failed: {source}")]
    Migration { version: i64, source: rusqlite::Error },
    #[error("database health check failed: {source}")]
    HealthCheck { source: rusqlite::Error },
    #[error("query failed: {source}")]
    Query { source: rusqlite::Error },
}
```

Two error types: `StoreOpenError` (construction can fail for different reasons than operation) and `StoreError` (operation errors). This separates "couldn't open" from "couldn't query" — different failure modes, different recovery paths.

### 5.9 Tests

- `Store::open_in_memory()` succeeds
- `Store::open_in_memory()` + `initialize()` creates schema_migrations
- `Store::schema_version()` returns 1 after migration
- Running `initialize()` twice is idempotent (no error, version stays 1)
- `Store::open("/nonexistent/path/db.sqlite")` returns `StoreOpenError::CreateDir`
- `Store::health_check()` returns `Ok(())` on healthy DB
- `Store::is_initialized()` returns `false` before `initialize()`, `true` after

---

## 6. `twin-app`

### 6.1 Purpose

Application workflows. Owns result types, path resolution, and orchestration. The only crate that knows about XDG dirs and business logic sequencing.

### 6.2 Dependencies

- `twin-core`
- `twin-store`
- `dirs` (XDG path resolution)
- `toml` (config serialization)
- `serde` (config deserialization)

### 6.3 Files (implemented)

```
crates/twin-app/src/
  lib.rs
  error.rs
  paths/
    mod.rs
    layout.rs
    xdg.rs
  config_io/mod.rs
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
crates/twin-app/tests/
  support/mod.rs
  init.rs
  doctor.rs
```

Full tree: `docs/code-layout.md`.

### 6.4 `TwinLayout` — path resolution (moved from twin-store)

Path resolution is an application concern. Commands take `&TwinLayout`; the CLI uses `TwinLayout::from_xdg()`. Tests use `TwinLayout::isolated(tempdir)` — no global mutexes or env-var hooks in production code.

```rust
pub struct TwinLayout { pub data_dir, pub config_dir, pub state_dir: PathBuf }

impl TwinLayout {
    pub fn from_xdg() -> Result<Self, PathError>;
    pub fn isolated(base: &Path) -> Self;
    pub fn config_file(&self, override_path: Option<&Path>) -> PathBuf;
    pub fn db_file(&self) -> PathBuf;
    pub fn log_file(&self) -> PathBuf;
    pub fn ensure_dirs(&self) -> io::Result<()>;
}

pub struct InitRequest { pub force: bool, pub config_override: Option<PathBuf> }
pub fn init(request: InitRequest) -> Result<InitResult, AppError>;
pub fn init_in(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, AppError>;
pub fn doctor_in(layout: &TwinLayout, config_override: Option<&Path>) -> Result<DoctorResult, AppError>;
```

XDG on Linux: `~/.local/share/twin/`, `~/.config/twin/`, `~/.local/state/twin/`.

### 6.5 `model/` — command result types

Results live in `twin-app/src/model/` (`init_result.rs`, `doctor_result.rs`), not in a separate output crate. `InitResult` includes `config_created`, `config_updated`, and `db_created`.

Why `Serialize` on result types? Because `--json` is a CLI rendering concern, and the simplest way to support it is to serialize the result type. No intermediate "report" type needed. The same struct serves both human and JSON output.

Why `PathBuf` instead of `Option<String>`? `PathBuf` is the natural Rust type for file paths. Forcing `String` loses type safety and creates `.display()` calls everywhere.

Why `Option<usize>` for process counts? `doctor` may not be able to count processes (e.g., `/proc` not mounted). `None` is the honest representation. `0` means "we checked and found zero."

### 6.6 `commands/init.rs` + `config_io`

`config_io::sync_default_template` handles create / force / refresh-when-still-default-values.

Logic:
1. `layout.ensure_dirs()`
2. Sync config at `layout.config_file(override)`
3. Open DB at `layout.db_file()`, `store.initialize()` if needed
4. Return `InitResult` (includes `config_created`, `config_updated`, `db_created`)

Key behaviors:
- Second `init` without `--force`: no overwrite if config unchanged
- `--force`: rewrite template
- Stale template with default retention values: refresh comments/format without `--force`
- DB already initialized: skip migration, `db_created: false`

### 6.7 `commands/doctor/`

```rust
pub fn run(layout: &TwinLayout, config_override: Option<&Path>) -> Result<DoctorResult, AppError>
```

Specific checks:

| Check | How | Graceful On Failure |
|-------|-----|---------------------|
| CLI | Always OK (we're running) | N/A |
| Config file | `std::fs::metadata(config_path)` | → `config_found: false` |
| Database | `Store::open` + `health_check` + `schema_version` | → `initialized: false` |
| WAL mode | `PRAGMA journal_mode` query | → `wal_mode: None` |
| Permission mode | `std::process::Command::new("id").arg("-u")` or `nix::geteuid()` | → `Unprivileged` |
| `/proc` access | `std::fs::read_dir("/proc")` | → `proc_accessible: false` |
| Readable processes | Count entries in `/proc` that we can `stat` | → `None` if `/proc` inaccessible |
| Restricted processes | Total `/proc` entries minus readable | → `None` if `/proc` inaccessible |

Doctor must never fail hard. Every check degrades to a best-effort result. If `/proc` isn't mounted (containers, weird systems), doctor reports what it can and fills the rest with `None`.

### 6.8 Error Model

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("init failed: {0}")]
    Init(#[from] InitError),
    #[error("doctor check failed: {0}")]
    Doctor(#[from] DoctorError),
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("cannot create directories: {0}")]
    Directory(#[source] std::io::Error),
    #[error("cannot write config: {0}")]
    ConfigWrite(#[from] ConfigError),
    #[error("cannot open database: {0}")]
    Database(#[from] StoreOpenError),
    #[error("cannot initialize database: {0}")]
    Migration(#[from] StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error("cannot open database: {0}")]
    Database(#[from] StoreOpenError),
    #[error("cannot query database: {0}")]
    Query(#[from] StoreError),
}
```

Every error is a named variant with typed source. No `String` error variants. Callers can match on specific failure modes and take specific action.

`AppError` composes via `From` impls. The CLI layer only sees `AppError` and renders it.

### 6.9 Tests

Integration tests in `crates/twin-app/tests/` with `support::IsolatedHome` (`TempDir` + `TwinLayout::isolated`). No real `~/.config/twin` writes in CI.

Coverage includes: first init creates files; idempotent second init; `--force`; stale-default template refresh; customized config left alone; doctor before/after init; config roundtrip.

See `docs/code-layout.md` § Testing conventions.

---

## 7. `twin-cli`

### 7.1 Purpose

Thin shell: parse args, call `twin-app`, render result. Zero business logic.

### 7.2 Dependencies

- `twin-app`
- `clap` (features: `derive`, `env`)

No `serde_json` dependency here. `twin-app` result types already derive `Serialize`. The CLI uses `serde_json::to_string_pretty` for `--json`. Add `serde_json` as a dep of `twin-cli`, or expose a `to_json()` convenience on result types.

### 7.3 Files (implemented)

```
crates/twin-cli/src/
  lib.rs
  main.rs
  cli/
    mod.rs
    args.rs
  output/
    mod.rs
    init.rs
    doctor.rs
    json.rs
crates/twin-cli/tests/
  output.rs
```

`lib.rs` exposes `cli` and `output` for integration tests. When output grows (slice 5+), consider extracting `twin-output`.

### 7.4 `args.rs`

```rust
use clap::{Parser, Subcommand, Args};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "twin", version, about = "Read-only operational twin for Linux")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct GlobalArgs {
    #[arg(long, global = true, help = "Output as JSON")]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
}

#[derive(Args)]
pub struct InitArgs {
    #[arg(long, help = "Overwrite existing config")]
    pub force: bool,
}

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,
}
```

Why only `--json` in global args? Because `--verbose` and `--no-color` aren't needed by init/doctor. They're not used yet. Add them when a command needs them.

Why no `--config` on `init`? Init creates the config. The config path is resolved from XDG dirs. Adding `--config` to init means "create config at an arbitrary path" which breaks XDG assumptions. If we need this later, add it. YAGNI.

Actually, on reflection, `--config` on `init` is useful for overriding the default path, same as `doctor`. Let me add it:

```rust
#[derive(Args)]
pub struct InitArgs {
    #[arg(long, help = "Overwrite existing config")]
    pub force: bool,

    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,
}
```

### 7.5 `render.rs`

```rust
use std::fmt;
use twin_app::result::{InitResult, DoctorResult, PermissionMode};

pub fn render_init(result: &InitResult) -> String {
    let status = if result.config_created || result.db_created {
        "Created"
    } else {
        "Already exists"
    };
    format!(
        "Twin init: {status}\n\n\
         Config: {}\n\
         Database: {}\n\
         Log: {}",
        result.config_path.display(),
        result.db_path.display(),
        result.log_path.display(),
    )
}

pub fn render_doctor(result: &DoctorResult) -> String {
    // ... structured human-readable output
}

pub fn render_json<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}
```

This is a flat `render` module, not a trait. No `Renderer` trait until there are enough formats to justify the abstraction. Three functions is enough.

### 7.6 `main.rs`

```rust
use std::process;

fn main() {
    let cli = args::Cli::parse();
    let result = run(cli);
    process::exit(result);
}

fn run(cli: args::Cli) -> i32 {
    match cli.command {
        args::Command::Init(args) => run_init(&cli.global, &args),
        args::Command::Doctor(args) => run_doctor(&cli.global, &args),
    }
}

fn run_init(global: &args::GlobalArgs, args: &args::InitArgs) -> i32 {
    match twin_app::init(args.force, args.config.as_deref()) {
        Ok(result) => {
            let output = if global.json {
                render::render_json(&result).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
            } else {
                render::render_init(&result)
            };
            println!("{output}");
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn run_doctor(global: &args::GlobalArgs, args: &args::DoctorArgs) -> i32 {
    // Similar pattern
}
```

Why integer exit codes in `run`? `main()` returns `!` so we use an explicit exit code. The match on Ok/Err is explicit. Errors always go to stderr, output always to stdout.

Why `e` formatting in JSON error fallback? This is a last-resort path. If `serde_json` fails to serialize a result type that derives `Serialize`, that's a programming error, and the user needs to know something went wrong.

### 7.7 Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error (any AppError) |

No specialized codes yet. When we have more commands and more error types, we may add codes 2, 3, etc. Two codes for two outcomes is sufficient.

### 7.8 Tests

- Integration test: run `init` in temp dir, assert `config.toml` and `twin.db` exist
- Integration test: run `init` twice, assert config not overwritten
- Integration test: run `init --force`, assert config overwritten
- Integration test: run `doctor` before `init`, assert "not initialized" in output
- Integration test: run `doctor --json`, parse output as valid JSON
- Unit test: `render_init` produces expected string
- Unit test: `render_doctor` produces expected string
- Unit test: `render_json` roundtrip for `InitResult`

Integration tests use `assert_cmd` crate for CLI testing or call `twin_app::init()` / `twin_app::doctor()` directly with temp dirs.

---

## 8. Workspace Setup

### 8.1 Root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/twin-cli",
    "crates/twin-app",
    "crates/twin-core",
    "crates/twin-store",
]
resolver = "2"

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
twin-core = { path = "crates/twin-core" }
twin-store = { path = "crates/twin-store" }
twin-app = { path = "crates/twin-app" }
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "1"
dirs = "5"
uuid = { version = "1", features = ["v4"] }
tempfile = "3"
```

Workspace dependencies ensure version consistency. Crate `Cargo.toml` files reference `workspace = true` for inherited fields and `workspace.dependencies` for shared crate versions.

### 8.2 Binary Naming

In `crates/twin-cli/Cargo.toml`:

```toml
[[bin]]
name = "twin"
path = "src/main.rs"
```

The crate is `twin-cli` (matches workspace convention), but the binary is `twin` (matches user-facing command).

---

## 9. Implementation Sequence

Build bottom-up, verify after each step.

### Step 1: Workspace + `twin-core`

1. Create root `Cargo.toml` with workspace
2. `cargo init --lib crates/twin-core`
3. Implement `config/` (`retention.rs`, `template.rs`) and `error.rs`
4. Add `tests/config.rs` (roundtrip, template)
5. `cargo test -p twin-core && cargo clippy -p twin-core -- -D warnings`

### Step 2: `twin-store`

1. `cargo init --lib crates/twin-store`
2. Implement `store/` submodules and `migration/`
3. Add `tests/store.rs`
4. `cargo test -p twin-store && cargo clippy -p twin-store -- -D warnings`

### Step 3: `twin-app`

1. `cargo init --lib crates/twin-app`
2. Implement `paths/`, `model/`, `config_io/`, `commands/`, `error.rs`
3. Add `tests/support/`, `tests/init.rs`, `tests/doctor.rs` with `TwinLayout::isolated`
4. `cargo test -p twin-app && cargo clippy -p twin-app -- -D warnings`

### Step 4: `twin-cli`

1. `cargo init` with `lib` + `bin` targets
2. Implement `cli/`, `output/`, `main.rs`
3. Add `tests/output.rs`
4. `cargo test -p twin-cli && cargo clippy -p twin-cli -- -D warnings`

### Step 5: Full Workspace

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Then manual verification:

```bash
cargo run -- init
cargo run -- doctor
cargo run -- doctor --json
cargo run -- --version
cargo run -- --help
```

---

## 10. Acceptance Criteria

From `plan-slices.md` Slice 1:

- [ ] `twin init` creates config and state dirs
- [ ] Running `twin init` twice does not overwrite config
- [ ] `twin doctor` reports config, DB, OS, permission mode
- [ ] `--json` works
- [ ] Normal CLI errors do not panic

From AGENTS.md rules:

- [ ] No `unwrap()` or `expect()` in non-test code
- [ ] No `todo!()` or `unimplemented!()` macros
- [ ] No `#![allow(dead_code)]` or `#[allow(dead_code)]`
- [ ] No `#[allow(clippy::...)]` without proven false positive and comment
- [ ] All struct fields private, public accessors where needed
- [ ] Domain types use newtypes (when they arrive in slice 3)
- [ ] Errors use `thiserror` derive, no `anyhow` in library crates
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check` passes

---

## 11. What This Slice Does NOT Include

| Excluded | Arrives In | Reason |
|----------|-----------|--------|
| NodeId, EdgeId, ObservationId, SnapshotId | Slice 3 | No graph until slice 3-4 |
| NodeKind, EdgeKind, EdgeClass | Slice 3 | No observations until slice 3 |
| RiskLevel, EvidenceStrength | Slice 8 | No impact analysis until slice 8 |
| NodeState, EdgeState | Slice 3 | No graph nodes until slice 3 |
| ObservationSource, ConfidenceHint | Slice 3 | No observation pipeline until slice 3 |
| CollectorName | Slice 4 | No collectors until slice 4 |
| observations, nodes, edges tables | Slice 2 | No data to store until slice 3 |
| twin-output crate | Slice 4-5 | Two renderers don't justify a crate |
| Path resolution in twin-store | N/A | Moved to twin-app where it belongs |
| twin-observation crate | Slice 3 | No observation pipeline yet |
| Async / tokio | Slice 13 | No watch mode or eBPF yet |
| Process /proc collectors | Slice 4 | No scan command yet |

---

## 12. Design Decisions Log

| Decision | Choice | Alternatives Considered | Rationale |
|----------|--------|------------------------|-----------|
| Output crate | No separate crate | Separate `twin-output` crate | Two command results don't justify a crate. Extract when DOT/Mermaid renderers arrive. |
| Store path discovery | App responsibility | Store discovers XDG dirs | Store is a library; paths are an app concern. Dependency inversion. |
| Initial schema | schema_migrations only | Full schema from tech doc | No code reads/writes those tables yet. Dead schema is dead code. |
| Domain types | Only what init/doctor use | All types from tech doc | AGENTS.md: dead code is a bug. Each type arrives in the slice that needs it. |
| Error model | Scoped enums with typed sources | `AppError::Config(String)` | Structured errors are matchable. String errors force callers to parse. |
| Sync only | No tokio | tokio from start | init and doctor are sync. No reason to add async complexity. |
| Config format | TOML | YAML, JSON | Human-readable, standard for Rust CLIs, supports comments. |
| SQLite backend | rusqlite + bundled | sqlx + sqlite, system sqlite | Self-contained binary, no system sqlite-dev dependency, WAL mode works reliably. |
| Binary name | `twin` | `twin-cli` | Users type `twin`, not `twin-cli`. The crate is internal naming. |
| CLI framework | clap derive | structopt, argh, hand-rolled | Most widely used, derive macro reduces boilerplate, good completion support. |
| Process counting in doctor | Best-effort with graceful degradation | Skip on error, require /proc | Not all systems have /proc. Doctor reports what it can. |
| Module layout | `commands/`, `model/`, `paths/`, subdirs per crate | Flat `init.rs` / `paths.rs` | Clear boundaries; room to grow without monoliths. |
| Test layout | `crates/*/tests/` + `TwinLayout` injection | Inline `mod tests`, global `TestDirGuard` | No test hooks in production; parallel-safe, explicit deps. |

---

## 13. File list (implemented)

Canonical tree: **`docs/code-layout.md`**. Summary:

```text
twin-core:     src/config/{mod,retention,template}.rs  tests/config.rs
twin-store:    src/{store/,migration/}                   tests/store.rs
twin-app:      src/{paths/,config_io/,commands/,model/} tests/{support/,init,doctor}.rs
twin-cli:      src/{cli/,output/,lib,main}.rs            tests/output.rs
```

---

## 14. Default Config Content

The config that `twin init` writes (if missing):

```toml
# twin configuration

[retention]
raw_observations_days = 7
graph_history_days = 30
ebpf_events_days = 3
```

This is intentionally minimal. New sections appear when features that need them arrive:
- `[collectors]` in slice 4
- `[ebpf]` in slice 14
- Additional retention fields in slice 12

Config is additive. Missing fields get defaults from `TwinConfig::default()`. This means upgrading twin never breaks an existing config file.