# Slice 8.7 — Ephemeral Capture, Socket Activation, systemd Graph Completion

**Status:** complete  
**Prerequisite:** slices 8, 8.5, 8.6

## Done

- `ScanRequest.samples` / `interval_secs`; CLI `--samples` (1–30), `--interval` (≥1s)
- Multi-sample loop with `proc_root/sample-N/` override for fixtures
- Observation metadata: `sample_index`, `sample_at_ns`
- Process collector run metadata: `samples_total`, `tcp_connection_count` (for doctor)
- Socket unit parse (`ListenStream`, `ListenDatagram`, `ListenSequentialPacket`, `Service=`)
- `SystemdSocketSeen`, `SystemdSocketActivates` observations; activation `depends_on` edges
- Template instance names (`foo@bar.service`); template `[Unit]` merge from `foo@.service`
- D-Bus `GetUnitByControlGroup` on `SystemdDBusReader` (+ fixture map via `TWIN_SYSTEMD_CGROUP_MAP`)
- Cgroup correction pass after service-owns inference (upserts corrected service node)
- `active_state` / `load_state` / `sub_state` / `unit_type` on service nodes from D-Bus
- Impact: `configured_dependents` vs runtime `direct_dependents`; risk from runtime count only
- Graph service view: `socket_activation`, `configured_dependents` sections
- Doctor: `ephemeral_capture_recommended` when single-sample scan saw TCP connections or unmapped actives
- Live D-Bus skipped when `TWIN_SYSTEMD_UNIT_ROOT` or `TWIN_SYSTEMD_CGROUP_MAP` set (test isolation)

## Decisions

- No `[Socket]` key named `Socket=` (systemd uses `Listen*` + `Service=` per man page)
- Cgroup path from proc; validation via D-Bus only (no local cgroup→unit translation)
- `samples=1` default preserves single-snapshot behavior
- Ephemeral doctor heuristic: single-sample + (`tcp_connection_count > 0` or unmapped active sockets)

## Blocked / deferred

- `Accept=yes` per-connection instances, `%i` expansion, `ListenFIFO`/vsock
- `twin watch` (slice 13)

## Verification

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Both green after slice 8.7 fixes.
