# Slice 8.8: Skipped simplify follow-ups

## Status: complete

## Implemented

- `GraphMetadata::socket_activation()` typed accessor; graph uses it for socket-activation edges.
- `RiskAssessment.level` is `twin_core::RiskLevel` end-to-end (JSON + CLI).
- `socket_endpoint_neighborhood` dedupes port/unix graph views.
- Impact dependent `reason` built from observation metadata, not parsed evidence lines.
- `scan_systemd_groups`: shared `group_unit_dep_observations`, `persist_depends_on_group`.
- `twin-collectors/systemd/observation.rs`: `unit_dependency_raw` for unit file + D-Bus deps.
- CLI impact renderer aggregates dependent evidence when top-level evidence is empty.
- `twin-store` batch APIs: `count_nodes_by_kind`, `get_nodes_by_ids`, `get_observations_by_ids`.
- `EvidenceLoadContext` / `NodeLoadContext`; graph socket neighborhoods batch-load peers/observations.
- `ScanEdgeCache` for scan persist edge lookups.
- Impact `TcpUnmappedIndex` / `UnixUnmappedIndex` for O(1) unmapped socket unknowns per port/path.
- Scan: `records_by_pid` map; `listener_services_for_port` uses in-memory map only.
- Cgroup corrections: `CgroupUnitLookup` from `list_units()` before per-path D-Bus fallback.
- D-Bus: `Properties.GetAll` per unit instead of four separate property calls.
- TCP FD walk skipped when net tables have no listeners/connections (existing guard retained).

## Verification

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```
