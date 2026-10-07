# Slice 17: Local Test Runner MVP

## Status: complete

## Implemented

- Crate `twin-test`. It parses `twin.yaml` and evaluates checks. It does not open the database and it does not run a shell. `twin-app` loads the stored graph, scores dependency evidence with the existing formula, and persists the report.
- Strict YAML, version 1 only. Unknown fields are rejected. A check sets exactly one of `node`, `port`, `service`, or `dependency`. `exists` defaults to true. `min_evidence` is only valid on a dependency, and falls back to `defaults.min_evidence` when the check omits it. Labels are `weak`, `moderate`, `strong`, and `very_strong`.
- `port` must be a `port:` id. `service` must be a `service:` id. Duplicate check names, an empty suite, and version other than 1 are lint errors.
- Keys named `shell`, `command`, `exec`, or `script` are rejected before evaluation. The `twin-test` sources are scanned for process and mutation strings.
- `twin test init [path]` writes the starter file (default `twin.yaml`) and refuses to overwrite it unless `--force`.
- `twin test lint [path]` validates the file and lists the checks. It does not need a database.
- `twin test run [path]` evaluates the file against the current graph. A missing node or dependency fails. A stale node or edge warns. Evidence below `min_evidence` fails. The report is stored in `test_runs` (schema v5). A failed check makes the command exit 1. Permission or parse problems return a typed error and do not panic.
- Human output is in `twin-cli/src/output/test_report.rs`. JSON is `--json`.

## Decisions

- Slice 18 checks are not accepted: no `emulate`, outbound allowlists, disk thresholds, or Kubernetes assertions. The strict parser rejects those fields.
- Dependency evidence uses `connect_evidence_strength`. An observed `connects_to` edge counts as a socket-table claim (strong). An observed `depends_on` edge counts as config (moderate). Linked `EbpfConnect` observations use the eBPF score. An inferred edge with no observations is weak, so a minimum of moderate fails.
- `exists: false` passes only when the node or dependency is absent.
- Warn does not fail the process. Fail does.
- Docker and Kubernetes collectors are not part of this slice.

## Deviations from Plan

- Tech document §15.4 also lists service state, containers, disk, outbound endpoints, emulate, and Kubernetes. Those are later slices. This slice implements node, port, service, dependency, and minimum evidence.
- `twin test list` from the PRD is not a command yet. Runs are stored and can be read from `test_runs`.
- The starter file includes a service check and a port check in addition to the plan's node and dependency examples, so every first check kind is generated.

## Acceptance Checklist

- [x] YAML validation rejects a bad version, a duplicate name, a port id that is not a port, unknown fields, and shell keys
- [x] Node, port, and service checks pass, warn when stale, and fail when missing
- [x] Dependency checks fail when the edge is missing or below `min_evidence`
- [x] Human output contains PASS, WARN, and FAIL, plus passed, warned, and failed counts
- [x] `twin test init`, `lint`, and `run` work from the CLI
- [x] A run is persisted in `test_runs`
- [x] No shell execution
- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All passed on implementation completion (412 tests).
