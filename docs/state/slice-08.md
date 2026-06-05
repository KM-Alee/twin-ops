# Slice 8: twin impact MVP

## Status: complete

## Implemented

- `twin-core`: `RiskLevel`, `EvidenceLabel`, `EvidenceStrength` in `risk.rs` with `Display`, `FromStr`, serde, and boundary tests.
- `twin-app`: expanded `ImpactResult` with `risk`, `evidence_strength`, structured `direct_dependents` (path, per-dependent evidence, reason), `listener_owners`, structured `unknowns`.
- `twin-app`: impact resolver for exact `NodeId`, service unit, service shorthand, and ambiguity/not-found errors.
- `twin-app`: direct incoming traversal for service `depends_on` and port `connects_to` / `listens_on` with active-edge filtering.
- `twin-app`: evidence aggregation per edge with observed vs inferred wording for TCP and cgroup observations.
- `twin-app`: unknown extraction from latest process collector run warnings and unmapped socket observations.
- `twin-app`: deterministic risk and evidence-strength scoring kept as separate report fields.
- `twin-cli`: polished human renderer with risk, evidence strength, listener owners, dependents, evidence, and unknowns sections.
- Tests: `twin-core/tests/risk.rs`, extended `scan_graph.rs` impact cases, `output.rs` renderer/JSON tests, `cli_integration.rs` impact command tests.

## In Progress

_(nothing)_

## Blocked

_(nothing)_

## Decisions

- Kept impact orchestration in `commands/impact.rs` without extracting `twin-graph` / `twin-rules` crates.
- Reused existing store APIs (`latest_collector_run`, `list_observations_by_source`) instead of adding schema migrations.
- Port listener owners reuse `GraphOwnedNode` for consistency with graph output.
- Service impact still includes only inferred `depends_on` edges (slice 7 behavior); configured unit-file deps deferred.
- Evidence strength caps at moderate when coverage unknowns weaken confidence.

## Deviations from Plan

- Did not split `impact.rs` into subdirectory modules; file remains readable at current size.
- Unknown detail for collector warnings uses `warning_count` only; per-warning detail table not persisted yet from scan.
- `impact_in_unmapped_active_surfaces_unknowns` uses port target when postgres service is absent in that fixture.

## Verification

```bash
cargo fmt --check
cargo test --workspace   # 204 passed
cargo clippy --workspace -- -D warnings
```

Acceptance criteria verified via app integration tests (`impact_in_*`) and CLI integration tests (`impact_*`).
