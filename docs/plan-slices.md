# `twin` Build Plan

## 23 Vertical Slices for Incremental Development

---

# 0. Development Rule

Every slice must end with something usable.

A slice is complete only when it has:

```text
1. A working CLI command or visible command improvement
2. A real user-facing output
3. Storage or graph behavior where needed
4. Fixture or integration tests
5. Clear acceptance criteria
6. No host mutation
```

Avoid building huge horizontal systems like “all graph logic” or “all storage” first.

Build like this instead:

```text
collect one useful fact
store it
connect it to the graph
show it in the CLI
test it
then expand
```

---

# Slice 1: CLI, Config, Init, and Doctor Foundation

## Goal

Create the usable `twin` binary and local state foundation.

## Working Demo

```bash
twin --help
twin --version
twin init
twin doctor
twin doctor --json
```

## Build

Create (slice 1 workspace — see `docs/code-layout.md`):

```text
crates/
  twin-cli/     # binary `twin` + lib (cli/, output/)
  twin-app/     # commands/, model/, paths/, config_io/
  twin-core/    # config/
  twin-store/   # store/, migration/
```

Do **not** create `twin-output` in this slice; render in `twin-cli/src/output/`.

Implement:

* `clap` CLI (`twin-cli/src/cli/`)
* clean error model (`thiserror` per crate)
* `twin init` / `twin doctor` via `twin-app` commands
* `TwinLayout` for XDG paths (injected in tests — no global test hooks)
* local config file + template sync (`config_io`)
* human + JSON output (`twin-cli/src/output/`)
* integration tests under each crate’s `tests/` (e.g. `twin-app/tests/support/IsolatedHome`)

Paths:

```text
~/.config/twin/config.toml
~/.local/share/twin/twin.db
~/.local/state/twin/twin.log
```

## Output Example

```text
Twin Doctor

Core:
- CLI: OK
- Config: OK
- Database: not initialized
- Permission mode: unprivileged
```

## Acceptance Criteria

* `twin init` creates config and state dirs
* running `twin init` twice does not overwrite config
* `twin doctor` reports config, DB, OS, permission mode
* `--json` works
* normal CLI errors do not panic

---

# Slice 2: SQLite Store and Migration System

## Goal

Create durable local storage.

## Working Demo

```bash
twin init
twin doctor
```

## Build

Create:

```text
crates/
  twin-store/
```

Implement:

* SQLite connection
* WAL mode
* schema migrations
* schema version table
* repository layer
* basic transaction wrapper

Initial tables:

```text
schema_migrations
observations
nodes
edges
edge_observations
collector_runs
```

SQLite settings:

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
```

## Output Example

```text
Database:
- path: ~/.local/share/twin/twin.db
- status: OK
- schema version: 1
- journal mode: WAL
```

## Acceptance Criteria

* DB is created by `twin init`
* migrations are idempotent
* `twin doctor` detects schema version
* corrupt or missing DB gives clean errors
* tests can create temporary DBs

---

# Slice 3: Core Domain Model and Observation Pipeline MVP

## Goal

Define the core language of the system before adding many collectors.

## Working Demo

```bash
twin doctor --json
cargo test core
```

## Build

Create:

```text
crates/
  twin-observation/
```

Implement typed models:

```text
NodeId
EdgeId
ObservationId
TimestampNs
NodeKind
EdgeKind
ObservationSource
RiskLevel
EvidenceStrength
CollectorName
```

Implement first pipeline shape:

```text
Raw observation
  ↓
Redaction
  ↓
Normalization
  ↓
Observation persistence
```

Initial node kinds:

```text
host
process
service
port
file
cgroup
```

Initial edge kinds:

```text
PARENT_OF
OWNS
IN_CGROUP
LISTENS_ON
CONNECTS_TO
CONFIGURED_BY
DEPENDS_ON
```

## Acceptance Criteria

* all domain-critical concepts use newtypes/enums
* no raw stringly-typed graph core
* observations can be stored and queried
* redaction hook exists even if simple
* unit tests cover IDs and normalization

---

# Slice 4: Process Scan and Process Graph

## Goal

Make `twin scan` produce the first real graph.

## Working Demo

```bash
twin scan
twin graph --kind process
twin graph process:pid:1234
```

## Build

Create process collector.

Read:

```text
/proc/[pid]/stat
/proc/[pid]/status
/proc/[pid]/cmdline
/proc/[pid]/exe
```

Emit observations:

```text
ProcessSeen
ProcessCommandSeen
ProcessExeSeen
ProcessParentSeen
```

Create nodes:

```text
process:pid:<pid>
```

Create edges:

```text
process parent PARENT_OF child
```

## Output Example

```text
Scan complete.

Nodes:
- processes: 143

Edges:
- parent-child: 128

Warnings:
- 4 processes disappeared during scan
```

## Acceptance Criteria

* works unprivileged
* disappearing processes do not crash scan
* repeated scans update `last_seen`
* process graph is visible
* process command values are redacted where suspicious

---

# Slice 5: Cgroups and systemd Service Mapping

## Goal

Move from process-level graph to service-level graph.

## Working Demo

```bash
twin scan
twin graph service:nginx.service
twin graph nginx
```

## Build

Read:

```text
/proc/[pid]/cgroup
/sys/fs/cgroup
```

Optional later:

```text
systemd D-Bus
```

Create nodes:

```text
cgroup:/system.slice/nginx.service
service:nginx.service
```

Create edges:

```text
process IN_CGROUP cgroup
service OWNS process
service OWNS cgroup
```

For MVP, infer systemd service from cgroup path.

## Output Example

```text
service:nginx.service

Owns:
- process:pid:1432 nginx
- process:pid:1433 nginx

Evidence:
- /proc/1432/cgroup contains /system.slice/nginx.service
```

## Acceptance Criteria

* common systemd services become nodes
* services own processes
* graph search finds service by name
* non-systemd systems degrade gracefully
* output says when service ownership is inferred, not directly confirmed

---

# Slice 6: TCP Socket Scan and Inode Ownership

## Goal

Map ports to processes/services correctly.

## Working Demo

```bash
twin scan
twin graph port:tcp:127.0.0.1:5432
twin graph service:postgresql.service
```

## Build

Read:

```text
/proc/net/tcp
/proc/net/tcp6
/proc/[pid]/fd
/proc/[pid]/fdinfo
```

Implement inode join:

```text
/proc/net/tcp socket inode
  ↓
/proc/[pid]/fd socket:[inode]
  ↓
process owns socket
  ↓
process maps to service
```

Create nodes:

```text
port:tcp:127.0.0.1:5432
```

Create edges:

```text
process LISTENS_ON port
service LISTENS_ON port
```

## Output Example

```text
port:tcp:127.0.0.1:5432

Listeners:
- process:pid:721 postgres
- service:postgresql.service

Evidence:
- /proc/net/tcp inode 12345
- /proc/721/fd/8 -> socket:[12345]
```

## Acceptance Criteria

* socket ownership uses inode mapping
* unmapped sockets are reported as unknowns
* IPv4 works
* IPv6 can be partial but should not crash
* service-level port ownership appears

---

# Slice 7: Active Connections and First Dependency Inference

## Goal

Detect service-to-service dependencies from active connections.

## Working Demo

```bash
twin scan
twin graph service:django.service
twin impact service:postgresql.service
```

## Build

Extend TCP parser for active connections.

Create edges:

```text
process CONNECTS_TO port
service CONNECTS_TO port
```

Inference rule:

```text
service A CONNECTS_TO port P
service B LISTENS_ON port P
=> service A DEPENDS_ON service B
```

## Output Example

```text
service:django.service

Dependencies:
- service:postgresql.service
  Evidence:
  - django process owns socket
  - postgres listens on tcp:127.0.0.1:5432
  - active connection observed
```

## Acceptance Criteria

* active TCP connections create graph edges
* service-level `DEPENDS_ON` is inferred
* evidence separates observed edges from inferred dependency
* `twin impact` can show direct dependents

---

# Slice 8: `twin impact` MVP

## Goal

Make the first truly useful DevOps command.

## Working Demo

```bash
twin impact postgresql.service
twin impact port:tcp:127.0.0.1:5432
```

## Build

Implement:

* target resolver
* incoming dependency traversal
* direct dependents
* simple evidence collection
* simple risk model
* unknowns reporting

## Output Example

```text
Impact: service:postgresql.service

Risk: MEDIUM
Evidence strength: MODERATE

Direct dependents:
- service:django.service
  Reason: active connection to tcp:127.0.0.1:5432

Unknowns:
- 6 processes hidden due to permissions
```

## Acceptance Criteria

* works for service and port targets
* risk and evidence are separate
* output shows evidence
* output shows unknowns
* no mutation is possible

---

# Slice 9: Overlay Emulation MVP

## Goal

Implement the core product idea: graph-overlay-based emulation.

## Working Demo

```bash
twin emulate restart postgresql.service
```

## Build

Create:

```text
crates/
  twin-emulate/
```

Implement:

```text
EmulationAction
GraphOverlay
EffectiveGraphView
Impact
EmulationReport
```

First overlay builder:

```text
RestartServiceOverlayBuilder
```

Overlay behavior:

```text
target service -> temporarily_unavailable
owned ports -> temporarily_unavailable
active connections -> interrupted
```

## Output Example

```text
Emulation: restart service:postgresql.service

Risk: MEDIUM
Evidence strength: MODERATE

Transient impact:
- service:django.service may lose DB connection

No action was performed.
```

## Acceptance Criteria

* no real service restart happens
* overlay is not persisted as real graph state
* affected direct dependents are shown
* emulation report says “No action was performed”
* test proves base graph remains unchanged

---

# Slice 10: Runtime vs Restart Impact and File Deletion Emulation

## Goal

Make emulation smarter than basic dependency traversal.

## Working Demo

```bash
twin emulate delete /etc/nginx/nginx.conf
```

## Build

Add file nodes.

Detect common config files:

```text
/etc/nginx/nginx.conf
/etc/systemd/system/*.service
/etc/postgresql/*
```

Create edges:

```text
service CONFIGURED_BY file
```

Add overlay builder:

```text
DeleteFileOverlayBuilder
```

Implement impact types:

```text
runtime impact
restart impact
transient impact
persistent impact
unknown impact
```

## Output Example

```text
Emulation: delete file:/etc/nginx/nginx.conf

Runtime impact: LOW
Reason: nginx is already running with loaded config.

Restart impact: CRITICAL
Reason: nginx may fail to reload or restart without this file.

No file was deleted.
```

## Acceptance Criteria

* delete emulation does not delete file
* runtime and restart impacts are separate
* service-config relationships are visible
* evidence cites config discovery source

---

# Slice 11: Transitive Impact Paths and Better Risk Scoring

## Goal

Make impact reports show full blast-radius paths.

## Working Demo

```bash
twin emulate restart postgresql.service --paths
twin impact postgresql.service --paths
```

## Build

Implement:

* recursive dependency traversal
* path preservation
* cycle detection
* max-depth setting
* first serious risk scoring

Risk inputs:

```text
direct dependent count
transitive dependent count
public exposure
active connection evidence
unknown process count
service criticality hints
```

## Output Example

```text
Impact paths:

service:postgresql.service
  <- DEPENDS_ON service:django.service
  <- PROXIES_TO service:nginx.service

Risk: HIGH
Reason:
- dependency reaches externally exposed nginx.service
```

## Acceptance Criteria

* transitive paths render clearly
* cycles do not break traversal
* risk reasoning is shown
* user can cap traversal depth

---

# Slice 12: Temporal History, What-Changed, and Snapshots

## Goal

Turn `twin` from a scanner into operational memory.

## Working Demo

```bash
twin scan
twin what-changed --since 10m
twin snapshot create before-change
twin snapshot list
twin diff snapshot:before-change current
```

## Build

Add tables:

```text
node_history
edge_history
snapshots
snapshot_nodes
snapshot_edges
```

Track:

```text
first_seen
last_seen
valid_from
valid_to
state
```

Implement:

* changed node detection
* changed edge detection
* new/missing/stale states
* named snapshots
* diff between snapshot and current

## Output Example

```text
Changes since 10m:

New:
- process:pid:2241 python
- port:tcp:127.0.0.1:8000

Disappeared:
- service:redis.service

Changed:
- file:/etc/nginx/nginx.conf hash changed
```

## Acceptance Criteria

* repeated scans update history
* stale/missing nodes are detected
* snapshots can be created and listed
* diff output is useful and readable

---

# Slice 13: Polling Watch Mode

## Goal

Continuously update graph without eBPF first.

## Working Demo

```bash
twin watch --interval 5s
```

## Build

Implement:

* scan loop
* graceful Ctrl+C
* incremental change summary
* watch duration
* simple event counter
* collector timing

## Output Example

```text
Watching system every 5s.

[12:00:05] +2 processes, -1 process, +1 connection
[12:00:10] no significant changes
[12:00:15] +1 listening port
```

## Acceptance Criteria

* watch mode stops cleanly
* no memory leak during basic watch
* repeated scans update graph safely
* user sees useful change stream

---

# Slice 14: eBPF Capability Checks and Exec Events

## Goal

Introduce eBPF safely with a simple event type.

## Working Demo

```bash
twin doctor --ebpf
twin watch --ebpf --events exec
```

## Build

Create:

```text
crates/
  twin-ebpf/
```

Implement:

* kernel version check
* BTF check
* capability check
* Aya backend skeleton
* exec event program
* ring buffer reader
* event decoder
* conversion to observation

Event:

```text
EbpfExec(pid, ppid, comm, cgroup_id, timestamp)
```

## Output Example

```text
eBPF:
- kernel: supported
- BTF: available
- capabilities: available
- exec tracing: available

[exec] process:pid:4421 curl
```

## Acceptance Criteria

* eBPF unavailable systems show clean warnings
* exec events appear in watch mode
* clean detach on exit
* event stream does not crash app
* no enforcement hooks

---

# Slice 15: eBPF TCP Runtime Dependency Evidence

## Goal

Use eBPF to strengthen dependency evidence.

## Working Demo

```bash
twin watch --ebpf --events tcp
twin graph service:django.service --evidence
```

## Build

Implement eBPF events:

```text
tcp connect
tcp accept
socket bind
```

Convert to observations:

```text
EbpfConnect
EbpfAccept
EbpfBind
```

Update graph:

```text
process CONNECTS_TO endpoint/port
service CONNECTS_TO endpoint/port
repeated runtime observations strengthen evidence
```

Add event rate limiting:

```text
per PID
per endpoint
global
```

## Output Example

```text
Runtime dependency observed:

service:django.service CONNECTS_TO service:postgresql.service

Evidence strength: very strong
Reason:
- eBPF observed 842 connect events in 1h
- socket inode mapping confirmed listener ownership
```

## Acceptance Criteria

* TCP eBPF events strengthen graph edges
* dropped event counters are tracked
* high event volume does not overwhelm storage
* impact/emulation reports use eBPF evidence

---

# Slice 16: Evidence Scoring, Unknowns, and Coverage Model

## Goal

Make `twin` trustworthy when evidence is incomplete.

## Working Demo

```bash
twin scan
twin impact postgres --evidence
twin emulate restart postgres --evidence
```

## Build

Implement evidence scoring v1.

Inputs:

```text
source type
runtime confirmation
static confirmation
recency
repeat count
independent source count
permission gaps
conflicting evidence
dropped eBPF events
```

Implement coverage model:

```text
readable processes
restricted processes
unmapped sockets
unavailable collectors
eBPF availability
Docker availability
Kubernetes availability
```

## Output Example

```text
Evidence strength: 87/100, very strong

Why:
- eBPF connect observed
- socket inode mapped to process
- process mapped to service
- relationship observed repeatedly

Unknowns:
- 8 processes hidden due to permissions
- 3 sockets could not be mapped
```

## Acceptance Criteria

* evidence scores are deterministic
* reports explain evidence score
* unknowns appear in scan, impact, and emulate
* missing permissions reduce confidence but do not crash

---

# Slice 17: Local Test Runner MVP

## Goal

Introduce `twin.yaml` for repeatable local operational checks.

## Working Demo

```bash
twin test init
twin test lint twin.yaml
twin test run twin.yaml
```

## Build

Create:

```text
crates/
  twin-test/
```

Implement:

* YAML schema
* strict parser
* generated starter file
* test run report
* test result persistence

First checks:

```text
node exists
port exists
service exists
dependency exists
minimum evidence threshold
```

Example YAML:

```yaml
version: 1

name: local-readiness

checks:
  - name: postgres is listening
    assert:
      node: port:tcp:127.0.0.1:5432
      exists: true

  - name: django depends on postgres
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: moderate
```

## Output Example

```text
Twin Test: local-readiness

PASS postgres is listening
PASS django depends on postgres

Summary:
Passed: 2
Failed: 0
```

## Acceptance Criteria

* YAML validation works
* node checks work
* dependency checks work
* output has pass/warn/fail
* no shell execution support

---

# Slice 18: Emulation and Endpoint Tests

## Goal

Make local tests powerful enough to validate operational assumptions.

## Working Demo

```bash
twin test run twin.yaml
```

## Build

Add test types:

```text
emulate risk check
endpoint allowlist
disk threshold
evidence requirement
unknowns policy
```

Example YAML:

```yaml
checks:
  - name: postgres restart risk acceptable
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
          - github.com:443
          - api.stripe.com:443
        fail_on_unknown: true
```

## Output Example

```text
WARN postgres restart risk acceptable
  Risk: HIGH
  Evidence: STRONG

FAIL no unexpected outbound endpoints
  Unexpected endpoint: telemetry.example.com:443
  Evidence: eBPF observed api.service connecting 42 times
```

## Acceptance Criteria

* test runner can call emulation engine
* endpoint allowlist works
* test failures include evidence
* test runner remains read-only

---

# Slice 19: Config Files, nginx Parser, and Config Impact

## Goal

Make `twin` understand common config-driven dependencies.

## Working Demo

```bash
twin scan
twin graph nginx
twin emulate delete /etc/nginx/nginx.conf
twin what-changed --since 1h
```

## Build

Create:

```text
crates/
  twin-config/
```

Implement:

* config file discovery
* config file hash tracking
* service-to-config edges
* nginx parser for `proxy_pass`
* config target resolution to port/service
* config change detection

Edges:

```text
service CONFIGURED_BY file
service PROXIES_TO service
file REFERENCES endpoint/port
```

## Output Example

```text
service:nginx.service

Configured by:
- file:/etc/nginx/nginx.conf

Proxies to:
- service:django.service

Evidence:
- proxy_pass http://127.0.0.1:8000
- django listens on tcp:127.0.0.1:8000
```

## Acceptance Criteria

* nginx proxy relationships are inferred
* malformed configs warn, not crash
* config file changes appear in `what-changed`
* delete-config emulation becomes more accurate

---

# Slice 20: Filesystem, Mounts, Disk Usage, and Fill-Disk Emulation

## Goal

Support disk-related operational reasoning.

## Working Demo

```bash
twin scan
twin graph mount:/var
twin emulate fill-disk /var --to 95%
twin test run twin.yaml
```

## Build

Read:

```text
/proc/mounts
statvfs or equivalent disk usage
```

Create nodes:

```text
mount:/var
directory:/var/lib/postgresql
directory:/var/log
```

Create edges:

```text
file/directory MOUNTED_ON mount
service LOGS_TO directory/file
service USES directory
```

Add emulation:

```text
FillMountOverlayBuilder
```

Add YAML check:

```yaml
checks:
  - name: var disk usage below limit
    assert:
      disk:
        mount: /var
        max_used_percent: 85
```

## Output Example

```text
Emulation: fill mount:/var to 95%

Risk: CRITICAL

Likely affected:
- postgresql.service uses /var/lib/postgresql
- journald writes to /var/log/journal
- docker uses /var/lib/docker
```

## Acceptance Criteria

* mount nodes exist
* disk usage is visible
* fill-disk emulation works
* disk checks work in tests

---

# Slice 21: Libraries, Packages, and Upgrade Emulation

## Goal

Predict package upgrade restart impact.

## Working Demo

```bash
twin scan
twin graph libssl.so.3
twin impact package:openssl
twin emulate upgrade package:openssl
```

## Build

Read:

```text
/proc/[pid]/maps
```

Create:

```text
library nodes
LOADS_LIBRARY edges
```

Package adapter v1:

```text
dpkg first
then pacman/rpm later
```

Create edges:

```text
library INSTALLED_BY package
service DEPENDS_ON package
```

Add emulation:

```text
UpgradePackageOverlayBuilder
```

## Output Example

```text
Emulation: upgrade package:openssl

Runtime impact: LOW
Reason:
running processes already have current libssl mapped.

Restart impact: HIGH
Affected on restart:
- nginx.service
- ssh.service
- api.service
```

## Acceptance Criteria

* loaded libraries are visible
* package ownership works on at least one package manager
* unsupported distros warn cleanly
* upgrade emulation does not run package manager

---

# Slice 22: Docker Container Graph and Container Emulation

## Goal

Make containers first-class graph entities.

## Working Demo

```bash
twin doctor --containers
twin scan
twin graph container:redis
twin emulate restart container:redis
```

## Build

Create:

```text
crates/
  twin-container/
```

Docker read-only adapter:

```text
list containers
inspect containers
inspect images
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
copy
commit
update
```

Create nodes:

```text
container
image
volume/directory
```

Create edges:

```text
container RUNS_IMAGE image
container MAPS_PORT port
container MOUNTS_VOLUME directory
container OWNS process
```

Add emulation:

```text
RestartContainerOverlayBuilder
```

## Output Example

```text
container:redis

Runs image:
- image:redis:7

Maps ports:
- container 6379 -> host 0.0.0.0:6379

Emulation restart container:redis
Risk: HIGH
Affected:
- service:api.service connects to redis port
```

## Acceptance Criteria

* Docker scan is read-only
* containers appear in graph
* container ports and volumes appear
* container restart emulation does not restart container
* no mutation methods exist in adapter

---

# Slice 23: Kubernetes Read-Only Graph and Kubernetes Emulation

## Goal

Add Kubernetes inspection without becoming a Kubernetes controller.

## Working Demo

```bash
twin doctor --k8s
twin k8s scan
twin k8s graph deployment/default/api
twin k8s impact service/default/api
twin emulate delete pod:default/api-123
twin emulate rollout deployment:default/api
```

## Build

Create:

```text
crates/
  twin-k8s/
```

Use read-only Kubernetes client.

Allowed:

```text
get
list
watch
read logs if allowed
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

Read resources:

```text
namespaces
pods
deployments
replicasets
services
endpoints/endpointslices
ingress
configmaps metadata
secret references only
PVCs
events
```

Create edges:

```text
deployment OWNS replicaset
replicaset OWNS pod
service SELECTS pod
ingress ROUTES_TO service
pod USES_CONFIGMAP configmap
pod USES_SECRET_REF secret_ref
pod USES_PVC pvc
pod RUNS_IMAGE image
```

Add overlays:

```text
DeleteK8sPodOverlayBuilder
RolloutK8sDeploymentOverlayBuilder
```

## Output Example

```text
k8s:deployment:default/api

Owns:
- replicaset:default/api-abc
- pod:default/api-123
- pod:default/api-124

Routed by:
- service:default/api
- ingress:default/api

Emulation: delete pod:default/api-123

Risk: LOW
Reason:
service still has 2 ready endpoints.
```

## Acceptance Criteria

* Kubernetes support is read-only
* secret values are never read or stored
* workload ownership graph works
* service-to-pod routing works
* pod delete emulation does not delete pod
* rollout emulation does not rollout deployment

---

# Recommended Build Order Summary

## Foundation

```text
1. CLI, Config, Init, and Doctor Foundation
2. SQLite Store and Migration System
3. Core Domain Model and Observation Pipeline MVP
```

## Local Linux Graph

```text
4. Process Scan and Process Graph
5. Cgroups and systemd Service Mapping
6. TCP Socket Scan and Inode Ownership
7. Active Connections and First Dependency Inference
```

## First Valuable Product

```text
8. twin impact MVP
9. Overlay Emulation MVP
10. Runtime vs Restart Impact and File Deletion Emulation
11. Transitive Impact Paths and Better Risk Scoring
```

At this point, the project is already impressive and demo-worthy.

## Operational Memory and Runtime Evidence

```text
12. Temporal History, What-Changed, and Snapshots
13. Polling Watch Mode
14. eBPF Capability Checks and Exec Events
15. eBPF TCP Runtime Dependency Evidence
16. Evidence Scoring, Unknowns, and Coverage Model
```

## Local Operational Testing

```text
17. Local Test Runner MVP
18. Emulation and Endpoint Tests
```

## Feature Modules

```text
19. Config Files, nginx Parser, and Config Impact
20. Filesystem, Mounts, Disk Usage, and Fill-Disk Emulation
21. Libraries, Packages, and Upgrade Emulation
22. Docker Container Graph and Container Emulation
23. Kubernetes Read-Only Graph and Kubernetes Emulation
```

---

# First Major Demo Milestone

The first serious milestone should be after Slice 11.

Demo:

```bash
twin init
twin doctor
twin scan
twin graph postgresql.service
twin impact postgresql.service
twin emulate restart postgresql.service
twin emulate delete /etc/nginx/nginx.conf
```

Expected story:

```text
1. twin scans local processes, cgroups, services, sockets, and connections.
2. twin maps postgres to port 5432.
3. twin detects another service connected to postgres.
4. twin infers a dependency.
5. twin shows the blast radius of postgres.
6. twin emulates a postgres restart without restarting it.
7. twin separates runtime and restart impact for deleting nginx config.
8. twin cites evidence and unknowns.
```

This proves the core product before eBPF, containers, packages, or Kubernetes.

---

# Second Major Demo Milestone

After Slice 18.

Demo:

```bash
twin watch --ebpf --events tcp
twin impact postgresql.service --evidence
twin emulate restart postgresql.service --evidence
twin test run twin.yaml
```

Expected story:

```text
1. twin observes real runtime connections through eBPF.
2. eBPF strengthens dependency evidence.
3. emulation uses runtime evidence.
4. twin.yaml validates operational assumptions.
5. unexpected endpoints or risky restarts become test failures.
```

This proves the unique value of the product.

---

# Final Rule

Module layout, `TwinLayout` injection, and test placement: **`docs/code-layout.md`**.

Each new module should follow this pattern:

```text
1. Read one new source safely.
2. Convert it into observations.
3. Add nodes and edges.
4. Show it in graph/impact.
5. Add one emulation use case or test check.
6. Add fixture tests.
```

If a slice cannot produce a command the user can run, it is too horizontal.
