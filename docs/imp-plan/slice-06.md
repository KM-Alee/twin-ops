# Slice 6 Implementation Plan: TCP Socket Scan and Inode Ownership

**Implemented layout and tests:** see `docs/code-layout.md` for current crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. This slice extends the shipped slice-5 scan path: processes, cgroups, and inferred service ownership already persist as observations, graph nodes, graph edges, and evidence links.

## 1. Overview

Slice 6 makes `twin` answer the first high-value operational question:

```text
What owns this listening port?
```

Working demo:

```bash
twin scan
twin graph port:tcp:127.0.0.1:5432
twin graph service:postgresql.service
```

The ideal outcome is:

1. `twin scan` reads `/proc/net/tcp`, `/proc/net/tcp6`, and process fd tables without mutating the host.
2. Listening sockets become canonical `port:tcp:<addr>:<port>` nodes.
3. Socket inode ownership is resolved through `/proc/<pid>/fd` symlinks, with `/proc/<pid>/fdinfo` used only for supporting metadata or fallback where useful.
4. Processes that own listener socket inodes get observed `listens_on` edges to port nodes.
5. Services that own those processes get inferred `listens_on` edges to the same port nodes.
6. Port and service graph output shows listeners with evidence and clearly separates observed process ownership from inferred service ownership.
7. Unmapped sockets, IPv6 surprises, vanished processes, and permission gaps are coverage warnings, not crashes.

This slice should stop at listener ownership. Active connections and dependency inference belong to Slice 7.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Scan command | `twin scan` collects process/cgroup observations, persists them, and materializes graph state in one store transaction. |
| Collector crate | `twin-collectors` has a concrete sync `ProcessCollector` with injectable `ProcReader`, `/proc` fixture tests, and warning aggregation. |
| Domain vocabulary | `NodeKind::Port`, `NodeId::port_tcp`, `EdgeKind::ListensOn`, `ObservationSource::ProcNetTcp`, `ObservationKind::TcpSocketSeen` already exist. |
| Graph domain | `GraphNode`, `GraphEdge`, typed store bridge, `GraphMetadata`, evidence links. |
| Service ownership | Slice 5 creates inferred `service -> process` and `service -> cgroup` `owns` edges from cgroup evidence. |
| CLI output | `twin-cli/src/output/format.rs`, scan renderer, graph renderer, JSON output. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| TCP parser | Parse `/proc/net/tcp` and `/proc/net/tcp6`, especially LISTEN rows and socket inodes. |
| FD inode join | Read `/proc/<pid>/fd` symlinks and map `socket:[inode]` to owning process ids. |
| Socket observations | Emit `TcpSocketSeen` and a process/socket ownership observation shape if needed for evidence clarity. |
| Graph materialization | Upsert port nodes and `process -> port` / `service -> port` `listens_on` edges. |
| Scan result | Count listening ports, listener edges, unmapped listener sockets, and socket warning coverage. |
| Graph query | Support exact port targets and service neighborhoods showing listening ports. |
| Output | Render port listener neighborhoods with evidence; add service "listens on" section. |
| Tests | Parser fixtures, fd join fixtures, unmapped sockets, IPv4 listener graph, IPv6 non-crash, service-level port ownership, CLI output. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| TCP table parsing | Read `/proc/net/tcp` and `/proc/net/tcp6`; parse local address, state, and inode. |
| Listener filtering | Materialize only TCP LISTEN rows (`0A`) in this slice. Preserve parser structure so Slice 7 can add established connections. |
| FD scan | Read `/proc/<pid>/fd` for each scanned process and map socket symlink targets to inodes. |
| Optional fdinfo read | Read `/proc/<pid>/fdinfo/<fd>` only as supporting metadata or fallback; do not require it for the primary join. |
| Observations | Emit `TcpSocketSeen` for each listener row and persist enough metadata to explain the inode join. |
| Port nodes | Create `port:tcp:<addr>:<port>` nodes with readable labels. |
| Process listener edges | Create observed `process -> port` `listens_on` edges when a socket inode maps to one or more processes. |
| Service listener edges | Create inferred `service -> port` `listens_on` edges when a listening process is owned by a service. |
| Evidence links | Link process listener edges directly to socket observations; link service listener edges to the same observations as supporting evidence. |
| Unknowns | Report unmapped listener socket inodes and fd permission gaps in scan output/JSON. |
| Graph output | Add exact port graph view and extend service graph view with listening ports. |
| Docs state | Update `docs/state/slice-06.md` during implementation and verify acceptance criteria on completion. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Active outbound connections | Slice 7 | This slice only maps listening ports to owners. |
| `connects_to` edges | Slice 7 | Requires non-LISTEN rows and direction-sensitive connection modeling. |
| `depends_on` inference | Slice 7 | Depends on both listeners and active connections. |
| UDP or Unix sockets | Later socket slices | The acceptance criteria are TCP listener ownership. |
| Endpoint nodes for remotes | Slice 7+ | Listener ports are local `port` nodes only. |
| `twin-graph` crate | Later, when traversal/inference grows | Store-backed graph queries are still enough. |
| Risk/evidence scoring model | Slice 8/16 | Use existing evidence count/labels; do not introduce `RiskLevel` yet. |
| Async collector traits | Later watch/eBPF slices | `/proc` reads are sync and local. |

---

## 4. Design Decisions

### 4.1 Extend the existing process collector pass

Keep one concrete collector. `ProcessCollector` already owns PID enumeration, process race handling, cgroup reads, and warning aggregation. Slice 6 should add socket collection to the same pass rather than adding a trait or a second collector:

```text
ProcessCollector
  -> ProcessBatch
     -> ProcessRecord values
     -> TcpSocketRecord values
     -> SocketOwner records
     -> RawObservation values
     -> ProcessWarning values
```

This keeps collection simple and preserves the current transaction flow in `twin-app/src/commands/scan.rs`.

### 4.2 Treat `/proc/net/tcp*` rows as socket facts, not conclusions

`/proc/net/tcp` proves a socket inode exists and is listening on an address. It does not prove process ownership by itself. Ownership is proven only after joining that inode to `/proc/<pid>/fd`:

```text
/proc/net/tcp inode 12345
  + /proc/721/fd/8 -> socket:[12345]
  => process:pid:721 listens_on port:tcp:127.0.0.1:5432
```

The collector should emit observations and records. The app scan workflow materializes graph edges because it owns the store transaction and can link evidence.

### 4.3 Listener edges are observed for processes and inferred for services

| Relationship | Edge class | Evidence wording |
|--------------|------------|------------------|
| `process -> port listens_on` | `observed` | `/proc/net/tcp inode <n>` joined with `/proc/<pid>/fd/<fd> -> socket:[<n>]` |
| `service -> port listens_on` | `inferred` | `service owns process from cgroup inference; process owns listener socket inode <n>` |

Do not mark service listener edges as observed. Service ownership itself is inferred in Slice 5, so the service-level listener is also inferred.

### 4.4 Keep TCP parsing isolated and heavily tested

The TCP table parser should be a small pure module:

```text
crates/twin-collectors/src/process/socket.rs
```

Suggested types:

```text
TcpSocketRecord
SocketState
SocketInode
SocketOwner
```

No trait, no generic parser framework, no dependency. The parser should take a string and return records plus malformed-row warnings. Tests should cover:

- IPv4 loopback: `0100007F:1538` -> `127.0.0.1:5432`;
- IPv4 wildcard: `00000000:0050` -> `0.0.0.0:80`;
- IPv6 loopback from `/proc/net/tcp6` fixtures -> `[::1]`;
- IPv6 wildcard -> `[::]`;
- LISTEN filter (`0A`);
- malformed address/state/inode rows.

IPv6 can be partial per acceptance criteria, but "partial" means unsupported rows produce warnings or are skipped safely. It must not mean panics or invalid IDs.

### 4.5 Inode is a domain detail, not a node

Do not add a socket-inode node kind. The current graph vocabulary has no `Socket` node, and adding one would make the user-facing graph noisier without helping this slice.

Store inode/fd data in observation and edge metadata:

```text
inode
fd_path
tcp_table
local_ip
local_port
address_family
socket_state
```

The graph should stay user-centered:

```text
process -> port
service -> port
```

### 4.6 Join by inode with duplicate owners allowed

One listening socket inode can appear in multiple processes after fork or descriptor passing. The correct MVP behavior is to create one observed listener edge per owning process:

```text
process:pid:721 listens_on port:tcp:127.0.0.1:5432
process:pid:722 listens_on port:tcp:127.0.0.1:5432
```

The scan result should count edges, not pretend there is a single owner.

### 4.7 Unknowns are first-class scan output

If a listener row has no matching fd owner, keep the port node and report an unknown:

```text
unmapped_listener_sockets: 3
```

Do not fail the scan. Reasons can include permission-denied fd directories, vanished processes, kernel-owned sockets, or a race between `/proc/net/tcp` and fd walking. The output should say "could not map socket inode to process" rather than guessing.

### 4.8 Reuse existing service ownership graph instead of recomputing services

The scan materializer should use the just-collected `ProcessRecord` memberships and/or stored `service -> process owns` edges to infer service listener edges. Prefer the in-memory records during the same scan because they avoid extra SQL queries, but do not duplicate service-inference parsing rules in a new module. Service inference remains owned by the Slice 5 cgroup parser.

---

## 5. Collector Changes

### 5.1 Module layout

Extend `twin-collectors`:

```text
crates/twin-collectors/src/process/
  socket.rs          # NEW: tcp table parser, fd socket target parser, socket records
  procfs.rs          # read_tcp_table, read_tcp6_table, read_fd_targets, optional read_fdinfo
  process_record.rs  # add socket owner records or keep batch-level owner map
  collector.rs       # collect tcp rows + fd inode owners
  warning.rs         # socket warning variants
```

Tests:

```text
crates/twin-collectors/tests/
  socket.rs          # parser and inode target tests
  process.rs         # full fixture collection with sockets
  warning.rs         # warning aggregation keys/text
```

### 5.2 `ProcReader` additions

Add narrow methods:

```text
read_tcp_table() -> Result<Option<String>, CollectorError>
read_tcp6_table() -> Result<Option<String>, CollectorError>
read_fd_entries(pid) -> Result<Vec<FdEntry>, CollectorError>
read_fdinfo(pid, fd) -> Result<Option<String>, CollectorError>
```

`FdEntry` should contain the fd number/path and symlink target string. Tests can implement these methods over fixture files. Production reads:

```text
/proc/net/tcp
/proc/net/tcp6
/proc/<pid>/fd/*
/proc/<pid>/fdinfo/<fd>
```

Primary ownership detection should use fd symlink targets such as `socket:[12345]`. `fdinfo` is optional and should never be the only reason scan succeeds.

### 5.3 TCP record shape

Suggested record fields:

```text
TcpSocketRecord {
    table: TcpTableKind,          # tcp or tcp6
    local_ip: String,             # normalized display form without brackets
    local_port: u16,
    state: SocketState,
    inode: u64,
    raw_line: usize,
}
```

`SocketState` needs only:

```text
Listen
Other(String)
```

Do not model every TCP state until Slice 7 needs active connections.

### 5.4 FD owner shape

Suggested record:

```text
SocketOwner {
    pid: u32,
    fd: u32,
    inode: u64,
    fd_path: PathBuf,
}
```

The join result can be kept as a batch-level map:

```text
HashMap<u64, Vec<SocketOwner>>
```

This avoids adding fd details to every `ProcessRecord` unless that proves cleaner in implementation.

### 5.5 Raw observations

For each LISTEN socket row:

| Field | Value |
|-------|-------|
| source | `ObservationSource::ProcNetTcp` |
| kind | `ObservationKind::TcpSocketSeen` |
| subject | `RawIdentity::TcpEndpoint { ip, port }` |
| object | none |
| raw_ref | `/proc/net/tcp:<line>` or `/proc/net/tcp6:<line>` |
| confidence | `High` |
| metadata | `inode`, `state="listen"`, `table`, `local_ip`, `local_port`, `owner_pids`, `owner_fds`, `mapped=true/false` |

Do not store command lines, env values, or secret material. Socket addresses and fd paths are safe operational metadata.

If ownership evidence feels too compressed in one observation, add a second observation kind only if it has a real consumer:

```text
ProcessSocketFdSeen
```

That addition is acceptable if it improves evidence lines and edge links. Do not add it preemptively if `TcpSocketSeen` metadata is enough.

### 5.6 Warnings

Add warning kinds:

```text
TcpTableMissing
TcpTablePermissionDenied
TcpTableMalformed
FdPermissionDenied
FdMalformed
FdVanished
SocketUnmapped
Tcp6UnsupportedRow
```

Behavior:

- missing `/proc/net/tcp6`: warning only if the read fails unexpectedly; absence should not fail IPv4 scan;
- malformed TCP row: warning with table path and line number; continue parsing other rows;
- permission-denied fd dir: aggregate warning and continue;
- fd symlink vanishes: aggregate warning and continue;
- unmapped listener: aggregate warning and include per-inode detail in JSON only for low-volume cases;
- IPv6 unsupported row: warning, not failure.

Use `ProcessWarningKind` naming only if that remains the shared collector warning enum. If the enum name becomes misleading, rename it only when the codebase can do so cleanly; do not add a new parallel warning system for one slice.

---

## 6. Core and Observation Changes

### 6.1 `NodeId`

`NodeId::port_tcp` already exists and should remain the only constructor for port IDs:

```text
port:tcp:127.0.0.1:5432
port:tcp:[::1]:5432
```

Add only tiny accessors if graph resolution needs them:

```text
NodeId::tcp_port_addr() -> Option<...>
```

Do not parse port IDs ad hoc in app or CLI code.

### 6.2 `GraphNode`

Add one focused constructor:

```text
GraphNode::tcp_port(ip, port, seen_at, existing)
```

Expected behavior:

- kind: `NodeKind::Port`;
- label: `tcp:<ip>:<port>` with bracketed IPv6 in the display label;
- metadata: `{"protocol":"tcp"}` plus optional address family if useful;
- preserve `first_seen` and `valid_from` from existing node.

### 6.3 `GraphEdge`

Add focused constructors:

```text
GraphEdge::observed_process_listens_on(process, port, seen_at, existing)
GraphEdge::inferred_service_listens_on(service, port, seen_at, existing)
```

Process edge:

- kind: `EdgeKind::ListensOn`;
- class: `EdgeClass::Observed`;
- metadata: include `{"source":"proc_socket_inode_join"}` or equivalent.

Service edge:

- kind: `EdgeKind::ListensOn`;
- class: `EdgeClass::Inferred`;
- metadata: include `{"inference":"service_owns_listening_process"}`.

No generic builder is needed.

### 6.4 Observation vocabulary

`ObservationSource::ProcNetTcp` and `ObservationKind::TcpSocketSeen` already exist. Add `ObservationSource::ProcFd` only if a separate fd observation is implemented. Otherwise keep fd evidence as metadata on `TcpSocketSeen`.

### 6.5 Normalizer

`RawIdentity::TcpEndpoint` already maps to `NodeId::port_tcp`. Strengthen tests for IPv6 and wildcard addresses if not already covered. Do not add a separate raw socket identity in this slice.

---

## 7. App Scan Materialization

### 7.1 Transaction flow

Keep the existing `persist_scan` shape:

```text
collect
  -> pipeline.process(raw)
  -> insert collector_run
  -> insert observations
  -> upsert graph nodes
  -> upsert graph edges
  -> link edge_observations
```

Add socket materialization inside the existing store transaction after process/service ownership is available.

### 7.2 Materialization algorithm

For each LISTEN `TcpSocketRecord`:

1. Build `port_id = NodeId::port_tcp(local_ip, local_port)`.
2. Upsert `GraphNode::tcp_port`.
3. Find socket owners from `owners_by_inode[inode]`.
4. If no owners exist:
   - increment `unmapped_listener_socket_count`;
   - keep the port node;
   - do not fabricate a process or service edge.
5. For each owner PID:
   - ensure the process node exists if it was seen in this scan or already persisted;
   - upsert observed `process -> port listens_on`;
   - link the socket observation as `direct`.
6. Resolve service owners for that process:
   - prefer service ownership produced in this scan;
   - otherwise read existing incoming `service -> process owns` edges if needed.
7. For each service owner:
   - upsert inferred `service -> port listens_on`;
   - link socket observation as `support`;
   - do not duplicate edges if multiple process owners map to the same service and port.

Use local `HashSet<NodeId>` / `HashSet<EdgeId>` caches like Slice 5's `upsert_node_once` pattern to avoid repeated work.

### 7.3 Observation ID lookup

Add a helper similar to `cgroup_observation_ids`:

```text
tcp_socket_observation_ids(observations) -> HashMap<u64, ObservationId>
```

If duplicate TCP rows share an inode, key by `(table, line)` or `(inode, local_ip, local_port)` to avoid wrong evidence links. Prefer a small typed key over string concatenation.

### 7.4 `ScanResult`

Extend `ScanResult` with:

```text
tcp_listener_count
port_count
process_listens_on_edge_count
service_listens_on_edge_count
unmapped_listener_socket_count
```

Update human output:

```text
persisted
├── processes: 143
├── cgroups: 12
├── services: 8
├── ports: 6
├── parent-of: 128
├── in-cgroup: 141
├── service-owns: 16
├── process-listens-on: 6
├── service-listens-on: 4
└── observations: 412
```

Warnings should include:

```text
3 listener sockets could not be mapped to processes
2 fd directories hidden by permissions
```

---

## 8. Graph Command and Output

### 8.1 Target support

`twin graph port:tcp:127.0.0.1:5432` should parse as an exact `NodeId` and route to a new port neighborhood.

`twin graph --kind port` is useful and should be implemented if it is small. If it risks bloating this slice, exact port graph view is the acceptance-critical path and list view can be limited to a simple sorted list.

Do not add shorthand parsing like `:5432` yet. Full canonical IDs keep ambiguity low.

### 8.2 Result model

Add a `GraphPortResult` variant:

```text
GraphPortResult {
    port: GraphNodeSummary,
    process_listeners: Vec<GraphListenerNode>,
    service_listeners: Vec<GraphListenerNode>,
    evidence: Vec<GraphEvidenceLine>,
    unknowns: Vec<GraphUnknownLine>,
}
```

`GraphListenerNode` can mirror `GraphOwnedNode` if that keeps code small:

```text
id
label
edge_class
observation_ids
```

Only add `GraphUnknownLine` if the port graph can actually retrieve unmapped-socket metadata from observations. If not, show unknowns in scan output only and defer graph unknown lines to Slice 16 coverage model.

### 8.3 Port neighborhood output

Human output should be terminal-polished and evidence-forward:

```text
twin graph
══════════
ok  view  port listener neighborhood

port:tcp:127.0.0.1:5432  tcp:127.0.0.1:5432

listeners
├── process:pid:721  postgres  observed
└── service:postgresql.service  postgresql.service  inferred

evidence
└── /proc/net/tcp:12 inode 12345 joined with /proc/721/fd/8 -> socket:[12345]
    relationship: process listener observed; service listener inferred from owning process
```

### 8.4 Service graph output

Extend `GraphServiceResult` with listening ports:

```text
listening_ports: Vec<GraphOwnedNode>
```

Human output:

```text
owns (inferred)
├── process:pid:721  postgres
└── cgroup:/system.slice/postgresql.service  /system.slice/postgresql.service

listens on (inferred)
└── port:tcp:127.0.0.1:5432  tcp:127.0.0.1:5432
```

Keep evidence lines deduplicated. Do not repeat the same `/proc/net/tcp` line ten times for a multi-process service.

---

## 9. Store Usage

No schema migration should be needed. Existing tables can store:

- port nodes in `nodes`;
- listener edges in `edges`;
- socket observations in `observations`;
- evidence links in `edge_observations`;
- socket metadata in `metadata_json`.

Add typed store helpers only if they reduce repeated boilerplate in app code. Good candidates:

```text
list_edges_by_kind_typed(kind)
list_edges_to_typed(node_id)
get_edge_typed(edge_id)
```

Do not add a repository abstraction layer beyond the existing `Store` methods.

---

## 10. Tests

### 10.1 `twin-collectors`

Add parser tests:

- parses IPv4 LISTEN row and inode;
- parses IPv6 loopback LISTEN row or records a controlled unsupported warning;
- skips non-LISTEN rows for this slice;
- rejects malformed hex address, port, state, and inode without panic;
- parses `socket:[12345]` fd target;
- ignores non-socket fd targets;
- reports vanished fd entries as warnings.

Add fixture collection tests:

- fixture with `/proc/net/tcp` listener and `/proc/721/fd/8 -> socket:[12345]` maps inode to owner;
- permission-denied fd directory produces warning and unmapped socket;
- missing `/proc/net/tcp6` does not fail IPv4 collection.

### 10.2 `twin-core`

Add constructor tests:

- `GraphNode::tcp_port` preserves first_seen on update;
- `GraphEdge::observed_process_listens_on` has observed class and increments evidence count;
- `GraphEdge::inferred_service_listens_on` has inferred class and expected metadata;
- `NodeId::port_tcp` IPv6 canonical form remains bracketed.

### 10.3 `twin-app`

Add scan/graph integration tests:

- scan persists port node and process `listens_on` edge;
- scan persists inferred service `listens_on` edge when cgroup service ownership exists;
- unmapped listener increments warning and does not fabricate owner edges;
- repeated scan updates `last_seen` and increments evidence count without duplicating nodes;
- `graph_in` for a port shows process and service listeners;
- service graph includes listening ports.

### 10.4 `twin-cli`

Add output tests:

- scan human output includes ports and listener edge counts;
- port graph output has `listeners` and `evidence` sections;
- service graph output has `listens on (inferred)`;
- JSON output includes new scan fields.

### 10.5 Safety and verification

Run:

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Tests must use fake `/proc` fixtures and isolated homes. No root, Docker, systemd D-Bus, eBPF, or host socket assumptions are allowed.

---

## 11. Implementation Sequence

1. Add pure TCP/fd parsing in `twin-collectors/src/process/socket.rs` with tests first.
2. Extend `ProcReader` and fixture support for `/proc/net/tcp`, `/proc/net/tcp6`, and `/proc/<pid>/fd`.
3. Extend `ProcessBatch` with TCP listener records and inode owner maps.
4. Emit `TcpSocketSeen` observations from listener records.
5. Add socket warning variants and CLI/JSON warning summaries.
6. Add `GraphNode::tcp_port` and listener edge constructors in `twin-core`.
7. Extend `twin-app/src/commands/scan.rs` to persist port nodes and listener edges inside the existing transaction.
8. Extend `ScanResult` and `twin-cli/src/output/scan.rs`.
9. Add `GraphPortResult` and route exact port node IDs in `twin-app/src/commands/graph.rs`.
10. Extend service graph results/rendering with listening ports.
11. Add app and CLI integration tests over fake `/proc`.
12. Update `docs/state/slice-06.md` with implemented items, decisions, deviations, and acceptance status.
13. Run full verification and fix warnings without `allow` suppressions.

---

## 12. Acceptance Criteria

| Criterion | Required outcome |
|-----------|------------------|
| Socket ownership uses inode mapping | `process -> port listens_on` exists only when `/proc/net/tcp*` inode joins to `/proc/<pid>/fd` socket target. |
| Unmapped sockets are reported as unknowns | Scan output/JSON reports unmapped listener sockets; no fake owner is created. |
| IPv4 works | Fixture IPv4 listener produces canonical `port:tcp:127.0.0.1:<port>` node and graph output. |
| IPv6 can be partial but should not crash | IPv6 loopback/wildcard is parsed if implemented; otherwise unsupported rows produce warnings and scan succeeds. |
| Service-level port ownership appears | Service graph shows inferred listening ports when service owns the listening process. |
| Evidence is visible | Port graph cites TCP table inode and fd socket target; service relationship is marked inferred. |
| Repeated scans update graph state | Existing port/listener edges preserve `first_seen`, update `last_seen`, and do not duplicate rows. |
| Read-only behavior is preserved | No mutating commands, no host writes outside the SQLite/config/log layout, tests use fixtures. |

The slice is complete only when the working demo succeeds, human output is polished, JSON output is stable, and full workspace build/test/clippy/fmt checks pass.
