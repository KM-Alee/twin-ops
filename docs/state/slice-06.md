# Slice 6: TCP Socket Scan and Inode Ownership

## Status: complete

## Goal (recap)

Answer: **what process/service owns this listening TCP port?** via read-only `/proc` only — no `ss`, netlink, or host mutation.

Working demo:

```bash
twin scan
twin graph port:tcp:127.0.0.1:5432
twin graph service:postgresql.service
```

## Implemented

### Collectors (`twin-collectors`)

- **`process/socket.rs`** (pure parser)
  - `parse_tcp_table` for `/proc/net/tcp` and `/proc/net/tcp6`
  - IPv4 address byte-reversal; IPv6 per 32-bit word reversal
  - LISTEN-only materialization (`st == 0A`); non-LISTEN rows skipped
  - `parse_socket_fd_target` for `socket:[inode]` symlinks
  - `owners_by_inode` helper from `SocketOwner` list
  - Unit tests: IPv4 listen, IPv6 loopback listen, non-LISTEN skip, malformed row warning, fd target parsing

- **`process/procfs.rs`**
  - `ProcReader::read_dir` required (no default stub); `read_dir_paths` for `StdProcReader`
  - `read_tcp_table_content` — missing `tcp` warns `TcpTableMissing`; missing `tcp6` silent
  - `read_fd_socket_owners` — walks `/proc/<pid>/fd`, permission/vanished/malformed warnings

- **`process/collector.rs`**
  - Same `ProcessCollector` pass as slice 5: after process records, collects TCP listeners + FD owners
  - `ProcessBatch`: `tcp_listeners`, `owners_by_inode`, accessors
  - `TcpSocketSeen` raw observations (`ProcNetTcp`, subject `TcpEndpoint`, metadata: inode, table, local_ip/port, mapped, owner_pids, owner_fds)
  - `SocketUnmapped` warning per listener inode with no FD owner (port still observed)

- **`process/warning.rs`**
  - New kinds: `TcpTableMissing`, `TcpTableMalformed`, `FdPermissionDenied`, `FdMalformed`, `FdVanished`, `SocketUnmapped`

- **Exports** (`lib.rs` / `process/mod.rs`): `TcpSocketRecord`, `TcpTableKind`, `SocketOwner`, `parse_tcp_table`, `parse_socket_fd_target`

- **Tests** (`tests/socket.rs`, `tests/support/mod.rs`): `write_tcp_table`, `write_socket_fd`; full collect with inode join; unmapped listener warning

### Core (`twin-core`)

- `GraphNode::tcp_port(ip, port, seen_at, existing)` — label `tcp:<ip>:<port>` (bracketed IPv6 in label)
- `GraphEdge::observed_process_listens_on` — `EdgeClass::Observed`, metadata `proc_socket_inode_join`
- `GraphEdge::inferred_service_listens_on` — `EdgeClass::Inferred`, metadata `service_owns_listening_process`
- Test: `tcp_port_and_listener_edges` in `tests/graph_constructors.rs`

### App (`twin-app`)

- **`commands/scan.rs`**
  - Resolves port IDs before transaction; materializes port nodes + `listens_on` inside existing `with_transaction`
  - Observed `process -> port` with `direct` evidence link on `TcpSocketSeen`
  - Inferred `service -> port` when slice-5 cgroup service ownership exists for listening PID (`pid_to_services` from scan records)
  - Dedupes service listener edges per `(service, port)` via `seen_service_listeners`
  - Unmapped listeners: port node upserted, no fabricated process/service edges, `unmapped_listener_socket_count` incremented
  - `tcp_socket_observation_ids` keyed by inode
  - Extended warning aggregation for all socket-related `ProcessWarningKind`s

- **`commands/graph.rs`**
  - `twin graph <port:tcp:…>` → `GraphResult::Port` (process + service listeners, socket evidence lines)
  - `twin graph --kind port` → sorted port list
  - Service neighborhood: `listening_ports` from outgoing inferred `listens_on` edges

- **Models**
  - `ScanResult`: `tcp_listener_count`, `port_count`, `process_listens_on_edge_count`, `service_listens_on_edge_count`, `unmapped_listener_socket_count`, `socket_owner_inode_count`
  - `GraphPortResult`, `GraphServiceResult.listening_ports`, `GraphResult::Port`
  - `ScanError::InvalidPortId` from `ParseError`

- **Integration tests** (`tests/scan_graph.rs`, `tests/support/mod.rs`)
  - Fixture: postgres PID 721, cgroup `postgresql.service`, TCP LISTEN `127.0.0.1:5432` inode 12345, `fd/8 -> socket:[12345]`
  - `scan_in_persists_port_and_listener_edges` — DB nodes/edges, counts
  - `graph_in_port_neighborhood` — listeners + evidence
  - `graph_in_lists_port_nodes`
  - `graph_in_service_shows_listening_ports`

### CLI (`twin-cli`)

- **`output/scan.rs`**: tree rows `ports`, `process-listens-on`, `service-listens-on`; socket warning summaries via `ProcessWarningKind::cli_summary`
- **`output/graph.rs`**: `render_port` (listeners + evidence); service section `listens on (inferred)`; `--kind port` list title
- **`tests/output.rs`**: scan counts include socket fields; service render includes listening ports

### Observation / store

- Reuses existing `ObservationKind::TcpSocketSeen`, `ObservationSource::ProcNetTcp`, `RawIdentity::TcpEndpoint` → `NodeId::port_tcp` (slice 3)
- No schema migration; port nodes, listener edges, observations, `edge_observations` as in slice 2–5

## Acceptance criteria

| Criterion | Status | Evidence |
|-----------|--------|----------|
| Socket ownership uses inode mapping | done | `process -> port listens_on` only when inode joins FD; collector + `scan_in_persists_port_and_listener_edges` |
| Unmapped sockets reported | done | `SocketUnmapped` warning + `unmapped_listener_socket_count`; no fake owner edges |
| IPv4 works | done | Fixture → `port:tcp:127.0.0.1:5432`, graph port view |
| IPv6 partial, no crash | done | Parser + test for `::1`; malformed rows → warnings, scan continues |
| Service-level port ownership | done | `graph_in_service_shows_listening_ports`, inferred `service -> port` |
| Evidence visible | done | Port graph `GraphEvidenceLine` (tcp line + inode/fd join statement) |
| Repeated scans update graph | done | Same pattern as slice 5 (`first_seen` preserved, `last_seen`/evidence on upsert) — covered by existing scan repeat tests for nodes; listener edges use same upsert helpers |
| Read-only preserved | done | Procfs-only collectors; fixture tests; safety rules unchanged |

## Decisions

- **Single collector pass** — extend `ProcessCollector`, not a second collector type or trait.
- **Facts vs conclusions** — `/proc/net/tcp*` proves listen socket + inode; ownership only after FD join.
- **Process listener = observed; service listener = inferred** — service ownership still from slice-5 cgroup inference.
- **Inode not a graph node** — inode/fd/tcp line live in observation metadata and evidence strings only.
- **Duplicate PIDs allowed** — multiple `process -> port` edges for same port when SO_REUSEPORT / shared inode across processes.
- **LISTEN only** — parser keeps structure for slice 7 (`connects_to`, established rows).
- **TCP column layout** — after `st`, skip exactly **three** fields (`tx_queue`, `rx_queue`, `tr`) before `uid`, `timeout`, `inode` (see post-ship note below).
- **`read_dir` on `ProcReader`** — required on all test doubles (e.g. `VanishPidReader` delegates to inner).

## Post-ship fix (parser bug)

Initial implementation skipped **four** fields after `st`, reading **inode from the wrong column** (often `1` instead of the real inode, e.g. `12345`). Symptom: `owners_by_inode` keyed correctly by FD scan but listener records had wrong inode → `SocketUnmapped`, `socket_owner_inode_count` mismatch, zero `listens_on` edges despite good FD symlinks. Fix: skip three fields, not four. Not a persist/scan architecture issue.

## Out of scope (unchanged from plan)

- Established TCP / `connects_to` / `depends_on` (slice 7)
- UDP, Unix sockets, endpoint nodes for remotes
- `twin-graph` crate, risk scoring, async collectors
- Shorthand port queries (`:5432` only)
## Deviations from plan

- **tcp6 row warnings** — plan mentioned `Tcp6UnsupportedRow`; bad IPv6 rows use `TcpTableMalformed` (no separate enum variant).
- **`ProcessSocketFdSeen`** — not added; `TcpSocketSeen` metadata carries fd paths.
- **`fdinfo`** — not read (plan allowed optional; FD symlink is sole join).
- No dedicated CLI output test for port graph renderer (covered indirectly via app graph tests + manual layout in `render_port`).

## Verification

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Last verified: workspace tests passing (171 tests), clippy clean, fmt clean.

## Simplify pass (post-ship)

- Removed unused `Tcp6UnsupportedRow` warning variant.
- `TcpTableKind::net_file_name()` replaces repeated `tcp`/`tcp6` matches; dropped unused `path()`.
- `collect_tcp_sockets` returns `owners_by_inode` once (no second HashMap build).
- `process_listens_on_edge_count` dedupes per `(process, port)`; avoids multi-FD over-count.
- Scan persist: borrow `owners_by_inode`, index listeners by inode, no full listener clones.
- Graph evidence: shared `collect_observation_evidence`; CLI `append_evidence_lines`.
- **`socket_owner_inode_count`** — count of distinct socket inodes seen via `/proc/<pid>/fd` (not listener-only).

## In progress / blocked

_(nothing)_
