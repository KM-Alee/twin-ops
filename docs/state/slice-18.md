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


## 2026-10-08 follow-up: outbound hostname allowlists

The app expands explicit hostname:port allowlist entries using a read-only DnsReader; twin-test performs no network or shell calls. IP literals are canonicalized, IPv6 uses brackets, hostnames are validated/lowercased, and each distinct hostname is queried once for both address families. TCP and UDP connected peers are checked. No connection to the allowlisted service is made. DNS queries use the configured system resolver (which may consult local hosts files or network DNS).

Live resolution uses the existing Tokio dependency, a five-second total deadline and at most 32 unique hosts. The sync entrypoint returns unavailable inside an existing async runtime; the async resolver is available separately. OS getaddrinfo is not cancellable: runtime shutdown does not wait for a timed-out resolver task, which can finish in the background. No repeated lookups are started after the deadline.

Reports preserve the DNS hostname, returned endpoint set, observation timestamp and an explicit inference statement: current DNS cannot prove hostname ownership at the time of a stored connection. Failed/empty/limited lookups and malformed entries cannot silently pass an empty graph; fail_on_unknown selects fail versus warn. Invalid raw endpoint values and resolver error details are never copied into reports. Expanded entries affect only this evaluation and never the stored graph. Tests inject DnsReader fixtures for IPv4/IPv6, UDP, deduplication, failures and secret-safe invalid input; collector tests cover empty queries and nested-runtime graceful degradation.

## 2026-10-08 explicit evaluation context CLI

`test run` exposes `--refresh`, `--max-age DURATION`, `--snapshot NAME`, and `--baseline NAME`. Refresh and snapshot are mutually exclusive. Duration parsing uses the app's shared bounded duration parser. Human reports identify the stored/snapshot graph, observation age, maximum age and separately collected live sources. Baseline comparisons list newly failing, newly uncertain and recovered checks with a correlation-not-causation note. CLI parser and renderer fixtures cover these options and sections; app orchestration owns historical coverage and omitted live-source semantics.

## 2026-10-09 trust and semantic health contracts

App tests now share typed evidence assessment with impact/why and require actual retained observations. Source-scoped missing coverage cannot prove absence. Service active state, owned local listeners, container running/health status and deployment ready/routed counts are implemented with Unknown for stale/incomplete facts. Pod ownership is deduplicated and ambiguous ownership is unknown. Future timestamps cannot establish freshness.

Snapshot checks use captured evidence references and as-of collector coverage, not current-incarnation links or current ownership penalties. Pruned evidence produces Unknown. Disk and emulation checks requiring uncaptured live inputs also report historical Unknown. Baselines retain before-check results plus newly failing/uncertain/recovered names. App fixtures cover age, health, source absence, captured listener proof/reset/pruning and historical emulation. See operational-workflows.md for YAML and command examples.

Final verification: cargo build, cargo test --workspace --no-fail-fast -- --nocapture, cargo clippy --workspace --all-targets -- -D warnings and cargo fmt --check pass. Runner: 633 passing tests across 127 suites; five explicitly runtime-gated server fixtures, 628 fully exercised bodies. Built CLI accepts the documented health YAML and reports Unknown on an unscanned isolated store.
