# Slice 8 Implementation Plan: `twin impact` MVP

**Implemented layout and tests:** see `docs/code-layout.md` for current crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. This slice upgrades the minimal slice-7 `twin impact` command into the first useful DevOps analysis report: target resolution, incoming dependency traversal, evidence collection, risk scoring, evidence-strength scoring, and unknown reporting.

## 1. Overview

Slice 8 makes `twin impact` answer:

```text
If this known service or port is unavailable, who is directly affected, how risky is that, how strong is the evidence, and what is still unknown?
```

Working demo:

```bash
twin scan
twin impact postgresql.service
twin impact port:tcp:127.0.0.1:5432
```

The ideal outcome is:

1. `twin impact` resolves exact service IDs, service shorthand, and exact port IDs without rescanning or mutating the host.
2. Service targets report incoming `depends_on` dependents.
3. Port targets report incoming `connects_to` callers and, when possible, the listener service that owns the port.
4. Reports separate observed facts, inferred relationships, risk, evidence strength, and unknowns.
5. Risk is simple but defensible: direct dependent count and dependency shape drive operational risk, while missing coverage can raise uncertainty.
6. Evidence strength is derived from stored edge evidence and observation kinds. It is never treated as risk.
7. Unknowns are visible, especially permission and mapping gaps from the latest scan.
8. Human output is polished and JSON output remains machine-oriented.
9. No new graph/rules/emulate crate is introduced until traversal or overlay behavior actually requires it.

This slice should stop at direct impact. Transitive paths and richer risk inputs belong to slice 11. Overlay actions belong to slice 9. Runtime-vs-restart impact belongs to slice 10.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Minimal impact command | Slice 7 added `twin impact` for service and port targets with direct dependents only. |
| Target parsing | CLI can pass exact `NodeId` targets or service query strings to `twin-app`. |
| Service graph | Services, cgroups, processes, listening ports, active connection ports, and `depends_on` edges are persisted. |
| Evidence links | Edges can link to observations through `edge_observations`; graph output already renders selected evidence lines. |
| Store queries | `list_edges_to`, `list_edges_from`, `list_edges_by_kind`, typed node/observation helpers, and edge rows are available. |
| CLI output | `twin-cli/src/output/impact.rs` renders target, direct dependents, and evidence. |
| Warnings | Scan result records collector warnings and unmapped socket counts, but impact does not yet surface them. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| Risk model | Add `RiskLevel` and deterministic scoring for direct impact reports. |
| Evidence-strength model | Add `EvidenceStrength`/label or an equivalent typed score model, consumed by impact. |
| Unknown model | Add structured unknowns instead of a plain string list. |
| Target resolver quality | Reuse graph-style resolution and improve errors for service shorthand and exact port targets. |
| Port impact context | For a port target, show callers and listener owners so the report explains what the port represents. |
| Edge evidence aggregation | Aggregate evidence per dependent/path, not just a global evidence list. |
| Latest scan coverage | Read the latest collector run and/or persisted warning metadata enough to report coverage gaps. |
| Output polish | Human output must show risk, evidence strength, direct dependents, evidence, and unknowns in stable sections. |
| Tests | App, CLI output, JSON shape, resolver, scoring, unknowns, and no-host-mutation behavior. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| Domain scoring types | Add minimal `RiskLevel`, `EvidenceLabel`, and `EvidenceStrength` in `twin-core`, with `Display`, `FromStr`, and serde. |
| Impact result model | Expand `ImpactResult` with `risk`, `evidence_strength`, structured `direct_dependents`, structured `unknowns`, and concise score reasons. |
| Target resolver | Resolve exact `NodeId`, exact service unit, service shorthand, and exact port IDs. Return clear not-found/ambiguous errors. |
| Direct traversal | Traverse only incoming `depends_on` and `connects_to` relationships for the target node. Preserve edge IDs and observation IDs. |
| Port ownership context | For port targets, report listener owners from incoming `listens_on` edges separately from callers. |
| Evidence aggregation | Link each dependent to the edge evidence that supports it; deduplicate global evidence lines for report readability. |
| Evidence scoring | Compute report-level evidence strength from edge evidence labels/counts and observation kinds. |
| Risk scoring | Compute report-level risk from direct dependent count and target/dependent shape. Keep it deterministic and explainable. |
| Unknowns | Surface coverage gaps such as unmapped active sockets/listeners and latest collector warnings that weaken confidence. |
| CLI output | Render risk, evidence strength, direct dependents, listener context, evidence, and unknowns with existing `Lines` helpers. |
| JSON output | Keep `--json` as direct serde output from `ImpactResult`; field names should be stable and obvious. |
| Docs state | Update `docs/state/slice-08.md` during implementation and verify every acceptance criterion on completion. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Transitive blast-radius paths | Slice 11 | Slice 8 acceptance says incoming traversal and direct dependents. |
| Overlay emulation | Slice 9 | `impact` analyzes current graph; it does not model a hypothetical action. |
| Runtime vs restart impact types | Slice 10 | Requires file/config relationships and action-specific semantics. |
| eBPF-strengthened scoring | Slice 15 | No eBPF observations exist yet. Make scoring extensible but do not add eBPF code. |
| `twin-graph` crate | Later | Store-backed direct traversal is still small. A crate would be mostly indirection. |
| `twin-rules` crate | Later | One impact scoring function is enough. No rule engine yet. |
| New SQLite migration | Prefer no | Use existing edge columns, observation links, and collector run rows. Add a migration only if current rows cannot represent required unknowns. |
| Manual checklists | Slice 9+ | Checklists are more natural in emulation reports. |
| Service criticality config | Later | Do not invent config fields before a real workflow needs them. |

---

## 4. Design Decisions

### 4.1 Keep impact logic in `twin-app`

The current direct traversal fits in the app layer:

```text
resolve target
  -> query current graph rows
  -> assemble direct impact report
  -> score risk/evidence
  -> render in CLI
```

Do not create `twin-graph`, `twin-rules`, or `twin-emulate` in this slice. The right extraction point is when traversal becomes reusable across `impact`, `emulate`, `why`, and tests. Slice 8 should keep code focused and obvious.

Suggested app layout:

```text
crates/twin-app/src/commands/
  impact.rs                 # public command entry and high-level orchestration
  impact/
    resolver.rs             # if impact.rs becomes too large
    traversal.rs            # direct traversal helpers
    scoring.rs              # risk/evidence score helpers
    evidence.rs             # evidence line conversion helpers
    unknowns.rs             # latest scan/coverage extraction helpers
```

Only split into the subdirectory when the current file is becoming hard to read. Do not create modules just to mirror this plan.

### 4.2 Add risk and evidence types in `twin-core`

Slice 1 intentionally deferred `RiskLevel` and `EvidenceStrength` until slice 8. This is their first real consumer.

Minimal types:

```text
RiskLevel:
  low
  medium
  high
  critical
  unknown

EvidenceLabel:
  weak
  moderate
  strong
  very_strong

EvidenceStrength:
  score: u8
  label: EvidenceLabel
```

The implementation should keep the constructors narrow:

```text
EvidenceStrength::new(score: u8)
EvidenceStrength::weak()
EvidenceStrength::moderate()
EvidenceStrength::strong()
EvidenceStrength::very_strong()
```

Clamp scores to `0..=100` if accepting raw scores. Avoid floating point. Add tests for label boundaries:

```text
0-30    weak
31-60   moderate
61-85   strong
86-100  very_strong
```

### 4.3 Risk and evidence strength are separate report fields

This must be structurally true in the result model:

```text
ImpactResult {
  risk: RiskAssessment,
  evidence_strength: EvidenceStrength,
  ...
}
```

Do not encode risk inside evidence labels. Do not use evidence score as the risk score. A high-risk report with weak evidence is valid and must render clearly:

```text
Risk: HIGH
Evidence strength: WEAK
```

### 4.4 Direct traversal is enough, but preserve path shape

Even though slice 8 does not recurse, each direct dependent should still carry a one-edge path. That keeps the result compatible with slice 11 without forcing a rewrite.

Suggested model shape:

```text
ImpactDependent {
  node: GraphNodeSummary
  relationship: EdgeKind
  edge_class: EdgeClass
  impact_kind: "runtime"
  path: Vec<ImpactPathStep>
  evidence: Vec<ImpactEvidenceLine>
}

ImpactPathStep {
  from: GraphNodeSummary
  edge_kind: EdgeKind
  edge_class: EdgeClass
  to: GraphNodeSummary
  edge_id: String
}
```

For slice 8 the `path` has one step. Slice 11 can extend it to multiple steps.

### 4.5 Service target semantics

For a service target:

```text
incoming depends_on edges
  service:<dependent> -> service:<target>
```

Direct dependents are the `from` nodes of incoming `depends_on` edges. Only include active edges. If the current `EdgeState` has only `active`, `stale`, and `gone`, skip `stale` and `gone` in the main dependent list and optionally mention them under unknowns or historical notes if needed.

Do not include `owns`, `in_cgroup`, or `listens_on` as dependents for a service target. Those are ownership/context edges, not consumers of the service.

### 4.6 Port target semantics

For a port target:

```text
incoming connects_to edges
  process/service -> port

incoming listens_on edges
  process/service -> port
```

Direct dependents are callers from `connects_to`.

Listener owners are context, not dependents. Render them separately:

```text
Target:
└── port:tcp:127.0.0.1:5432 tcp:127.0.0.1:5432

Owned by:
└── service:postgresql.service postgresql.service

Direct dependents:
└── service:django.service active connection to port
```

This avoids saying the service that listens on the port depends on itself.

### 4.7 Unknowns should be structured

Replace `Vec<String>` with a structured type:

```text
ImpactUnknown {
  kind: String
  detail: String
  source: Option<String>
  weakens_evidence: bool
}
```

Examples:

```text
kind: "unmapped_active_sockets"
detail: "3 active TCP sockets could not be mapped to a process"
source: "latest scan"
weakens_evidence: true

kind: "permission_gap"
detail: "6 process fd directories were unreadable"
source: "process collector"
weakens_evidence: true
```

Do not fabricate unknowns that cannot be supported by stored scan/collector data. If the current store cannot recover detailed warning kinds, use the latest `collector_runs.warning_count` and any persisted warning metadata already available. The report should prefer a specific unknown over a vague one.

### 4.8 Latest coverage comes from stored scan provenance

Impact reads the stored graph; it should not run collectors. Unknowns must come from persisted scan provenance:

1. latest process collector run;
2. warning count and any warning details already stored;
3. graph evidence gaps visible from edge/observation links;
4. unmapped socket observations if currently persisted in observation metadata.

If slice-7 warnings were not persisted with enough detail, add the smallest repository query that can read what is already available. Do not add a large coverage subsystem. If a migration is unavoidable, keep it tiny and justify it in the slice state doc.

### 4.9 Evidence lines should identify source, observation, and relationship

Each `ImpactEvidenceLine` should carry:

```text
source
statement
relationship
strength
observation_id
```

Source examples:

```text
/proc/net/tcp:2
/proc/8841/fd/12
/proc/8841/cgroup
```

Evidence wording must keep facts and inference separate:

```text
Observed: process fd joined socket inode 456
Inferred: service dependency inferred from active connection and listener match
```

### 4.10 Scoring should be simple and auditable

Risk scoring for slice 8 should be deterministic, conservative, and small.

Recommended risk levels:

| Condition | Risk |
|-----------|------|
| target not found or unsupported | error, not a scored report |
| no direct dependents and no unknowns | low |
| one direct dependent | medium |
| two to four direct dependents | high |
| five or more direct dependents | critical |
| no direct dependents but significant unknowns | unknown |
| direct dependents plus significant unknowns | do not lower risk; add uncertainty reason |

This model is intentionally blunt. It is still useful because the report says why:

```text
Risk: HIGH
Reasons:
- 3 direct dependents are known
- target is a service dependency target
- 2 active sockets could not be mapped to processes
```

Evidence strength should be separate:

| Evidence input | Suggested score contribution |
|----------------|------------------------------|
| observed TCP connection evidence | strong |
| inferred service dependency supported by connection + listener observations | moderate to strong |
| service ownership inferred from cgroup only | moderate |
| edge has no observation links | weak |
| warning/coverage gap weakens evidence | subtract or cap at moderate |

Prefer a cap-based model over complex arithmetic:

```text
base = best linked evidence strength
if only inferred edges exist, cap at strong
if no observation links, weak
if unknowns weaken evidence, cap at moderate
```

Do not introduce weights for public exposure, redundancy, service criticality, or historical instability yet. Those are explicitly later-slice inputs.

### 4.11 Human output should be operational, not verbose

Use existing `Lines` helpers. The target output shape:

```text
twin impact
═══════════
ok view: impact report

target
└── service:postgresql.service postgresql.service

risk
├── level HIGH
├── evidence strength MODERATE
└── reason 3 direct dependents; 2 coverage gaps

direct dependents
└── service:django.service django.service
    relationship: depends_on inferred
    reason: active connection to tcp:127.0.0.1:5432

evidence
├── /proc/net/tcp:2 inode 456 established from 127.0.0.1:50122 to 127.0.0.1:5432
└── /proc/8841/cgroup service ownership inferred from /system.slice/django.service

unknowns
└── 2 active sockets could not be mapped to processes
```

Do not render debug structs. Do not bury risk under evidence. Do not omit unknowns when present.

---

## 5. Implementation Plan

### 5.1 `twin-core`: scoring vocabulary

Add one focused module if it keeps files clean:

```text
crates/twin-core/src/risk.rs
```

Expose:

```text
RiskLevel
EvidenceLabel
EvidenceStrength
```

Tests:

```text
crates/twin-core/tests/risk.rs
```

Coverage:

- `Display` / `FromStr` round trip;
- serde round trip;
- score-to-label boundaries;
- no panics for out-of-range constructor input.

### 5.2 `twin-app`: result model

Expand `crates/twin-app/src/model/impact_result.rs`.

Suggested public types:

```text
ImpactResult
ImpactTarget
ImpactDependent
ImpactPathStep
ImpactEvidenceLine
ImpactUnknown
RiskAssessment
```

Keep fields `Serialize`. Use strings only at CLI/report boundaries where serde output benefits from stable IDs. Internally, use `NodeId`, `EdgeKind`, `EdgeClass`, `RiskLevel`, and `EvidenceStrength`.

Minimum result content:

```text
target
target_label
risk
evidence_strength
direct_dependents
listener_owners
evidence
unknowns
```

`listener_owners` can be empty for service targets.

### 5.3 `twin-app`: resolver

Improve the existing resolver in `commands/impact.rs`:

Resolution order:

1. exact parsed `NodeId` exists;
2. exact service unit if query ends in `.service`;
3. service shorthand by appending `.service`;
4. unique substring match against service labels/IDs;
5. clear ambiguity error;
6. clear not-found error.

Port targets must be exact for now:

```text
port:tcp:127.0.0.1:5432
```

Do not invent shorthand like `port:5432` yet. The PRD lists it as a future command shape, but current node constructors and CLI parsing already support canonical IDs. Shorthand can arrive when ambiguity rules are defined.

### 5.4 `twin-app`: traversal

Create helper functions around store queries:

```text
incoming_edges(store, target, allowed_kinds)
active_graph_edge(row)
node_summary_for(store, node_id)
observations_for(edge_id)
```

For service targets:

```text
incoming depends_on -> direct dependents
```

For port targets:

```text
incoming connects_to -> direct dependents
incoming listens_on -> listener owners
```

Deduplicate by `(dependent_id, relationship, edge_id)`. Sort by node ID for stable output.

### 5.5 `twin-app`: evidence aggregation

Evidence collection should be per edge first, then report-level:

```text
edge -> observation links -> observation rows -> evidence lines
```

Do not silently swallow decode errors that indicate store corruption. Normal missing/deleted observations can become an unknown:

```text
kind: "missing_evidence"
detail: "edge <id> has no readable observation links"
weakens_evidence: true
```

Evidence line conversion should support current observation kinds:

| Observation kind | Evidence wording |
|------------------|------------------|
| `TcpConnectionSeen` | active connection observed in `/proc/net/tcp*`; include inode and endpoint pair |
| `TcpSocketSeen` | listener socket observed and inode joined to fd |
| `ProcessBelongsToCgroup` | service ownership inferred from cgroup path |
| `ProcessSeen` / parent observations | only include if they directly support a path; otherwise omit from impact evidence |

Keep evidence statements concise and factual.

### 5.6 `twin-app`: unknown extraction

Implement small helpers that inspect persisted data:

```text
latest_collector_run(store, "process")
coverage_unknowns_from_run(run)
edge_evidence_unknowns(edge, observation_links)
socket_mapping_unknowns_from_observations(store)
```

The current store may not have a perfect warning-detail table. Use what exists. The plan goal is to make unknowns visible without inventing unsupported detail.

Unknown examples:

- latest scan had warning count > 0;
- active socket observation had `mapped=false`;
- listener socket observation had no `owner_fds`;
- edge has no observation links;
- observation ID linked to an edge no longer decodes.

### 5.7 `twin-app`: scoring

Implement focused scoring functions:

```text
score_risk(target_kind, direct_dependents, unknowns) -> RiskAssessment
score_evidence(direct_dependents, evidence_lines, unknowns) -> EvidenceStrength
```

`RiskAssessment` should include reasons:

```text
level: RiskLevel
reasons: Vec<String>
```

Reasons should be user-facing and stable enough for output tests:

```text
"3 direct dependents are known"
"coverage gaps may hide additional dependents"
"no direct dependents are known"
```

Do not make scoring configurable in this slice.

### 5.8 `twin-cli`: renderer

Update `crates/twin-cli/src/output/impact.rs`.

Render sections in this order:

1. command title/status;
2. target;
3. risk;
4. listener owners, only when non-empty;
5. direct dependents;
6. evidence;
7. unknowns, only when non-empty.

Use `ok`, `warn`, or `fail` status tags consistently:

```text
ok   low/medium risk with no unknowns
warn high/critical risk or unknowns
fail unsupported target/error paths are handled before rendering
```

Do not colorize manually. Keep JSON rendering unchanged through `output::json`.

### 5.9 `twin-store`: queries only if needed

Prefer existing store APIs. If a query is repeated enough to make impact code noisy, add a small repository helper:

```text
Store::latest_collector_run(name)
Store::list_edges_to_by_kind(node_id, kind)
Store::list_edges_from_by_kind(node_id, kind)
```

Do not add a generic query builder. Do not change schema unless required to satisfy unknown reporting.

---

## 6. Test Plan

### 6.1 Core tests

Add `crates/twin-core/tests/risk.rs`:

- risk level display/from-str;
- evidence label display/from-str;
- evidence strength label boundaries;
- serde output uses stable lowercase labels.

### 6.2 App integration tests

Extend `crates/twin-app/tests/scan_graph.rs` or add `impact.rs`.

Required cases:

1. service target with one dependent returns `RiskLevel::Medium` and at least moderate evidence;
2. service target with multiple dependents raises risk;
3. port target reports callers as direct dependents;
4. port target reports listener owners separately from dependents;
5. exact port target works;
6. service shorthand resolves;
7. ambiguous service shorthand errors clearly;
8. no dependents returns low risk when no unknowns exist;
9. edge without observation links produces a weak-evidence unknown;
10. latest scan warnings appear as unknowns without failing impact.

Use isolated layouts and fake `/proc` fixtures. Do not read the host.

### 6.3 CLI output tests

Update `crates/twin-cli/tests/output.rs`:

- renderer includes `risk`;
- renderer includes `evidence strength`;
- direct dependents include relationship and edge class;
- listener owners render for port targets;
- unknowns section renders only when unknowns exist;
- JSON includes structured `risk`, `evidence_strength`, `direct_dependents`, `listener_owners`, and `unknowns`.

### 6.4 CLI integration tests

Extend `crates/twin-cli/tests/cli_integration.rs`:

- `twin impact postgresql.service` after fixture scan exits success and shows risk/evidence;
- `twin impact port:tcp:127.0.0.1:5432` exits success and shows caller/listener context;
- `twin impact missing.service` exits failure with clean error;
- `twin impact` before init fails cleanly.

### 6.5 Verification commands

Run:

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

If formatting fails, run `cargo fmt` and then rerun `cargo fmt --check`.

---

## 7. Acceptance Criteria

Slice 8 is complete only when:

1. `twin impact postgresql.service` works after `twin scan`.
2. `twin impact port:tcp:127.0.0.1:5432` works after `twin scan`.
3. service targets show direct dependents from incoming `depends_on` edges.
4. port targets show direct callers from incoming `connects_to` edges.
5. port targets show listener owners as context, not as dependents.
6. risk and evidence strength are separate fields in human output and JSON.
7. evidence lines cite concrete sources and distinguish observed facts from inferred dependency.
8. unknowns are shown when coverage or evidence gaps exist.
9. unsupported targets fail with clean errors and no panic.
10. no code path mutates the host, restarts services, kills processes, writes system files, or shells out to mutating tools.
11. tests cover success, no-dependent, unknown, resolver, JSON, and output formatting cases.
12. `docs/state/slice-08.md` records implementation status, decisions, deviations, and verification.

---

## 8. Ideal Outcome

The best slice-8 outcome is not a clever scoring engine. It is a report that a sysadmin can trust because every line says what it knows, why it believes it, and what it cannot see.

Example:

```text
twin impact
═══════════
warn view: impact report

target
└── service:postgresql.service postgresql.service

risk
├── level HIGH
├── evidence strength MODERATE
├── reason 3 direct dependents are known
└── reason coverage gaps may hide additional dependents

direct dependents
├── service:django.service django.service
│   relationship: depends_on inferred
│   reason: active connection to port:tcp:127.0.0.1:5432
└── service:worker.service worker.service
    relationship: depends_on inferred
    reason: active connection to port:tcp:127.0.0.1:5432

evidence
├── /proc/net/tcp:2 inode 456 established from 127.0.0.1:50122 to 127.0.0.1:5432
└── /proc/8841/cgroup service ownership inferred from /system.slice/django.service

unknowns
└── 2 active sockets could not be mapped to processes
```

JSON should carry the same structure without terminal decoration.

The implementation should be small enough that a future slice can reuse the traversal and scoring pieces, but not so abstract that slice 8 spends more code on framework than on impact analysis.
