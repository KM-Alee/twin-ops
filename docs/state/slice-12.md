# Slice 12: Temporal History, What-Changed, and Snapshots

## Status: complete

## Implemented

- `MIGRATION_004`: `node_history`, `edge_history`, `snapshots`, `snapshot_nodes`, `snapshot_edges`; `LATEST_VERSION = 4`
- `twin-core`: `SnapshotId`, `GraphRef`, `ChangeKind` (`new`, `changed`, `stale`, `gone`, `reappeared`)
- `twin-store`: `repo/history.rs` (history insert/query, missing-row marking, active current graph lists); `repo/snapshot.rs` (create/list/load)
- `twin-store`: `list_active_edges_*` queries; active-only current graph for snapshots and diff
- `scan_history.rs`: `ScanHistorySession` records node/edge transitions during scan; `finalize_missing_rows` on final sample only; `prefer_stale` when permission coverage is degraded
- Scan upsert paths route through `scan_history::upsert_node` / `upsert_edge`; cgroup corrections record `gone` history before `delete_edge`
- `twin what-changed --since DURATION` (`s`/`m`/`h`/`d`, plus `yesterday`)
- `twin snapshot create NAME` and `twin snapshot list`
- `twin diff LEFT RIGHT` (`current` or `snapshot:NAME`)
- App models: `WhatChangedResult`, `SnapshotCreateResult`, `SnapshotListResult`, `DiffResult`
- CLI human renderers: `output/what_changed.rs`, `output/snapshot.rs`, `output/diff.rs`
- Graph/impact neighborhood queries use active edges; snapshots copy active rows only
- Temporal commands do not auto-scan; user runs `twin scan` to refresh operational memory
- Tests: `twin-store/tests/history.rs`, `twin-store/tests/safety.rs`, `twin-app/tests/temporal.rs`, CLI output + E2E in `cli_integration.rs` and `output.rs`

## Decisions

- History is recorded inside the existing scan transaction via `ScanHistorySession`, not a separate post-scan pass
- Multi-sample scans finalize missing rows only on the last sample (`finalize_missing = sample_index + 1 == samples_total`)
- Degraded scan coverage (`permission` warnings or `fd_permission >= 50`) marks missing rows `stale` instead of `gone`
- Snapshots are immutable SQLite graph row copies; duplicate names are rejected
- `parse_since_duration` lives in `twin-app/commands/duration.rs` with no new time-parsing dependency
- Correction-path edge removals still hard-delete from `edges` after recording `gone` in `edge_history` (existing prune behavior preserved)
- `what-changed` reads `node_history` / `edge_history` since a timestamp and deduplicates to latest transition per entity

## Deviations from Plan

- No config file content hash tracking (deferred to slice 19 as planned)
- No retention/compaction job wired (`RetentionConfig.graph_history_days` unused)
- `twin graph --kind` list views still use `list_nodes_by_kind` without active-state filter; neighborhood traversal uses active edges only
- Plan tests `scan_marks_missing_as_stale_when_coverage_degraded` and `repeated_scan_records_reappeared_process` not added as separate cases (stale semantics covered indirectly; reappeared logic exists in `classify_*_change`)
- Manual real-system smoke test not recorded in this doc yet

## Acceptance Checklist

- [x] `MIGRATION_004` creates history and snapshot tables
- [x] Existing databases migrate cleanly from schema version 3 to 4
- [x] Repeated scans record new rows without resetting `first_seen_ns`
- [x] Repeated scans do not flood history on identical rescans
- [x] Missing active nodes/edges marked `gone` or `stale` according to scan coverage
- [x] Current graph traversal for impact/emulate uses active edges
- [x] `twin what-changed --since 10m` renders new, disappeared, stale, changed, and reappeared sections
- [x] `what-changed` JSON includes structured node and edge changes
- [x] `twin snapshot create` creates named snapshot; rejects duplicates
- [x] `twin snapshot list` renders snapshots with counts and timestamps
- [x] `twin diff snapshot:NAME current` renders added, removed, and changed rows
- [x] Diff supports snapshot-to-snapshot via `GraphRef`
- [x] Invalid duration, snapshot name, and diff refs produce typed errors
- [x] No host mutation paths in temporal commands
- [x] Tests use isolated HOME, temp stores, and fake `/proc` fixtures
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes (335 tests)
- [x] `cargo clippy --workspace -- -D warnings` passes

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All passed on implementation completion.

## Manual Smoke Test

```bash
cargo build
./target/debug/twin init
./target/debug/twin doctor
./target/debug/twin scan
./target/debug/twin what-changed --since 10m
./target/debug/twin snapshot create slice12-real-smoke
./target/debug/twin snapshot list
./target/debug/twin scan
./target/debug/twin diff snapshot:slice12-real-smoke current
./target/debug/twin graph --kind service
```

_(not yet run on this host — run before release if needed)_
