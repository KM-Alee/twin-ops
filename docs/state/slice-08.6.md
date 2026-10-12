# Slice 8.6: Runtime Unix Sockets, systemd D-Bus, Enable Graph

## Status: complete

## Implemented

- `twin-core`: `NodeKind::UnixSocket`, `NodeId::unix_socket`, `GraphNode::unix_socket`; edges `inferred_service_depends_on_unix`, `observed_service_depends_on_dbus`, `observed_service_depends_on_enable`.
- `twin-observation`: sources `ProcNetUnix`, `SystemdDBus`, `SystemdEnableSymlink`; kinds `UnixSocketSeen`, `UnixConnectionSeen`, `SystemdUnitStateSeen`, `SystemdUnitWantedBy`.
- `twin-collectors`: `/proc/net/unix` parser (listen via `Flags & 0x10000`); inode join; `SystemdDBusReader` + `StdSystemdDBusReader` (`zbus`); enable `.wants`/`.requires` symlink scan; `SystemdRuntimeCollector`.
- `twin-app`: scan unix graph block + `listeners_by_unix_path` inference; `scan_systemd_runtime` persistence; graph `unix:` neighborhood; impact routes `unix:` through port impact path; `ScanResult` unix/dbus/enable counts.
- `twin-cli`: `graph` / `impact` accept `unix:` targets; human output for unix socket neighborhood and scan counts.
- `scan_quality` / doctor: unix unmapped counts in collector metadata; D-Bus availability note from latest `systemd_runtime` run.
- Tests: collector unix parse/join, enable symlinks, `scan_graph` unix dependency + graph neighborhood, core node id roundtrip.
- Research: `docs/imp-plan/slice-08.6-research.md`.

## Decisions

- Canonical node id prefix **`unix:`** (not PRD `socket:unix:`).
- Unix listen detection uses **accept-con flag** (`0x10000`), not TCP-style `St`.
- Fake-proc scans: `TWIN_SYSTEMD_UNIT_ROOT` honored only when under the same parent directory as the proc fixture (avoids picking up developer shell env).
- Runtime D-Bus skipped when systemd unit roots empty (fixture proc without explicit test env).
- Socket activation (`[Socket]` `Service=`) deferred; unit-file deps from 8.5 remain.

## Deviations

- No dedicated `unix_socket_coverage` doctor field; covered via `scan_quality` reasons (`unix_socket_unmapped`, D-Bus unavailable message).

## Review fixes (2026-06-05)

- `impact.rs`: unix socket targets now surface unmapped unix observations and unix connection/listener evidence lines (not TCP-only).
- `graph.rs`: `socket_evidence_line` / connect / dependency evidence helpers handle `UnixSocketSeen` and `UnixConnectionSeen`.
- `scan_quality.rs`: D-Bus unavailable reason no longer gated on `enable_symlink_count > 0`; degraded health note says "socket" not "TCP".
- `twin-cli` scan output: added missing unix edge counts, unmapped unix listeners, unix connections, and `dbus-available`.
- Tests: `impact_in_unix_socket_target_lists_callers`, `assess_scan_quality_notes_dbus_unavailable`.

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Runtime follow-up: systemd Unit.InvocationID is decoded as a nonzero 16-byte identity and stored as safe lowercase hex metadata. Missing/zero/malformed IDs are absent. This supports inferred restart/reactivation history without reading environment values or claiming a restart cause. Fixture and property-decoder tests cover the new field.

## 2026-10-08 dependency semantics follow-up

An active service's optional systemd `Wants=` declaration remains configured context unless independent runtime evidence establishes a dependency. `Requires=` retains its explicit stop/restart propagation behavior for an active dependent. File and D-Bus groups prefer a retained `Requires` declaration when both keys exist for the same pair. A fixture checks active Wants vs Requires vs inferred connection behavior through impact and restart emulation. This does not yet model `PartOf`, `BindsTo`, `Upholds` or every systemd dependency directive.
