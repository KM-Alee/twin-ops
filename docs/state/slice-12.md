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
- Duration parsing lives in `twin-app/commands/duration.rs`; chrono parses RFC3339 graph timestamps.
- Correction-path edge removals still hard-delete from `edges` after recording `gone` in `edge_history` (existing prune behavior preserved)
- `what-changed` reads `node_history` / `edge_history` since a timestamp and deduplicates to latest transition per entity
- `scan_history::finalize_missing_rows` computes the candidate set itself and calls `mark_nodes_missing` / `mark_edges_missing`. Host scans never observe Kubernetes objects, so rows whose metadata source is `k8s` and edges touching those rows are excluded from finalization. Without this, any command that refreshes the host scan staled the whole k8s graph (see slice 23)

## Deviations from Plan

- No config file content hash tracking (deferred to slice 19 as planned)
- Scan retention is wired and tested; snapshots/current graph remain outside history pruning.
- Graph listings default to active state; explicit state, label, and limit filters are supported.
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

This real-host sequence is a suggested release check, not a recorded result. Automated fixture CLI coverage exercises temporal commands.

## 2026-10-08 follow-up: History summaries, retention, and historical graph

what-changed uses the preceding entity history row (timestamp and row-ID tie order) and shares diff field summaries. Pruned predecessors are explicitly unavailable. eBPF-created nodes and edges record history. Retention tests cover independent source cutoffs, equality boundaries, edge-link cleanup, preserved snapshots/current graph, idempotence, and configured scan retention. graph --at accepts RFC3339 or elapsed duration, reconstructs latest retained history per entity, and never falls back to current rows. Linked observations may be pruned; output identifies retained-history scope. chrono is used for RFC3339 parsing. Domain-specific restart/upgrade categories remain deferred. Fixture CLI temporal smoke tests pass; no new real-host smoke result is claimed.

## Additional 2026-10-08 follow-up

Known metadata changes now summarize package version, config fingerprint, systemd active/substate, pod readiness, and ready endpoints with before/after values. Arbitrary metadata values are not dumped. Package-version observation is implemented for dpkg, pacman, and RPM SQLite. Service re-observation preserves previously observed runtime metadata instead of generating artificial reset/change history. Domain-level restarted-service categories remain unimplemented; no restart is inferred from a PID change alone.

## Domain summaries — 2026-10-08

Status: implemented. `commands/operational_changes.rs` derives `OperationalChange` entries from all retained transitions in the requested window. `ChangeCategory` lives in twin-core. Existing lifecycle sections remain intact. Each domain entry includes canonical entity ID, history row/run, timestamp, safe observation summary, observed/inferred class, and the shared core evidence score. Config fingerprints/package versions require retained predecessors. Multiple changes, including rollback, remain visible. Listener transitions replay edge state at scan boundaries and preserve shared ownership. Peer endpoints from proc data do not prove outbound initiation; they are inferred. Changed systemd invocation identities imply likely restart/reactivation, with the cause explicitly unknown. No restart is inferred from a label or arbitrary metadata change. Missing identity/predecessor means no restart claim.

Tests: temporal.rs covers all domain categories and repeated package transitions; output.rs locks terminal sections and sourced observations. The D-Bus collector projects only a nonzero 16-byte InvocationID as safe identity metadata and preserves it on service rows. Missing runtime properties do not erase previously observed identity.


Restart summaries additionally validate both retained InvocationID values as nonzero 16-byte hexadecimal identities; malformed stored values cannot establish a restart inference.


Continuation: source finalization now considers stale rows as unresolved candidates. A later complete scan can transition stale nodes/edges to gone exactly once; repeated incomplete scans do not manufacture additional stale history. Active-only snapshot/diff selection is unchanged. Store tests cover unfinalized selection and idempotent transitions. A temporal regression also covers empty, zero, wrong-length and non-hex InvocationIDs on either side of a comparison.

## 2026-10-08 operator incident timeline

Implemented `what-changed --target SERVICE_OR_NODE --category CATEGORY --limit N --max-depth N`.
Target scoping follows explicit dependency/ownership/config/listener/library/container/Kubernetes links in both current/stale rows and retained window history, so vanished relationships still associate changes with the workload. Parent-process, cgroup and mount fan-out is excluded. Depth is bounded 1–4 (default 2). Canonical IDs or uniquely resolved service names are accepted; unknown targets fail rather than returning global results.

Operational changes now render chronologically with RFC3339 UTC timestamps. The newest 50 entries are retained by default (maximum 10000), with an explicit earlier-entry omission count; `--verbose` only expands generic lifecycle sections. Repeatable snake_case category filters select operational entries and suppress unrelated generic sections in human output. JSON retains scoped generic lifecycle sections alongside the explicit category filter. Coverage unknowns remain visible under filters. This is a temporal association, never proof that one change caused another.

Fixture acceptance: gone ownership edges keep process events attributable; unrelated processes are excluded; chronological order/category selection/newest-entry truncation are tested. CLI parsing rejects unknown categories, depth >4 and invalid limits. Terminal rendering checks the timestamp and evidence source.

## 2026-10-09 namespace topology review

Process socket collection reads unique observed network namespaces through `/proc/PID/net` without `setns` or host changes. Namespace socket ownership joins only descriptors from processes in that namespace; identical endpoint addresses and inode numbers in different namespaces remain separate. Scanner namespace endpoints retain existing host IDs; other internet endpoints carry `:netns:INODE`, and Unix endpoints use `unix:netns:INODE:PATH`. Namespace identities are reread after collection; changed/vanished identities discard the socket sample and record a coverage gap. Table warning and evidence references identify the actual PID namespace source. FD observations include namespace identity.

Wildcard listeners support inferred local dependencies only for loopback or concrete local addresses observed in the same namespace, never arbitrary remote internet peers. Namespaced edges retain the normal relation-specific inference metadata and observation links; Unix sockets do not acquire TCP metadata. Missing namespace/table evidence preserves uncertainty rather than asserting disappearance. Fixture verification: `twin-app/tests/network_namespaces.rs` covers duplicate endpoints/inodes, lost identity, host/Unix separation and remote wildcard rejection; `twin-collectors/tests/network_namespaces.rs` covers host/non-host identity changes and vanishing identities. Known boot changes reset endpoint and runtime-edge incarnation boundaries while retaining canonical aliases, close prior runtime edges, and remove previous-incarnation evidence links. Unknown boot reads produce coverage gaps and boot_identity_known=false; the last known boot is retained solely to detect a later boundary. Reappearing gone endpoints reset their lifetime. Namespace inode reuse without an observed absence within the same boot remains a kernel identity limit; these endpoint IDs are not permanent workload identities. Completely missing identities suppress all guessed host ownership and record coverage gaps. Scan counts include namespaced endpoints, relationships, sockets and namespace-scoped inode totals.

## 2026-10-09 snapshot proof identity

Schema 6 adds snapshot_edge_observations. New snapshots capture graph rows and supporting observation IDs/roles within one transaction. Current-edge reincarnation clearing its links does not rewrite a prior snapshot's proof references. Observation retention may prune the rows themselves; retained IDs then support an explicit Unknown rather than substitution with current evidence. Legacy snapshots backfill only still-linked observations whose timestamps are at or before snapshot capture; unavailable references cannot be reconstructed. Store regressions cover capture, reset, retention, timestamp-limited backfill and snapshot deletion cascade.

Process incarnation follow-up: known start ticks and boot identities survive temporarily missing reads with explicit identity-known flags; they are retained for comparison, not new observation claims. Changed identities or gone-process reappearance close active and stale touching edges, clear previous-incarnation observation links, and reset evidence/lifetime counters. New process lifetimes never inherit an old start identity when the current read is missing. Fixture regressions cover known → unknown → changed start/boot, stale eBPF evidence, and same-identity gone reappearance.

Final integration verification on 2026-10-09: workspace build, all workspace tests, strict all-targets Clippy and fmt check pass. The suite reports 633 passing functions across 127 suites; five local listener fixture tests return with explicit capability explanations because the environment denies socket bind, leaving 628 fully exercised test bodies. No test is ignored or filtered. Logs: `/tmp/twin-depth-final-build.log`, `/tmp/twin-depth-final-tests.log`, `/tmp/twin-depth-final-clippy.log`, `/tmp/twin-depth-final-fmt.log`.
