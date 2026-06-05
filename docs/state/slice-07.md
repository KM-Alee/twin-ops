# Slice 7: Active Connections and First Dependency Inference

## Status: complete

## Implemented

- TCP parser returns `TcpConnectionRecord` for ESTABLISHED rows (`socket.rs`); LISTEN unchanged.
- `ObservationKind::TcpConnectionSeen` with proc-net metadata and fd join fields.
- `ProcessBatch` carries `tcp_connections`; collector warns `ActiveSocketUnmapped` when inode has no fd owner.
- Scan materializes observed `process -> port connects_to`, inferred `service -> port connects_to`, inferred `service -> service depends_on` when connect target matches a listener port (exact + `0.0.0.0` / `::` wildcard).
- `GraphEdge` constructors for connects_to and depends_on with edge-observation links (direct/support).
- Graph service/port views: connected ports, dependencies, dependents, callers; evidence lines distinguish observed vs inferred.
- `twin impact` for service and port targets (direct dependents only; no risk scoring).
- Scan/impact/graph CLI human output and integration tests.

## Decisions

- Reused single `parse_tcp_table` returning listeners + connections (no second parse pass).
- Port nodes for remote endpoints; no endpoint node kind.
- `listener_port_candidates` matches wildcard listeners for IPv4/IPv6 connect targets.
- Impact command in `twin-app` / `twin-cli` only; no new graph/rules crates.

## Deviations from Plan

- None material. Slice state doc was updated at review completion (was still `not-started` during implementation).

## Verification

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Acceptance criteria in `docs/imp-plan/slice-07.md` section 11 verified via integration tests in `twin-app/tests/scan_graph.rs`, `twin-collectors/tests/socket.rs`, `twin-cli/tests/output.rs`.
