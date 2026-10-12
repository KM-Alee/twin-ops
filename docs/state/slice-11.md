# Slice 11: Transitive Impact Paths and Better Risk Scoring

## Status: complete

## Implemented

- `--paths` and `--max-depth` (default 4, range 1–8) on `twin impact` and `twin emulate restart`
- `ImpactPath` model with terminal, depth, steps, evidence, cycle/depth cap flags
- `impact_paths.rs` — bounded incoming `depends_on` DFS with cycle and depth caps
- `impact_scoring.rs` — risk reasons for direct/transitive counts, public listener hints, criticality hints
- Port targets: direct caller paths only when `--paths` is set
- `twin-emulate` receives `EmulationImpactPath` + path scoring input (no store access)
- Human output: `impact paths` section in impact and restart emulation reports
- JSON: `impact_paths`, `paths_requested`, `max_depth` on impact and emulation results
- Integration tests: `crates/twin-app/tests/impact_paths.rs`, CLI output + E2E tests

## Decisions

- Traversal stays in `twin-app` (not a new graph crate)
- Path steps stored outer→inner (terminal first) for readable chains
- Public exposure = non-loopback `listens_on` on path nodes (graph hint only)
- Criticality hints hard-coded in app layer (postgres, redis, kafka, etc.)
- Direct risk thresholds adjusted: 1–2 direct = medium, 3–4 = high, 5+ = critical

## Deviations from Plan

- Restart emulation scoring note still mentions direct-only thresholds in CLI; path-aware reasons are added when `--paths` is used
- `twin-emulate` scoring extended with optional `RestartPathScoringInput` rather than duplicating full impact scoring

## Acceptance Checklist

- [x] `twin impact SERVICE --paths` renders direct and transitive service dependency paths
- [x] `twin emulate restart SERVICE --paths` renders path-aware blast radius and `No action was performed.`
- [x] `--max-depth N` caps traversal and reports capped paths
- [x] Cycles terminate deterministically with concise cycle note
- [x] Existing direct dependent output remains present
- [x] Port impact supports `--paths` with direct caller paths only
- [x] Risk reasons include direct/transitive counts, public exposure, criticality hints, unknowns
- [x] Evidence strength separate from risk in human and JSON output
- [x] JSON includes structured path data
- [x] Missing edge evidence creates unknown instead of panic
- [x] Restart overlay does not mark transitive dependents unavailable
- [x] No schema migration
- [x] Tests use fixtures and temporary stores only
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` passes

## Manual Smoke Test

- `twin impact postgresql.service --paths` → clean error (service not in graph on this host)
- `twin scan` + `twin impact nginx.service --paths --max-depth 2` → renders `impact paths` section (empty when no dependents), risk/evidence/unknowns intact, read-only

## 2026-10-08 follow-up: Transitive overlays and criticality

Restart-service overlays mark transitive runtime dependents temporarily unavailable and preserve actual relationship kinds. Base graph rows remain unchanged. Default output summarizes chain impact; --paths prints full chains. Risk criticality hints derive from observed persistent-storage uses edges rather than service names. Port-target chains traverse upstream services. Tests: impact_paths, restart_service, CLI output. Container default traversal still needs parity.

## Additional 2026-10-08 follow-up

Container restart now performs bounded transitive analysis by default and marks direct and transitive affected services unavailable in the hypothetical overlay. The base graph remains unchanged. Detailed paths remain controlled by --paths. Tests: containers and restart_container.

## 2026-10-08 reasoning audit fixes

Complete paths are retained when a leaf is exactly at `--max-depth`; only unexplored incoming relationships mark a path depth-capped. Restart overlays therefore retain those affected nodes. Transitive counts include intermediate service nodes, exclude the target and all direct dependents, and deduplicate converging paths. Regression fixtures cover the exact-depth leaf, the restart overlay, intermediate nodes and a service reachable both directly and transitively. Existing cycle/depth-cap fixtures remain.

Focused verification: impact_paths integration tests (10) and evidence_scoring integration tests (3) pass in `/tmp/twin-path-systemd.log`; strict selected all-targets Clippy passes in `/tmp/twin-reasoning-clippy.log`.
