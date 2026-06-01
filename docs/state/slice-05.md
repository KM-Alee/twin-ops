# Slice 5: Cgroups and systemd Service Mapping

## Status: complete

## Implemented

- `/proc/<pid>/cgroup` parsing (cgroup v1 and v2) in `twin-collectors`
- `ProcessBelongsToCgroup` observations with `ProcCgroup` source
- Cgroup and service graph nodes with conservative `.service` unit inference
- Observed `process -> cgroup` `in_cgroup` edges and inferred `service -> process|cgroup` `owns` edges
- Scan persistence with evidence links (`direct` / `support`)
- Extended `ScanResult` counts and CLI scan output
- Service graph neighborhood (`twin graph service:nginx.service`, `twin graph nginx`)
- `--kind service` list view
- Collector, core, app, and CLI E2E tests (`cli_integration.rs`, `TWIN_PROC_ROOT`)
- `sudo` uses invoking user's XDG data dir via `SUDO_USER` / `SUDO_UID`

## Decisions

- Extended `ProcessCollector` in-process rather than a separate collector trait
- Service ownership is `EdgeClass::Inferred` with metadata `systemd_cgroup_path`
- Graph service name resolution lives in `twin-app` (`target_query` from CLI for shorthand names)
- No systemd D-Bus or unit-file parsing in this slice
- Cgroup path canonicalization uses `twin_core::lexical_canonical` (same as `NodeId::cgroup`)
- `TWIN_PROC_ROOT` env for CLI integration tests only (not user-facing)

## Post-ship cleanup (simplify pass)

- Single-pass `parse_cgroup_memberships` (no triple parse in procfs)
- `scan.rs` helpers: `upsert_node_once`, `load_existing_edge`, `upsert_edge_with_link`
- `HashSet<NodeId>` for seen cgroup/service nodes; one cgroup observation lookup per membership
- `GraphEdge::inferred_service_owns` public; scan uses it for both owns targets
- Warning JSON-detail policy test moved to `twin-collectors/tests/warning.rs`

## Deferred (not in simplify pass)

- Shared `write_proc_fixture_*` across app/collectors/cli test crates (needs `twin-fixtures` crate)
- In-transaction node/edge caches or SQL upsert without pre-`get_edge` (scan perf at host scale)
- Batch graph neighborhood queries for large services
- `get_edge_typed` on store

## Deviations from Plan

- None
