# Slice 2: SQLite Store and Migration System

## Status: complete

## Implemented

- `MIGRATION_002`: `collector_runs`, `nodes`, `observations`, `edges`, `edge_observations` (tables created in FK-safe order)
- `LATEST_VERSION` = 2
- Repository layer: `ObservationRow`, `NodeRow`, `EdgeRow`, `CollectorRunRow` + CRUD/upsert/list/count
- `edge_observations` link/list
- `Store::transaction()` RAII guard (`DerefMut` to `Store` for in-txn ops)
- `StoreError` variants: `Insert`, `Upsert`, `TransactionBegin`, `TransactionCommit`
- Integration tests: `migration.rs`, `repo.rs`, `transaction.rs`, `tests/support/`
- `InitResult.schema_version`; `twin init` always runs `initialize()` (applies pending migrations)
- CLI init output shows schema version
- Doctor test expects schema v2 after init

## Blocked

_(nothing)_

## Decisions

- Row structs use plain String/i64, no newtypes — domain types arrive in slice 3
- metadata_json as TEXT DEFAULT '{}' — avoids schema churn
- Transaction is RAII guard with explicit `commit()`; rollback on drop without commit
- repo/ module separate from store/ — domain storage vs infrastructure
- collector_runs.id is AUTOINCREMENT (local-only); observations use UUID TEXT PK
- Node upsert: `ON CONFLICT DO UPDATE` preserves `first_seen_ns` / `valid_from_ns`
- Edge upsert: row values win on conflict (`excluded.evidence_count`); caller sets counts
- MIGRATION_002 table order: collector_runs → nodes → observations → edges → edge_observations (plan SQL had observations before collector_runs; reordered for FK)
- No twin-core changes in this slice

## Deviations from Plan

- Table creation order in `MIGRATION_002` reordered so `collector_runs` exists before `observations` FK
- `twin init` now always calls `store.initialize()` (not only on first init) so existing v1 DBs migrate to v2
- Post-slice hardening: `is_initialized() -> Result`, FK tests use `SQLITE_CONSTRAINT_FOREIGNKEY`, bulk rollback test, `with_transaction` for nested txns, edge evidence from caller row not SQL `+1`, tests use `LATEST_VERSION` not magic `2`

## Acceptance criteria

- [x] DB is created by `twin init`
- [x] Migrations are idempotent
- [x] `twin doctor` detects schema version
- [x] Corrupt or missing DB gives clean errors (unchanged from slice 1)
- [x] Tests can create temporary DBs
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --check` passes
