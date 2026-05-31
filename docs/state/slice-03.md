# Slice 3: Core Domain Model and Observation Pipeline MVP

## Status: complete

## Implemented

- **twin-core graph vocabulary:** `TimestampNs`, `ObservationId`, `CollectorName`; `NodeId` with canonical constructors (host, process, service, port, file, cgroup); `NodeKind`, `NodeState`; `EdgeId`, `EdgeKind`, `EdgeClass`, `EdgeState`; `ParseError`; `Display`/`FromStr`/`serde` on all.
- **twin-observation crate:** `RawObservation`, `RawIdentity`, `RawEvidenceRef`, `Observation`, `ObservationMetadata`; observation enums (`ObservationSource`, `ObservationKind`, `ConfidenceHint`, `RedactionState`); `Redactor` + `BasicRedactor`; `Normalizer`; `Pipeline` (redact → normalize → `Observation`).
- **twin-store bridge:** `From<&Observation> for ObservationRow`, `TryFrom<&ObservationRow> for Observation`; `raw_ref` stored under `metadata_json` key `__raw_ref`; `insert_observation_typed` / `get_observation_typed`; `StoreError::Decode`.
- **Tests:** `twin-core` (`ids`, `node_id`, `enums`); `twin-observation` (`redaction`, `normalization`, `pipeline`); `twin-store` typed observation round-trip in `repo.rs`.
- **Workspace:** `twin-observation` member; `uuid` workspace dependency.

## In Progress

_(none)_

## Blocked

_(none)_

## Decisions

- **Risk/evidence types deferred to slice 8** (`RiskLevel`, `EvidenceStrength`, `EvidenceLabel`) — no scoring consumer in this slice; matches slice 1 schedule.
- **`raw_ref` in `metadata_json`** under `__raw_ref` — no `MIGRATION_003`; slice 2 catch-all honored.
- **Observation enums live in `twin-observation`**, not `twin-core` — pipeline vocabulary vs graph vocabulary split.
- **IPv4 normalization** strips leading-zero octets before formatting (`127.000.000.001` → `127.0.0.1`) because `std::net::IpAddr` rejects that form.
- **Working demo tests:** `cargo test -p twin-core -p twin-observation` (and `twin-store` observation tests). Plain `cargo test core` matches no test names in this repo.

## Deviations from Plan

- None material.

## Post-review hardening (after subagent review)

- IPv6 TCP ports use bracket form: `port:tcp:[::1]:80`.
- `NodeId::from_str` / `EdgeId::from_str` validate canonical grammar (store typed decode rejects garbage node ids).
- `BasicRedactor` redacts credential-less `scheme://` URLs (except `file://`); `CollectorName::FromStr` added.
- Store encode strips duplicate `__raw_ref` from metadata before persisting typed `raw_ref`.
- Decode error matrix tests in `twin-store/tests/repo.rs`.

## Acceptance Criteria

- [x] All domain-critical concepts use newtypes/enums (IDs, kinds, states, classes, sources)
- [x] No raw stringly-typed graph core (canonical `NodeId`/`EdgeId` constructors; enums for kinds/states/classes)
- [x] Observations can be stored and queried (`insert_observation_typed` / `get_observation_typed` round-trip)
- [x] Redaction hook exists (`Redactor` trait + `BasicRedactor`)
- [x] Unit tests cover IDs and normalization
- [x] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` pass
- [x] `twin doctor --json` unchanged (schema v2)
