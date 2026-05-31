# Slice 4 Implementation Plan: Process Scan and Process Graph

**Implemented layout and tests:** see `docs/code-layout.md` for the current crate boundaries, `TwinLayout`, and integration-test conventions. This slice adds the first real host scan while keeping the CLI thin and all OS reads behind fixture-testable adapters.

## 1. Overview

Slice 4 makes `twin scan` produce the first real graph from process data and makes `twin graph` able to show that graph.

Working demo:

```bash
twin scan
twin graph --kind process
twin graph process:pid:1234
```

The useful result is intentionally narrow:

1. Read process facts from `/proc/[pid]/stat`, `/proc/[pid]/status`, `/proc/[pid]/cmdline`, and `/proc/[pid]/exe`.
2. Emit and persist process observations.
3. Materialize process nodes and parent-child `PARENT_OF` edges in SQLite.
4. Report scan counts and warnings.
5. Render process graph views from stored graph rows.

This is the first vertical slice where host facts become durable graph state. It must still be read-only, unprivileged, and resilient to normal `/proc` races.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| CLI/app foundation | `twin init`, `twin doctor`, JSON output, `TwinLayout` injection. |
| Store | SQLite migrations through version 2, `nodes`, `edges`, `observations`, `collector_runs`, and `edge_observations`. |
| Core IDs | `NodeId`, `EdgeId`, `ObservationId`, `TimestampNs`, `NodeKind`, `NodeState`, `EdgeKind`, `EdgeClass`, `EdgeState`. |
| Observation pipeline | `RawObservation -> redact -> normalize -> Observation`, typed observation row bridge. |
| Graph row repositories | String row APIs for upserting/listing nodes and edges. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| Collector crate | `twin-collectors` with a process collector and a fake `/proc` fixture path. |
| Typed graph structs | Minimal `Node` and `Edge` domain structs, or typed store bridge helpers, so app code does not hand-build string rows. |
| Scan command | App workflow that collects, persists observations, materializes graph rows, and records a collector run. |
| Graph command | App workflow and CLI rendering for process graph listings and a single process neighborhood. |
| Process observation variants | `ProcessCommandSeen`, `ProcessExeSeen`, `ProcessParentSeen` in addition to `ProcessSeen`. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| `twin-collectors` crate | Add a small sync process collector with an injectable proc root. |
| `/proc` parsing | Parse pid directories, `stat` pid/ppid/comm, selected `status` fields, NUL-separated `cmdline`, and `exe` symlink target. |
| Observations | Emit `ProcessSeen`, `ProcessCommandSeen`, `ProcessExeSeen`, `ProcessParentSeen`. |
| Redaction | Redact suspicious command-line argument values before persistence. Preserve argument names and safe command shape. |
| Graph materialization | Upsert `process:pid:<pid>` nodes and observed `ParentOf` edges. Link parent edges to their direct observations. |
| `twin scan` | Add CLI args, app result model, human renderer, JSON output via existing global `--json`. |
| `twin graph` | Add `--kind process` listing and exact `process:pid:<pid>` neighborhood. |
| Tests | Fixture `/proc`, disappearing-process races, redaction, repeated-scan `last_seen`, graph command output, no host mutation safety checks in scope of this crate. |
| Docs state | Update `docs/state/slice-04.md` as implementation progresses. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Cgroup and systemd ownership | Slice 5 | Process-only graph first. |
| Socket scan and port ownership | Slice 6 | Requires `/proc/net` and fd inode joins. |
| Active connections and dependency inference | Slice 7 | Needs sockets and service mapping. |
| `twin impact` | Slice 8 | Depends on service/port dependency edges. |
| `twin-graph` crate | Later, when traversal/inference grows | The store already supports listing nodes/edges; a crate now would be mostly wrappers. |
| Async collector trait | Later watch/eBPF slices | `/proc` process scan is sync and local. |
| PID reuse guarantees across boot | Later temporal model refinement | Current canonical ID is `process:pid:<pid>` per plan. Store command metadata to help humans, but do not invent process instance IDs yet. |

---

## 4. Design Decisions

### 4.1 Add `twin-collectors`, but keep it small

Slice 4 is the first real collector, so it should introduce the crate named in `AGENTS.md` and the tech document:

```text
crates/twin-collectors/
  src/
    lib.rs
    error.rs
    process/
      mod.rs
      collector.rs
      procfs.rs
      process_record.rs
      warning.rs
  tests/
    process.rs
    support/
      mod.rs
```

Dependency direction:

```text
twin-collectors -> twin-core, twin-observation
twin-app        -> twin-core, twin-observation, twin-store, twin-collectors
twin-cli        -> twin-app
```

The collector reads only from a configured proc root. Production uses `/proc`; tests use a temp fixture tree. No production env-var hooks and no global test mutexes.

### 4.2 Use a concrete collector, not a trait

Only one collector exists in this slice. A generic `Collector` trait would be speculative. Use:

```rust
pub struct ProcessCollector {
    proc_root: PathBuf,
}

impl ProcessCollector {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self;
    pub fn collect(&self, started_at: TimestampNs) -> Result<ProcessBatch, CollectorError>;
}
```

When multiple collectors need common orchestration, introduce a trait then.

### 4.3 Treat `/proc` races as warnings, not errors

Processes can disappear between listing `/proc` and reading individual files. That is normal. The collector should:

- skip vanished PIDs;
- increment `vanished_processes`;
- include a warning with source path and observation;
- continue scanning.

Malformed files and permission-denied files should also be warnings unless the whole proc root is unreadable. The scan command should return success with warnings when partial coverage is available.

### 4.4 Keep observed facts separate from materialized edges

Collector output remains observations. The app scan workflow materializes the process graph from those observations in this slice:

```text
ProcessParentSeen(parent, child)
  -> process parent node
  -> process child node
  -> observed ParentOf edge parent -> child
```

No inferred edges are created in slice 4. Every edge class is `observed`.

### 4.5 Redact command values before persistence

The command line is sensitive. `ProcessCommandSeen` metadata must not store likely secret values. Redaction rules should be conservative and simple:

- redact `--password=value`, `--token=value`, `--secret=value`, `--key=value`, `--api-key=value`;
- redact values following separate sensitive flags, such as `--password value` or `--token value`;
- redact URI userinfo in arguments, such as `postgres://user:pass@host/db`;
- preserve executable name, argument names, and non-sensitive arguments;
- mark `RedactionState::Partial` when any value changes.

This extends the existing redaction pipeline rather than bypassing it.

### 4.6 Graph command reads stored graph rows

`twin graph` should not rescan. It reads the SQLite graph state built by the most recent scan. If no database or graph rows exist, return a clear app error such as "database is not initialized" or a graph result with zero nodes, depending on the existing doctor/init error pattern.

---

## 5. Core and Observation Changes

### 5.1 `twin-observation` vocabulary

Extend `ObservationKind`:

```rust
pub enum ObservationKind {
    ProcessSeen,
    ProcessCommandSeen,
    ProcessExeSeen,
    ProcessParentSeen,
    TcpSocketSeen,
    ProcessBelongsToCgroup,
}
```

Use `ObservationSource::Proc` for all process observations in this slice.

Expected observation shapes:

| Kind | Subject | Object | Metadata |
|------|---------|--------|----------|
| `ProcessSeen` | child process node | none | `pid`, `comm`, optional `state`, optional `uid`, optional `gid`. |
| `ProcessCommandSeen` | process node | none | redacted `argv_json`, `argc`, `command_redacted`. |
| `ProcessExeSeen` | process node | file node | optional `exe_path`, `exe_readable`. |
| `ProcessParentSeen` | parent process node | child process node | `ppid`, `pid`. |

For `ProcessExeSeen`, the object should be `NodeId::file(path)` when `read_link` succeeds. If `exe` is unreadable or missing, emit no object and record a warning instead of fabricating a file node.

### 5.2 Minimal typed graph domain

Add only what the scan app needs to avoid manual string rows:

```text
crates/twin-core/src/
  graph_node.rs
  graph_edge.rs
```

Suggested types:

```rust
pub struct GraphNode {
    id: NodeId,
    kind: NodeKind,
    label: String,
    state: NodeState,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
    valid_from: TimestampNs,
    valid_to: Option<TimestampNs>,
    metadata: GraphMetadata,
}

pub struct GraphEdge {
    id: EdgeId,
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
    class: EdgeClass,
    state: EdgeState,
    evidence_count: u32,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
    metadata: GraphMetadata,
}
```

Do not add risk or real evidence scoring here. Store defaults can remain:

```text
evidence_score = 100
evidence_label = "strong"
```

for directly observed parent-child edges, or a small typed evidence helper can be added only if the row conversion needs it. Risk stays out of slice 4.

If implementing full structs feels too large, the acceptable narrower alternative is typed conversion helpers in `twin-store`:

```rust
impl Store {
    pub fn upsert_process_node(...);
    pub fn upsert_parent_edge(...);
}
```

The preferred path is `GraphNode`/`GraphEdge` because they are immediately consumed by scan and graph results.

### 5.3 Store typed bridge

Add conversions:

```rust
impl From<&GraphNode> for NodeRow;
impl TryFrom<&NodeRow> for GraphNode;
impl From<&GraphEdge> for EdgeRow;
impl TryFrom<&EdgeRow> for GraphEdge;
```

Add thin wrappers:

```rust
impl Store {
    pub fn upsert_node_typed(&mut self, node: &GraphNode) -> Result<(), StoreError>;
    pub fn upsert_edge_typed(&mut self, edge: &GraphEdge) -> Result<(), StoreError>;
    pub fn get_node_typed(&self, id: &NodeId) -> Result<Option<GraphNode>, StoreError>;
    pub fn list_nodes_by_kind_typed(&self, kind: NodeKind) -> Result<Vec<GraphNode>, StoreError>;
}
```

Keep existing row APIs. They are useful for low-level tests and do not need churn.

---

## 6. Process Collector Design

### 6.1 Data model

```rust
pub struct ProcessRecord {
    pid: u32,
    ppid: Option<u32>,
    comm: Option<String>,
    state: Option<String>,
    uid: Option<u32>,
    gid: Option<u32>,
    argv: Vec<String>,
    exe: Option<PathBuf>,
}
```

Accessors should be used rather than public fields unless the type stays entirely crate-local. Store no raw secret values after the redaction step.

### 6.2 Batch result

```rust
pub struct ProcessBatch {
    observations: Vec<RawObservation>,
    records: Vec<ProcessRecord>,
    warnings: Vec<ProcessWarning>,
    started_at: TimestampNs,
    ended_at: TimestampNs,
}
```

`records` are for graph materialization without reparsing observation metadata. They are app-internal inputs, not persisted as raw structs.

### 6.3 Warnings

```rust
pub enum ProcessWarningKind {
    Vanished,
    PermissionDenied,
    Malformed,
    ExeUnreadable,
}

pub struct ProcessWarning {
    kind: ProcessWarningKind,
    path: PathBuf,
    detail: String,
}
```

User-facing scan output should aggregate warning counts:

```text
Warnings:
- 4 processes disappeared during scan
- 2 process exe links unreadable
```

JSON can include both aggregate counts and detailed warnings.

### 6.4 Parsing details

`/proc/[pid]/stat`:

- parse `pid`, `comm`, `state`, `ppid`;
- handle `comm` inside parentheses and allow spaces in `comm`;
- malformed rows become a warning for that PID.

`/proc/[pid]/status`:

- parse `Uid:` first value as real uid;
- parse `Gid:` first value as real gid;
- ignore unknown lines.

`/proc/[pid]/cmdline`:

- split on `\0`;
- empty file is valid for kernel threads;
- preserve empty argv as `[]`.

`/proc/[pid]/exe`:

- use `read_link`;
- permission denied or vanished process becomes a warning;
- do not follow or open the target.

### 6.5 Fixture design

Test fixtures should mirror `/proc` minimally:

```text
tests/support/proc_fixture.rs
tmp/
  proc/
    1/
      stat
      status
      cmdline
      exe -> /usr/lib/systemd/systemd
    42/
      stat
      status
      cmdline
      exe -> /usr/bin/nginx
```

Use temp dirs and symlinks where the platform supports them. If symlinks are awkward in one test, isolate that test behind Unix-only `#[cfg(unix)]`; this project targets Linux, so that is acceptable.

---

## 7. Scan Workflow

### 7.1 App API

Add to `twin-app/src/lib.rs`:

```rust
pub struct ScanRequest {
    pub config_override: Option<PathBuf>,
}

pub fn scan(request: ScanRequest) -> Result<ScanResult, AppError>;
pub fn scan_in(layout: &TwinLayout, request: ScanRequest, proc_root: &Path) -> Result<ScanResult, AppError>;
```

The injected `proc_root` variant is for tests. Production `scan` passes `/proc`.

### 7.2 App modules

```text
crates/twin-app/src/
  commands/
    scan.rs
    graph.rs
  model/
    scan_result.rs
    graph_result.rs
```

Keep scan orchestration in app. The collector only reads and emits raw observations. The store only persists rows.

### 7.3 Scan steps

```text
1. Resolve `TwinLayout`.
2. Open initialized store at `layout.db_file()`.
3. Run `ProcessCollector` against proc root.
4. Pass raw observations through existing observation pipeline.
5. Insert `collector_runs` row with collector = "proc_process".
6. Insert observations, associated with that collector run.
7. Materialize process nodes from records.
8. Materialize observed parent-child edges.
9. Link parent edges to `ProcessParentSeen` observations where available.
10. Return `ScanResult`.
```

The current typed observation insert sets `collector_run_id` to `None`. Slice 4 should add either:

```rust
pub fn insert_observation_typed_for_run(&mut self, obs: &Observation, run_id: i64) -> Result<(), StoreError>;
```

or a row helper that keeps the typed conversion but fills `collector_run_id`.

### 7.4 Repeated scans and timestamps

For existing nodes/edges:

- preserve `first_seen_ns` from the existing row;
- set `last_seen_ns` to this scan time;
- keep `valid_from_ns` unchanged;
- keep state `active`.

Use store reads before upsert or change the SQL upsert to preserve `first_seen_ns`:

```sql
first_seen_ns = nodes.first_seen_ns,
valid_from_ns = nodes.valid_from_ns
```

The current `upsert_node` update path does not overwrite `first_seen_ns`, which is good. Make sure new callers pass sane `first_seen_ns` for first insert and rely on conflict behavior for repeats. Verify with tests.

### 7.5 Result model

```rust
pub struct ScanResult {
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub process_count: usize,
    pub parent_edge_count: usize,
    pub observation_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<ScanWarning>,
}
```

Warnings can be detailed in JSON and aggregated in human output.

Human output:

```text
Scan complete.

Nodes:
- processes: 143

Edges:
- parent-child: 128

Warnings:
- 4 processes disappeared during scan
```

If there are no warnings, omit the section or render `Warnings: none` consistently with existing output style.

---

## 8. Graph Workflow

### 8.1 CLI shape

Add:

```rust
pub enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
    Scan(ScanArgs),
    Graph(GraphArgs),
}

pub struct ScanArgs {
    pub config: Option<PathBuf>,
}

pub struct GraphArgs {
    pub kind: Option<String>,
    pub target: Option<String>,
}
```

Clap behavior:

```bash
twin graph --kind process
twin graph process:pid:1234
```

Do not add DOT/Mermaid or time travel in this slice.

### 8.2 App API

```rust
pub struct GraphRequest {
    pub config_override: Option<PathBuf>,
    pub kind: Option<NodeKind>,
    pub target: Option<NodeId>,
}

pub fn graph(request: GraphRequest) -> Result<GraphResult, AppError>;
pub fn graph_in(layout: &TwinLayout, request: GraphRequest) -> Result<GraphResult, AppError>;
```

Parse CLI strings to `NodeKind`/`NodeId` at the boundary. If `--kind` is not `process`, return a clean unsupported-kind error for now.

### 8.3 Results

For `--kind process`:

```rust
pub struct GraphListResult {
    pub kind: NodeKind,
    pub nodes: Vec<GraphNodeSummary>,
}
```

For `process:pid:<pid>`:

```rust
pub struct GraphNodeResult {
    pub node: GraphNodeSummary,
    pub outgoing: Vec<GraphEdgeSummary>,
    pub incoming: Vec<GraphEdgeSummary>,
}
```

For process parent-child, incoming edges are parents where the target is child; outgoing edges are children where the target is parent.

Human output example:

```text
Processes

- process:pid:1 systemd
- process:pid:42 nginx
```

Single process example:

```text
process:pid:42 nginx

Parents:
- process:pid:1 systemd

Children:
- process:pid:43 nginx: worker process

Evidence:
- /proc/42/stat ppid=1
```

Evidence can be minimal in this slice: edge summaries should list linked observation IDs or raw refs when available. Do not claim inferred dependencies.

---

## 9. CLI Rendering

Add:

```text
crates/twin-cli/src/output/
  scan.rs
  graph.rs
```

Keep JSON generic through existing `output::json::render`.

Main dispatch additions:

```rust
Command::Scan(args) => run_scan(&cli.global, &args),
Command::Graph(args) => run_graph(&cli.global, &args),
```

CLI remains a thin shell:

```text
parse args -> call twin_app -> render result
```

No store access, no `/proc` parsing, and no graph business logic in `twin-cli`.

---

## 10. Error Handling

### 10.1 Collector errors

`twin-collectors/src/error.rs`:

```rust
pub enum CollectorError {
    ProcRootRead { path: PathBuf, source: std::io::Error },
    InvalidProcRoot { path: PathBuf },
}
```

Per-PID failures are warnings, not `Err`.

### 10.2 App errors

Extend `AppError` with:

```rust
Collector { source: twin_collectors::CollectorError }
InvalidGraphTarget { value: String, source: twin_core::ParseError }
UnsupportedGraphKind { kind: String }
```

Use structured variants carrying source errors. Do not collapse errors into strings.

### 10.3 No panics

Non-test code must not use `unwrap()` or `expect()`. Parsing failures return `Result` or warnings. Vanishing paths are expected.

---

## 11. Tests

### 11.1 `twin-collectors`

Integration tests:

| Test | Assertion |
|------|-----------|
| `collects_process_records_from_fixture_proc` | Reads pid, ppid, comm, uid/gid, argv, exe. |
| `handles_comm_with_spaces_in_stat` | Parses `stat` correctly when comm contains spaces. |
| `empty_cmdline_is_valid` | Kernel-thread-like process produces empty argv, not error. |
| `malformed_stat_becomes_warning` | Bad process file is skipped and scan continues. |
| `vanished_process_becomes_warning` | Listed PID deleted mid-scan is reported without crashing. |
| `redacts_sensitive_command_values` | Secret-looking args are redacted before observations are persisted. |

If simulating deletion during scan needs a hook, add an injected filesystem reader struct in tests rather than production env vars.

### 11.2 `twin-store`

Add typed graph round-trip tests:

- `graph_node_round_trips_through_store`;
- `graph_edge_round_trips_through_store`;
- `upsert_preserves_first_seen_and_updates_last_seen`;
- `edge_observation_link_round_trips`.

### 11.3 `twin-app`

Use `IsolatedHome` plus fake proc root:

- `scan_in_persists_process_nodes_and_parent_edges`;
- `scan_in_records_collector_run`;
- `scan_in_repeated_scan_updates_last_seen`;
- `graph_in_lists_process_nodes`;
- `graph_in_returns_process_neighborhood`;
- `graph_in_rejects_unsupported_kind_cleanly`.

### 11.4 `twin-cli`

Renderer tests:

- human scan output includes process and parent-child counts;
- warning aggregation renders correctly;
- graph list output is stable;
- graph node output separates parents and children.

### 11.5 Verification command

Final verification for the slice:

```bash
rtk cargo build
rtk cargo test --workspace
rtk cargo clippy --workspace -- -D warnings
rtk cargo fmt --check
```

---

## 12. Acceptance Criteria Mapping

| Acceptance criterion | Implementation evidence |
|----------------------|-------------------------|
| Works unprivileged | Collector only reads `/proc` files and symlinks; permission errors become warnings. |
| Disappearing processes do not crash scan | Vanished PID test and warning aggregation. |
| Repeated scans update `last_seen` | Store/app test verifies `last_seen_ns` changes while `first_seen_ns` remains stable. |
| Process graph is visible | `twin graph --kind process` and `twin graph process:pid:<pid>` render stored graph rows. |
| Process command values are redacted where suspicious | Redaction tests cover flag values, `key=value`, and URI userinfo. |
| No host mutation | Collector uses read-only filesystem operations only; tests run against temp proc fixtures. |

---

## 13. Implementation Order

1. Add `twin-collectors` crate to the workspace with error type, process modules, and fixture tests.
2. Extend `ObservationKind` with process command/exe/parent variants and tests for display/from-str.
3. Implement process file parsing against an injectable proc root.
4. Implement command-line redaction and raw observation emission.
5. Add minimal typed graph structs or typed store bridge helpers; add store round-trip tests.
6. Implement `ScanResult`, `scan_in`, and `scan`; persist collector run, observations, nodes, edges, and edge-observation links.
7. Add `Scan` CLI args, dispatch, renderer, and renderer tests.
8. Implement `GraphRequest`, `GraphResult`, `graph_in`, and `graph`.
9. Add `Graph` CLI args, dispatch, renderer, and renderer tests.
10. Update `docs/state/slice-04.md` with status, decisions, deviations, and acceptance status.
11. Run full verification and fix warnings.

---

## 14. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| `/proc/[pid]/stat` parser breaks on process names with spaces or parentheses | Test realistic stat lines and parse by locating the last `)` before state/ppid fields. |
| Command-line metadata leaks secrets | Run all process command observations through redaction and test common secret forms. |
| Typed graph structs grow into premature graph engine | Keep them as data carriers plus constructors only. No traversal, no inference rules. |
| Scan command mutates graph first and observations second | Persist observations and graph rows in one store transaction after collection. If the transaction fails, no partial scan state should be committed. |
| PIDs are reused | Accept for slice 4 because canonical ID is specified as `process:pid:<pid>`. Store labels and timestamps to make recency visible. Revisit with process start time when temporal accuracy becomes a requirement. |
| `graph` output implies conclusions | Label only observed parent-child relationships. Do not use dependency language. |

---

## 15. Documentation Updates During Implementation

Update `docs/state/slice-04.md` as work lands:

- `Status`: `in-progress` at first code change, `complete` only after verification passes.
- `Implemented`: collector, scan, graph, tests.
- `Decisions`: concrete choices such as no collector trait, process-only graph, redaction rules.
- `Deviations from Plan`: anything intentionally different from this implementation plan.

If a better convention emerges while implementing, update `AGENTS.md` and the slice state file together, per the self-evolution rule.
