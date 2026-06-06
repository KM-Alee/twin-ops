# Slice 10: Runtime vs Restart Impact and File Deletion Emulation

## Status: complete

## Implemented

- Observation vocabulary: `ConfigFileDiscovery` source, `ConfigFileSeen`, `ServiceConfiguredByFile` kinds
- Config file discovery in `twin-collectors/src/systemd/config_files.rs` (unit files, drop-ins, known nginx path)
- Scan persistence: file nodes and `service configured_by file` edges with evidence (`scan_config_files.rs`)
- Graph: service `configured by` section; `twin graph file:/path` and absolute path shorthand
- `twin emulate delete PATH_OR_FILE_ID` with `DeleteFile` action and `HypotheticallyDeleted` overlay
- Delete-file impact buckets: runtime, restart, persistent, unknown
- Separate risk and evidence scoring for delete-file emulation
- CLI human/JSON output with `No file was deleted.` and `No action was performed.`
- Tests: collectors, twin-emulate, twin-app (`emulate_delete.rs`), twin-cli output, safety scan extensions

## Decisions

- Known config mapping starts with `nginx.service` → `/etc/nginx/nginx.conf` only; PostgreSQL directory deferred
- `TWIN_HOST_ROOT` env maps host paths in tests (same pattern as `TWIN_SYSTEMD_UNIT_ROOT`)
- `EmulateRequest` uses `EmulateActionRequest` enum for restart vs delete
- Restart emulation keeps `transient_impacts` / `configured_impacts`; delete uses separate bucket fields

## Deviations from Plan

- PostgreSQL known-path mapping omitted (directory nodes not modeled in this slice)
- Used `MissingEvidence` unknown kind when file not in graph (no `CoverageGap` variant in core)

## Acceptance Checklist

- [x] `twin scan` creates file nodes for supported service config sources
- [x] `twin scan` creates active `service configured_by file` edges with linked evidence
- [x] `twin graph service:nginx.service` shows configured files
- [x] `twin graph file:/etc/nginx/nginx.conf` shows services configured by that file
- [x] `twin emulate delete /etc/nginx/nginx.conf` works after scan
- [x] `twin emulate delete file:/etc/nginx/nginx.conf` works after scan
- [x] Relative paths rejected with clear error
- [x] Non-file delete targets rejected with clear error
- [x] Delete emulation never mutates host
- [x] Overlay not persisted
- [x] Runtime and restart impact as separate fields/sections
- [x] Persistent impact when configured file hypothetically deleted
- [x] Unknown impact when graph cannot connect file to services
- [x] Risk and evidence strength separate
- [x] Evidence cites config discovery source
- [x] Human output: `No file was deleted.` and `No action was performed.`
- [x] JSON includes `action_performed: false`
- [x] Tests use fixtures; no host `/etc` reads
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` passes

## Verification

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

All passed on implementation completion.
