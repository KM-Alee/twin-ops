# Slice 4: Process Scan and Process Graph

## Status: complete

## Implemented

- `twin-collectors` crate with sync `ProcessCollector`, injectable `ProcReader`, `/proc` parsing, and warning aggregation.
- Observation kinds: `ProcessCommandSeen`, `ProcessExeSeen`, `ProcessParentSeen`; argv redaction in `BasicRedactor`.
- Typed graph domain: `GraphNode`, `GraphEdge`, store typed bridge (`upsert_*_typed`, `list_nodes_by_kind_typed`).
- `twin scan` / `scan_in`: collector run, observations with `collector_run_id`, process nodes, observed `parent_of` edges, edge–observation links.
- `twin graph` / `graph_in`: process listing and neighborhood; CLI accepts `twin graph`, `twin graph process`, `twin graph 1234`, or full node ids.
- CLI human + JSON renderers for scan and graph.
- Integration tests: collectors (fixture `/proc`), store typed round-trip, app scan/graph, CLI output.

## In Progress

_(nothing)_

## Blocked

_(nothing)_

## Decisions

- Concrete `ProcessCollector` (no `Collector` trait) until a second collector needs shared orchestration.
- `/proc` races → warnings; scan succeeds with partial coverage.
- Kernel threads have no `/proc/[pid]/exe`; absent exe after successful stat is normal, not a warning.
- Canonical process ID remains `process:pid:<pid>` (no instance IDs this slice).
- Observed edges only (`EdgeClass::Observed`); evidence score/label `100` / `strong` on materialized parent edges.
- `GraphNodeParts` / `GraphEdgeParts` structs to satisfy clippy and keep row conversion explicit.

## Deviations from Plan

- `twin-graph` crate not added (plan allowed deferral; store listing suffices).
- `with_transaction` on `Store` made public for scan orchestration (was `pub(crate)`).
- `ScanError` / `GraphError` remain for command-internal failures; plan’s `AppError::Collector` / `InvalidGraphTarget` / `UnsupportedGraphKind` are top-level variants with `From` bridging.
- `warning_details` in JSON omits homogeneous bulk coverage gaps (`exe_unreadable`, `permission_denied`); counts stay in aggregated `warnings` only. Per-path detail kept for `vanished` and `malformed`.

## Post-simplify cleanup (codebase review)

- `ProcessWarningKind` owns aggregate/detail JSON keys, JSON detail policy, and CLI summary text (`twin-collectors`).
- `NodeId::process_pid()` replaces ad-hoc `process:pid:` stripping in scan/graph paths.
- Store typed decode uses shared `repo/decode.rs::parse_field`.
- Scan pipeline drains raw observations without cloning each `RawObservation`.
- CLI human output uses shared `output/format.rs` (`Lines`, status tags, trees); graph usage hints live in `twin-cli`, not `twin-app` errors.
- Missing `/proc/[pid]/exe` (kernel threads) is not a warning; permission-denied on exe remains `ExeUnreadable`.

## Acceptance

| Criterion | Status |
|-----------|--------|
| Works unprivileged | Met — read-only `/proc`; permission errors are warnings |
| Disappearing processes do not crash scan | Met — `vanished_process_becomes_warning` test |
| Repeated scans update `last_seen` | Met — `scan_in_repeated_scan_updates_last_seen` |
| Process graph visible | Met — `graph_in_lists_process_nodes`, `graph_in_returns_process_neighborhood` |
| Command values redacted | Met — argv redaction tests (flags, separate args, URI userinfo) |
| No host mutation | Met — read-only FS; tests use temp proc fixtures |
