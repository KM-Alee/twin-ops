# Slice 2 Implementation Plan: SQLite Store and Migration System

## 1. Overview

This slice makes `twin-store` a real persistence layer. Slice 1 shipped a store that could open a database, run one migration (the `schema_migrations` table), and check health. Slice 2 adds the six tables that observations, nodes, edges, and collector runs need — and the repository methods to insert and query them.

The user-facing outcome: `twin init` creates a fully schema'd database, and `twin doctor` reports the new schema version.

```bash
twin init
twin doctor
twin doctor --json
```

---

## 2. Scope — What This Slice Does

| Area | Detail |
|------|--------|
| **New migration** | `MIGRATION_002`: creates `observations`, `nodes`, `edges`, `edge_observations`, `collector_runs` |
| **Store methods** | Repository layer: insert/query/watch for each table |
| **Doctor update** | `DoctorDatabase.schema_version` already points here; new version (2) will appear after `init` |
| **Transaction wrapper** | `Store::transaction()` for atomic batch writes |
| **Tests** | Integration tests for every repository method, migration idempotency, transaction rollback |

---

## 3. Scope — What This Slice Does NOT Do

| Excluded | Arrives In | Reason |
|----------|-----------|--------|
| Domain types (NodeId, EdgeKind, etc.) | Slice 3 | No graph until slice 3-4. The store stores rows; domain logic lives in `twin-core`. |
| Observation pipeline (collect → redact → normalize) | Slice 3 | The store is a sink, not a pipeline. |
| Graph engine / in-memory graph | Slices 4-7 | The store persists; the graph engine loads. |
| `twin-observation` crate | Slice 3 | No observation types to emit yet. |
| Collector implementations | Slice 4 | No real `/proc` reading. |
| Async | Slice 13 | All store ops are sync for now. |
| WAL-monitoring metrics | Slice 13+ | YAGNI until watch mode. |
| Snapshot tables (`snapshots`, `snapshot_nodes`, `snapshot_edges`) | Slice 12 | No snapshot feature until temporal history. |
| `node_history`, `edge_history` tables | Slice 12 | Temporal history arrives later. |
| `emulation_runs`, `test_runs` | Slices 9, 17 | No emulation or test features yet. |

---

## 4. Design Decisions

### 4.1 Domain IDs are stored as TEXT, not added as newtypes yet

Slice 3 introduces `NodeId`, `EdgeId`, `ObservationId`, etc. as newtypes in `twin-core`. Slice 2 defines the *storage shape* — `TEXT PRIMARY KEY` columns — but the store API uses plain `String` for now. When slice 3 lands, the store gains conversion functions that accept/return the newtyped IDs.

Rationale: AGENTS.md says "Dead code = bug." Defining `NodeId` now with no code that uses it is dead code. The store's repository methods take strings in this slice; slice 3 replaces them with typed wrappers.

### 4.2 Observation schema stores raw_ref as TEXT, not a foreign key

The `raw_ref` column stores a reference to the evidence source (e.g., `/proc/1234/status`). It is a descriptive string for provenance, not a foreign key into another table. This avoids premature normalization.

### 4.3 edge_observations uses a composite primary key

`(edge_id, observation_id)` is the natural key. An observation can support multiple edges, and an edge can have multiple supporting observations. The `role` column qualifies each link.

### 4.4 JSON metadata is TEXT, not a separate table

Node/edge metadata and observation metadata are stored as JSON strings in `metadata_json` columns. This avoids creating dozens of metadata columns that vary by `NodeKind` / `ObservationKind`. The `metadata_json` field is always valid JSON (or `"{}"` as default). When slice 3 introduces typed observation structs, they will serialize to this column.

Rationale: Rapid iteration on node/observation shapes without ALTER TABLE per new field. JSON inside SQLite is queryable via `json_extract()` when needed. The store offers no opinion on the JSON schema — that's the domain layer's job.

### 4.5 Observations use a UUID TEXT primary key

Slice 3 will bring `ObservationId(Uuid)`. The store uses `TEXT` storing the hyphenated UUID. This avoids an integer autoincrement that would be meaningless across scan batches and machines.

### 4.6 collector_runs tracks batch provenance

Every scan or watch tick produces a `CollectorRun`. It records which collector ran, when, how long, and whether it succeeded. This gives us:

- Provenance for every observation (`collector_run_id` FK)
- Engine for "what changed" queries (slice 12)
- Coverage gap tracking (which collectors failed)

### 4.7 Transaction wrapper is explicit, not implicit

`Store::transaction()` returns a guard that commits on drop and rolls back on panic. Callers opt in. No auto-transaction magic.

### 4.8 No `twin-observation` crate yet

The observation pipeline (collect → redact → normalize) is slice 3. The store just has a table to receive rows. No new crate in this slice.

### 4.9 Timestamps are nanosecond i64, notchrono

`TimestampNs` is `i64` — nanoseconds since Unix epoch. This is consistent with the tech document's `first_seen`, `last_seen`, `valid_from`, `valid_to` fields. Chrono is not a dependency. `std::time` provides what we need.

---

## 5. Schema: `MIGRATION_002`

```sql
-- schema_migrations already exists from MIGRATION_001

CREATE TABLE IF NOT EXISTS observations (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject_node_id TEXT,
    object_node_id TEXT,
    timestamp_ns INTEGER NOT NULL,
    confidence_hint TEXT NOT NULL DEFAULT 'moderate',
    redaction_state TEXT NOT NULL DEFAULT 'none',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    collector_run_id INTEGER,

    FOREIGN KEY (collector_run_id) REFERENCES collector_runs(id)
);

CREATE INDEX IF NOT EXISTS idx_observations_source ON observations(source);
CREATE INDEX IF NOT EXISTS idx_observations_kind ON observations(kind);
CREATE INDEX IF NOT EXISTS idx_observations_subject ON observations(subject_node_id);
CREATE INDEX IF NOT EXISTS idx_observations_timestamp ON observations(timestamp_ns);

CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL,
    valid_from_ns INTEGER NOT NULL,
    valid_to_ns INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_state ON nodes(state);
CREATE INDEX IF NOT EXISTS idx_nodes_last_seen ON nodes(last_seen_ns);

CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    class TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    evidence_score INTEGER NOT NULL DEFAULT 0,
    evidence_label TEXT NOT NULL DEFAULT 'weak',
    evidence_count INTEGER NOT NULL DEFAULT 0,
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',

    FOREIGN KEY (from_node_id) REFERENCES nodes(id),
    FOREIGN KEY (to_node_id) REFERENCES nodes(id)
);

CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
CREATE INDEX IF NOT EXISTS idx_edges_class ON edges(class);
CREATE INDEX IF NOT EXISTS idx_edges_state ON edges(state);

CREATE TABLE IF NOT EXISTS edge_observations (
    edge_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'direct',

    PRIMARY KEY (edge_id, observation_id),

    FOREIGN KEY (edge_id) REFERENCES edges(id),
    FOREIGN KEY (observation_id) REFERENCES observations(id)
);

CREATE INDEX IF NOT EXISTS idx_edge_obs_role ON edge_observations(role);

CREATE TABLE IF NOT EXISTS collector_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collector TEXT NOT NULL,
    started_at_ns INTEGER NOT NULL,
    ended_at_ns INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'success',
    observation_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,

    FOREIGN KEY () -- no FK; collector_runs is self-contained
);

CREATE INDEX IF NOT EXISTS idx_collector_runs_collector ON collector_runs(collector);
CREATE INDEX IF NOT EXISTS idx_collector_runs_started ON collector_runs(started_at_ns);
```

### Design notes on the schema

- **`observations.subject_node_id` / `object_node_id`** are nullable because some observations (e.g., `EbpfExec`) may not have both subject and object.
- **`nodes.valid_to_ns`** is nullable because active nodes have no end time.
- **`edges.evidence_score`** is `INTEGER` (0-100) following the tech document's evidence scoring model.
- **`edges.class`** holds `observed`, `inferred`, or `predicted` (from `EdgeClass` in the tech document).
- **`collector_runs.status`** is `success`, `partial`, or `failed`. A `partial` run produced some observations but also errors.
- **`edge_observations.role`** is `direct`, `supporting`, `conflicting`, `historical`, `runtime_confirmation`, or `static_confirmation`.
- **`metadata_json` columns** default to `'{}'`. When slice 3 adds typed structs, they serialize into this column.

---

## 6. Repository Layer

### 6.1 Module layout (within `twin-store`)

```
crates/twin-store/src/
  lib.rs
  error.rs
  migration/
    mod.rs              # MIGRATION_001, MIGRATION_002, LATEST_VERSION
  store/
    mod.rs              # Store struct (unchanged)
    open.rs             # open, open_in_memory, pragmas (unchanged)
    migrate.rs          # initialize, schema_version, is_initialized (unchanged)
    health.rs           # health_check, journal_mode_wal (unchanged)
    transaction.rs      # NEW: begin/commit/rollback wrapper
  repo/
    mod.rs              # pub mod declarations
    observation.rs      # NEW: insert, query, list
    node.rs             # NEW: upsert, get, list, delete_stale
    edge.rs             # NEW: insert, get, list by from/to, delete_stale
    edge_observation.rs # NEW: link, list for edge
    collector_run.rs    # NEW: insert, list, latest
```

### 6.2 Why `repo/` alongside `store/`

`store/` owns the `Connection` and生命周期 concerns (open, migrate, health, transaction). `repo/` owns the CRUD operations on domain tables. This splits the "how do I connect" concern from the "what rows do I write" concern.

### 6.3 Repository method signatures

All methods take `&self` (shared reference, since `rusqlite::Connection` supports shared read via `Mutex` or will use `&mut` for writes). For now, writes use `&mut self` on the `Store` or a `Transaction` guard.

**transaction.rs**

```rust
pub struct Transaction<'a> {
    store: &'a mut Store,
    committed: bool,
}

impl Store {
    pub fn transaction(&mut self) -> Result<Transaction<'_>, StoreError> { ... }
}

impl<'a> Transaction<'a> {
    pub fn commit(mut self) -> Result<(), StoreError> { ... }
}

impl<'a> Drop for Transaction<'a> {
    // ROLLBACK if not committed
}
```

**observation.rs**

```rust
pub struct ObservationRow {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub subject_node_id: Option<String>,
    pub object_node_id: Option<String>,
    pub timestamp_ns: i64,
    pub confidence_hint: String,
    pub redaction_state: String,
    pub metadata_json: String,
    pub collector_run_id: Option<i64>,
}

impl Store {
    pub fn insert_observation(&mut self, obs: &ObservationRow) -> Result<(), StoreError>;
    pub fn insert_observations(&mut self, obs: &[ObservationRow]) -> Result<(), StoreError>;
    pub fn get_observation(&self, id: &str) -> Result<Option<ObservationRow>, StoreError>;
    pub fn list_observations_by_source(&self, source: &str) -> Result<Vec<ObservationRow>, StoreError>;
    pub fn list_observations_by_subject(&self, node_id: &str) -> Result<Vec<ObservationRow>, StoreError>;
    pub fn count_observations(&self) -> Result<i64, StoreError>;
}
```

**node.rs**

```rust
pub struct NodeRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub first_seen_ns: i64,
    pub last_seen_ns: i64,
    pub valid_from_ns: i64,
    pub valid_to_ns: Option<i64>,
    pub metadata_json: String,
}

impl Store {
    pub fn upsert_node(&mut self, node: &NodeRow) -> Result<(), StoreError>;
    pub fn upsert_nodes(&mut self, nodes: &[NodeRow]) -> Result<(), StoreError>;
    pub fn get_node(&self, id: &str) -> Result<Option<NodeRow>, StoreError>;
    pub fn list_nodes_by_kind(&self, kind: &str) -> Result<Vec<NodeRow>, StoreError>;
    pub fn count_nodes(&self) -> Result<i64, StoreError>;
    // slice 12 adds: mark_stale, delete_older_than
}
```

`upsert` uses `INSERT OR REPLACE` (SQLite UPSERT). If a node already exists, `last_seen_ns` and `metadata_json` update. `first_seen_ns` and `valid_from_ns` are preserved from the existing row.

**edge.rs**

```rust
pub struct EdgeRow {
    pub id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub kind: String,
    pub class: String,
    pub state: String,
    pub evidence_score: i64,
    pub evidence_label: String,
    pub evidence_count: i64,
    pub first_seen_ns: i64,
    pub last_seen_ns: i64,
    pub metadata_json: String,
}

impl Store {
    pub fn upsert_edge(&mut self, edge: &EdgeRow) -> Result<(), StoreError>;
    pub fn upsert_edges(&mut self, edges: &[EdgeRow]) -> Result<(), StoreError>;
    pub fn get_edge(&self, id: &str) -> Result<Option<EdgeRow>, StoreError>;
    pub fn list_edges_from(&self, node_id: &str) -> Result<Vec<EdgeRow>, StoreError>;
    pub fn list_edges_to(&self, node_id: &str) -> Result<Vec<EdgeRow>, StoreError>;
    pub fn list_edges_by_kind(&self, kind: &str) -> Result<Vec<EdgeRow>, StoreError>;
    pub fn count_edges(&self) -> Result<i64, StoreError>;
}
```

**edge_observation.rs**

```rust
impl Store {
    pub fn link_edge_observation(&mut self, edge_id: &str, observation_id: &str, role: &str) -> Result<(), StoreError>;
    pub fn list_observations_for_edge(&self, edge_id: &str) -> Result<Vec<(String, String)>, StoreError>;
}
```

**collector_run.rs**

```rust
pub struct CollectorRunRow {
    pub id: Option<i64>,
    pub collector: String,
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub status: String,
    pub observation_count: i64,
    pub warning_count: i64,
    pub error_message: Option<String>,
}

impl Store {
    pub fn insert_collector_run(&mut self, run: &CollectorRunRow) -> Result<i64, StoreError>;
    pub fn list_collector_runs(&self, collector: Option<&str>) -> Result<Vec<CollectorRunRow>, StoreError>;
    pub fn latest_collector_run(&self, collector: &str) -> Result<Option<CollectorRunRow>, StoreError>;
}
```

### 6.4 Row structs are in `twin-store`, not `twin-core`

These `*Row` structs are storage representations. They map 1:1 to SQL columns. Domain types (`NodeId`, `ObservationKind`, etc.) arrive in slice 3 in `twin-core` and will have conversion methods. This avoids slice 2 needing to define domain concepts it doesn't use yet.

### 6.5 Bulk operations

`insert_observations`, `upsert_nodes`, `upsert_edges` accept `&[Row]` and execute within a single transaction. This is how a scan batch will write to the store — one transaction per batch, not one per observation. The `Transaction` guard makes this explicit for callers.

---

## 7. Error Model Updates

Add new variants to `StoreError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    // existing
    #[error("migration v{version} failed: {source}")]
    Migration { version: i64, source: rusqlite::Error },
    #[error("database health check failed: {source}")]
    HealthCheck { source: rusqlite::Error },
    #[error("query failed: {source}")]
    Query { source: rusqlite::Error },

    // new
    #[error("insert failed: {source}")]
    Insert { source: rusqlite::Error },
    #[error("upsert failed: {source}")]
    Upsert { source: rusqlite::Error },
    #[error("transaction begin failed: {source}")]
    TransactionBegin { source: rusqlite::Error },
    #[error("transaction commit failed: {source}")]
    TransactionCommit { source: rusqlite::Error },
}
```

`StoreOpenError` stays unchanged — construction errors don't grow with new tables.

---

## 8. `twin-app` Changes

### 8.1 `commands/init.rs`

`init` already calls `store.initialize()`. After `MIGRATION_002` is added, re-running `twin init` will apply the new migration. The `InitResult` gets a new field:

```rust
pub struct InitResult {
    pub config_created: bool,
    pub config_updated: bool,
    pub db_created: bool,
    // NEW:
    pub schema_version: i64,
}
```

This way `twin init` output tells the user which schema version they have, and `twin doctor` already reports it.

### 8.2 `commands/doctor/database.rs`

`check()` already reads `schema_version` and reports it. After the new migration, `schema_version` returns `2` instead of `1`. No doctor logic changes needed.

### 8.3 New CLI access: none yet

No new commands in this slice. `twin init` and `twin doctor` are the user-facing surface. The repository methods are library-only until slice 4 adds `twin scan`.

---

## 9. Test Plan

### 9.1 Migration tests (`tests/store.rs` — extend existing)

| Test | What it proves |
|------|---------------|
| `initialize_applies_migration_002` | After `initialize()`, tables `observations`, `nodes`, `edges`, `edge_observations`, `collector_runs` exist |
| `schema_version_is_2_after_full_migration` | `schema_version()` returns 2 |
| `initialize_twice_is_idempotent` | Running `initialize()` twice still results in version 2 |
| `migration_002_from_fresh_db` | Open fresh DB, initialize, check all 6 tables exist via `SELECT COUNT(*) FROM <table>` |
| `migration_002_from_existing_v1` | Open DB that already has `schema_migrations` at v1, initialize, confirm v2 and new tables exist |

### 9.2 Repository tests (`tests/store.rs` — extend with §10 helpers)

| Test | What it proves |
|------|---------------|
| `insert_and_get_observation` | Insert one observation, get it back by ID |
| `insert_observations_bulk` | Insert 3 observations in one call, count = 3 |
| `list_observations_by_source` | Insert observations with different sources, filter by source |
| `list_observations_by_subject` | Insert observations with subjects, filter by subject_node_id |
| `upsert_node_creates` | Upsert a new node, get it back |
| `upsert_node_updates_last_seen` | Upsert same node ID twice, `last_seen_ns` updates, `first_seen_ns` preserves |
| `list_nodes_by_kind` | Insert nodes of different kinds, filter by kind |
| `upsert_edge_creates` | Insert edge, get it back |
| `upsert_edge_updates_evidence` | Upsert same edge ID, evidence_count increments |
| `list_edges_from_node` | Insert edges from node A, query `list_edges_from("A")` |
| `list_edges_to_node` | Insert edges to node B, query `list_edges_to("B")` |
| `link_edge_observation` | Link an observation to an edge, query it back |
| `insert_collector_run` | Insert a collector run, get ID back |
| `latest_collector_run` | Insert two runs, query latest by collector name |
| `transaction_commit` | Insert nodes in a transaction, commit, verify they exist |
| `transaction_rollback` | Insert nodes in a transaction, drop without commit, verify they do not exist |
| `count_functions` | `count_nodes()`, `count_edges()`, `count_observations()` match actual row counts |
| `get_nonexistent_returns_none` | `get_node("nope")` returns `None`, same for edge, observation |

### 9.3 Foreign key tests

| Test | What it proves |
|------|---------------|
| `edge_foreign_keys_enforced` | Inserting an edge with a `from_node_id` that doesn't exist in `nodes` fails with `Insert` error |
| `edge_observation_fk_enforced` | Linking an `observation_id` that doesn't exist fails |
| `observation_collector_run_fk` | Inserting an observation with a `collector_run_id` that doesn't exist fails |

These require `PRAGMA foreign_keys=ON` (already set). The test inserts bad data and asserts that `rusqlite` returns a constraint error.

### 9.4 Upsert idempotency tests

| Test | What it proves |
|------|---------------|
| `upsert_node_idempotent` | Same node ID upserted 3 times: 1 row, `last_seen_ns` = third value |
| `upsert_edge_idempotent` | Same edge ID upserted 3 times: 1 row, evidence fields update |

---

## 10. Test Helpers

Add a `tests/support/` directory in `twin-store` (convention from code-layout.md):

```
crates/twin-store/tests/
  support/
    mod.rs      # helper functions
  store.rs          # existing — extend
  migration.rs      # NEW: migration-specific tests
  repo.rs           # NEW: repository method tests
  transaction.rs    # NEW: transaction tests
```

**`support/mod.rs`** provides:

```rust
pub fn blank_store() -> Store {
    let store = Store::open_in_memory().expect("open in-memory DB");
    store.initialize().expect("run migrations");
    store
}

pub fn node_row(id: &str, kind: &str, label: &str, ts_ns: i64) -> NodeRow {
    NodeRow {
        id: id.to_string(),
        kind: kind.to_string(),
        label: label.to_string(),
        state: "active".to_string(),
        first_seen_ns: ts_ns,
        last_seen_ns: ts_ns,
        valid_from_ns: ts_ns,
        valid_to_ns: None,
        metadata_json: "{}".to_string(),
    }
}

pub fn edge_row(id: &str, from: &str, to: &str, kind: &str, ts_ns: i64) -> EdgeRow {
    EdgeRow {
        id: id.to_string(),
        from_node_id: from.to_string(),
        to_node_id: to.to_string(),
        kind: kind.to_string(),
        class: "observed".to_string(),
        state: "active".to_string(),
        evidence_score: 0,
        evidence_label: "weak".to_string(),
        evidence_count: 0,
        first_seen_ns: ts_ns,
        last_seen_ns: ts_ns,
        metadata_json: "{}".to_string(),
    }
}

pub fn observation_row(id: &str, source: &str, kind: &str, ts_ns: i64) -> ObservationRow {
    ObservationRow {
        id: id.to_string(),
        source: source.to_string(),
        kind: kind.to_string(),
        subject_node_id: None,
        object_node_id: None,
        timestamp_ns: ts_ns,
        confidence_hint: "moderate".to_string(),
        redaction_state: "none".to_string(),
        metadata_json: "{}".to_string(),
        collector_run_id: None,
    }
}

pub const TS: i64 = 1_700_000_000_000_000_000; // ~2023-11-15
```

---

## 11. Implementation Sequence

Build bottom-up, verify after each step.

### Step 1: `MIGRATION_002` in `migration/mod.rs`

1. Add `MIGRATION_002` SQL constant
2. Update `LATEST_VERSION` from `1` to `2`
3. Add `2 =>` match arm in `run_migration`
4. Run: `cargo test -p twin-store`
5. New test: `migration_002_creates_all_tables`
6. Verify: `cargo clippy -p twin-store -- -D warnings`

### Step 2: `Store::transaction()` — `store/transaction.rs`

1. Create `store/transaction.rs`
2. Implement `Transaction` struct with `commit()` and `Drop` (ROLLBACK)
3. Expose `Store::transaction(&mut self) -> Result<Transaction>`
4. Tests: commit persists, rollback discards

### Step 3: `repo/observation.rs`

1. Create `repo/mod.rs` and `repo/observation.rs`
2. Define `ObservationRow` struct, `Insert`/`Upsert` error variant in `StoreError`
3. Implement `insert_observation`, `insert_observations`, `get_observation`, `list_observations_by_source`, `list_observations_by_subject`, `count_observations`
4. Tests for all methods
5. Verify: `cargo test -p twin-store && cargo clippy -p twin-store -- -D warnings`

### Step 4: `repo/node.rs`

1. Create `repo/node.rs`
2. Define `NodeRow`, implement `upsert_node`, `upsert_nodes`, `get_node`, `list_nodes_by_kind`, `count_nodes`
3. Tests for upsert idempotency (first_seen preservation, last_seen update)
4. Foreign key test for edges referencing nonexistent nodes

### Step 5: `repo/edge.rs`

1. Create `repo/edge.rs`
2. Define `EdgeRow`, implement `upsert_edge`, `upsert_edges`, `get_edge`, `list_edges_from`, `list_edges_to`, `list_edges_by_kind`, `count_edges`
3. Tests for all methods + foreign key enforcement

### Step 6: `repo/edge_observation.rs`

1. Create `repo/edge_observation.rs`
2. Implement `link_edge_observation`, `list_observations_for_edge`
3. Test FK enforcement

### Step 7: `repo/collector_run.rs`

1. Create `repo/collector_run.rs`
2. Define `CollectorRunRow`, implement `insert_collector_run`, `list_collector_runs`, `latest_collector_run`
3. Tests

### Step 8: Update `twin-app` init result

1. Add `schema_version: i64` to `InitResult` in `model/init_result.rs`
2. Update `commands/init.rs` to populate `schema_version` from `store.schema_version()`
3. Update CLI `output/init.rs` to display schema version
4. Verify: `cargo test -p twin-app && cargo test -p twin-cli`

### Step 9: Full workspace verification

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
```

Expected `doctor --json` database section:

```json
{
  "database": {
    "initialized": true,
    "schema_version": 2,
    "db_path": "/home/user/.local/share/twin/twin.db",
    "wal_mode": true
  }
}
```

---

## 12. Acceptance Criteria

From `plan-slices.md` Slice 2:

- [ ] DB is created by `twin init`
- [ ] Migrations are idempotent
- [ ] `twin doctor` detects schema version
- [ ] Corrupt or missing DB gives clean errors
- [ ] Tests can create temporary DBs

From AGENTS.md rules (additional):

- [ ] No `unwrap()` or `expect()` in non-test code
- [ ] No `todo!()` or `unimplemented!()` macros
- [ ] No `#![allow(dead_code)]` or `#[allow(dead_code)]`
- [ ] No `#[allow(clippy::...)]` without proven false positive and comment
- [ ] Error variants carry context (no `String` error variants)
- [ ] `pub(crate)` default; export only what other crates need
- [ ] Struct fields private; accessor methods where needed
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check` passes

---

## 13. What This Slice Does NOT Ship

| Table | Slice | Reason |
|-------|-------|--------|
| `node_history` | 12 | Temporal history needs `what-changed` and snapshot features |
| `edge_history` | 12 | Same as above |
| `snapshots` | 12 | Named snapshots need a snapshot command |
| `snapshot_nodes` | 12 | Needs snapshot feature |
| `snapshot_edges` | 12 | Needs snapshot feature |
| `emulation_runs` | 9 | Needs emulation engine |
| `test_runs` | 17 | Needs test runner |
| `twin-observation` crate | 3 | Needs observation domain types |

---

## 14. Design Decisions Log

| Decision | Choice | Alternatives Considered | Rationale |
|----------|--------|------------------------|-----------|
| Row structs in `twin-store` | Plain `String`/`i64` fields, no newtypes | Reuse `twin-core` domain IDs | Domain types don't exist yet (slice 3). Pre-defining them is dead code. Row structs convert to domain types later. |
| Metadata as JSON TEXT | `metadata_json TEXT NOT NULL DEFAULT '{}'` | Separate metadata tables, JSON columns | SQLite JSON support is sufficient; avoids schema churn during early development. |
| Transaction guard | RAII guard with commit-on-drop and explicit `commit()` | Always-autocommit | Scan batches need atomic writes. RAII ensures rollback on panic. |
| Repo module separate from store module | `store/` owns connection; `repo/` owns domain ops | All in `store/` | Clear concern separation. `store/` is infrastructure; `repo/` is domain storage. |
| Bulk insert methods | `insert_observations(&[ObservationRow])` inside a transaction | One-at-a-time | Scan batches produce many observations. Individual inserts are slow. |
| `collector_runs.id` is AUTOINCREMENT | Integer PK, auto-generated | UUID PK | Collector runs are local-only, never replicated. Integer is simpler. Observations use UUID because they may be exchanged in future. |
| UPSERT for nodes/edges | `INSERT OR REPLACE` (SQLite UPSERT) | Check-then-insert | Scan updates `last_seen_ns`. UPSERT is atomic and avoids read-then-write races. |
| Foreign keys ON | `PRAGMA foreign_keys=ON` in `apply_pragmas()` | Optional FK enforcement | Data integrity is important; the store should reject orphaned edges. Tests verify enforcement. |
| No `twin-core` changes | Slice 2 doesn't touch `twin-core` | Add `NodeId` etc. now | Slice 3 introduces domain types. Adding them now creates dead types with no consumers. |

---

## 15. File List (after slice 2)

```
crates/twin-store/src/
  lib.rs                          # pub mod store, repo; re-exports
  error.rs                        # StoreError, StoreOpenError (+ Insert, Upsert, Transaction variants)
  migration/
    mod.rs                        # MIGRATION_001, MIGRATION_002, LATEST_VERSION = 2
  store/
    mod.rs                        # Store struct (unchanged)
    open.rs                       # unchanged
    migrate.rs                    # unchanged (version dispatch updated)
    health.rs                     # unchanged
    transaction.rs                # NEW: Transaction guard
  repo/
    mod.rs                        # pub mod node, edge, observation, edge_observation, collector_run
    observation.rs                # ObservationRow + CRUD
    node.rs                       # NodeRow + upsert/get/list
    edge.rs                       # EdgeRow + upsert/get/list
    edge_observation.rs            # link/list
    collector_run.rs              # CollectorRunRow + insert/list/latest

crates/twin-store/tests/
  support/
    mod.rs                        # blank_store(), node_row(), edge_row(), observation_row(), TS
  store.rs                        # existing — extend with migration v2 tests
  migration.rs                    # fresh/existing DB migration tests
  repo.rs                         # repository method tests
  transaction.rs                  # transaction commit/rollback tests

crates/twin-app/src/
  model/
    init_result.rs               # add schema_version field
  commands/
    init.rs                      # populate schema_version from store

crates/twin-cli/src/
  output/
    init.rs                      # display schema version

# No changes to:
#   crates/twin-core/
#   crates/twin-app/src/commands/doctor/
```