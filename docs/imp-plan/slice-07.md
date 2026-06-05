# Slice 7 Implementation Plan: Active Connections and First Dependency Inference

**Implemented layout and tests:** see `docs/code-layout.md` for current crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. This slice extends the shipped slice-6 socket path: processes, cgroups, services, listening ports, listener ownership, graph nodes, graph edges, observations, and evidence links already exist.

## 1. Overview

Slice 7 makes `twin` answer the first dependency question:

```text
Which services are actively depending on this service right now?
```

Working demo:

```bash
twin scan
twin graph service:django.service
twin impact service:postgresql.service
```

The ideal outcome is:

1. `twin scan` reads active TCP rows from `/proc/net/tcp` and `/proc/net/tcp6` without mutating the host.
2. Established TCP rows become observed `process -> port connects_to` edges when the socket inode joins to a process fd.
3. Service-level `service -> port connects_to` edges are inferred from the process' service ownership.
4. Service-level `service A -> service B depends_on` edges are inferred only when `A connects_to port P` and `B listens_on port P`.
5. Graph output for a service shows outgoing dependencies and incoming dependents with evidence.
6. `twin impact <service-or-port>` shows direct dependents from stored graph edges.
7. Unknowns, permission gaps, loopback ambiguity, duplicate owners, and vanished sockets are reported as coverage warnings, not crashes.

This slice should stop at active TCP dependency inference and direct impact visibility. Full risk scoring, transitive traversal, path rendering, and overlay emulation belong to later slices.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Scan command | `twin scan` collects process, cgroup, service, and listener facts; persists observations and graph state in one transaction. |
| Socket parser | `twin-collectors/src/process/socket.rs` parses `/proc/net/tcp*`, currently filtering to LISTEN rows. |
| FD inode join | `read_fd_socket_owners` maps `/proc/<pid>/fd/* -> socket:[inode]` to `SocketOwner`. |
| Graph vocabulary | `EdgeKind::ConnectsTo` and `EdgeKind::DependsOn` already exist. |
| Listener graph | Slice 6 creates port nodes, observed `process -> port listens_on`, and inferred `service -> port listens_on`. |
| Service ownership | Slice 5 creates inferred `service -> process` ownership from cgroup evidence. |
| Graph output | Service and port neighborhoods render ownership, listeners, and evidence. |
| Store | Typed node/edge/observation APIs and edge-observation links already exist. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| Active TCP parser | Preserve non-LISTEN established rows with remote endpoint data and inode. |
| Connection observations | Emit `TcpConnectionSeen` or extend socket observations cleanly for active connections. |
| Connection materialization | Upsert observed `process -> port connects_to` and inferred `service -> port connects_to`. |
| Dependency inference | Add explicit service dependency rule: service connects to a port that another service listens on. |
| Impact command | Add minimal `twin impact` command for direct dependents, without full Slice 8 risk model. |
| Output | Render dependencies/dependents in graph and impact views with observed vs inferred evidence. |
| Tests | Parser fixtures, FD join for established rows, dependency inference, direct impact, ambiguity/unknown handling, CLI output. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| TCP table parsing | Extend existing parser to keep LISTEN and ESTABLISHED rows from `/proc/net/tcp` and `/proc/net/tcp6`. |
| Direction model | Model active outbound dependencies from rows where a local socket owned by process `P` has remote address/port `R`. |
| Port nodes | Reuse `port:tcp:<addr>:<port>` for local listener ports and active remote ports. |
| Observed process edges | Create observed `process -> port connects_to` edges when established-row inode maps to process fd owner(s). |
| Inferred service edges | Create inferred `service -> port connects_to` edges from service ownership of connected processes. |
| Inferred dependencies | Create inferred `service A -> service B depends_on` edges when `A connects_to P` and `B listens_on P`. |
| Evidence links | Link process connection edges to direct TCP observations; link service connection and dependency edges with existing `direct` / `support` edge-observation roles. |
| Graph output | Extend service graph with `connects to`, `depends on`, and `depended on by` sections. |
| Impact output | Add `twin impact TARGET` with direct dependents for service and port targets. |
| Graceful degradation | Report unmapped active sockets and permission gaps without failing scan. |
| Docs state | Update `docs/state/slice-07.md` during implementation and verify acceptance criteria on completion. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Full `twin impact` risk model | Slice 8 | Slice 7 only needs direct dependents. Do not introduce `RiskLevel` early. |
| Transitive impact paths | Slice 11 | This slice creates direct `depends_on` edges only. |
| Overlay emulation | Slice 9 | No hypothetical graph changes here. |
| Endpoint node kind | Later endpoint/config slices | Reuse `port` nodes for active TCP targets to keep graph vocabulary stable. |
| UDP or Unix sockets | Later socket slices | Slice 7 acceptance is active TCP. |
| eBPF runtime confirmation | Slice 15 | `/proc/net/tcp*` gives current active state; eBPF later strengthens evidence. |
| `twin-rules` crate | Later, when inference rules multiply | One local explicit rule is enough now. |
| `twin-graph` crate | Later, when traversal grows | Store-backed queries are still sufficient. |
| Async collectors | Later watch/eBPF slices | `/proc` scan remains sync. |

---

## 4. Design Decisions

### 4.1 Extend the existing socket parser, not a new collector

Slice 6 deliberately kept active connections out but left the parser structure ready. Continue with the same concrete `ProcessCollector`:

```text
ProcessCollector
  -> ProcessBatch
     -> ProcessRecord values
     -> TCP listener records
     -> TCP active connection records
     -> socket owners by inode
     -> RawObservation values
     -> ProcessWarning values
```

Do not introduce a `Collector` trait or a separate socket collector. One PID walk and one fd owner map are already enough.

### 4.2 Split listener rows from active connection rows

Do not overload `TcpSocketRecord` until it becomes ambiguous. The current type is listener-shaped. Add a second focused record if that keeps the code clearer:

```text
TcpListenerRecord
TcpConnectionRecord
```

If renaming `TcpSocketRecord` would churn too much code, keep it as the listener record and add `TcpConnectionRecord` beside it. The important point is that connection records carry both local and remote endpoint data:

```text
table
local_ip
local_port
remote_ip
remote_port
state
inode
raw_line
```

The parser should preserve `SocketState::Listen` and add `SocketState::Established`. Other states can remain `Other(String)` and be skipped for materialization.

### 4.3 `/proc/net/tcp` proves active socket state, not service dependency

Observed:

```text
/proc/net/tcp row inode 456 state ESTABLISHED remote 127.0.0.1:5432
/proc/8841/fd/12 -> socket:[456]
=> process:pid:8841 connects_to port:tcp:127.0.0.1:5432
```

Inferred:

```text
service:django.service owns process:pid:8841
=> service:django.service connects_to port:tcp:127.0.0.1:5432

service:postgresql.service listens_on port:tcp:127.0.0.1:5432
=> service:django.service depends_on service:postgresql.service
```

This separation is non-negotiable. Observed facts, inferred relationships, evidence strength, and risk remain separate concepts.

### 4.4 Match dependencies through canonical port IDs

The dependency rule should match exactly on `NodeId::port_tcp(remote_ip, remote_port)` and existing listener port IDs.

This is intentionally conservative. It catches the important local cases:

```text
127.0.0.1:5432 -> 127.0.0.1:5432
0.0.0.0:80 listener plus 127.0.0.1:80 connection
:: listener plus ::1 connection
```

Wildcard listener matching is the one place where exact ID matching is insufficient. Add a tiny resolver that maps a remote port to matching listener ports in this order:

1. exact `port:tcp:<remote_ip>:<remote_port>`;
2. IPv4 wildcard `port:tcp:0.0.0.0:<remote_port>` when remote IP is IPv4;
3. IPv6 wildcard `port:tcp:[::]:<remote_port>` when remote IP is IPv6.

Do not add CIDR logic, routing logic, DNS, or endpoint aliasing in this slice.

### 4.5 Dependencies are service-to-service only

Create `depends_on` edges only between service nodes in Slice 7. Process-level dependency can be represented by observed `process -> port connects_to`. Do not create `process -> service depends_on`, `process -> process depends_on`, or `port -> service depends_on` edges unless a later user-facing workflow needs them.

This keeps the first dependency graph readable:

```text
service:django.service depends_on service:postgresql.service
```

### 4.6 Avoid self-dependencies

If the source service also listens on the target port, do not create `service A depends_on service A`.

Self-connections can exist, but they are not useful as operational blast-radius edges in the MVP. Keep the observed `connects_to` edge so evidence is not lost.

### 4.7 Duplicate owners and shared sockets are normal

One active socket inode can have multiple fd owners. Create one observed process connection edge per owner. Deduplicate inferred service connection and service dependency edges per `(from, to)` pair during a scan.

### 4.8 Unknowns weaken evidence but do not block graph updates

If an established TCP row has no matching fd owner:

- keep the remote port node;
- emit the connection observation with `mapped=false`;
- increment `unmapped_active_socket_count`;
- do not fabricate process, service, or dependency edges.

Reasons include permission-denied fd dirs, process races, kernel sockets, and timing gaps between `/proc/net/tcp` and fd walking. The CLI should say "could not map active socket inode to process" rather than guessing.

### 4.9 Minimal impact command belongs in `twin-app`, not a new crate

Slice 7 acceptance requires:

```text
twin impact can show direct dependents
```

Implement only that. Add `commands/impact.rs` and an `ImpactResult` model in `twin-app`; render in `twin-cli/src/output/impact.rs`.

Do not add `twin-graph`, `twin-rules`, or `twin-emulate` crates yet. Do not add risk scores. Slice 8 can expand the same command result or replace it with richer fields.

---

## 5. Collector Changes

### 5.1 Module layout

Extend existing files:

```text
crates/twin-collectors/src/process/
  socket.rs          # extend parser: established rows + connection records
  procfs.rs          # reuse tcp table reads and fd owner walk
  collector.rs       # add active connections to ProcessBatch
  warning.rs         # add active-connection warning summaries if distinct wording is needed
```

Tests:

```text
crates/twin-collectors/tests/
  socket.rs          # parser tests for ESTABLISHED rows
  process.rs         # full fixture collection with active connection + fd owner
  warning.rs         # warning aggregation text and JSON detail policy
```

### 5.2 TCP parser behavior

The parser should return both listener and active connection records:

```text
parse_tcp_table(table, content) -> TcpTableParse {
    listeners,
    connections,
    warnings,
}
```

If preserving the current tuple return is cleaner, add a new function such as `parse_tcp_table_full` and migrate call sites once. Do not run two independent parses over the same content.

Parser requirements:

- `0A` -> listener row;
- `01` -> established connection row;
- inode `0` -> skip;
- malformed row -> warning, continue;
- IPv4 little-endian parsing remains as Slice 6 fixed it;
- IPv6 parsing keeps existing no-crash behavior;
- local and remote addresses are both normalized display strings.

### 5.3 Connection record shape

Suggested record:

```text
TcpConnectionRecord {
    table
    local_ip
    local_port
    remote_ip
    remote_port
    state
    inode
    raw_line
}
```

Do not store TCP sequence, queue, uid, timeout, or retransmit fields unless a test or output needs them.

### 5.4 Raw observations

Preferred addition:

```text
ObservationKind::TcpConnectionSeen
```

Use:

| Field | Value |
|-------|-------|
| source | `ObservationSource::ProcNetTcp` |
| kind | `ObservationKind::TcpConnectionSeen` |
| subject | `RawIdentity::TcpEndpoint { ip: remote_ip, port: remote_port }` |
| object | none |
| raw_ref | `/proc/net/tcp:<line>` or `/proc/net/tcp6:<line>` |
| confidence | `High` |
| metadata | `inode`, `state="established"`, `table`, `local_ip`, `local_port`, `remote_ip`, `remote_port`, `owner_pids`, `owner_fds`, `mapped` |

Why a new observation kind: listener and active connection observations have different semantics. Reusing `TcpSocketSeen` would force graph/evidence code to branch on metadata state and make evidence wording less precise.

### 5.5 Warnings

Add one warning if current names cannot express the active case clearly:

```text
ActiveSocketUnmapped
```

Reuse existing warning kinds for malformed TCP rows and fd permission/race issues:

```text
TcpTableMissing
TcpTableMalformed
FdPermissionDenied
FdMalformed
FdVanished
```

Behavior:

- missing `/proc/net/tcp6` remains non-fatal;
- malformed established rows warn and do not affect listener rows;
- unmapped active rows warn and continue;
- fd permission-denied is aggregated, not repeated noisily in human output.

---

## 6. Core and Observation Changes

### 6.1 Observation vocabulary

Add only:

```text
ObservationKind::TcpConnectionSeen
```

Do not add eBPF observation variants in this slice. Do not add endpoint node kinds.

### 6.2 `GraphEdge`

Add focused constructors:

```text
GraphEdge::observed_process_connects_to(process, port, seen_at, existing)
GraphEdge::inferred_service_connects_to(service, port, seen_at, existing)
GraphEdge::inferred_service_depends_on(source_service, target_service, seen_at, existing)
```

Metadata:

| Constructor | Edge kind | Class | Metadata |
|-------------|-----------|-------|----------|
| process connects | `ConnectsTo` | `Observed` | `{"source":"proc_tcp_established_inode_join"}` |
| service connects | `ConnectsTo` | `Inferred` | `{"inference":"service_owns_connected_process"}` |
| service depends | `DependsOn` | `Inferred` | `{"inference":"active_connection_to_listening_service"}` |

No generic edge builder. No evidence scoring changes.

### 6.3 `NodeId`

Reuse `NodeId::port_tcp`. Add tiny accessors only if wildcard matching needs them:

```text
NodeId::tcp_port_parts() -> Option<(IpAddr-like string, u16)>
```

Do not parse port IDs ad hoc throughout app code.

### 6.4 `GraphNode`

Reuse `GraphNode::tcp_port` for remote connection targets. A remote endpoint is still represented as a `port` node in this slice.

---

## 7. App Scan Materialization

### 7.1 Transaction flow

Keep the same transaction shape:

```text
collect
  -> pipeline.process(raw)
  -> insert collector_run
  -> insert observations
  -> upsert graph nodes
  -> upsert observed edges
  -> upsert inferred edges
  -> link edge_observations
```

Add connection materialization after listener materialization, then run dependency inference after both `connects_to` and `listens_on` edges are available.

### 7.2 Observation lookup

Add:

```text
tcp_connection_observation_ids(observations) -> HashMap<ConnectionObservationKey, ObservationId>
```

Key by at least:

```text
inode
remote_ip
remote_port
raw_line
table
```

Do not key active connections by inode alone. Inode-only was acceptable for listener MVP but is too lossy once connection and listener rows coexist.

### 7.3 Connection materialization algorithm

For each `TcpConnectionRecord`:

1. Build `remote_port_id = NodeId::port_tcp(remote_ip, remote_port)`.
2. Upsert the remote port node.
3. Find socket owners from `owners_by_inode[inode]`.
4. If no owners exist:
   - increment `unmapped_active_socket_count`;
   - keep the port node and observation;
   - do not create process/service edges.
5. For each owner PID:
   - ensure process node exists;
   - upsert observed `process -> remote_port connects_to`;
   - link the connection observation as `direct`.
6. Resolve service owners for that process using the same `services_by_pid` map Slice 6 uses.
7. For each source service:
   - upsert inferred `service -> remote_port connects_to`;
   - link the connection observation as `support`.

Use local `HashSet<(NodeId, NodeId)>` caches for process and service connection edge counts.

### 7.4 Dependency inference algorithm

Run after connection edges and listener edges are materialized.

Inputs:

```text
source service -> connected port
listener port -> target service(s)
```

Build `listeners_by_port` from:

- current scan's inferred `service -> port listens_on` edges, plus
- stored `service -> port listens_on` edges if needed to support a dependency after a partial scan.

For each source service connection:

1. Resolve candidate listener ports with exact/wildcard matching.
2. For each target service listening on a candidate port:
   - skip if source service == target service;
   - upsert inferred `source_service -> target_service depends_on`;
   - link the connection observation as `direct`;
   - link the listener observation as `support` when available.

Preferred evidence roles:

| Edge | Evidence role |
|------|---------------|
| `process -> port connects_to` | `direct` connection observation |
| `service -> port connects_to` | `support` connection observation |
| `service -> service depends_on` | `direct` connection observation, `support` listener observation |

This makes the final dependency explainable without pretending the service-level dependency was directly observed.

### 7.5 `ScanResult`

Extend with:

```text
tcp_connection_count
process_connects_to_edge_count
service_connects_to_edge_count
service_depends_on_edge_count
unmapped_active_socket_count
```

Human output should add rows in the existing `persisted` section:

```text
├── active-connections: 3
├── process-connects-to: 3
├── service-connects-to: 2
├── service-depends-on: 1
```

Warnings should include:

```text
2 active sockets could not be mapped to processes
```

---

## 8. Graph Command and Output

### 8.1 Service neighborhood

Extend `GraphServiceResult` with:

```text
connected_ports
dependencies
dependents
```

Where:

- `connected_ports` are outgoing inferred `service -> port connects_to`;
- `dependencies` are outgoing inferred `service -> service depends_on`;
- `dependents` are incoming inferred `service -> service depends_on`.

Keep existing `owned_processes`, `owned_cgroups`, `listening_ports`, and `evidence`.

Human output:

```text
service:django.service  django.service

owns (inferred)
└── process:pid:8841 gunicorn

connects to (inferred from active TCP)
└── port:tcp:127.0.0.1:5432 tcp:127.0.0.1:5432

depends on (inferred)
└── service:postgresql.service postgresql.service

evidence
├── /proc/net/tcp:12 active connection inode 456 remote 127.0.0.1:5432
└── /proc/net/tcp:4 listener inode 12345 joined with /proc/721/fd/8
```

### 8.2 Port neighborhood

Extend `GraphPortResult` with callers:

```text
process_callers
service_callers
```

This makes `twin graph port:tcp:127.0.0.1:5432` useful from both sides:

```text
listeners
└── service:postgresql.service inferred

callers
└── service:django.service inferred
```

### 8.3 Evidence lines

Add `connection_evidence_line` beside `socket_evidence_line`:

```text
/proc/net/tcp:12 inode 456 established from 127.0.0.1:50122 to 127.0.0.1:5432 joined with /proc/8841/fd/12
relationship: process connection observed; service dependency inferred from listener match
```

Keep evidence concise. The graph renderer should not dump metadata JSON.

---

## 9. Impact Command MVP

### 9.1 CLI and app shape

Add:

```text
twin impact TARGET
twin impact --json TARGET
```

Files:

```text
crates/twin-app/src/commands/impact.rs
crates/twin-app/src/model/impact_result.rs
crates/twin-cli/src/output/impact.rs
```

Request:

```text
ImpactRequest {
    config_override
    target
    target_query
}
```

Use the existing graph target resolver style. Exact `NodeId` should work. Service shorthand like `postgresql.service` or `postgresql` should work if unambiguous.

### 9.2 Result model

Keep it direct and future-compatible:

```text
ImpactResult {
    target
    direct_dependents
    evidence
    unknowns
}
```

Each dependent should include:

```text
node id
label
relationship
edge class
observation ids
```

Do not include `risk` or `evidence_strength` yet. Slice 8 owns those fields.

### 9.3 Target behavior

For service target:

- direct dependents = incoming `depends_on` service edges;
- include listener-port context in evidence, not as separate dependents.

For port target:

- direct dependents = incoming `connects_to` service/process edges;
- include listener services as context, not dependents.

Unsupported target kinds return a clear error.

### 9.4 Human output

Example:

```text
twin impact
═══════════
ok  view: direct dependents

target
└── service:postgresql.service postgresql.service

direct dependents
└── service:django.service django.service
    relationship: depends_on inferred

evidence
├── /proc/net/tcp:12 active connection inode 456 to 127.0.0.1:5432
└── /proc/net/tcp:4 listener inode 12345 joined with /proc/721/fd/8
```

Avoid phrases like "will break". This command reports known direct dependents, not predicted operational damage.

---

## 10. Tests

### 10.1 Collector tests

Add fixtures for:

- established IPv4 row: local ephemeral port to remote `127.0.0.1:5432`;
- established IPv6 row no-crash;
- non-established non-listen row skipped;
- active row inode joined through fd symlink;
- unmapped active row warning;
- malformed active row warning.

### 10.2 Core tests

Add constructor tests:

- observed process `connects_to`;
- inferred service `connects_to`;
- inferred service `depends_on`;
- metadata matches expected source/inference strings;
- repeated constructor preserves `first_seen` and increments evidence count.

### 10.3 App integration tests

Use a fake `/proc` with:

```text
process 8841 -> django.service
process 721  -> postgresql.service
postgres listener: 127.0.0.1:5432 inode 12345 fd/8
django active connection: local 127.0.0.1:50122 remote 127.0.0.1:5432 inode 456 fd/12
```

Tests:

- `scan_in_persists_active_connection_edges`;
- `scan_in_infers_service_dependency`;
- `graph_in_service_shows_dependencies_and_dependents`;
- `graph_in_port_shows_callers`;
- `impact_in_service_shows_direct_dependents`;
- `impact_in_port_shows_direct_callers`;
- `unmapped_active_connection_does_not_create_dependency`;
- wildcard listener matching: listener `0.0.0.0:5432`, connection `127.0.0.1:5432`.

### 10.4 CLI output tests

Add stable renderer tests for:

- scan output includes active connection counts;
- graph service output includes `connects to`, `depends on`, and `depended on by`;
- port output includes callers;
- impact output includes target, direct dependents, and evidence.

CLI E2E should cover at least:

```bash
twin scan
twin graph django
twin impact postgresql
```

with `TWIN_PROC_ROOT` fixture, as current CLI tests already do.

---

## 11. Acceptance Criteria

| Criterion | Required outcome |
|-----------|------------------|
| Active TCP connections create graph edges | `process -> port connects_to` observed edges exist when inode joins fd owner. |
| Service-level `DEPENDS_ON` is inferred | `service A -> service B depends_on` appears when A connects to a port B listens on. |
| Evidence separates observed from inferred | Output shows active connection as observed evidence and dependency as inferred relationship. |
| `twin impact` can show direct dependents | Service and port targets return direct dependents/callers from stored graph state. |
| Read-only preserved | Only `/proc` and SQLite local state are read/written; no host mutation commands or APIs. |
| Graceful degradation | Unmapped active sockets and fd permission gaps are warnings, not failures. |
| Tests ship with slice | Collector, core, app, CLI output, and CLI E2E tests cover normal and error paths. |

---

## 12. Implementation Order

1. Extend `ObservationKind` with `TcpConnectionSeen`; add tests for enum round-trip if needed.
2. Extend the TCP parser to return established connection records and update parser tests.
3. Extend `ProcessBatch` and collector observations for active connections.
4. Add `GraphEdge` constructors for `connects_to` and `depends_on`.
5. Extend scan materialization for process/service `connects_to`.
6. Add dependency inference inside the scan transaction.
7. Extend `ScanResult` and scan CLI output.
8. Extend graph result models and graph renderers.
9. Add minimal `impact` command, model, CLI args, renderer, and tests.
10. Update `docs/state/slice-07.md` as implementation progresses.
11. Verify:

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

---

## 13. Quality Bar

- No mutation APIs, shell-outs, restarts, kills, firewall edits, or service commands.
- No `unwrap()` or `expect()` in non-test code.
- No new crate unless the slice has multiple real consumers for it.
- No generic rule framework for one rule.
- No raw string graph IDs in app logic; use `NodeId` and `EdgeId` constructors.
- No metadata JSON dumping in human output.
- No risk wording in Slice 7 impact output.
- No self-dependency edges.
- No loss of observed facts when inference cannot be made.
- Every conclusion in output must be traceable to at least one observation line.
