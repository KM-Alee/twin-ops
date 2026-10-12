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
- Template instance names (`foo@bar.service`); template fallback only when the explicit instance file is absent
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

- Runtime FIFO/vsock ownership and remaining unit specifiers; declarations and common instance expansion are now implemented
- Polling watch and bundled exec/connect tracing producers are implemented in slices 13–15; successful live kernel attachment remains capability-dependent and unverified in this runtime

## Verification

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Both green after slice 8.7 fixes.

## Instance/activation follow-up — 2026-10-08

Status: implemented with explicit runtime limits. Enabled symlink names, proc cgroup service units and D-Bus unit snapshots supply concrete template instances. Uninstantiated template files do not become active services or warnings. Literal instance files override templates; highest-priority main files win, /dev/null and empty files mask lower-priority files, and same-named drop-ins respect search-path/instance precedence. Template and instance drop-ins expand %i, %n, %N, %p and %%. Unsupported specifiers generate warnings and suppress unresolved relationships. Instance strings containing dots keep the final unit suffix when selecting templates.

Accept=yes on stream/sequential sockets selects a service template; datagram/FIFO-only units keep ordinary targets. Invalid booleans and incompatible explicit targets warn. Template graph nodes are marked service_template and have no fabricated runtime active_state. D-Bus Unit.TriggeredBy supplies concrete per-connection instance relationships as observed declarations, without guessing generated instance names or claiming why a service started. FIFO paths and vsock addresses persist only as safe socket configuration observations. They do not establish live endpoint ownership; that remains a separate collector gap.

Tests: systemd_unit.rs covers enabled/live instances, expanded dependencies, template/instance drop-ins, file priority, masks, Accept handling and FIFO/vsock declarations. systemd_dbus_fixture.rs covers concrete trigger relationships. app scan_graph.rs verifies cgroup-driven template fallback and template activation nodes. No services are started or changed.


Socket-only drop-ins now update the effective socket settings instead of being rejected for a missing [Unit] section. Empty listener assignments clear earlier addresses across supported listener kinds; later entries append in filename order. Accept and Service overrides preserve unspecified fields. Invalid overrides warn and are applied atomically, retaining the prior valid settings. Socket main-file [Unit] dependencies are also retained. Collector regression tests verify resets, overrides, dependency retention and malformed drop-in handling.

## 2026-10-09 syntax and drop-in depth

Systemd parsing now joins continued lines across comments, supports quoted lists and bounded character escapes, rejects malformed relevant values with sanitized parse errors, and leaves unrelated Description/content values uninterpreted. Type-wide and dash-prefix drop-ins participate in specificity, load-path priority, lexicographic file ordering and masking. Additional pure unit-name specifiers %I/%P/%J/%j/%f resolve escaped names/paths without consulting host identity. Host/user/credential-dependent specifiers remain unsupported and warn; runtime FIFO/vsock ownership remains a separate missing capability.

Collector fixtures cover continuation/quote errors, prefix/type specificity and masks, and escaped instance/file paths. Reference semantics: official systemd `systemd.syntax` and `systemd.unit` manuals/source. No shell or host write is involved.

Systemd unit reads are capped at 1 MiB. Missing optional `[Unit]` sections represent empty dependency declarations; malformed relevant declarations in socket main/template files are explicit parse gaps rather than silently discarded dependencies. Drop-in dependency and socket changes commit together only after both parse successfully. Tests reject oversized files and malformed socket declarations, and verify that an invalid socket drop-in cannot partially introduce dependency edges.


## 2026-10-09 socket manager context

- Read-only D-Bus `org.freedesktop.systemd1.Socket.Listen` captures typed stream, datagram, sequential-packet and FIFO addresses for concrete observed socket units. Addresses are bounded (256 per unit, 4096 bytes each), reject control characters, and carry an explicit property source.
- These are observed **manager configuration**, not observed open descriptors. Active/substate is retained separately; neither creates a `listens_on`/ownership edge. FIFO and VSOCK declarations remain insufficient for runtime ownership.
- An unreadable, malformed, oversized or unsupported Listen property records a per-unit `systemd_socket_property_unreadable` gap and `socket_config_known=false`. Successful empty properties are distinguishable from unknown properties. Node persistence replaces prior configuration metadata so missing/current reads cannot retain old addresses as current facts.
- Fixture tests cover FIFO/VSOCK context, per-unit unreadability, no invented ownership, input bounds and control characters. Focused collector tests and strict collector/app Clippy pass without D-Bus/root.
- Source: [systemd D-Bus API](https://www.freedesktop.org/wiki/Software/systemd/dbus/) exposes Socket.Listen separately from unit state. Actual FIFO ownership needs descriptor metadata and stable process identity; VSOCK requires read-only sock_diag (Linux `vm_sockets_diag.h`), with kernel-version-specific namespace semantics. These descriptor/diag adapters and remaining host/user-context specifiers remain open.
