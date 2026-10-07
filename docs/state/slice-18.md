# Slice 18: Emulation and Endpoint Tests

## Status: complete

## Implemented

- `twin test run` accepts four more check kinds on top of slice 17, still with no shell execution:
  - `emulate` plus `expect.max_risk` and `expect.require_evidence`. Restart calls the existing service emulation engine. Delete calls the existing file-delete emulation. The command does not restart or delete anything.
  - `assert.outbound_endpoints.allowed` with `fail_on_unknown`. Observed `connects_to` port targets that are not on the list fail, and the failure carries the observation evidence.
  - `assert.disk.mount` and `max_used_percent`. Usage is read with `statvfs`. A missing mount fails the check and does not crash.
  - `assert.unknowns` with `max_count` and/or `forbid`. Counts come from the existing coverage report.
- Risk at the configured maximum warns. Risk above it fails. Risk below it passes. `require_evidence: true` fails when the emulation report has no evidence lines.
- Human output prints the risk or unexpected endpoint as the detail and the evidence sentence on an `evidence` row. JSON includes the same fields.

## Decisions

- `expect` sits beside `emulate`, matching the slice example, not inside it.
- Restart targets must be `service:` ids. Delete targets are file paths or `file:` ids, passed through to the existing delete emulation.
- An emulation error, such as a missing service, becomes a failed check with `Risk: UNKNOWN` and the error text as evidence. The rest of the suite still runs.
- Endpoint strings are the `port:tcp:` suffix (`10.0.0.8:443`). Allowlist comparison is exact.
- Disk percentage is blocks minus blocks available to unprivileged callers. The check does not write to the mount.
- Unknown kinds use the coverage report names (`unmapped_sockets`, `permission_gap`, `unavailable_collector`).

## Deviations from Plan

- Kubernetes resource checks and outbound hostname resolution are not added. The allowlist matches the endpoint string already stored on the graph.
- `require_evidence` is a field of the emulate check, which is how the slice example writes an evidence requirement, not a separate assertion key.

## Acceptance Checklist

- [x] The test runner calls the emulation engine for restart
- [x] An endpoint outside the allowlist fails and includes the evidence sentence
- [x] Emulation output includes risk and evidence
- [x] Disk over the threshold fails; an unknown mount fails without panicking
- [x] Unknowns over `max_count` fail and include the unknown detail
- [x] Shell keys are still rejected
- [x] Existing node, port, service, and dependency checks still pass
- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
