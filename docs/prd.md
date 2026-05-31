# `twin` PRD

## Rust-First Read-Only Operational Twin for Linux, eBPF, Emulation, and Local DevOps Tests

---

# 1. Product Summary

## 1.1 Product Name

`twin`

## 1.2 One-Line Definition

`twin` is a Rust-based, read-only CLI tool that builds an evidence-backed operational graph of a Linux system, observes runtime behavior with eBPF, and uses lightweight graph-overlay emulation to predict the impact of risky operations without changing the real machine.

## 1.3 Product Positioning

`twin` is not a monitoring dashboard.

`twin` is not another `top`, `htop`, `btop`, `lsof`, or `ss`.

`twin` is not a process manager.

`twin` is not a remediation tool.

`twin` is not a VM sandbox.

`twin` is not a Kubernetes platform.

`twin` is a **local, read-only operational twin** for sysadmins, DevOps engineers, backend developers, and Linux power users.

It helps users answer:

```bash
twin scan
twin watch --ebpf
twin graph nginx
twin impact postgres
twin emulate restart postgres
twin emulate delete /etc/nginx/nginx.conf
twin emulate block endpoint api.stripe.com:443
twin what-changed --since 1h
twin why nginx
twin test run twin.yaml
twin k8s graph deployment/api
```

The product’s main promise is:

> Tell me what this system depends on, what changed, and what would likely break if I performed this action, without performing the action.

---

# 2. Product Goal

`twin` should become the perfect local CLI companion for a sysadmin or DevOps engineer.

It should help users:

1. Understand what is running.
2. Understand what depends on what.
3. Understand what changed recently.
4. Observe real runtime dependencies through eBPF.
5. Emulate risky operations without touching the real system.
6. Predict blast radius using a temporal dependency graph.
7. Run repeatable local operational tests.
8. Inspect containers and Kubernetes resources without mutating them.
9. Explain every conclusion with evidence.

---

# 3. Core Product Thesis

Most Linux tools answer:

> What is happening right now?

`twin` answers:

> What does this machine depend on, what changed, and what would likely break if I did this?

The differentiator is:

> A Rust-built, read-only, eBPF-enhanced, temporal operational graph with fast overlay-based impact emulation.

---

# 4. Hard Product Rules

## 4.1 Read-Only Rule

`twin` must never mutate the system.

It must not:

* kill processes
* restart services
* stop services
* reload daemons
* delete files
* edit files
* change firewall rules
* block traffic
* install packages
* upgrade packages
* stop containers
* restart containers
* remove volumes
* mutate Kubernetes resources
* apply manifests
* patch deployments
* scale workloads
* modify secrets or config maps

`twin` may only:

* read system state
* inspect files
* inspect logs
* inspect `/proc`
* inspect sockets
* inspect systemd metadata
* inspect container metadata
* inspect Kubernetes metadata
* attach read-only tracing programs
* collect eBPF runtime evidence
* build graphs
* emulate hypothetical changes
* run local read-only tests
* produce reports

## 4.2 No Full System Cloning

`twin emulate` must not copy the system.

It must not:

* clone the root filesystem
* start duplicate services
* create VM snapshots
* run fake containers by default
* replay the whole OS
* simulate Linux at syscall level
* duplicate production workloads

This would be too slow, too heavy, and too risky.

The default emulation model must be:

> Lightweight graph-overlay emulation.

## 4.3 eBPF Is Observation Only

eBPF must be used only for visibility.

Allowed eBPF behavior:

* observe `execve`
* observe process exit
* observe `connect`
* observe `accept`
* observe `bind`
* observe file open metadata
* observe file read/write metadata
* observe DNS-related behavior where feasible
* observe cgroup/container context

Forbidden eBPF behavior:

* blocking syscalls
* denying network traffic
* killing processes
* rewriting packets
* enforcing policies
* changing system behavior

---

# 5. Target Users

## 5.1 Primary Users

| User               | Need                                                                  |
| ------------------ | --------------------------------------------------------------------- |
| Linux sysadmins    | Understand host dependencies before touching services                 |
| DevOps engineers   | Predict blast radius before operational actions                       |
| Backend developers | Debug VPS/self-hosted app dependencies                                |
| SRE learners       | Learn real operational reasoning from machine evidence                |
| Homelab users      | Understand complex local systems                                      |
| Platform learners  | Practice Linux, containers, eBPF, systemd, networking, and Kubernetes |

## 5.2 Secondary Users

| User                 | Need                                          |
| -------------------- | --------------------------------------------- |
| Security researchers | Inspect process, file, and network behavior   |
| Incident responders  | Identify recent changes and dependency shifts |
| Container users      | Understand host-container relationships       |
| Kubernetes learners  | Analyze workload relationships safely         |

---

# 6. Core Product Pillars

## Pillar 1: Operational Graph

`twin` models the machine as a graph.

Nodes represent entities:

* processes
* services
* files
* directories
* ports
* sockets
* endpoints
* packages
* libraries
* containers
* images
* mounts
* users
* cgroups
* namespaces
* Kubernetes resources

Edges represent relationships:

* owns
* listens on
* connects to
* depends on
* configured by
* logs to
* loads library
* mounted on
* maps port
* runs image
* routes to
* uses config map
* uses secret reference

## Pillar 2: eBPF Runtime Evidence

Static scans are not enough.

`twin` should use eBPF to observe real behavior:

* which process actually connected to which service
* which process actually opened which file
* which service actually talks to which endpoint
* which dependency is active right now
* which process restarted recently
* which containerized process made which connection

eBPF evidence should usually produce stronger evidence than static inference.

## Pillar 3: Overlay-Based Emulation

`twin emulate` is the primary feature.

It must not perform the action.

It must not clone the system.

It must apply a hypothetical patch to the graph and analyze consequences.

Example:

```bash
twin emulate restart postgresql.service
```

Internal model:

```text
Base graph:
- postgresql.service is active
- postgresql.service owns process 721
- process 721 listens on tcp:127.0.0.1:5432
- django.service connects to tcp:127.0.0.1:5432

Overlay:
- postgresql.service is temporarily unavailable
- tcp:127.0.0.1:5432 is temporarily unavailable
- active connections to postgres are interrupted

Effective graph:
- analyze all dependents as if postgres is unavailable
```

The real system is untouched.

## Pillar 4: Local Operational Tests

`twin` should let users write repeatable read-only tests in YAML.

Example:

```bash
twin test run twin.yaml
```

The tests should validate assumptions like:

* nginx must listen on port 80
* postgres must be reachable by django
* redis restart risk must stay below high
* no unexpected outbound endpoint should appear
* disk usage must stay below 85%
* Kubernetes deployment must have expected ready replicas

This is similar in spirit to CI YAML, but it is not CI/CD.

It is local operational validation.

## Pillar 5: Read-Only Kubernetes Awareness

`twin` should understand Kubernetes enough to model workload dependencies.

It should not become a Kubernetes controller.

Allowed:

```bash
twin k8s scan
twin k8s graph deployment/api
twin k8s impact service/postgres
twin emulate rollout deployment/api
twin emulate delete pod/api-123
```

Forbidden:

```bash
kubectl apply
kubectl delete
kubectl patch
kubectl scale
kubectl rollout restart
```

---

# 7. Main User Problems

## 7.1 Unknown Dependencies

Users do not know:

* what depends on Redis
* what uses PostgreSQL
* what reads a config file
* what owns a port
* what container maps to a host socket
* what Kubernetes service points to which pods
* what depends on an external endpoint

`twin` should make these dependencies visible.

## 7.2 No Historical Memory

Most Linux tools show current state only.

`twin` should remember:

* what existed before
* what changed
* what disappeared
* what restarted
* which ports appeared
* which ports disappeared
* which endpoints are new
* which files changed
* which containers changed
* which Kubernetes objects changed

Commands:

```bash
twin what-changed --since 1h
twin diff before-upgrade after-upgrade
twin graph nginx --at "yesterday 18:00"
```

## 7.3 Unsafe Manual Operations

Linux lets users run dangerous actions blindly:

```bash
systemctl restart postgres
kill -9 1421
rm /etc/nginx/nginx.conf
iptables -A OUTPUT -d api.stripe.com -j DROP
docker restart redis
kubectl delete pod api-123
```

`twin` must not perform these actions.

It should emulate their likely impact.

## 7.4 Weak Root-Cause Context

Users manually jump between:

```bash
systemctl status
journalctl
ss
lsof
ps
strace
dmesg
docker ps
docker inspect
docker logs
kubectl get
kubectl describe
kubectl logs
cat config files
```

`twin` should correlate these signals into one evidence-backed explanation.

## 7.5 Operational Checks Are Not Reusable

Sysadmins often manually check the same things repeatedly.

`twin` should let them encode checks once:

```yaml
checks:
  - name: nginx listens on HTTP
    assert:
      node: port:tcp:0.0.0.0:80
      exists: true

  - name: postgres restart blast radius is acceptable
    emulate:
      action: restart
      target: service:postgresql.service
    expect:
      max_risk: high
```

Then run:

```bash
twin test run twin.yaml
```

---

# 8. Emulation Model

## 8.1 Emulation Must Be Graph-Level

`twin emulate` does not emulate the full operating system.

It emulates operational consequences using:

* current graph
* historical graph
* observed runtime evidence
* static config evidence
* eBPF evidence
* dependency rules
* risk rules
* hypothetical graph overlay

## 8.2 Base Graph

The base graph is the current known system state.

Example:

```text
service:nginx.service
  CONFIGURED_BY file:/etc/nginx/nginx.conf
  PROXIES_TO service:django.service

service:django.service
  DEPENDS_ON service:postgresql.service
  CONNECTS_TO port:tcp:127.0.0.1:5432

service:postgresql.service
  LISTENS_ON port:tcp:127.0.0.1:5432
  USES directory:/var/lib/postgresql
```

## 8.3 Overlay Patch

An overlay patch represents the hypothetical action.

Example:

```bash
twin emulate restart postgresql.service
```

Overlay:

```text
service:postgresql.service state = restarting
port:tcp:127.0.0.1:5432 state = temporarily_unavailable
connections_to_postgres state = interrupted
```

The base graph is not changed.

The database is not changed.

The host is not changed.

## 8.4 Effective Graph

The effective graph is:

```text
base graph + overlay patch
```

The query engine reads overlay values first.

If the overlay says Postgres is unavailable, the emulation engine treats it as unavailable for the analysis.

## 8.5 Emulation Flow

```text
1. Load graph from SQLite.
2. Resolve target.
3. Parse user action into EmulationAction.
4. Build hypothetical overlay patch.
5. Create effective graph view.
6. Find direct dependents.
7. Find transitive dependents.
8. Apply action-specific impact rules.
9. Separate runtime, restart, transient, and persistent impact.
10. Score risk.
11. Score evidence strength.
12. Report affected nodes, evidence, and unknowns.
```

## 8.6 Supported Emulation Actions

Required:

```bash
twin emulate restart service:postgresql.service
twin emulate stop service:redis.service
twin emulate kill process:pid:1432
twin emulate delete file:/etc/nginx/nginx.conf
twin emulate block port:tcp:5432
twin emulate block endpoint:api.stripe.com:443
twin emulate fill-disk mount:/var --to 95%
twin emulate upgrade package:openssl
twin emulate restart container:redis
twin emulate delete pod:api-123
twin emulate rollout deployment:api
```

## 8.7 Impact Types

`twin` must separate impact types.

| Impact Type       | Meaning                                                |
| ----------------- | ------------------------------------------------------ |
| Runtime impact    | What breaks while current processes keep running       |
| Restart impact    | What breaks after restart, reload, reboot, or redeploy |
| Transient impact  | Temporary failure during restart/rollout               |
| Persistent impact | Failure that remains until fixed                       |
| Unknown impact    | Cannot be determined from available evidence           |

Example:

```bash
twin emulate delete /etc/nginx/nginx.conf
```

Expected result:

```text
Runtime impact: LOW
nginx has already loaded its current config.

Restart impact: CRITICAL
nginx may fail to reload or restart without /etc/nginx/nginx.conf.
```

## 8.8 Emulation Accuracy

`twin` must not claim perfect certainty.

Bad:

```text
This will definitely break.
```

Good:

```text
This is likely to break based on observed dependencies.

Risk: HIGH
Evidence strength: STRONG
Unknowns:
- 8 processes hidden due to permissions
```

`twin emulate` is an impact predictor, not a full OS simulator.

---

# 9. Product Principles

## P1. Evidence First

Every meaningful conclusion must show evidence.

Bad:

```text
nginx depends on django.
```

Good:

```text
nginx.service depends on django.service.

Evidence:
- nginx config contains proxy_pass http://127.0.0.1:8000
- django.service owns process 8841
- process 8841 listens on tcp:127.0.0.1:8000
- eBPF observed nginx connecting to tcp:127.0.0.1:8000 12,430 times in 24h

Evidence strength: 94/100
Risk if django restarts: HIGH
```

## P2. Observed Facts and Inferred Conclusions Are Separate

Observed facts:

```text
process 8841 listens on 127.0.0.1:8000
nginx config proxies to 127.0.0.1:8000
eBPF observed nginx connecting to that socket
```

Inferred conclusions:

```text
nginx.service PROXIES_TO django.service
nginx.service DEPENDS_ON django.service
```

## P3. Local First

`twin` must work on one Linux machine without requiring:

* SaaS backend
* cloud account
* Kubernetes
* distributed tracing
* metrics stack
* LLM API
* remote agent

## P4. Historical State Is Core

Required:

```bash
twin graph --at "yesterday"
twin diff before after
twin what-changed --since 1h
```

## P5. Risk and Evidence Strength Are Separate

Risk means possible operational damage.

Evidence strength means confidence in the conclusion.

Example:

```text
Risk: HIGH
Evidence strength: MODERATE

Reason:
The service has known dependents, but some process ownership could not be resolved due to permissions.
```

## P6. Rust Safety Is a Product Requirement

Requirements:

* no panic during normal use
* clean errors
* defensive parsing
* isolated unsafe code
* safe APIs around eBPF/FFI/syscalls
* typed graph model
* robust CLI UX
* reliable watch mode
* graceful degradation under missing permissions

---

# 10. Core Commands

## 10.1 `twin init`

Initialize local state.

```bash
twin init
```

Creates:

```text
~/.local/share/twin/twin.db
~/.config/twin/config.toml
~/.local/state/twin/twin.log
```

Requirements:

* create SQLite database
* apply schema migrations
* create default config
* detect available collectors
* detect eBPF support
* detect Docker/containerd availability
* detect Kubernetes config
* detect permission mode
* not overwrite config unless `--force`

## 10.2 `twin doctor`

Diagnose environment.

```bash
twin doctor
```

Checks:

* kernel version
* BTF availability
* eBPF support
* required capabilities
* `/proc` access
* systemd availability
* Docker socket
* containerd socket
* Kubernetes config
* database health
* collector availability
* readable process count
* restricted process count
* retention settings

## 10.3 `twin scan`

Perform immediate read-only scan.

```bash
twin scan
```

Requirements:

* read process metadata
* read parent/child relationships
* read cgroups
* read namespaces
* read file descriptors
* read mapped libraries
* read TCP/UDP/Unix sockets
* resolve socket inode ownership
* map processes to services
* map processes to containers
* detect listening ports
* detect active connections
* parse supported configs
* detect mount points
* detect package ownership where supported
* update graph
* store snapshot
* report coverage and warnings

## 10.4 `twin watch`

Continuously observe the system.

```bash
twin watch
twin watch --ebpf
twin watch --duration 10m
```

Requirements:

* run collectors periodically
* subscribe to eBPF streams where available
* update graph incrementally
* detect changes
* record temporal observations
* avoid high CPU overhead
* degrade gracefully without privileges

## 10.5 `twin emulate`

Primary command.

```bash
twin emulate restart postgresql.service
twin emulate stop redis.service
twin emulate kill pid:1432
twin emulate delete /etc/nginx/nginx.conf
twin emulate block endpoint api.stripe.com:443
twin emulate fill-disk /var --to 95%
twin emulate upgrade package openssl
twin emulate restart container redis
twin emulate rollout deployment/api
```

Requirements:

* never perform the action
* never clone the system
* create graph overlay patch
* calculate direct impact
* calculate transitive impact
* separate runtime impact from restart impact
* show affected nodes
* show risk level
* show evidence strength
* show unknowns
* show suggested manual checklist

## 10.6 `twin impact`

Analyze dependency impact without specifying a hypothetical action.

```bash
twin impact redis
twin impact port:5432
twin impact /etc/nginx/nginx.conf
twin impact endpoint:api.stripe.com:443
```

Shows:

* direct dependents
* transitive dependents
* runtime dependents
* restart/reload dependents
* evidence
* risk

## 10.7 `twin graph`

Show graph around an entity.

```bash
twin graph nginx
twin graph service:postgresql.service
twin graph container:redis
twin graph --at "yesterday 18:00" nginx
twin graph --format dot nginx
twin graph --format mermaid nginx
```

Formats:

* human terminal
* JSON
* DOT
* Mermaid

## 10.8 `twin what-changed`

Show recent changes.

```bash
twin what-changed --since 1h
twin what-changed --since yesterday
```

Tracks:

* new processes
* disappeared processes
* restarted services
* changed configs
* new listening ports
* disappeared ports
* new outbound endpoints
* new containers
* changed images
* changed package versions
* changed Kubernetes objects

## 10.9 `twin why`

Explain likely cause or context.

```bash
twin why nginx
twin why service:api.service
twin why port:5432
```

This is not an LLM chatbot.

It is a deterministic evidence-based explanation engine.

It should correlate:

* service status
* process state
* logs
* recent changes
* socket state
* dependency state
* config parse results
* eBPF events
* graph changes

## 10.10 `twin test`

Run local operational routines.

```bash
twin test init
twin test lint twin.yaml
twin test run twin.yaml
twin test run checks/prod-readiness.yaml
twin test list
```

Purpose:

* local validation
* repeatable sysadmin checks
* operational assertions
* dependency regression checks
* pre-change confidence
* post-change verification

Not allowed:

* shell execution by default
* deployment
* service restart
* container mutation
* Kubernetes mutation
* file editing

---

# 11. Local Routine/Test YAML

## 11.1 Default File

```text
twin.yaml
```

Other valid names:

```text
.twin.yaml
checks/twin.yaml
ops/twin.yaml
```

## 11.2 Example

```yaml
version: 1

name: local-prod-readiness

defaults:
  min_evidence: moderate
  max_risk: medium

checks:
  - name: nginx is listening on HTTP
    assert:
      node: port:tcp:0.0.0.0:80
      exists: true

  - name: postgres is listening locally
    assert:
      node: port:tcp:127.0.0.1:5432
      exists: true

  - name: django depends on postgres with strong evidence
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: strong

  - name: nginx config exists
    assert:
      node: file:/etc/nginx/nginx.conf
      exists: true

  - name: deleting nginx config is only restart-critical
    emulate:
      action: delete
      target: file:/etc/nginx/nginx.conf
    expect:
      runtime_max_risk: medium
      restart_max_risk: critical

  - name: redis restart blast radius is acceptable
    emulate:
      action: restart
      target: service:redis.service
    expect:
      max_risk: high
      require_evidence: true

  - name: no unexpected external endpoints
    assert:
      outbound_endpoints:
        allowed:
          - api.stripe.com:443
          - github.com:443
          - registry-1.docker.io:443
        fail_on_unknown: true

  - name: disk usage under limit
    assert:
      disk:
        mount: /var
        max_used_percent: 85

  - name: kubernetes api deployment has replicas
    k8s:
      assert:
        resource: deployment/api
        namespace: default
        min_ready_replicas: 2
```

## 11.3 Test Result Output

```text
Twin Test: local-prod-readiness

PASS nginx is listening on HTTP
PASS postgres is listening locally
PASS django depends on postgres with strong evidence
PASS nginx config exists
WARN deleting nginx config is only restart-critical
PASS redis restart blast radius is acceptable
FAIL no unexpected external endpoints
  Unexpected endpoint: telemetry.example.com:443
  Evidence: eBPF observed service:api.service connect() 42 times in 1h

Summary:
Passed: 5
Warned: 1
Failed: 1
```

---

# 12. eBPF Requirements

## 12.1 eBPF Must Be Primary But Optional

`twin` must work without eBPF.

But when eBPF is available, it should become the strongest evidence source.

Modes:

```bash
twin scan
twin watch --ebpf
twin doctor --ebpf
```

## 12.2 Required eBPF Observations

Early eBPF support should observe:

* process exec
* process exit
* TCP connect
* TCP accept
* socket bind
* file open
* file read/write metadata
* DNS-related activity where feasible
* service/container process activity through cgroup mapping

## 12.3 eBPF Evidence Strength

Example:

| Evidence                               | Strength    |
| -------------------------------------- | ----------- |
| config says service may connect        | moderate    |
| socket table shows active connection   | strong      |
| eBPF repeatedly observed connect calls | very strong |

## 12.4 eBPF Safety Requirements

* no enforcement programs
* no syscall blocking
* no packet modification
* no process mutation
* bounded memory usage
* event rate limiting
* clean detach on exit
* capability checks before attach
* clear warnings when unavailable

---

# 13. Kubernetes Scope

## 13.1 Kubernetes Is Supported, Not Central

`twin` should support Kubernetes as another graph source.

It should not become a Kubernetes platform.

## 13.2 Read-Only Kubernetes Commands

```bash
twin k8s scan
twin k8s graph deployment/api
twin k8s impact service/postgres
twin k8s what-changed --since 1h
twin emulate rollout deployment/api
twin emulate delete pod/api-123
```

## 13.3 Kubernetes Data Sources

Read-only access to:

* namespaces
* pods
* deployments
* replica sets
* services
* endpoints
* ingress
* config maps metadata
* secret references only
* persistent volume claims
* events
* logs where allowed

## 13.4 Kubernetes Graph Nodes

Required node types:

* `k8s:namespace`
* `k8s:pod`
* `k8s:deployment`
* `k8s:replicaset`
* `k8s:service`
* `k8s:endpoint`
* `k8s:configmap`
* `k8s:secret_ref`
* `k8s:pvc`
* `k8s:ingress`
* `k8s:container`

## 13.5 Kubernetes Non-Goals

`twin` must not:

* apply manifests
* delete resources
* scale deployments
* restart deployments
* patch objects
* mutate secrets/config maps
* become a replacement for `kubectl`

---

# 14. Graph Model

## 14.1 Node

A node is a system entity.

Examples:

```text
host:current
service:nginx.service
process:pid:1432
file:/etc/nginx/nginx.conf
directory:/var/log/nginx
port:tcp:0.0.0.0:80
socket:unix:/run/postgresql/.s.PGSQL.5432
endpoint:api.stripe.com:443
library:/lib/x86_64-linux-gnu/libssl.so.3
package:openssl
container:redis
image:redis:7
mount:/var
user:www-data
cgroup:/system.slice/nginx.service
namespace:netns:4026531993
k8s:deployment:default/api
k8s:service:default/postgres
```

## 14.2 Edge

An edge is a relationship.

Examples:

```text
service:nginx.service OWNS process:1432
process:1432 LISTENS_ON port:tcp:0.0.0.0:80
service:nginx.service CONFIGURED_BY file:/etc/nginx/nginx.conf
service:django.service CONNECTS_TO service:postgresql.service
service:django.service DEPENDS_ON service:postgresql.service
container:redis EXPOSES port:tcp:0.0.0.0:6379
process:8841 LOADS_LIBRARY library:libssl.so.3
k8s:service:default/api ROUTES_TO k8s:pod:default/api-123
```

## 14.3 Observation

An observation is evidence.

Examples:

```text
/proc/[pid]/fd showed socket:[12345]
/proc/net/tcp showed inode 12345 listening on 127.0.0.1:5432
systemd showed PID 8841 belongs to django.service
nginx config contained proxy_pass http://127.0.0.1:8000
eBPF observed process 8841 connect() to 127.0.0.1:5432
Docker reported redis maps host port 6379 to container port 6379
Kubernetes service api selects pods with label app=api
```

## 14.4 Evidence Strength

```text
0-30    weak
31-60   moderate
61-85   strong
86-100  very strong
```

## 14.5 Risk Levels

```text
LOW
MEDIUM
HIGH
CRITICAL
UNKNOWN
```

---

# 15. Data Sources

## 15.1 Linux Sources

| Source                   | Purpose                          |
| ------------------------ | -------------------------------- |
| `/proc`                  | process metadata                 |
| `/proc/[pid]/fd`         | open file/socket descriptors     |
| `/proc/[pid]/fdinfo`     | descriptor metadata              |
| `/proc/[pid]/maps`       | mapped libraries/files           |
| `/proc/[pid]/cgroup`     | service/container/cgroup mapping |
| `/proc/[pid]/ns`         | namespace mapping                |
| `/proc/net/tcp`          | TCP socket table                 |
| `/proc/net/tcp6`         | IPv6 socket table                |
| `/proc/net/udp`          | UDP socket table                 |
| `/proc/net/unix`         | Unix socket table                |
| `/proc/mounts`           | mount points                     |
| `/sys/fs/cgroup`         | cgroup hierarchy                 |
| systemd metadata         | service ownership and state      |
| journal logs             | service failure context          |
| config files             | static dependencies              |
| package manager metadata | package ownership                |
| Docker/containerd APIs   | container metadata               |
| Kubernetes API           | cluster metadata                 |
| eBPF events              | runtime evidence                 |

## 15.2 Socket Mapping Requirement

`twin` must resolve socket ownership through inode mapping.

Required flow:

```text
/proc/net/tcp shows socket inode
        ↓
/proc/[pid]/fd contains socket:[inode]
        ↓
PID owns socket
        ↓
PID maps to service/container
        ↓
service/container owns port or connection
```

`twin` must not assume `/proc/net/tcp` directly gives process ownership.

---

# 16. Rust Implementation Requirements

## 16.1 Rust-First Architecture

`twin` must be primarily written in Rust.

Rust should be used for:

* CLI
* domain model
* collectors
* graph engine
* storage layer
* emulation engine
* eBPF integration
* test runner
* YAML parser
* Kubernetes reader
* container adapters
* config parsers
* output serializers

## 16.2 Suggested Crate Layout

Target workspace (full product):

```text
crates/
  twin-cli/
  twin-app/
  twin-core/
  twin-graph/
  twin-store/
  twin-observation/
  twin-collectors/
  twin-ebpf/
  twin-emulate/
  twin-test/
  twin-k8s/
  twin-container/
  twin-package/
  twin-config/
  twin-output/
  twin-fixtures/
```

**Slice 1 (shipped):** only `twin-cli`, `twin-app`, `twin-core`, `twin-store`. No `twin-output` yet — rendering lives under `twin-cli/src/output/`. See `docs/code-layout.md` for module trees, `TwinLayout`, and test conventions.

## 16.3 Type Safety Requirements

Use typed domain objects for:

* `NodeId`
* `EdgeId`
* `ObservationId`
* `TimestampNs`
* `NodeKind`
* `EdgeKind`
* `RiskLevel`
* `EvidenceStrength`
* `CollectorName`
* `EmulationAction`
* `SnapshotId`
* `TestCheckId`
* `K8sResourceRef`

Avoid passing raw strings where enums/newtypes are safer.

## 16.4 Error Handling Requirements

No panic during normal operation.

Normal failures include:

* unreadable `/proc` files
* disappearing processes
* missing permissions
* malformed config files
* missing Docker socket
* unavailable eBPF
* invalid YAML
* missing Kubernetes context
* database lock conflicts
* missing systemd

All must produce clean CLI errors or warnings.

## 16.5 Unsafe Code Policy

Unsafe code is allowed only at operating-system boundaries.

Allowed areas:

* eBPF loading/attachment
* syscall wrappers
* FFI boundaries
* low-level kernel interactions

Unsafe code must be:

* isolated
* documented
* tested
* hidden behind safe Rust APIs
* banned from business logic

---

# 17. Privacy and Security Requirements

`twin` must be local-first and privacy-conscious.

By default, it must not store:

* raw secret environment values
* Kubernetes secret values
* private key contents
* token values
* full request payloads
* sensitive file contents

It may store:

* environment variable names
* secret references
* config file paths
* metadata
* hashes
* timestamps
* dependency evidence
* redacted log snippets

Users should be able to configure retention.

---

# 18. MVP Scope

## v0.1: Local Graph and Basic Emulation

Required:

* `twin init`
* `twin doctor`
* `twin scan`
* process collector
* socket collector
* fd/socket inode mapping
* systemd mapping
* SQLite graph storage
* `twin graph`
* `twin impact`
* `twin emulate restart service`
* `twin emulate delete file`
* human output
* JSON output

## v0.2: eBPF Watch Mode

Required:

* `twin watch --ebpf`
* observe process exec/exit
* observe TCP connect
* observe TCP accept
* event rate limiting
* evidence strengthening
* clean detach
* doctor checks for BTF/capabilities

## v0.3: Better Emulation Engine

Required:

* graph overlay patch model
* runtime impact vs restart impact
* direct impact traversal
* transitive impact traversal
* unknowns reporting
* risk scoring
* evidence scoring
* emulation report persistence

## v0.4: Local Test Runner

Required:

* `twin test init`
* `twin test lint`
* `twin test run`
* YAML schema
* node checks
* dependency checks
* port checks
* disk checks
* emulation checks

## v0.5: Containers

Required:

* Docker read-only adapter
* container nodes
* image nodes
* volume edges
* port mapping edges
* container process mapping
* `twin emulate restart container`

## v0.6: Kubernetes Read-Only Support

Required:

* read pods
* read deployments
* read replica sets
* read services
* read endpoints
* read config map metadata
* read secret references only
* graph Kubernetes ownership
* `twin k8s graph`
* `twin k8s impact`
* `twin emulate rollout deployment`

---

# 19. Explicit Non-Goals

`twin` will not support early:

* GUI dashboard
* SaaS backend
* autonomous remediation
* automatic service restart
* automatic rollback
* firewall enforcement
* Kubernetes mutation
* package installation
* package upgrades
* process killing
* config editing
* LLM chatbot interface
* full distributed tracing
* full metrics dashboard
* default VM-based sandboxing
* default full-system cloning

---

# 20. Success Criteria

`twin` is successful if it can answer these reliably:

```bash
twin emulate restart postgresql.service
```

Expected:

* shows direct dependents
* shows transitive dependents
* separates runtime/transient/restart impact
* cites evidence
* shows unknowns
* does not restart anything

```bash
twin emulate delete /etc/nginx/nginx.conf
```

Expected:

* says runtime may be fine
* says restart/reload may fail
* explains why
* does not delete anything

```bash
twin watch --ebpf
```

Expected:

* observes real runtime dependencies
* strengthens graph edges
* does not mutate the host

```bash
twin test run twin.yaml
```

Expected:

* validates local operational assumptions
* runs repeatably
* produces pass/warn/fail
* never changes the system

```bash
twin k8s graph deployment/api
```

Expected:

* reads Kubernetes state
* maps deployment to pods/services/config refs
* does not mutate the cluster

---

# 21. Final Product Definition

`twin` is a read-only operational twin for Linux and DevOps systems.

Its main job is not monitoring.

Its main job is not fixing.

Its main job is not deployment.

Its main job is:

> Build an evidence-backed graph of a system, observe real runtime behavior with eBPF, emulate risky operational actions using lightweight graph overlays, and let users write repeatable local tests that prove the system still matches their expectations.

The ideal workflow:

```bash
twin init
twin doctor
twin scan
twin watch --ebpf
twin emulate restart postgresql.service
twin test run twin.yaml
```

The user should walk away knowing:

* what exists
* what depends on what
* what changed
* what runtime evidence proves it
* what would likely break
* what is unknown
* what local checks pass or fail
* all without `twin` changing the machine
