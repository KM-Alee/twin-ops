# Slice 9: Overlay Emulation MVP

## Status: complete

## Implemented

- `crates/twin-emulate` — overlay domain: `EmulationAction`, `GraphOverlay`, `EffectiveGraphView`, `RestartServiceInput`, restart builder, risk/evidence scoring
- `twin emulate restart TARGET` CLI with scan refresh (same pattern as graph/impact)
- `twin-app` `emulate` / `emulate_in` orchestration; `EmulationResult` app model
- Shared `load_service_dependent_analysis` extracted from impact for consistent dependent classification
- Human output in `twin-cli/src/output/emulate.rs`; JSON via `--json`
- Tests: twin-emulate unit/integration, twin-app emulate + graph immutability, CLI output + E2E, safety string scan

## In Progress

_(nothing)_

## Blocked

_(nothing)_

## Decisions

- Reuse impact dependent loading via `load_service_dependent_analysis`; emulate-specific risk uses slice-9 thresholds (0→low, 1–2→medium, 3+→high) in `twin-emulate/scoring.rs`
- `EffectiveGraphView` maps overlay unavailability to `NodeState::Stale` for hypothetical state reads
- No `twin-safety` crate yet; forbidden-string scan lives in `twin-emulate/tests/safety.rs`

## Deviations from Plan

- Impact risk scoring unchanged; emulate uses separate scoring per slice 9.8 (impact uses 2–4→high, 5+→critical)

## Post-review hardening (slice 9 quality pass)

- `DependentImpactKind` and `UnknownKind` enums in `twin-core` replace scattered `"runtime"` / `"configured"` / unknown-kind strings
- Shared `cap_dependent_evidence_score` in `twin-core` deduplicates evidence caps (30/60/85)
- `TypedServiceDependent` eliminates fragile ImpactDependent→EmulationDependent string round-trip
- `EffectiveGraphView` drives overlay summary on the production path
- `RESTART_ACTION` and `SAFETY_STATEMENT` exported from `twin-emulate`; removed dead `EmulateError` in emulate crate
- `EmulationResult::from_domain` replaces manual field copy; domain report types no longer serde-duplicated
- CLI `sections.rs` centralizes output section titles; `InvalidEmulateTarget` for emulate parse errors
- Extended safety scan covers twin-app emulate + twin-cli emulate paths
- Added missing tests: configured context, unix socket overlay, target-scoped unknowns, ambiguous service, empty-section output, interrupted relationships

## Acceptance Criteria

- [x] `twin emulate restart postgresql.service` runs after `twin scan`
- [x] `twin emulate restart service:postgresql.service` resolves exact service IDs
- [x] Service shorthand resolution works and ambiguous shorthand errors are clear
- [x] Unsupported targets such as `port:tcp:127.0.0.1:5432` are rejected for restart-service emulation
- [x] Report includes risk and evidence strength as separate fields
- [x] Report includes an overlay summary with target service temporarily unavailable
- [x] Report includes owned TCP and Unix listening sockets as temporarily unavailable when present
- [x] Runtime direct dependents appear under transient impact
- [x] Configured-only dependents appear separately and do not inflate runtime risk
- [x] Report includes target-scoped unknowns and scan-health notes
- [x] Human output always says `No action was performed.`
- [x] JSON output includes `action_performed: false`
- [x] No real service restart, process kill, shell command, Docker action, or Kubernetes action is possible from the code path
- [x] Overlay is not persisted as real graph state
- [x] Test proves relevant base graph nodes/edges are unchanged after emulation
- [x] Direct-only behavior is covered; transitive dependents are not reported yet
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
