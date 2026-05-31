# `twin` Technical Design Document

## Rust-First Read-Only Operational Twin for Linux, eBPF Runtime Evidence, Overlay-Based Emulation, Local Tests, and Kubernetes Inspection

---

# 1. Technical Vision

`twin` is a Rust-based, read-only CLI tool that builds an evidence-backed operational twin of a Linux system.

It does not manage the machine.

It does not mutate the machine.

It does not clone the machine.

It reads the machine, observes runtime behavior, builds a temporal graph, and predicts operational impact using lightweight graph overlays.

The core technical identity is:

```text
Read-only collectors
    ↓
Evidence observations
    ↓
Temporal operational graph
    ↓
Overlay-based emulation
    ↓
Risk, evidence, and local test reports
```

`twin` should feel like a serious systems tool for sysadmins and DevOps engineers.

It should answer:

```text
What exists?
What changed?
What depends on what?
What runtime behavior proves it?
What would likely break if I performed this action?
Which local operational assumptions still pass?
What is unknown because of missing permissions or incomplete evidence?
```

The ideal end state:

```text
twin becomes a local read-only operational twin for Linux:
an eBPF-enhanced temporal graph that lets users reason about risky operations
without touching the real system.
```

---

# 2. Core Technical Promises

## 2.1 Read-Only Promise

`twin` must never perform mutating actions.

Forbidden operations:

```text
kill process
restart service
stop service
reload daemon
delete file
edit file
change firewall rule
block traffic
install package
upgrade package
restart container
stop container
remove volume
apply Kubernetes YAML
delete Kubernetes resource
patch Kubernetes object
scale Kubernetes workload
modify secret/config map
```

Allowed operations:

```text
read files
read /proc
read /sys
read socket tables
read systemd metadata
read journal logs
read package metadata
read Docker/containerd metadata
read Kubernetes metadata
attach read-only eBPF tracing programs
store local observations
build graph state
build hypothetical graph overlays
run local read-only tests
render reports
```

The architecture must enforce this.

Read-only behavior should not rely only on developer discipline.

---

## 2.2 Observation Promise

Every external fact enters the system as an `Observation`.

Examples:

```text
ProcessSeen
ProcessHasFd
TcpSocketSeen
SystemdUnitSeen
ProcessBelongsToCgroup
ProcessMapsLibrary
ConfigProxyPass
DockerContainerSeen
K8sServiceSelectsPods
EbpfConnect
EbpfFileOpen
```

Collectors do not directly create final conclusions.

Collectors emit observations.

The graph builder, resolver, inference engine, and evidence scorer decide how observations become graph edges.

This keeps the architecture extensible.

---

## 2.3 Evidence Promise

Every meaningful output must cite evidence.

Bad output:

```text
nginx depends on django.
```

Good output:

```text
nginx.service depends on django.service.

Evidence:
- nginx config contains proxy_pass http://127.0.0.1:8000
- django.service owns process 8841
- process 8841 listens on tcp:127.0.0.1:8000
- eBPF observed nginx connecting to tcp:127.0.0.1:8000 12,430 times in 24h

Evidence strength: 94/100
```

Observed facts, inferred relationships, predictions, risk, and evidence strength must remain separate.

---

## 2.4 Temporal Promise

`twin` is not a one-time scanner.

It must remember state over time.

Required questions:

```bash
twin what-changed --since 1h
twin graph nginx --at "yesterday 18:00"
twin diff snapshot:before-upgrade snapshot:after-upgrade
twin why nginx --since 30m
```

Nodes and edges must have temporal validity:

```text
first_seen
last_seen
valid_from
valid_to
state
```

Possible states:

```text
active
inactive
missing
stale
contradicted
historical_only
unknown
```

---

## 2.5 eBPF Runtime Evidence Promise

eBPF is a primary observation source.

It should strengthen the graph with runtime facts that static scans cannot prove.

Required early eBPF events:

```text
process exec
process exit
tcp connect
tcp accept
socket bind
file open metadata
file read/write metadata
cgroup/container context
```

eBPF is only for observation.

It must not:

```text
block syscalls
deny connections
rewrite packets
kill processes
enforce policies
modify system behavior
```

---

## 2.6 Overlay Emulation Promise

`twin emulate` must not clone the machine.

It must not start duplicate services.

It must not run VM snapshots by default.

It must not perform the requested action.

It should use:

```text
base graph + hypothetical overlay patch = effective graph
```

Example:

```bash
twin emulate restart postgresql.service
```

Internal overlay:

```text
service:postgresql.service -> temporarily_unavailable
port:tcp:127.0.0.1:5432 -> temporarily_unavailable
active_connections_to_postgres -> interrupted
```

Then the graph engine analyzes direct and transitive impact.

The real system is untouched.

---

## 2.7 Local Test Promise

`twin test` runs local read-only operational checks.

It is not CI/CD.

It is not deployment automation.

It is an assertion engine over:

```text
current graph
historical graph
eBPF evidence
emulation reports
Kubernetes metadata
```

Example:

```bash
twin test run twin.yaml
```

---

# 3. High-Level Architecture

```text
┌──────────────────────────────────────────────────────────────┐
│                         User Interface                        │
│ CLI | JSON | DOT | Mermaid | future TUI                       │
└───────────────────────────────┬──────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                         App Layer                             │
│ command workflows | report assembly | policy orchestration    │
└───────────────────────────────┬──────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                    Read-Only Safety Boundary                  │
│ no mutation APIs | capability checks | forbidden action guard  │
└───────────────────────────────┬──────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                         Core Domain                           │
│ observations | nodes | edges | evidence | risk | overlays      │
└───────────────────────────────┬──────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                Observation Processing Pipeline                 │
│ collect | redact | normalize | resolve | infer | score         │
└───────────────────────────────┬──────────────────────────────┘
                                │
              ┌─────────────────┼──────────────────┐
              │                 │                  │
┌─────────────▼──────┐ ┌────────▼────────┐ ┌───────▼────────────┐
│ Collector Layer    │ │ Temporal Store   │ │ Reasoning Engines  │
│ proc systemd ebpf  │ │ SQLite WAL       │ │ graph impact why   │
│ docker k8s package │ │ observations     │ │ emulate tests      │
└────────────────────┘ └─────────────────┘ └────────────────────┘
```

---

# 4. Rust Workspace Architecture

## 4.1 Workspace Layout

```text
twin/
  Cargo.toml
  crates/
    twin-cli/
    twin-app/
    twin-core/
    twin-observation/
    twin-collectors/
    twin-store/
    twin-graph/
    twin-rules/
    twin-emulate/
    twin-test/
    twin-ebpf/
    twin-container/
    twin-k8s/
    twin-package/
    twin-config/
    twin-output/
    twin-fixtures/
    twin-safety/
```

### 4.1.1 Implemented workspace (slice 1)

Only these crates exist today. Layout and testing rules: **`docs/code-layout.md`**.

```text
crates/
  twin-cli/    # cli/, output/; lib + bin `twin`
  twin-app/    # paths/, commands/, model/, config_io/
  twin-core/   # config/
  twin-store/  # store/, migration/
```

Slice 1 does **not** ship `twin-output`; CLI owns human/JSON renderers until DOT/Mermaid justify a shared crate. Commands take `TwinLayout` (XDG or isolated temp dir in tests).

## 4.2 Crate Responsibilities

| Crate              | Responsibility                                                 |
| ------------------ | -------------------------------------------------------------- |
| `twin-cli`         | CLI parsing, subcommands, shell completion, exit codes         |
| `twin-app`         | Application workflows for scan, watch, emulate, graph, test    |
| `twin-core`        | Domain types, IDs, enums, invariants, errors                   |
| `twin-observation` | Observation schema, redaction, normalization contracts         |
| `twin-collectors`  | Linux collectors for `/proc`, `/sys`, sockets, systemd, mounts |
| `twin-store`       | SQLite schema, migrations, repositories, WAL mode, retention   |
| `twin-graph`       | In-memory graph, indexes, traversal, graph view, diff          |
| `twin-rules`       | Inference rules and evidence scoring                           |
| `twin-emulate`     | Overlay patch model, impact traversal, risk scoring            |
| `twin-test`        | YAML parser, test schema, assertion engine, test reports       |
| `twin-ebpf`        | eBPF programs, userspace loaders, event decoding               |
| `twin-container`   | Docker/containerd read-only adapters                           |
| `twin-k8s`         | Kubernetes read-only scanner and graph mapper                  |
| `twin-package`     | dpkg/rpm/pacman package ownership adapters                     |
| `twin-config`      | nginx/systemd/Docker Compose/config parsers                    |
| `twin-output`      | Human, JSON, DOT, Mermaid renderers                            |
| `twin-fixtures`    | Fake `/proc`, fake systemd, fake k8s, fake eBPF streams        |
| `twin-safety`      | Mutation-deny checks, permission model, source safety tests    |

## 4.3 Dependency Direction

Dependencies should point inward.

```text
twin-cli
  depends on twin-app

twin-app
  depends on core, store, graph, collectors, emulate, test, output

collectors
  depend on core + observation

emulate/test/why
  depend on core + graph + rules

core
  depends on almost nothing
```

Forbidden dependency direction:

```text
twin-core -> twin-ebpf
twin-core -> twin-k8s
twin-core -> twin-container
twin-graph -> twin-cli
twin-emulate -> twin-collectors
```

Reason:

* core reasoning must stay independent from operating-system mess
* eBPF/container/Kubernetes code must not leak into the core model
* tests must be able to run from fixture graphs without real system access

---

# 5. Recommended Implementation Stack

## 5.1 CLI

Recommended:

```text
clap
```

Use typed subcommands:

```rust
enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
    Scan(ScanArgs),
    Watch(WatchArgs),
    Graph(GraphArgs),
    Impact(ImpactArgs),
    Emulate(EmulateArgs),
    Test(TestArgs),
    K8s(K8sArgs),
}
```

CLI should remain thin.

It should parse arguments, call `twin-app`, and render the returned report.

---

## 5.2 Async Runtime

Recommended:

```text
tokio
```

Use async for:

```text
watch mode
eBPF event streams
Docker API calls
Kubernetes API calls
background store writes
signal handling
```

Avoid making simple commands unnecessarily complex.

`twin scan` may internally run concurrent collectors, but command behavior should remain predictable.

---

## 5.3 Storage

Recommended options:

```text
rusqlite
sqlx sqlite
```

Design preference:

```text
rusqlite for maximum local control
sqlx if compile-time checked queries become valuable
```

SQLite should use WAL mode.

Storage must support:

```text
observation log
materialized current graph
temporal history
snapshots
test runs
emulation runs
collector metadata
schema migrations
```

---

## 5.4 eBPF

Recommended primary path:

```text
Aya
```

Reason:

* Rust-native eBPF workflow
* Rust userspace loader
* Rust-side event decoding
* good fit for a Rust-first systems tool

Alternative path:

```text
libbpf-rs
```

Use only if:

* Aya lacks needed kernel support
* CO-RE workflow becomes easier through libbpf
* existing libbpf examples are much stronger for a probe

Internal design must hide this behind:

```rust
trait EbpfBackend {
    async fn load(&self, program: EbpfProgramSpec) -> Result<LoadedProgram>;
    async fn attach(&self, loaded: LoadedProgram) -> Result<AttachedProgram>;
    async fn events(&self) -> Result<EventStream<EbpfRawEvent>>;
}
```

This lets the project switch or mix backends later.

---

## 5.5 Linux Inspection

Recommended:

```text
procfs crate where suitable
manual parsers for critical files
```

Manual parsers are acceptable when:

* the required field is not exposed cleanly
* performance matters
* exact Linux format handling is needed
* permission-degraded behavior needs more control

Important files:

```text
/proc/[pid]/stat
/proc/[pid]/status
/proc/[pid]/cmdline
/proc/[pid]/exe
/proc/[pid]/fd
/proc/[pid]/fdinfo
/proc/[pid]/maps
/proc/[pid]/cgroup
/proc/[pid]/ns
/proc/net/tcp
/proc/net/tcp6
/proc/net/udp
/proc/net/unix
/proc/mounts
/sys/fs/cgroup
```

---

## 5.6 systemd

Recommended:

```text
systemd D-Bus API through a Rust D-Bus client
```

Fallback:

```text
parse unit files
read cgroup paths
infer service ownership from /proc/[pid]/cgroup
```

The systemd collector must degrade gracefully.

Not all Linux systems have systemd.

---

## 5.7 Containers

Recommended approach:

```text
Docker Engine API over Unix socket
containerd API where available
cgroup and namespace mapping as fallback
```

Allowed operations:

```text
GET/list/inspect only
```

Forbidden operations:

```text
restart
stop
kill
remove
exec
copy
commit
update
```

---

## 5.8 Kubernetes

Recommended:

```text
kube-rs
```

Use read-only API calls only:

```text
get
list
watch
logs where allowed
```

Forbidden:

```text
create
update
patch
delete
deletecollection
exec
port-forward
attach
apply
scale
rollout restart
```

The Kubernetes adapter should be usable with a least-privilege read-only RBAC role.

---

## 5.9 YAML Tests

Recommended:

```text
serde
serde_yaml or maintained YAML parser alternative
schemars for JSON schema generation if useful
```

The test format must be strict.

Unknown fields should warn or fail depending on strict mode.

No arbitrary shell execution by default.

---

# 6. Core Domain Model

## 6.1 IDs

Use typed IDs.

```rust
struct NodeId(String);
struct EdgeId(String);
struct ObservationId(Uuid);
struct SnapshotId(String);
struct TestRunId(Uuid);
struct EmulationRunId(Uuid);
```

Avoid passing raw strings around the core.

---

## 6.2 Node

```rust
struct Node {
    id: NodeId,
    kind: NodeKind,
    label: String,
    state: NodeState,
    metadata: NodeMetadata,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
    valid_from: TimestampNs,
    valid_to: Option<TimestampNs>,
}
```

Node kinds:

```rust
enum NodeKind {
    Host,
    Process,
    Service,
    File,
    Directory,
    Library,
    Package,
    Port,
    UnixSocket,
    Endpoint,
    Mount,
    User,
    Cgroup,
    Namespace,
    Container,
    Image,
    K8sNamespace,
    K8sPod,
    K8sDeployment,
    K8sReplicaSet,
    K8sService,
    K8sEndpoint,
    K8sConfigMap,
    K8sSecretRef,
    K8sPvc,
    K8sIngress,
}
```

---

## 6.3 Edge

```rust
struct Edge {
    id: EdgeId,
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
    class: EdgeClass,
    state: EdgeState,
    evidence_strength: EvidenceStrength,
    evidence_count: u32,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
}
```

Edge classes:

```rust
enum EdgeClass {
    Observed,
    Inferred,
    Predicted,
}
```

Edge kinds:

```rust
enum EdgeKind {
    Owns,
    ParentOf,
    HasFdOpen,
    ListensOn,
    ConnectsTo,
    CommunicatesWith,
    LoadsLibrary,
    ConfiguredBy,
    LogsTo,
    UsesEnvName,
    MountedOn,
    InstalledBy,
    RunsImage,
    MapsPort,
    MountsVolume,
    InNamespace,
    InCgroup,
    DependsOn,
    ProxiesTo,
    ReadObserved,
    WriteObserved,
    PossiblyReads,
    PossiblyWrites,
    RoutesTo,
    Selects,
    UsesConfigMap,
    UsesSecretRef,
    UsesPvc,
    Exposes,
}
```

---

## 6.4 Observation

Observation is the atomic evidence unit.

```rust
struct Observation {
    id: ObservationId,
    source: ObservationSource,
    kind: ObservationKind,
    subject: Option<NodeId>,
    object: Option<NodeId>,
    timestamp: TimestampNs,
    raw_ref: Option<RawEvidenceRef>,
    confidence_hint: ConfidenceHint,
    redaction_state: RedactionState,
    metadata: ObservationMetadata,
}
```

Observation sources:

```rust
enum ObservationSource {
    Proc,
    ProcFd,
    ProcFdInfo,
    ProcMaps,
    ProcNetTcp,
    ProcNetUdp,
    ProcNetUnix,
    ProcCgroup,
    ProcNamespace,
    MountInfo,
    SystemdDbus,
    SystemdUnitFile,
    Journal,
    ConfigParser,
    DockerApi,
    ContainerdApi,
    PackageManager,
    K8sApi,
    EbpfExec,
    EbpfTcp,
    EbpfFile,
    EbpfDns,
}
```

Observation examples:

```text
ProcessSeen(pid=8841, comm=gunicorn)
ProcessHasFd(pid=8841, target=socket:[12345])
TcpSocketSeen(inode=12345, local=127.0.0.1:5432, state=LISTEN)
ProcessBelongsToCgroup(pid=8841, cgroup=/system.slice/django.service)
EbpfConnect(pid=8841, remote=127.0.0.1:5432)
K8sServiceSelectsPods(service=api, selector=app=api)
```

---

## 6.5 Risk and Evidence

```rust
enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
    Unknown,
}

struct EvidenceStrength {
    score: u8,
    label: EvidenceLabel,
}
```

Evidence labels:

```text
0-30    weak
31-60   moderate
61-85   strong
86-100  very strong
```

Risk and evidence strength must never be merged.

A result can be:

```text
Risk: HIGH
Evidence: MODERATE
```

This means the possible damage is high, but confidence is only moderate.

---

# 7. Observation Pipeline

## 7.1 Pipeline Overview

```text
Collector
    ↓
RawObservation
    ↓
Redaction
    ↓
Normalization
    ↓
Entity Resolution
    ↓
Observation Persistence
    ↓
Observed Edge Creation
    ↓
Inference Rules
    ↓
Evidence Scoring
    ↓
Temporal Graph Update
```

---

## 7.2 Collector Contract

```rust
#[async_trait]
trait Collector {
    fn name(&self) -> CollectorName;
    fn source(&self) -> ObservationSource;
    fn capabilities(&self) -> CollectorCapabilities;

    async fn collect(&self, ctx: CollectorContext)
        -> Result<CollectorBatch, CollectorError>;
}
```

A collector must return:

```rust
struct CollectorBatch {
    observations: Vec<Observation>,
    warnings: Vec<CollectorWarning>,
    coverage: CollectorCoverage,
    started_at: TimestampNs,
    ended_at: TimestampNs,
}
```

Collectors must not:

```text
write system files
execute mutating commands
call systemctl restart
call docker stop/restart
call kubectl patch/delete/apply
```

---

## 7.3 Redaction

Redaction happens before persistence.

Redact:

```text
environment variable values
API keys
tokens
passwords
connection strings
authorization headers
private keys
Kubernetes secret values
sensitive config fragments
```

Store:

```text
environment variable names
secret reference names
file paths
process names
socket addresses
metadata
hashes
timestamps
redacted log snippets
```

Example:

```text
DATABASE_URL=postgres://user:pass@host/db
```

Stored as:

```text
env_name=DATABASE_URL
value_redacted=true
maybe_endpoint=host:5432
```

---

## 7.4 Normalization

Normalization creates canonical identities.

Examples:

```text
/etc/nginx/../nginx/nginx.conf
=> file:/etc/nginx/nginx.conf

127.000.000.001:5432
=> port:tcp:127.0.0.1:5432

PID 8841
=> process:pid:8841

system.slice/nginx.service
=> service:nginx.service

deployment api in namespace default
=> k8s:deployment:default/api
```

---

## 7.5 Entity Resolution

Entity resolution performs critical joins.

Required joins:

```text
socket inode -> process fd -> process
process -> cgroup -> systemd service
process -> namespace -> container
process -> cgroup -> container
socket inode -> /proc/net/tcp row -> port
config target -> listening port -> service
library path -> package owner
file path -> mount point
container port -> host port
container process -> host process
k8s service selector -> pods
k8s pod owner -> replicaset -> deployment
k8s pod container -> image
k8s volume ref -> pvc/configmap/secret_ref
```

---

## 7.6 Observed Edge Creation

Observed edges must be direct facts.

Examples:

```text
service:nginx.service OWNS process:pid:1432
process:pid:1432 LISTENS_ON port:tcp:0.0.0.0:80
process:pid:8841 CONNECTS_TO port:tcp:127.0.0.1:5432
process:pid:8841 LOADS_LIBRARY library:/lib/libssl.so.3
container:docker:redis MAPS_PORT port:tcp:0.0.0.0:6379
k8s:service:default/api SELECTS k8s:pod:default/api-123
```

---

## 7.7 Inference Rules

Inference rules create higher-level relationships.

Examples:

```text
nginx config proxy_pass target + service owns listener
=> nginx.service PROXIES_TO django.service

process connects to port + other service listens on port
=> source service DEPENDS_ON target service

process maps library + package owns library
=> service DEPENDS_ON package

container owns process + process listens on port
=> container EXPOSES port

k8s service selects pod + pod is owned by deployment
=> k8s service ROUTES_TO deployment
```

Rules should be small, explicit, testable units.

```rust
trait InferenceRule {
    fn name(&self) -> RuleName;
    fn apply(&self, graph: &GraphView) -> Vec<InferredEdge>;
}
```

---

# 8. Temporal Storage Design

## 8.1 SQLite Design

`twin` should use SQLite as the local embedded evidence store.

Recommended settings:

```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
```

Database location:

```text
~/.local/share/twin/twin.db
```

Config location:

```text
~/.config/twin/config.toml
```

Log location:

```text
~/.local/state/twin/twin.log
```

---

## 8.2 Main Tables

```sql
observations
nodes
edges
edge_observations
node_history
edge_history
snapshots
snapshot_nodes
snapshot_edges
collector_runs
emulation_runs
test_runs
schema_migrations
```

---

## 8.3 Observation Log

Purpose:

```text
preserve raw evidence
enable recomputation
support explainability
support debugging
support historical analysis
```

Simplified schema:

```sql
CREATE TABLE observations (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject_node_id TEXT,
    object_node_id TEXT,
    timestamp_ns INTEGER NOT NULL,
    confidence_hint TEXT NOT NULL,
    redaction_state TEXT NOT NULL,
    metadata_json TEXT NOT NULL
);
```

---

## 8.4 Materialized Graph

Purpose:

```text
fast current queries
fast impact traversal
fast graph rendering
```

Simplified schema:

```sql
CREATE TABLE nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    state TEXT NOT NULL,
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL,
    valid_from_ns INTEGER NOT NULL,
    valid_to_ns INTEGER,
    metadata_json TEXT NOT NULL
);

CREATE TABLE edges (
    id TEXT PRIMARY KEY,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    class TEXT NOT NULL,
    state TEXT NOT NULL,
    evidence_score INTEGER NOT NULL,
    evidence_label TEXT NOT NULL,
    evidence_count INTEGER NOT NULL,
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL
);
```

---

## 8.5 Evidence Links

```sql
CREATE TABLE edge_observations (
    edge_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    PRIMARY KEY (edge_id, observation_id)
);
```

Roles:

```text
direct
supporting
conflicting
historical
runtime_confirmation
static_confirmation
```

---

## 8.6 Atomic Update Model

Each scan or watch batch should run inside one transaction.

```text
begin transaction
store observations
upsert nodes
upsert observed edges
run inference
upsert inferred edges
update history
commit
```

Partial collector failure should not corrupt graph state.

If a collector fails:

```text
record warning
record coverage gap
continue with other collectors if safe
```

---

# 9. In-Memory Graph Design

## 9.1 Graph Structure

```rust
struct Graph {
    nodes: HashMap<NodeId, Node>,
    edges: HashMap<EdgeId, Edge>,
    outgoing: HashMap<NodeId, Vec<EdgeId>>,
    incoming: HashMap<NodeId, Vec<EdgeId>>,
    by_kind: HashMap<NodeKind, Vec<NodeId>>,
}
```

Additional indexes:

```rust
struct GraphIndexes {
    service_to_processes: HashMap<NodeId, Vec<NodeId>>,
    process_to_service: HashMap<NodeId, NodeId>,
    port_to_listeners: HashMap<NodeId, Vec<NodeId>>,
    endpoint_to_callers: HashMap<NodeId, Vec<NodeId>>,
    file_to_users: HashMap<NodeId, Vec<NodeId>>,
    package_to_loaded_by: HashMap<NodeId, Vec<NodeId>>,
    k8s_service_to_pods: HashMap<NodeId, Vec<NodeId>>,
}
```

---

## 9.2 Graph Loading Modes

Supported graph views:

```text
current graph
graph at timestamp
named snapshot graph
diff graph
emulation effective graph
```

API:

```rust
trait GraphRepository {
    fn load_current(&self) -> Result<Graph>;
    fn load_at(&self, t: TimestampNs) -> Result<Graph>;
    fn load_snapshot(&self, id: SnapshotId) -> Result<Graph>;
}
```

---

# 10. Overlay-Based Emulation Engine

## 10.1 Core Idea

Emulation is not system execution.

It is graph transformation.

```text
Current Graph
    ↓
EmulationAction
    ↓
GraphOverlay
    ↓
EffectiveGraphView
    ↓
ImpactTraversal
    ↓
RiskScoring
    ↓
EvidenceReport
```

---

## 10.2 Emulation Action Model

```rust
enum EmulationAction {
    RestartService(NodeId),
    StopService(NodeId),
    KillProcess(NodeId),
    DeleteFile(NodeId),
    BlockPort(NodeId),
    BlockEndpoint(NodeId),
    FillMount { mount: NodeId, used_percent: u8 },
    UpgradePackage(NodeId),
    RestartContainer(NodeId),
    StopContainer(NodeId),
    DeleteK8sPod(NodeId),
    RolloutK8sDeployment(NodeId),
    RebootHost,
}
```

The names describe hypothetical actions only.

They must not call real system actions.

---

## 10.3 Graph Overlay

```rust
struct GraphOverlay {
    node_overrides: HashMap<NodeId, NodeOverride>,
    edge_overrides: HashMap<EdgeId, EdgeOverride>,
    predicted_edges: Vec<Edge>,
    predicted_observations: Vec<PredictedObservation>,
    notes: Vec<OverlayNote>,
}
```

Example:

```bash
twin emulate restart postgresql.service
```

Overlay:

```text
service:postgresql.service => temporarily_unavailable
port:tcp:127.0.0.1:5432 => temporarily_unavailable
edges LISTENS_ON from postgres => temporarily_inactive
connections to postgres => interrupted
```

---

## 10.4 Effective Graph View

```rust
struct EffectiveGraphView<'a> {
    base: &'a Graph,
    overlay: &'a GraphOverlay,
}
```

Read algorithm:

```text
if overlay has node override:
    return overlay value
else:
    return base graph value
```

This avoids copying the graph.

Only the hypothetical differences are stored.

---

## 10.5 Emulation Flow

```text
1. Parse user action.
2. Resolve target node.
3. Load current or requested historical graph.
4. Build action-specific overlay.
5. Build effective graph view.
6. Find direct dependents.
7. Find transitive dependents.
8. Classify impact types.
9. Score risk.
10. Score evidence strength.
11. Detect unknowns.
12. Render report.
```

---

## 10.6 Impact Types

| Impact Type       | Meaning                                          |
| ----------------- | ------------------------------------------------ |
| Runtime impact    | What breaks while current processes keep running |
| Restart impact    | What breaks after restart/reload/reboot          |
| Transient impact  | Temporary interruption during action             |
| Persistent impact | Failure remains until manually fixed             |
| Unknown impact    | Missing evidence prevents confident prediction   |

Example:

```bash
twin emulate delete /etc/nginx/nginx.conf
```

Expected classification:

```text
Runtime impact: LOW
Restart impact: CRITICAL
```

Reason:

```text
nginx may continue running with already-loaded config,
but reload/restart may fail if config file is missing.
```

---

## 10.7 Traversal Rules

For target node `T`:

```text
direct_dependents = incoming DEPENDS_ON/CONNECTS_TO/CONFIGURED_BY/PROXIES_TO edges
transitive_dependents = recursive upstream/downstream traversal
runtime_dependents = dependents through active runtime edges
restart_dependents = dependents through config/package/library edges
```

Traversal must preserve paths.

Example path:

```text
file:/etc/nginx/nginx.conf
  CONFIGURED_BY <- service:nginx.service
  PROXIES_TO -> service:django.service
  DEPENDS_ON -> service:postgresql.service
```

Reports should show the path, not just the final affected node.

---

## 10.8 Action-Specific Overlay Builders

Each emulation action has one overlay builder.

```rust
trait OverlayBuilder {
    fn supports(&self, action: &EmulationAction) -> bool;
    fn build(&self, graph: &Graph, action: &EmulationAction) -> Result<GraphOverlay>;
}
```

Required builders:

```text
RestartServiceOverlayBuilder
StopServiceOverlayBuilder
KillProcessOverlayBuilder
DeleteFileOverlayBuilder
BlockPortOverlayBuilder
BlockEndpointOverlayBuilder
FillMountOverlayBuilder
UpgradePackageOverlayBuilder
RestartContainerOverlayBuilder
StopContainerOverlayBuilder
DeleteK8sPodOverlayBuilder
RolloutK8sDeploymentOverlayBuilder
RebootHostOverlayBuilder
```

---

## 10.9 Risk Scoring

Risk score inputs:

```text
number of direct dependents
number of transitive dependents
edge evidence strength
recent eBPF traffic volume
public exposure
service criticality
lack of redundancy
restart sensitivity
historical instability
permission gaps
unknown graph zones
Kubernetes replica redundancy
mount/disk sensitivity
```

Example scoring:

```text
Risk: HIGH

Reasons:
- 4 services depend on postgresql.service
- eBPF observed active traffic in last 10 minutes
- django.service is reachable through nginx.service
- no redundant DB target observed
- 6 processes hidden due to permissions
```

---

## 10.10 Emulation Report

```rust
struct EmulationReport {
    action: EmulationAction,
    target: NodeId,
    risk: RiskLevel,
    evidence_strength: EvidenceStrength,
    direct_impacts: Vec<Impact>,
    transitive_impacts: Vec<Impact>,
    runtime_impacts: Vec<Impact>,
    restart_impacts: Vec<Impact>,
    unknowns: Vec<Unknown>,
    evidence: Vec<EvidenceItem>,
    manual_checklist: Vec<ManualCheck>,
}
```

Manual checklist examples:

```text
confirm recent backup exists
check service-specific health endpoint
verify redundant instance exists
verify config test command manually if desired
check hidden processes by running with sudo
```

The checklist must never execute automatically.

---

# 11. eBPF Architecture

## 11.1 eBPF Subsystem Structure

```text
twin-ebpf/
  src/
    lib.rs
    backend/
      aya_backend.rs
      libbpf_backend.rs
    events/
      exec.rs
      tcp.rs
      file.rs
      dns.rs
    decode.rs
    rate_limit.rs
    capabilities.rs
  ebpf/
    exec.bpf.rs
    tcp.bpf.rs
    file.bpf.rs
    dns.bpf.rs
```

---

## 11.2 Event Flow

```text
Kernel hook
    ↓
eBPF program
    ↓
ring buffer / perf buffer
    ↓
userspace decoder
    ↓
EbpfRawEvent
    ↓
Observation
    ↓
normal pipeline
```

---

## 11.3 Required Events

### Exec Event

```rust
struct ExecEvent {
    pid: u32,
    ppid: u32,
    uid: u32,
    cgroup_id: u64,
    comm: [u8; 16],
    timestamp_ns: u64,
}
```

Observation:

```text
EbpfExecObserved(process:pid)
```

### TCP Connect Event

```rust
struct TcpConnectEvent {
    pid: u32,
    cgroup_id: u64,
    saddr: IpAddr,
    daddr: IpAddr,
    dport: u16,
    timestamp_ns: u64,
}
```

Observation:

```text
EbpfConnect(process -> endpoint/port)
```

### File Open Event

```rust
struct FileOpenEvent {
    pid: u32,
    cgroup_id: u64,
    path_hash: u64,
    access_flags: u32,
    timestamp_ns: u64,
}
```

Path handling must be privacy-aware.

Full path collection should be configurable.

---

## 11.4 eBPF Safety

Required:

```text
bounded maps
bounded event structs
rate limiting
dropped event counters
clean detach on shutdown
capability checks
kernel/BTF checks
fallback to non-eBPF scan
clear warning on verifier failure
```

Forbidden:

```text
LSM enforcement mode
packet rewrite
traffic drop
syscall denial
process kill
```

---

## 11.5 Event Rate Limiting

High-volume events must be controlled.

Rate limit dimensions:

```text
per event type
per PID
per cgroup
per endpoint
global
```

Dropped event counters must become observations:

```text
EbpfDroppedEvents(source=ebpf_tcp, count=1234)
```

This matters because dropped events weaken evidence quality.

---

# 12. Linux Collector Architecture

## 12.1 Process Collector

Reads:

```text
/proc/[pid]/stat
/proc/[pid]/status
/proc/[pid]/cmdline
/proc/[pid]/exe
```

Emits:

```text
ProcessSeen
ProcessCommandSeen
ProcessExeSeen
ProcessParentSeen
```

Failure handling:

```text
process disappeared -> warning, continue
permission denied -> coverage gap, continue
malformed file -> parser warning, continue
```

---

## 12.2 FD Collector

Reads:

```text
/proc/[pid]/fd
/proc/[pid]/fdinfo
```

Emits:

```text
ProcessHasFd
ProcessHasSocketFd
ProcessHasFileFd
ProcessHasPipeFd
```

Important:

FD collector must not assume a file descriptor target is stable.

Processes can disappear between listing and reading.

---

## 12.3 Network Collector

Reads:

```text
/proc/net/tcp
/proc/net/tcp6
/proc/net/udp
/proc/net/unix
```

Emits:

```text
TcpSocketSeen
UdpSocketSeen
UnixSocketSeen
```

Socket ownership resolution:

```text
/proc/net/tcp inode
    ↓
/proc/[pid]/fd socket:[inode]
    ↓
process owns socket
    ↓
process maps to service/container
```

Never assume `/proc/net/tcp` directly gives process ownership.

---

## 12.4 Cgroup and Namespace Collectors

Reads:

```text
/proc/[pid]/cgroup
/proc/[pid]/ns/*
/sys/fs/cgroup
```

Emits:

```text
ProcessInCgroup
ProcessInNamespace
CgroupSeen
NamespaceSeen
```

Used for:

```text
systemd ownership
container ownership
Kubernetes pod/container context
```

---

## 12.5 systemd Collector

Primary:

```text
D-Bus systemd manager and unit objects
```

Fallback:

```text
unit files
cgroup paths
service name inference
```

Emits:

```text
SystemdUnitSeen
SystemdUnitStateSeen
SystemdUnitOwnsCgroup
SystemdUnitRequires
SystemdUnitWants
```

Forbidden:

```text
StartUnit
StopUnit
RestartUnit
ReloadUnit
KillUnit
```

---

# 13. Container Architecture

## 13.1 Docker Adapter

Use Docker Engine API through Unix socket.

Allowed:

```text
list containers
inspect container
list images
inspect image
read container metadata
read port mappings
read mounts
```

Forbidden:

```text
restart
stop
kill
remove
exec
commit
update
copy
```

Emits:

```text
ContainerSeen
ContainerRunsImage
ContainerMapsPort
ContainerMountsVolume
ContainerHasCgroup
ContainerNameSeen
```

---

## 13.2 containerd Adapter

Later support.

Use for systems where Docker is not present.

Read:

```text
containers
tasks
namespaces
images
labels
runtime metadata
```

No mutation APIs.

---

## 13.3 Container-to-Host Mapping

Resolution path:

```text
container metadata
    ↓
container cgroup
    ↓
host process cgroup
    ↓
host PID
    ↓
socket/file/process observations
```

This lets `twin` connect runtime behavior to containers.

---

# 14. Kubernetes Architecture

## 14.1 Scope

Kubernetes is a read-only graph source.

`twin` should not become a Kubernetes operator.

It should model:

```text
namespace
deployment
replicaset
pod
container
service
endpoint
ingress
configmap metadata
secret references
PVC
events
logs where allowed
```

---

## 14.2 Kubernetes Collector

Allowed verbs:

```text
get
list
watch
```

Optional:

```text
read pod logs if RBAC allows
```

Forbidden:

```text
create
update
patch
delete
deletecollection
exec
attach
port-forward
apply
scale
rollout restart
```

---

## 14.3 Kubernetes Graph Mapping

Edges:

```text
deployment OWNS replicaset
replicaset OWNS pod
pod RUNS_IMAGE image
service SELECTS pod
ingress ROUTES_TO service
pod USES_CONFIGMAP configmap
pod USES_SECRET_REF secret_ref
pod USES_PVC pvc
```

Example:

```text
k8s:ingress:default/api
  ROUTES_TO k8s:service:default/api

k8s:service:default/api
  SELECTS k8s:pod:default/api-123

k8s:pod:default/api-123
  OWNED_BY k8s:replicaset:default/api-abc

k8s:replicaset:default/api-abc
  OWNED_BY k8s:deployment:default/api
```

---

## 14.4 Kubernetes Emulation

Supported hypothetical actions:

```bash
twin emulate delete pod:default/api-123
twin emulate rollout deployment:default/api
twin emulate delete service:default/api
twin emulate delete configmap:default/api-config
```

These actions only create graph overlays.

Example:

```text
Delete pod overlay:
- pod becomes unavailable
- service loses one endpoint
- deployment desired replicas unchanged
- risk depends on ready replica count
```

---

# 15. Local Test Runner Architecture

## 15.1 Purpose

`twin test` validates read-only operational assumptions.

It runs over:

```text
current graph
snapshots
emulation reports
Kubernetes metadata
eBPF evidence
```

It must not run arbitrary shell commands by default.

---

## 15.2 YAML Schema

Default file:

```text
twin.yaml
```

Example:

```yaml
version: 1

name: local-prod-readiness

defaults:
  min_evidence: moderate
  max_risk: medium

checks:
  - name: nginx listens on HTTP
    assert:
      node: port:tcp:0.0.0.0:80
      exists: true

  - name: django depends on postgres
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: strong

  - name: postgres restart is acceptable
    emulate:
      action: restart
      target: service:postgresql.service
    expect:
      max_risk: high
      require_evidence: true

  - name: no unexpected outbound endpoints
    assert:
      outbound_endpoints:
        allowed:
          - api.stripe.com:443
          - github.com:443
        fail_on_unknown: true

  - name: Kubernetes api deployment has replicas
    k8s:
      assert:
        resource: deployment/api
        namespace: default
        min_ready_replicas: 2
```

---

## 15.3 Test Flow

```text
load YAML
    ↓
validate schema
    ↓
load graph
    ↓
run graph assertions
    ↓
run emulation assertions
    ↓
run Kubernetes assertions
    ↓
calculate pass/warn/fail
    ↓
store test run
    ↓
render report
```

---

## 15.4 Check Types

| Check Type           | Meaning                                 |
| -------------------- | --------------------------------------- |
| `node exists`        | entity exists in graph                  |
| `dependency exists`  | graph edge exists with minimum evidence |
| `port listening`     | port node exists and is active          |
| `service state`      | service state matches expected          |
| `container state`    | container exists/running                |
| `disk usage`         | mount below threshold                   |
| `outbound endpoints` | observed endpoints match allowlist      |
| `emulate`            | hypothetical action risk is acceptable  |
| `k8s resource`       | Kubernetes object state check           |
| `k8s emulate`        | Kubernetes overlay impact check         |

---

## 15.5 Test Result Model

```rust
enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skipped,
    Unknown,
}

struct TestRunReport {
    name: String,
    passed: u32,
    warned: u32,
    failed: u32,
    skipped: u32,
    unknown: u32,
    checks: Vec<CheckResult>,
}
```

Output:

```text
Twin Test: local-prod-readiness

PASS nginx listens on HTTP
PASS django depends on postgres
WARN postgres restart is acceptable
FAIL no unexpected outbound endpoints
  Unexpected endpoint: telemetry.example.com:443
  Evidence: eBPF observed service:api.service connect() 42 times in 1h

Summary:
Passed: 2
Warned: 1
Failed: 1
```

---

# 16. Reasoning Engines

## 16.1 Impact Engine

Used by:

```text
twin impact
twin emulate
twin test
twin why
```

Core functions:

```rust
fn direct_dependents(graph: &GraphView, node: NodeId) -> Vec<ImpactPath>;
fn transitive_dependents(graph: &GraphView, node: NodeId) -> Vec<ImpactPath>;
fn upstream_dependencies(graph: &GraphView, node: NodeId) -> Vec<ImpactPath>;
fn runtime_dependencies(graph: &GraphView, node: NodeId) -> Vec<ImpactPath>;
fn restart_dependencies(graph: &GraphView, node: NodeId) -> Vec<ImpactPath>;
```

---

## 16.2 Why Engine

`twin why` is deterministic.

It is not an LLM chatbot.

Inputs:

```text
recent changes
service status
logs
graph dependencies
eBPF events
config parse results
socket state
container/k8s metadata
```

Output:

```text
possible causes
evidence
timeline
unknowns
manual checks
```

Example:

```text
Possible cause:
nginx returns 502 because django.service is not listening on 127.0.0.1:8000.

Evidence:
- nginx config proxies to 127.0.0.1:8000
- port 127.0.0.1:8000 was active 20 minutes ago
- django.service restarted 4 times in 10 minutes
- current scan found no listener on 127.0.0.1:8000
```

---

# 17. Output Architecture

## 17.1 Typed Reports

Every command returns a typed report.

```rust
enum Report {
    Init(InitReport),
    Doctor(DoctorReport),
    Scan(ScanReport),
    Watch(WatchReport),
    Graph(GraphReport),
    Impact(ImpactReport),
    Emulation(EmulationReport),
    WhatChanged(WhatChangedReport),
    Why(WhyReport),
    Test(TestRunReport),
    K8s(K8sReport),
}
```

Renderers:

```text
human
json
dot
mermaid
```

No command should format output directly inside business logic.

---

## 17.2 Human Output Principles

Human output should be:

```text
compact by default
evidence-rich when requested
clear about unknowns
clear about risk vs confidence
terminal-friendly
```

Flags:

```bash
--json
--format mermaid
--format dot
--verbose
--evidence
--no-color
```

---

# 18. Read-Only Safety Architecture

## 18.1 Safety Boundary

Create a dedicated crate:

```text
twin-safety
```

Responsibilities:

```text
mutation denylist
forbidden command scanner
capability mode detection
read-only adapter wrappers
test runner sandbox policy
Kubernetes verb policy
Docker API method policy
```

---

## 18.2 Forbidden Command Scan

Tests should scan source code for accidental mutating strings:

```text
systemctl restart
systemctl stop
systemctl reload
kill
rm -rf
iptables
nft
docker restart
docker stop
docker rm
kubectl apply
kubectl delete
kubectl patch
kubectl scale
kubectl rollout restart
```

This is not sufficient for security, but it catches accidental violations.

---

## 18.3 Adapter-Level Restrictions

Use explicit read-only clients.

Example:

```rust
trait DockerReadOnly {
    async fn list_containers(&self) -> Result<Vec<ContainerSummary>>;
    async fn inspect_container(&self, id: ContainerId) -> Result<ContainerInspect>;
}
```

Do not expose:

```rust
restart_container
stop_container
kill_container
remove_container
```

Same for Kubernetes:

```rust
trait K8sReadOnly {
    async fn list_pods(&self, ns: Namespace) -> Result<Vec<Pod>>;
    async fn list_services(&self, ns: Namespace) -> Result<Vec<Service>>;
    async fn list_deployments(&self, ns: Namespace) -> Result<Vec<Deployment>>;
}
```

No mutation methods.

---

# 19. Privacy Architecture

## 19.1 Default Privacy Policy

Do not store:

```text
secret values
environment variable values
Kubernetes secret contents
private keys
full sensitive config contents
request payloads
authorization headers
```

May store:

```text
secret reference names
environment variable names
config file paths
file hashes
endpoint names
port numbers
process names
package names
library paths
redacted log snippets
```

---

## 19.2 Retention

Config example:

```toml
[retention]
raw_observations_days = 7
graph_history_days = 30
ebpf_events_days = 3
test_runs_days = 30
emulation_runs_days = 30
```

Compaction should preserve materialized graph history even when old raw observations are pruned.

---

# 20. Performance Requirements

## 20.1 Scan Performance

Initial targets:

```text
small VPS: scan under 2 seconds
developer laptop: scan under 5 seconds
large host: scan under 15 seconds with warnings if exceeded
```

## 20.2 Emulation Performance

Emulation should usually be fast.

Target:

```text
common service emulation: under 300 ms after graph load
large graph emulation: under 1 second after graph load
```

Reason:

Only overlay changes should be created.

No full system copy.

No service start.

No sandbox.

---

## 20.3 Watch Overhead

Targets:

```text
low CPU usage at idle
bounded memory
bounded event queues
configurable sample/rate limits
clear dropped-event counters
```

---

# 21. Testing Strategy

## 21.0 Conventions (slice 1 onward)

Implemented rules (see `docs/code-layout.md`):

```text
integration tests in crates/<name>/tests/
shared fixtures in tests/support/ (e.g. IsolatedHome + TwinLayout::isolated)
inject TwinLayout / adapters — no global test state in production modules
avoid inline #[cfg(test)] for command and I/O paths
```

Future slices add `twin-fixtures` for fake `/proc`, systemd, K8s, eBPF streams per §21.2.

## 21.1 Unit Tests

Required:

```text
ID normalization tests
/proc parser tests
socket inode mapping tests
systemd mapping tests
graph traversal tests
inference rule tests
evidence scoring tests
risk scoring tests
overlay builder tests
YAML schema tests
Kubernetes mapping tests
redaction tests
```

---

## 21.2 Integration Tests

Required:

```text
fixture /proc scan
fake systemd D-Bus responses
fake Docker Engine API responses
fake Kubernetes API responses
fake eBPF event stream
SQLite migration tests
graph reconstruction tests
emulation report tests
test runner pass/warn/fail tests
permission-degraded scan tests
```

---

## 21.3 Snapshot Tests

Snapshot output for:

```text
twin doctor
twin scan
twin graph nginx
twin impact postgres
twin emulate restart postgres
twin emulate delete /etc/nginx/nginx.conf
twin test run twin.yaml
twin k8s graph deployment/api
```

---

## 21.4 Safety Tests

Required:

```text
no forbidden mutating command strings
Docker adapter exposes only read methods
Kubernetes adapter exposes only get/list/watch/log methods
test runner cannot execute arbitrary shell by default
emulation overlay never persists as real graph state
eBPF programs are tracing-only
```

---

# 22. Development Phases

## Phase 0: Workspace and Core

Deliver:

```text
Rust workspace
twin-cli          # cli/ + output/ (no twin-output crate yet)
twin-app          # commands/, model/, paths/, config_io/
twin-core
twin-store
config loading
error model
twin init
twin doctor
```

Module layout: `docs/code-layout.md`.

---

## Phase 1: Local Scan Graph

Deliver:

```text
process collector
fd collector
network collector
socket inode resolution
cgroup collector
basic systemd mapping
SQLite persistence
current graph loading
twin scan
twin graph
twin impact
```

---

## Phase 2: Overlay Emulation MVP

Deliver:

```text
GraphOverlay
EffectiveGraphView
RestartService overlay
DeleteFile overlay
direct impact traversal
transitive impact traversal
runtime vs restart impact
risk scoring
evidence report
twin emulate restart service
twin emulate delete file
```

---

## Phase 3: eBPF Watch

Deliver:

```text
Aya backend
capability detection
exec events
tcp connect events
tcp accept events
ring/perf buffer reader
rate limiting
dropped-event counters
evidence strengthening
twin watch --ebpf
```

---

## Phase 4: Local Test Runner

Deliver:

```text
twin test init
twin test lint
twin test run
YAML schema
node exists checks
dependency checks
port checks
disk checks
emulation checks
test result persistence
```

---

## Phase 5: Containers

Deliver:

```text
Docker read-only adapter
container nodes
image nodes
volume edges
port mapping edges
container process mapping
container-aware emulation overlays
```

---

## Phase 6: Kubernetes Read-Only

Deliver:

```text
kube-rs adapter
read pods/deployments/replicasets/services/endpoints
read configmap metadata
read secret references only
read PVCs
graph Kubernetes ownership
twin k8s scan
twin k8s graph
twin k8s impact
Kubernetes emulation overlays
```

---

# 23. Final Technical Definition

`twin` is technically defined as:

```text
A local read-only observation pipeline that converts Linux, eBPF, container,
and Kubernetes evidence into a temporal graph, then uses graph overlays
to emulate operational impact without mutating the real system.
```

The technical center is:

```text
Observation
    ↓
Temporal Evidence Graph
    ↓
Graph Overlay Emulation
    ↓
Risk + Evidence Report
    ↓
Local Operational Tests
```

The main engineering bets are:

```text
1. Every external source emits observations.
2. The core model is typed and Rust-first.
3. eBPF is a high-confidence runtime evidence source.
4. SQLite stores both raw evidence and materialized graph state.
5. Emulation is graph-overlay based, not system-cloning based.
6. Local tests are assertions over graph and emulation results.
7. Kubernetes support is read-only graph inspection.
8. Safety is enforced by crate boundaries and adapter interfaces.
9. Every conclusion remains explainable.
```

The ideal workflow:

```bash
twin init
twin doctor
twin scan
twin watch --ebpf
twin emulate restart postgresql.service
twin test run twin.yaml
twin k8s graph deployment/api
```

The system should give the user:

```text
what exists
what changed
what depends on what
what runtime evidence proves it
what would likely break
how severe the risk is
how strong the evidence is
what is unknown
what checks pass or fail
```

And it should do all of that without changing the machine.
