# Slice 5 Implementation Plan: Cgroups and systemd Service Mapping

**Implemented layout and tests:** see `docs/code-layout.md` for crate boundaries and test conventions. This slice extends the shipped slice-4 scan path: process facts already persist as observations, process nodes, and `parent_of` edges. Slice 5 adds the smallest service ownership layer that makes `twin graph nginx` useful without introducing direct systemd D-Bus yet.

## 1. Overview

Slice 5 moves `twin` from a process graph to a service-aware graph by reading `/proc/[pid]/cgroup`, creating cgroup and systemd service nodes, and materializing ownership edges.

Working demo:

```bash
twin scan
twin graph service:nginx.service
twin graph nginx
```

The ideal outcome is:

1. Every process with a usable cgroup path gets a `process -> cgroup` `in_cgroup` edge.
2. Common systemd service cgroup paths create canonical `service:<unit>` nodes.
3. Service ownership is explicit in graph state: `service -> process` and `service -> cgroup` `owns` edges.
4. Graph output separates observed facts from inferred service ownership.
5. Non-systemd hosts, missing cgroup files, permission issues, and malformed cgroup lines produce coverage warnings, not crashes.

This slice remains read-only. It does not call `systemctl`, does not talk to systemd D-Bus, does not modify cgroups, and does not inspect containers beyond preserving cgroup paths for later slices.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Scan command | `twin scan` reads processes, persists observations, nodes, edges, and collector run rows. |
| Collector crate | `twin-collectors` has a fixture-testable process collector with `ProcReader`. |
| Domain vocabulary | `NodeKind::{Process, Service, Cgroup}`, `EdgeKind::{Owns, InCgroup}`, `NodeId::service`, `NodeId::cgroup`, typed `GraphNode`, typed `GraphEdge`. |
| Observation vocabulary | `ObservationSource::ProcCgroup`, `ObservationKind::ProcessBelongsToCgroup` already exist. |
| Graph command | Process-only graph list and process neighborhood output. |
| Store | Typed node, edge, observation, and edge-observation APIs. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| Cgroup collection | Read and parse `/proc/[pid]/cgroup` for each scanned process. |
| Service inference | Derive `.service` units from systemd cgroup paths only when the path shape supports it. |
| Graph materialization | Upsert cgroup/service nodes and `in_cgroup`/`owns` edges with evidence links. |
| Scan result | Count cgroups, services, and service ownership edges. |
| Graph query | Resolve exact service IDs and service-name search terms like `nginx`. |
| Output | Render service neighborhoods with ownership and evidence sections. |
| Tests | Fixture cgroups, malformed cgroups, non-systemd paths, repeated scans, graph search, and output stability. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| Cgroup parsing | Read `/proc/[pid]/cgroup`; support cgroup v2 (`0::/path`) and v1 (`id:controllers:/path`) formats. |
| Observations | Emit `ProcessBelongsToCgroup` observations from cgroup files. |
| Cgroup nodes | Create `cgroup:<path>` nodes for normalized non-empty cgroup paths. |
| Service nodes | Create `service:<unit>` nodes when a cgroup path identifies a systemd `.service` unit. |
| Edges | Create observed `process -> cgroup` `in_cgroup`; inferred `service -> process` `owns`; inferred `service -> cgroup` `owns`. |
| Evidence links | Link `in_cgroup` edges directly to `ProcessBelongsToCgroup` observations; link inferred ownership edges to the same observation as supporting evidence. |
| Scan output | Add persisted counts for cgroups, services, `in_cgroup`, and service ownership edges. |
| Graph output | Support `twin graph service:nginx.service` and `twin graph nginx`, showing owned processes/cgroups and evidence. |
| Graceful degradation | Report non-systemd or unreadable cgroup coverage gaps without failing the scan. |
| Docs state | Update `docs/state/slice-05.md` while implementing and verify acceptance criteria on completion. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| systemd D-Bus | Later systemd refinement | The plan explicitly says optional later. Cgroup inference is enough for MVP. |
| `systemctl` calls | Never | Mutating tools and command shell-outs are outside the read-only design. |
| Unit file dependency parsing | Later config/service slices | Slice 5 is ownership, not `Requires=` or `Wants=` dependency graph. |
| Socket and port ownership | Slice 6 | Requires `/proc/net` and fd inode joins. |
| Container ownership | Later container slice | Preserve cgroup paths now; do not infer container graph prematurely. |
| Namespace collection | Later | The tech document groups namespaces with cgroups, but slice 5 acceptance criteria are service mapping only. |
| New graph crate | Later, when traversal/inference grows | Existing store-backed graph commands are enough. |
| Async collector traits | Later watch/eBPF slices | `/proc` cgroup reading is sync and local. |

---

## 4. Design Decisions

### 4.1 Extend the process collector instead of adding a separate collector crate path

The current `ProcessCollector` already enumerates PIDs and owns the race handling around `/proc/[pid]`. Reading `/proc/[pid]/cgroup` during that same pass avoids a second PID walk and keeps vanished-process behavior consistent.

Do not introduce a `Collector` trait yet. Add cgroup records to the existing concrete batch:

```text
ProcessCollector
  -> ProcessBatch
     -> ProcessRecord
     -> CgroupMembership records
     -> RawObservation values
     -> ProcessWarning values
```

This is less abstract and fits the current code.

### 4.2 Keep cgroup parsing small and explicit

Implement one parser with a narrow output:

```text
CgroupEntry {
    hierarchy_id: String,
    controllers: Vec<String>,
    path: String,
}
```

Rules:

- cgroup v2 line `0::/system.slice/nginx.service` is valid;
- cgroup v1 line `2:cpu,cpuacct:/system.slice/nginx.service` is valid;
- empty path or missing fields is malformed;
- multiple valid lines can exist for one PID;
- use the best systemd-looking path for service inference, but persist every valid membership as cgroup evidence.

Do not parse controllers into enums. They are metadata only in this slice.

### 4.3 Service ownership is inferred, not observed

`/proc/[pid]/cgroup` proves process membership in a cgroup. It does not directly prove systemd unit ownership. Therefore:

| Relationship | Edge class | Evidence wording |
|--------------|------------|------------------|
| `process -> cgroup in_cgroup` | `observed` | `/proc/<pid>/cgroup contains <path>` |
| `service -> process owns` | `inferred` | `inferred from systemd cgroup path <path>` |
| `service -> cgroup owns` | `inferred` | `inferred from systemd cgroup path <path>` |

This separation satisfies the evidence promise and the slice acceptance criterion: output must say ownership is inferred, not directly confirmed.

### 4.4 Systemd service inference is conservative

Only infer a service when a cgroup path segment ends in `.service`.

Accepted examples:

```text
/system.slice/nginx.service                         -> nginx.service
/system.slice/postgresql@14-main.service            -> postgresql@14-main.service
/user.slice/user-1000.slice/user@1000.service       -> user@1000.service
/system.slice/docker.service/container.scope        -> docker.service
```

Rejected examples:

```text
/
/user.slice/user-1000.slice/session-2.scope
/kubepods.slice/...
/docker/...
```

Rejected paths still produce cgroup nodes and `in_cgroup` edges, but no service node. This is the right degradation path on non-systemd, container-heavy, or minimal systems.

### 4.5 Canonical IDs remain the contract

Use existing constructors:

```text
NodeId::process(pid)             -> process:pid:<pid>
NodeId::cgroup(path)             -> cgroup:/system.slice/nginx.service
NodeId::service(unit_or_path)    -> service:nginx.service
EdgeId::new(from, kind, to)
```

If `NodeId::service` normalization is too permissive for cgroup paths, tighten tests before changing behavior. Domain constructors stay in `twin-core`; collectors and app code should not hand-build IDs.

### 4.6 Store materialization stays in the app command

Collectors emit observations and lightweight records. The scan command continues to own graph materialization because it already has the store transaction and can link observations to edges.

Flow:

```text
read /proc facts
  -> RawObservation values
  -> Pipeline normalize/redact
  -> persist observations
  -> upsert nodes
  -> upsert edges
  -> link edge_observations
```

Do not let `twin-collectors` depend on `twin-store`. Do not let collectors create final graph edges.

### 4.7 Add only the graph constructors this slice needs

Extend `GraphNode` and `GraphEdge` with focused constructors:

```text
GraphNode::cgroup(path, seen_at, existing)
GraphNode::service(unit, seen_at, existing)
GraphEdge::observed_in_cgroup(process, cgroup, seen_at, existing)
GraphEdge::inferred_service_owns_process(service, process, seen_at, existing)
GraphEdge::inferred_service_owns_cgroup(service, cgroup, seen_at, existing)
```

No generic builder. No risk scoring. No service-state model. Metadata should be minimal and useful, such as `{"inference":"systemd_cgroup_path"}` on inferred ownership edges.

### 4.8 Graph search resolves service aliases at the app boundary

`twin graph service:nginx.service` should parse as an exact `NodeId`.

`twin graph nginx` should resolve in this order:

1. exact node ID if the string parses as `NodeId`;
2. exact service unit if appending `.service` finds one service node;
3. substring match against service node labels/IDs if exactly one match exists;
4. clear ambiguity error if multiple services match;
5. normal not-found error if none match.

Keep this resolution in `twin-app/src/commands/graph.rs`, not in CLI rendering. CLI stays parse-call-render.

---

## 5. Collector Changes

### 5.1 Module layout

Extend the existing collector crate:

```text
crates/twin-collectors/src/process/
  collector.rs        # add cgroup observation generation
  process_record.rs   # add cgroup memberships, or expose separate batch records
  procfs.rs           # read_cgroup(pid)
  cgroup.rs           # NEW: CgroupEntry, CgroupMembership, service inference parser
  warning.rs          # add cgroup-specific warning variants
```

Tests:

```text
crates/twin-collectors/tests/
  process.rs          # existing process scan expectations expand
  cgroup.rs           # parser + collection tests
  warning.rs          # warning aggregation behavior
```

### 5.2 `ProcReader` additions

Add the smallest method needed:

```text
read_cgroup(proc_root, pid) -> Result<Option<String>, CollectorError>
```

The standard reader reads `/proc/<pid>/cgroup`. Test readers can simulate vanished, permission-denied, and malformed data without filesystem tricks where useful.

### 5.3 Raw observation shape

For each valid membership:

| Field | Value |
|-------|-------|
| source | `ObservationSource::ProcCgroup` |
| kind | `ObservationKind::ProcessBelongsToCgroup` |
| subject | `RawIdentity::Process { pid }` |
| object | `RawIdentity::Cgroup { path }` |
| raw_ref | `/proc/<pid>/cgroup` |
| confidence | `High` |
| metadata | `pid`, `cgroup_path`, `hierarchy_id`, `controllers`, optional `service_unit`, `service_inference=true` |

If `RawIdentity` does not yet have `Cgroup`, add only that variant. Do not add systemd unit raw identity unless the normalizer needs it. Service nodes can be materialized from metadata and cgroup parser output in the app layer.

### 5.4 Warnings

Add warning kinds:

```text
CgroupMissing
CgroupPermissionDenied
CgroupMalformed
```

Behavior:

- missing cgroup file: warning only when the PID otherwise exists;
- permission denied: warning, continue;
- malformed line: warning with path and line context, continue parsing other lines;
- no systemd-looking service: not a warning by itself; it is normal coverage information.

Aggregate these warnings in `ScanResult` and CLI output. Detailed JSON should include path/detail for malformed lines; high-volume missing/permission warnings should aggregate like existing process warnings.

---

## 6. Core and Observation Changes

### 6.1 `RawIdentity`

Add:

```text
RawIdentity::Cgroup { path: String }
```

Normalizer maps it to `NodeId::cgroup(path)`.

### 6.2 `ObservationKind`

`ProcessBelongsToCgroup` already exists. Keep its display value as `process_belongs_to_cgroup`. Do not add `SystemdUnitSeen`; D-Bus/unit-file collection is out of scope.

### 6.3 `GraphNode`

Add constructors only:

```text
GraphNode::cgroup(path, seen_at, existing)
GraphNode::service(unit, seen_at, existing)
```

Labels:

- cgroup label: full normalized cgroup path;
- service label: unit name, e.g. `nginx.service`.

Metadata:

- cgroup: optionally `{"source":"proc_cgroup"}`;
- service: optionally `{"source":"systemd_cgroup_inference"}`.

### 6.4 `GraphEdge`

Add constructors:

```text
observed_in_cgroup
inferred_service_owns_process
inferred_service_owns_cgroup
```

Set:

- `class=Observed` only for `in_cgroup`;
- `class=Inferred` for service `owns` edges;
- `state=Active`;
- `evidence_count` increments on repeated scans;
- `first_seen` is preserved from existing edge;
- `last_seen` updates to the current scan timestamp.

---

## 7. App Scan Workflow

### 7.1 Persist order

Inside the existing scan transaction:

1. Insert collector run.
2. Insert all normalized observations.
3. Upsert process and file nodes as slice 4 already does.
4. Upsert cgroup nodes from cgroup memberships.
5. Upsert service nodes from inferred service units.
6. Upsert `process -> cgroup` `in_cgroup` edges.
7. Upsert `service -> process` `owns` edges.
8. Upsert `service -> cgroup` `owns` edges.
9. Link observations to all cgroup-derived edges.

Foreign keys require nodes before edges. Keep all writes inside one transaction.

### 7.2 Edge evidence lookup

Build a map from `(pid, cgroup_path)` to `ObservationId` after pipeline normalization:

```text
(1234, "/system.slice/nginx.service") -> observation uuid
```

Use it to link:

- `process -> cgroup in_cgroup` with role `direct`;
- inferred `service -> process owns` with role `supporting`;
- inferred `service -> cgroup owns` with role `supporting`.

Prefer the existing role vocabulary if only `"support"` is currently used; standardize to `"supporting"` only if store/output tests are updated consistently.

### 7.3 Scan result model

Extend `ScanResult`:

```text
process_count
parent_edge_count
cgroup_count
service_count
in_cgroup_edge_count
service_owns_edge_count
observation_count
warning_count
warnings
warning_details
```

The human renderer should show a polished persisted section:

```text
persisted
├── processes: 143
├── cgroups: 24
├── services: 17
├── parent-of: 128
├── in-cgroup: 141
├── service-owns: 62
└── observations: 522
```

Keep JSON field names stable and explicit. Do not hide inference in generic edge counts.

---

## 8. Graph Command and Output

### 8.1 App result shape

Current `GraphResult` is process-centric. Extend it without breaking process output:

```text
GraphResult::List(GraphListResult)
GraphResult::Node(GraphNodeResult)
GraphResult::Service(GraphServiceResult)
```

Suggested service result:

```text
GraphServiceResult {
    service: GraphNodeSummary,
    owned_processes: Vec<GraphOwnedNode>,
    owned_cgroups: Vec<GraphOwnedNode>,
    evidence_refs: Vec<GraphEvidenceLine>,
}

GraphOwnedNode {
    id: String,
    label: String,
    edge_class: String,      # inferred
    observation_ids: Vec<String>,
}

GraphEvidenceLine {
    source: String,          # /proc/<pid>/cgroup
    statement: String,       # contains /system.slice/nginx.service
    strength: String,        # high
    relationship: String,    # inferred ownership
}
```

Use strings in the app model for rendering/JSON only after graph domain types have done validation.

### 8.2 Query behavior

Support:

```bash
twin graph service:nginx.service
twin graph nginx
twin graph --kind service
```

`--kind service` should list service nodes and owned process counts. If implementing counts requires too much query code, list service nodes only and keep ownership detail in the exact service view. Do not block the core acceptance criteria on a rich service list.

### 8.3 Human output

Service output should read like evidence-backed operational ownership, not a debug dump:

```text
twin graph
══════════
[ok] view: service neighborhood

service:nginx.service  nginx.service

owns (inferred)
├── process:pid:1432  nginx
├── process:pid:1433  nginx
└── cgroup:/system.slice/nginx.service  /system.slice/nginx.service

evidence
└── /proc/1432/cgroup contains /system.slice/nginx.service
    relationship: service ownership inferred from systemd cgroup path
```

If no service inference exists for a target process/cgroup, graph output should say that no service owner is known rather than implying absence.

### 8.4 Evidence wording

Use precise labels:

- `observed`: process membership in cgroup;
- `inferred`: service ownership from systemd cgroup path;
- never say "confirmed by systemd" in this slice.

---

## 9. Tests

### 9.1 Collector tests

Add fixtures under `crates/twin-collectors/tests/support/`:

```text
proc/
  1432/
    stat
    status
    cmdline
    cgroup
```

Cases:

- v2 systemd service path emits `ProcessBelongsToCgroup`;
- v1 systemd service path emits membership and service metadata;
- multiple cgroup lines preserve multiple memberships;
- malformed line becomes warning and does not abort scan;
- missing cgroup file becomes warning and process observations still emit;
- non-systemd cgroup path emits cgroup membership with no service inference.

### 9.2 Core/observation tests

- `RawIdentity::Cgroup` normalizes to `NodeId::cgroup`.
- `NodeId::service("/system.slice/nginx.service")` canonicalizes to `service:nginx.service`.
- `GraphNode::service` preserves `first_seen` on repeated construction.
- `GraphEdge` constructors assign correct `EdgeKind` and `EdgeClass`.

### 9.3 App integration tests

Use `IsolatedHome` and fake proc roots:

- `twin_app::scan_in` persists service and cgroup nodes.
- repeated scans update `last_seen` and preserve `first_seen`.
- `service -> process owns` and `service -> cgroup owns` edges are inferred.
- `process -> cgroup in_cgroup` edges are observed.
- `edge_observations` links exist for direct and supporting evidence.
- non-systemd fixtures complete with zero service nodes and no error.
- `graph service:nginx.service` returns owned processes and cgroups.
- `graph nginx` resolves to `service:nginx.service`.
- ambiguous graph search returns a structured error.

### 9.4 CLI output tests

Extend `crates/twin-cli/tests/output.rs`:

- scan renderer includes cgroup/service counts;
- service graph renderer includes `owns (inferred)`;
- evidence section includes `/proc/<pid>/cgroup`;
- service output uses tree connectors and no debug formatting;
- process graph output remains stable.

### 9.5 Safety tests

No new host-mutation strings are needed. If the safety crate is not implemented yet, rely on review plus `rg` before completion:

```bash
rg "systemctl|restart|stop|kill|delete|kubectl|docker stop|docker kill" crates docs
```

Any appearance must be documentation/examples only, never executable code.

---

## 10. Error Handling and Degradation

Expected conditions:

| Condition | Behavior |
|-----------|----------|
| `/proc/<pid>/cgroup` missing | Warning, keep process graph. |
| Permission denied | Warning, keep process graph. |
| Malformed cgroup line | Warning, skip that line. |
| Empty cgroup path | Warning, skip that line. |
| Non-systemd path | Persist cgroup membership, no service inference. |
| Service target not found | Structured `GraphError::NodeNotFound`. |
| Service search ambiguous | Structured graph error listing candidate service IDs. |

Do not use `unwrap()` or `expect()` in non-test code. Do not make malformed cgroup data fatal unless the whole proc root cannot be read.

---

## 11. Implementation Order

1. Add cgroup parser and service inference tests in `twin-collectors`.
2. Add `RawIdentity::Cgroup` normalization and tests.
3. Extend `ProcessCollector` to read cgroup files and emit `ProcessBelongsToCgroup`.
4. Add `GraphNode` and `GraphEdge` constructors for cgroup/service graph rows.
5. Extend `scan.rs` persistence in one transaction and update `ScanResult`.
6. Update scan human/JSON output tests.
7. Extend `graph.rs` service target resolution and service neighborhood result.
8. Add service graph renderer and output tests.
9. Update `docs/state/slice-05.md`.
10. Run verification:

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Use `rtk` when running commands in this repository.

---

## 12. Acceptance Criteria

Slice 5 is complete when:

- `twin scan` works unprivileged on systems with and without systemd cgroup paths.
- common systemd services become `service:<unit>` nodes.
- cgroup paths become `cgroup:<path>` nodes.
- services own processes through inferred `owns` edges.
- services own their cgroups through inferred `owns` edges.
- processes have observed `in_cgroup` edges to cgroups.
- `twin graph service:nginx.service` shows owned processes/cgroups and evidence.
- `twin graph nginx` resolves to the service when unambiguous.
- non-systemd systems degrade gracefully and report no inferred service ownership.
- output explicitly says service ownership is inferred from cgroup evidence.
- repeated scans update `last_seen` without resetting `first_seen`.
- tests cover parser, collector, app persistence, graph query, output, and error paths.
- `cargo build`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and `cargo fmt --check` pass.

---

## 13. Quality Bar

The best version of this slice is boring in the right way:

- cgroup parsing is deterministic, isolated, and well tested;
- service inference is conservative and easy to audit;
- graph edge classes encode truthfulness instead of optimistic wording;
- all OS reads are behind fixture-testable code;
- scan remains a single atomic persistence operation;
- CLI output is readable enough for a sysadmin to trust at a glance;
- no new abstraction exists without a current caller;
- no host mutation path exists in code.

The slice should make the graph meaningfully more useful while preserving the central product promise: every conclusion is evidence-backed, and every inference is labeled as an inference.
