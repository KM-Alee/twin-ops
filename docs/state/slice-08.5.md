# Slice 8.5: Declared systemd Dependencies and Scoped Impact Unknowns

## Status: complete

## Implemented

- `twin-observation`: `SystemdUnitFile` source; `SystemdUnitSeen`, `SystemdUnitRequires`, `SystemdUnitWants` kinds.
- `twin-collectors`: `systemd/` unit-file collector (`Requires`/`Wants`/`BindsTo`), drop-in merge (including `/etc/*.service.d` when main unit lives under `/usr/lib`), glob skip warnings, fixtures under `tests/fixtures/systemd/`.
- `twin-core`: `GraphEdge::observed_service_depends_on_declared` with `EdgeClass::Observed`; metadata via `serde_json` (safe paths/keys).
- `twin-store`: migration v3 `collector_runs.metadata_json` for process warning breakdown.
- `twin-app`: scan runs systemd unit collector after process batch; declared `depends_on` edges; `declared_depends_on_edge_count` / `systemd_unit_count` on `ScanResult`; systemd warnings merged into scan warnings; `observation_count` includes process + unit (+ runtime when present).
- `twin-app`: `inferred_service_depends_on` does not downgrade an existing `Observed` edge; systemd pass runs after TCP inference so declared edges win on upsert.
- `twin-app`: impact accepts observed + inferred `depends_on`; target-scoped unmapped TCP unknowns (cached per call); non-weakening `scan_health` unknown.
- `twin-app`: `scan_quality` assessment (`good` / `partial` / `degraded`) for doctor; D-Bus-unavailable reason only when runtime scan collected enable symlinks and bus was unavailable.
- `twin-app`: shared `resolve_service` and `evidence/systemd` for graph/impact strength labels.
- `twin-cli`: doctor scan quality section; graph declared vs runtime deps; impact configured vs runtime + scan health section.

## Test hook and fixture scans

- **`TWIN_SYSTEMD_UNIT_ROOT`**: only honored when `proc_root != /proc`. Value must not be one of the host default unit paths (`/etc/systemd/system`, `/run/systemd/system`, `/usr/lib/systemd/system`) so a leaked shell env cannot pull host units into fixture scans.
- **Real scans** (`proc_root == /proc`): always use `default_search_paths()`; env var is ignored.
- **Fake-proc scans**: empty systemd roots unless `TWIN_SYSTEMD_UNIT_ROOT` points at a test fixture directory.
- **`FAKE_PROC_SCAN_LOCK`**: serializes fake-proc `scan_in` calls so parallel tests do not race on `TWIN_SYSTEMD_UNIT_ROOT`.
- **`scan_graph` tests**: `SystemdUnitRootGuard` clears env on drop (including panic paths).

## Decisions

- Manual `[Unit]` parser (no extra dependency).
- `Observed` edge class for unit-file deps; metadata `source: systemd_unit_file`.
- `scan_health` unknowns use `weakens_evidence: false`; impact risk stays low when only scan-quality notes apply.
- Doctor propagates `scan_quality_error` instead of swallowing assess failures.

## Tests

- `twin-collectors`: unit parse, drop-in cross-root, collector fixtures.
- `twin-app`: `scan_quality`, scan/impact/graph (declared deps, observed preserved after TCP inference, degraded scan + low risk).
- `twin-cli`: `output.rs` doctor/graph/impact/scan surfaces.
- `twin-core`: declared-edge metadata escaping.

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
