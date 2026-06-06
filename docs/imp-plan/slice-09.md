# Slice 9 Implementation Plan: Overlay Emulation MVP

**Implemented layout and tests:** see `docs/code-layout.md` for current crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. Slice 9 introduces the first `twin-emulate` crate because overlay behavior is now a product concept shared by future `emulate`, `test`, and impact workflows. Keep the slice narrow: restart-service emulation, direct dependents, no persisted overlay, no host mutation.

## 1. Overview

Slice 9 makes `twin` answer:

```text
If I hypothetically restart this service, which direct dependents may be interrupted, what evidence supports that, and what did twin leave untouched?
```

Working demo:

```bash
twin scan
twin emulate restart postgresql.service
```

The ideal outcome is:

1. `twin emulate restart <service>` resolves the same service target forms as `twin impact`.
2. The CLI refreshes the stored graph through the existing read-only scan path before analysis, just like `graph` and `impact`.
3. A new `twin-emulate` crate owns the overlay domain: action, overlay patch, effective graph view, impact section, report assembly helpers, and no store writes.
4. Restart-service emulation marks the target service and its owned listening sockets as temporarily unavailable in an in-memory overlay.
5. The effective view uses overlay state first and base graph state second; it never mutates SQLite rows.
6. Direct runtime dependents are found from current `depends_on` edges and existing impact evidence, then classified as transient impact.
7. Configured-only dependents remain visible as "may be affected after restart/reload" context, but they do not inflate runtime risk in this MVP.
8. Risk and evidence strength stay separate, using the existing `RiskLevel` and `EvidenceStrength` types.
9. Human output is polished and explicit: "No action was performed."
10. Tests prove no real restart is possible and the base graph remains unchanged after emulation.

This slice should stop at service restart. File deletion, runtime-vs-restart file semantics, transitive paths, endpoint blocking, package/container/K8s emulation, and stored emulation history belong to later slices.

---

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Service graph | Services, processes, cgroups, TCP ports, Unix sockets, active connections, declared dependencies, socket activation, and D-Bus/enable relationships are materialized. |
| Impact command | `twin impact` resolves service/port/unix targets, reports risk/evidence/unknowns, separates runtime and configured dependents, and reuses scan quality notes. |
| Evidence loading | `twin-app/src/commands/evidence/` converts observations and graph edges into human-readable evidence lines. |
| Target resolution | `commands/resolve_service.rs` handles exact node IDs, unit names, shorthand, ambiguity, and not-found errors. |
| Store graph APIs | Nodes, edges, edge evidence links, batch loads, latest collector runs, and typed graph decoding exist. |
| CLI pattern | CLI parses args, refreshes scan for graph/impact, calls `twin-app`, and renders in `twin-cli/src/output/`. |
| Domain types | `RiskLevel`, `EvidenceStrength`, `NodeId`, `NodeKind`, `EdgeKind`, `EdgeClass`, `EdgeState`, `GraphNode`, and `GraphEdge` exist. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| `twin-emulate` crate | Overlay and effective-view domain concepts need a reusable home. |
| Emulation CLI | `twin emulate restart TARGET` command and output renderer. |
| App orchestration | Resolve action/target, load current graph, build overlay, call emulator, convert to app report. |
| Overlay model | In-memory patches for node state and edge/relationship interruption. |
| Effective graph view | Query helper that reads overlay state before base graph state. |
| Restart builder | Finds target service, owned processes, owned listening TCP/Unix sockets, and direct dependent edges. |
| Emulation result model | JSON-serializable app model with action, target, transient impacts, risk, evidence, unknowns, and safety statement. |
| Base graph immutability test | Test that store nodes/edges and edge states are unchanged after emulation. |
| Safety tests | Static and behavioral checks that no restart/kill/systemctl mutation path exists. |

---

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| Workspace | Add `crates/twin-emulate` to the workspace. |
| Dependency direction | `twin-emulate -> twin-core`; `twin-app -> twin-emulate`; no dependency from `twin-emulate` to `twin-store`, `twin-app`, `twin-cli`, or collectors. |
| Action model | Add `EmulationAction::RestartService { target: NodeId }` and reject unsupported actions before report generation. |
| Overlay model | Add `GraphOverlay` with node state overrides and interrupted relationship records. |
| Effective view | Add a small `EffectiveGraphView` over base graph snapshots plus overlay; it answers "what state should this node have under the hypothetical action?" |
| Base graph input | `twin-app` loads only the graph rows needed for restart-service emulation and passes plain domain structs into `twin-emulate`. |
| Restart overlay builder | Mark service unavailable; mark its listening TCP/Unix sockets unavailable; mark inbound runtime dependencies as interrupted. |
| Direct impact | Report affected direct runtime dependents from `depends_on` edges as transient impact. |
| Configured context | Preserve configured dependents from declared dependencies separately from runtime transient impact. |
| Safety wording | Every human and JSON report includes `action_performed: false` and a rendered "No action was performed." line. |
| Risk scoring | Reuse the slice-8 shape: runtime dependent count drives risk; configured-only dependents add context, not runtime risk. |
| Evidence strength | Reuse existing edge/observation evidence strength and cap or weaken it when scan coverage unknowns apply. |
| Unknowns | Include target-scoped socket mapping gaps and scan quality notes through existing impact/scan-quality helpers. |
| CLI output | Add stable sections for action, target, risk, evidence strength, overlay summary, transient impact, configured context, unknowns, and safety statement. |
| JSON output | Serialize the app report directly under `--json` with stable snake_case fields. |
| Tests | Add crate unit tests, app integration tests, CLI output tests, CLI E2E tests, and base-graph immutability checks. |
| Docs state | During implementation, update `docs/state/slice-09.md`; on completion, verify every acceptance criterion. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| File deletion emulation | Slice 10 | Needs file/config relationships and runtime-vs-restart semantics. |
| Runtime vs restart impact categories beyond transient restart | Slice 10 | This slice has one action with one primary impact type. |
| Transitive paths | Slice 11 | Direct dependents satisfy slice 9 acceptance; recursive traversal would sprawl. |
| Persisted emulation runs | Later | Acceptance requires overlay not persisted as real graph state. Add history only when reports need retention. |
| `twin-graph` crate | Later | The app can load the needed rows; extraction is justified when traversal is shared across more commands. |
| `twin-rules` crate | Later | One restart scoring function is clearer than a rule engine. |
| Endpoint blocking | Slice 18+ | No endpoint node model exists yet. |
| Container/K8s emulation | Slices 22-23 | Requires read-only container/K8s adapters and graph vocabulary. |
| Service restart duration modeling | Later | Avoid fake precision. Report temporary unavailability without guessing downtime. |
| Shelling out to system tools | Never | No `systemctl`, `kill`, `service`, `docker`, or `kubectl` command execution. |

---

## 4. Architectural Decisions

### 4.1 Introduce `twin-emulate` now, but keep it domain-only

Slice 8 correctly kept impact inside `twin-app`; slice 9 is the first time the product needs a durable overlay abstraction. Put overlay domain behavior in `twin-emulate` so future file, package, container, K8s, and local-test emulation can reuse it.

Dependency rule:

```text
twin-emulate -> twin-core
twin-app     -> twin-core, twin-store, twin-emulate
twin-cli     -> twin-app
```

`twin-emulate` must not open SQLite, inspect `/proc`, render CLI output, parse CLI args, or call collectors. It receives base graph data already loaded by `twin-app`.

Suggested crate layout:

```text
crates/twin-emulate/
  Cargo.toml
  src/
    lib.rs
    action.rs              # EmulationAction
    error.rs               # EmulateError
    graph_snapshot.rs      # BaseGraphSnapshot, BaseNode, BaseEdge
    overlay.rs             # GraphOverlay, NodeOverlay, InterruptedEdge
    effective_view.rs      # EffectiveGraphView
    restart_service.rs     # RestartServiceOverlayBuilder
    report.rs              # EmulationImpact, EmulationReport
    scoring.rs             # small restart risk/evidence helpers
  tests/
    restart_service.rs
    effective_view.rs
```

Keep modules private by default. Export only the types `twin-app` needs.

### 4.2 Overlay is an in-memory patch, not a graph clone

The overlay should store only changes:

```text
GraphOverlay
  node_overrides: NodeId -> OverlayNodeState
  interrupted_edges: EdgeId -> InterruptedRelationship
  notes: Vec<OverlayNote>
```

The base graph snapshot remains separate. Do not clone every node and edge into a second mutable graph.

For this slice, `OverlayNodeState` only needs:

```text
TemporarilyUnavailable
```

Avoid adding `Deleted`, `Blocked`, `Restarting`, `Missing`, or `Degraded` until a supported action needs them. A restart-service report can render "temporarily unavailable" without a broader state machine.

### 4.3 Effective view reads overlay first

`EffectiveGraphView` should be tiny and explicit:

```text
node_state(node) -> overlay override or base node state
is_temporarily_unavailable(node) -> bool
interrupted_edges_to(node) -> iterator
```

This gives slice 9 a real overlay model without pretending to be a full graph query engine.

Do not move all graph traversal into `twin-emulate`. The app still owns store-backed loading and target resolution. The emulator owns the hypothetical effect once base rows are provided.

### 4.4 Base graph snapshot should be purpose-built

Do not create a complete in-memory graph crate. For restart-service, the app needs:

```text
target service node
outgoing owns/listens_on edges from target service
listening socket nodes owned by target service
incoming depends_on edges to target service
dependent service nodes
edge observation ids and evidence lines already used by impact
target-scoped unknowns and scan health notes
```

Represent that as `BaseGraphSnapshot` or `RestartServiceBaseGraph` in `twin-emulate`, populated by `twin-app`.

If a generic name makes the implementation more complex, prefer the action-specific input:

```text
RestartServiceInput
```

The best slice-9 design is reusable in concepts, not abstract in code.

### 4.5 Reuse impact logic carefully

`twin emulate restart service:X` and `twin impact service:X` should agree about direct dependents, evidence, unknowns, and configured-vs-runtime classification. But do not make `twin-emulate` depend on `ImpactResult`.

Preferred approach:

1. Keep target resolution in shared app helper `resolve_service_target`.
2. Extract small app helpers from `commands/impact.rs` only when needed:
   - load incoming `depends_on` edges;
   - classify runtime vs configured dependent;
   - load evidence lines for an edge;
   - load target-scoped unknowns.
3. Convert those app-loaded summaries into the `twin-emulate` input model.

Do not blindly move the whole impact command into shared modules. Extract the minimum reusable functions to keep code readable.

### 4.6 Restart-service semantics

Observed facts:

```text
service:postgresql.service listens_on port:tcp:127.0.0.1:5432
service:django.service depends_on service:postgresql.service
```

Hypothetical overlay:

```text
service:postgresql.service -> temporarily_unavailable
port:tcp:127.0.0.1:5432 -> temporarily_unavailable
service:django.service -> service:postgresql.service depends_on edge -> interrupted
```

Report language:

```text
service:django.service may lose dependency while postgresql.service restarts
```

Do not say it will definitely break. Restart impact is a prediction from graph evidence, not a fact.

### 4.7 Treat configured dependencies honestly

Slice 8.5 and 8.7 distinguish runtime active dependencies from configured or declared dependencies. Preserve that split:

| Dependency type | Emulation section | Risk effect |
|-----------------|-------------------|-------------|
| Runtime inferred active dependency | Transient impact | Counts toward restart runtime risk. |
| Declared but not runtime-active dependency | Configured context | Shows possible impact after start/reload; does not inflate runtime risk. |
| Socket activation dependency | Configured context unless runtime active metadata proves active use | Does not inflate runtime risk by itself. |

This keeps reports useful on hosts where many units declare dependencies but few are actively using them.

### 4.8 Risk scoring stays simple and defensible

Suggested restart-service risk:

```text
0 runtime dependents                       -> low
1-2 runtime dependents                     -> medium
3+ runtime dependents                      -> high
coverage gap affecting target              -> unknown unless evidence is otherwise strong
critical                                   -> not used in slice 9
```

Configured-only dependents should add a reason such as:

```text
2 configured dependents may be affected after restart or reload.
```

They should not change `low` to `high` by themselves in this MVP.

### 4.9 Evidence strength is not risk

Report-level evidence strength should be calculated from evidence lines supporting the affected relationships and weakened by target-scoped unknowns. A report can be:

```text
Risk: HIGH
Evidence strength: MODERATE
```

That means many direct dependents are present, but coverage is imperfect. Keep this structural separation in the result model.

### 4.10 Safety is structural, not only a warning

There should be no code path capable of performing a restart:

- CLI accepts `restart` as an emulation action name only.
- `twin-app` never shells out.
- `twin-emulate` has no OS adapter and no command runner.
- Tests scan source for forbidden restart/mutation strings in executable contexts.

The output line "No action was performed" is required, but it is not the safety mechanism. The safety mechanism is the absence of mutation APIs.

---

## 5. Domain Model

### 5.1 `EmulationAction`

Minimal action enum:

```text
EmulationAction
  RestartService { target: NodeId }
```

Validation:

- target must be `NodeKind::Service`;
- target must exist in store;
- CLI shorthand is resolved before constructing the action;
- unsupported action strings return a clear CLI/app error.

Do not add action variants for future slices.

### 5.2 `GraphOverlay`

Suggested fields:

```text
GraphOverlay {
  action: EmulationAction,
  node_overrides: Vec<NodeOverlay>,
  interrupted_relationships: Vec<InterruptedRelationship>,
}

NodeOverlay {
  node_id: NodeId,
  state: OverlayNodeState,
  reason: String,
}

InterruptedRelationship {
  edge_id: EdgeId,
  from: NodeId,
  to: NodeId,
  kind: EdgeKind,
  reason: String,
}
```

Use `Vec` plus deterministic sorting unless lookup volume requires `HashMap`. Slice 9 inputs are small. If the effective view needs lookup, build a temporary map inside the view.

### 5.3 Base graph input

Suggested input:

```text
RestartServiceInput {
  target: EmulationNode,
  unavailable_nodes: Vec<EmulationNode>,
  runtime_dependents: Vec<EmulationDependent>,
  configured_dependents: Vec<EmulationDependent>,
  unknowns: Vec<EmulationUnknown>,
}
```

`unavailable_nodes` should include:

- the target service;
- service-owned TCP port nodes from `service -> port listens_on`;
- service-owned Unix socket nodes from `service -> unix listens_on`.

Do not include owned processes as unavailable unless a user-facing report uses them. For restart semantics, service and socket availability are enough.

### 5.4 Emulation report

The `twin-emulate` crate can return a domain report. `twin-app` can wrap or convert it into a serde model for CLI/JSON.

Required app-facing result shape:

```text
EmulationResult {
  action: "restart",
  target: String,
  target_label: String,
  action_performed: false,
  safety_statement: "No action was performed.",
  risk: RiskAssessment,
  evidence_strength: EvidenceStrengthView,
  overlay: EmulationOverlaySummary,
  transient_impacts: Vec<EmulationImpact>,
  configured_impacts: Vec<EmulationImpact>,
  unknowns: Vec<ImpactUnknown or EmulationUnknown>,
}
```

Using `ImpactUnknown` is acceptable in `twin-app` if it avoids duplicate app models. Do not expose `ImpactResult` itself as the emulation result.

---

## 6. App Layer Changes

### 6.1 Public API

Add request and command functions in `twin-app/src/lib.rs`:

```text
EmulateRequest {
  config_override: Option<PathBuf>,
  action: EmulateActionRequest,
  target: Option<NodeId>,
  target_query: Option<String>,
}

emulate(request) -> Result<EmulationResult, AppError>
emulate_in(layout, request) -> Result<EmulationResult, AppError>
```

Keep action parsing at the CLI boundary or as a small app request enum. Do not pass raw `Vec<String>` from CLI into the app.

### 6.2 Command layout

Suggested files:

```text
crates/twin-app/src/commands/
  emulate.rs              # public command entry and orchestration
  emulate/
    load_restart.rs       # only if emulate.rs grows too large
```

Start with one `emulate.rs`. Split only when the file becomes difficult to scan.

### 6.3 Target resolution

For restart-service:

1. Parse exact `NodeId` if supplied.
2. Resolve service shorthand using `resolve_service_target`.
3. Load node and require `NodeKind::Service`.
4. Return `UnsupportedTarget` for ports/unix/process/file.

Error text should nudge users toward valid input:

```text
twin emulate restart postgresql.service
twin emulate restart service:postgresql.service
```

### 6.4 Loading base data

`commands/emulate.rs` should load:

- target node;
- active outgoing `listens_on` edges from target service;
- socket nodes for those edges;
- active incoming `depends_on` edges to target service;
- dependent nodes;
- edge observations and evidence lines;
- target-scoped unknowns and scan health notes.

Reuse store batch APIs from slice 8.8 where they reduce repeated queries. Avoid a schema migration.

### 6.5 Relationship classification

Use the same classification as impact:

- inferred runtime dependency from active connection -> transient impact;
- observed declared/systemd dependency without runtime active metadata -> configured impact;
- observed dependency with runtime-active metadata -> transient impact.

If the current helpers live inside `impact.rs`, extract them narrowly. Keep naming neutral enough for both commands, for example:

```text
classify_dependent_impact_kind
load_dependent_evidence
target_unknowns_for_service
```

Do not introduce a `commands/analysis` directory unless multiple extracted helpers make it clearly worthwhile.

### 6.6 Store immutability

The emulation command must open the store read-only in behavior. `Store::open` may not support SQLite read-only mode yet; do not add broad storage changes just for this slice. Instead:

- use only `&Store` methods in emulate app code;
- do not call `upsert_*`, `insert_*`, `delete_*`, `initialize`, or transaction methods;
- add a test that snapshots target node/edge rows before and after `emulate_in`.

A future store read-only connection mode can be added when more commands benefit from it.

---

## 7. CLI Changes

### 7.1 Command shape

Add:

```bash
twin emulate restart TARGET
twin emulate restart TARGET --json
twin emulate restart TARGET --config <path>
```

Suggested clap shape:

```text
Command::Emulate(EmulateArgs)

EmulateArgs {
  config: Option<PathBuf>,
  action: EmulateActionArgs
}

EmulateActionArgs::Restart { target: String }
```

Use a subcommand for `restart` rather than a free-form action string. That gives clear help output and prevents unsupported actions from reaching app logic.

### 7.2 Scan refresh

Follow `graph` and `impact`: refresh scan before emulation so the report reflects current graph state.

The existing `TWIN_PROC_ROOT` test hook in CLI should work the same way. Do not add new production env hooks.

### 7.3 Human output

Use `twin-cli/src/output/format.rs` helpers. Output should be direct and operational:

```text
Emulation: restart service:postgresql.service
═══════════════════════════════════════════════

Risk: medium
Evidence strength: moderate

Overlay:
├── service:postgresql.service temporarily unavailable
└── port:tcp:127.0.0.1:5432 temporarily unavailable

Transient impact:
└── service:django.service may lose dependency while postgresql.service restarts
    ├── path: service:django.service depends_on service:postgresql.service
    └── evidence: active connection observed through proc socket graph

Configured context:
└── service:backup.service declares dependency but no active runtime use was observed

Unknowns:
└── scan quality: partial coverage from latest scan

No action was performed.
```

Keep sections absent when empty except the safety statement, which must always render.

### 7.4 JSON output

JSON should include the same meaning without prose-heavy formatting:

```json
{
  "action": "restart",
  "target": "service:postgresql.service",
  "action_performed": false,
  "overlay": {
    "unavailable_nodes": []
  },
  "transient_impacts": [],
  "configured_impacts": [],
  "unknowns": []
}
```

Do not include ANSI styling or terminal tree characters in JSON fields.

---

## 8. `twin-emulate` Behavior

### 8.1 Restart builder flow

```text
RestartServiceOverlayBuilder
  -> validate target service input
  -> add target service unavailable override
  -> add owned listening sockets unavailable overrides
  -> add interrupted relationships for runtime dependents
  -> build report with transient/configured impact sections
  -> score risk and evidence
```

The builder name is fine because there is real multi-step construction. Do not create a generic builder for all actions.

### 8.2 Overlay summary

Overlay summary should be user-facing and deterministic:

- sort unavailable nodes by ID;
- sort interrupted relationships by edge ID;
- include reasons;
- avoid duplicate socket nodes when multiple listener edges point to the same node.

### 8.3 Direct impact only

If:

```text
frontend depends_on api
api depends_on postgres
```

Restarting postgres should show `api` in slice 9. It should not show `frontend` until slice 11 transitive traversal.

Add a test for this to prevent accidental scope creep.

### 8.4 Unknowns

Use unknowns when:

- latest scan has target-scoped unmapped sockets;
- latest scan quality is degraded or partial;
- a dependent edge has no linked observations;
- the target service has no listener sockets and no dependents.

The last case should be phrased carefully:

```text
No runtime dependents were found in the current graph.
```

That is a finding, not proof that none exist.

---

## 9. Tests

### 9.1 `twin-emulate` crate tests

Add focused unit/integration tests:

```text
restart_marks_service_and_sockets_unavailable
restart_interrupts_runtime_dependents
restart_keeps_configured_dependents_separate
effective_view_reads_overlay_before_base
direct_only_does_not_include_transitive_dependents
restart_report_action_performed_is_false
```

Use tiny in-memory domain structs. No SQLite fixtures in this crate.

### 9.2 `twin-app` tests

Extend `crates/twin-app/tests/scan_graph.rs` or add `emulate.rs` if the file is getting too large.

Test cases:

```text
emulate_restart_service_reports_runtime_dependent
emulate_restart_service_reports_configured_context
emulate_restart_service_marks_owned_tcp_port_unavailable
emulate_restart_service_marks_owned_unix_socket_unavailable
emulate_restart_service_unknowns_are_target_scoped
emulate_restart_does_not_persist_overlay_or_modify_graph
emulate_restart_rejects_port_target
emulate_restart_resolves_service_shorthand
```

Use fake `/proc` and fixture systemd roots through existing test mechanisms. Do not inspect the real host.

### 9.3 CLI output tests

Extend `crates/twin-cli/tests/output.rs`:

```text
emulate_restart_output_has_overlay_transient_impact_and_safety_statement
emulate_restart_output_omits_empty_sections_cleanly
emulate_restart_json_contains_action_performed_false
```

Add E2E tests in `cli_integration.rs`:

```text
emulate_restart_service_with_fake_proc
emulate_restart_service_json_with_fake_proc
```

### 9.4 Safety tests

At minimum:

- assert `twin-emulate` has no dependencies on process execution crates;
- source scan for forbidden mutation strings in `crates/twin-emulate`, `crates/twin-app/src/commands/emulate*`, and `crates/twin-cli/src`;
- behavioral test that fake service graph is unchanged after emulation.

Forbidden strings should include:

```text
systemctl restart
systemctl stop
service restart
kill -9
docker restart
kubectl delete
kubectl rollout
```

Avoid over-broad string scans that fail on documentation or test names unless the existing safety-test pattern already handles contexts.

---

## 10. Acceptance Criteria

Slice 9 is complete only when:

- [ ] `twin emulate restart postgresql.service` runs after `twin scan`.
- [ ] `twin emulate restart service:postgresql.service` resolves exact service IDs.
- [ ] Service shorthand resolution works and ambiguous shorthand errors are clear.
- [ ] Unsupported targets such as `port:tcp:127.0.0.1:5432` are rejected for restart-service emulation.
- [ ] Report includes risk and evidence strength as separate fields.
- [ ] Report includes an overlay summary with target service temporarily unavailable.
- [ ] Report includes owned TCP and Unix listening sockets as temporarily unavailable when present.
- [ ] Runtime direct dependents appear under transient impact.
- [ ] Configured-only dependents appear separately and do not inflate runtime risk.
- [ ] Report includes target-scoped unknowns and scan-health notes.
- [ ] Human output always says `No action was performed.`
- [ ] JSON output includes `action_performed: false`.
- [ ] No real service restart, process kill, shell command, Docker action, or Kubernetes action is possible from the code path.
- [ ] Overlay is not persisted as real graph state.
- [ ] Test proves relevant base graph nodes/edges are unchanged after emulation.
- [ ] Direct-only behavior is covered; transitive dependents are not reported yet.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace -- -D warnings` passes.

---

## 11. Implementation Order

1. Add `twin-emulate` crate and workspace entry.
   - Verify: `cargo test -p twin-emulate` compiles with empty/minimal tests.
2. Define action, overlay, effective view, restart input, and domain report types.
   - Verify: unit tests for overlay lookup and deterministic ordering.
3. Implement `RestartServiceOverlayBuilder`.
   - Verify: crate tests for unavailable service/socket nodes and interrupted runtime relationships.
4. Add `twin-app` emulation request/result models and command skeleton.
   - Verify: app test for missing DB / unsupported target mirrors impact error style.
5. Load restart-service base graph data in `twin-app`.
   - Verify: app tests with existing fake proc/systemd fixtures.
6. Wire CLI parsing and output.
   - Verify: renderer tests and CLI E2E with `TWIN_PROC_ROOT`.
7. Add immutability and safety tests.
   - Verify: before/after store rows match; forbidden mutation strings absent from executable paths.
8. Update `docs/state/slice-09.md`.
   - Verify: every acceptance criterion is checked off or explicitly documented as blocked.
9. Run full verification.
   - Verify: `cargo fmt --check`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`.

---

## 12. Quality Bar

The best slice-9 implementation is not the most general one. It is the smallest correct foundation for overlay emulation:

- action-specific where the product only supports one action;
- typed at app/domain boundaries;
- deterministic in output and tests;
- structurally read-only;
- honest about observed facts vs predictions;
- clear about unknowns;
- easy to extend in slice 10 without rewriting the restart path.

Do not compromise on the read-only promise, evidence separation, or output clarity. Do compromise on abstraction breadth: defer generic graph engines and rule systems until multiple supported actions make them pay for themselves.
