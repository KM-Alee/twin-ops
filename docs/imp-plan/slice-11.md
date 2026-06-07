# Slice 11 Implementation Plan: Transitive Impact Paths and Better Risk Scoring

**Implemented layout and tests:** see `docs/code-layout.md` for crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. Slice 11 builds on the completed impact and emulation paths from slices 8-10. Keep this slice focused: path traversal, cycle safety, user depth control, and risk reasons that explain the blast radius. Do not introduce temporal history, eBPF evidence, a rule engine, or a broad graph crate yet.

## 1. Overview

Slice 11 makes `twin` answer:

```text
If this service is unavailable or hypothetically restarted, what is the full dependency path from the target to each affected service, how far does the blast radius reach, and why did twin score it that way?
```

Working demo:

```bash
twin scan
twin impact postgresql.service --paths
twin emulate restart postgresql.service --paths
twin impact postgresql.service --paths --max-depth 2
```

The ideal outcome is:

1. `twin impact <service> --paths` shows direct and transitive runtime dependency paths that terminate at the target.
2. `twin emulate restart <service> --paths` uses the same traversal result for restart blast-radius reporting without mutating host state.
3. Reports preserve the direct dependent sections users already have, then add a clear `Impact paths` section when requested.
4. Cycles are detected and shown as capped paths instead of causing infinite traversal or duplicate noisy output.
5. Users can cap traversal depth with `--max-depth N`; the default should be useful and bounded.
6. Risk scoring considers direct dependents, transitive dependents, public exposure, active runtime evidence, unknown scan coverage, and conservative service criticality hints.
7. Risk and evidence strength remain separate fields in both human and JSON output.
8. JSON remains stable and explicit: path data is structured, depth-limited paths are marked, and cycle detection is visible.
9. Tests prove real graph behavior with small service dependency fixtures, not synthetic scoring-only cases.

This slice should make the first major demo milestone feel serious. It should not become a generalized graph analytics framework.

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Impact target resolution | `twin impact` resolves exact node IDs and service shorthand. |
| Direct dependency analysis | `load_typed_service_dependents` returns runtime/configured service dependents with evidence and one-edge path steps. |
| Emulation input | Restart emulation already receives runtime/configured dependents and renders paths as strings. |
| Path model seed | `ImpactPathStep` and `EmulationPathStep` already exist for one-edge paths. |
| Evidence model | Edge evidence lines include source, statement, relationship, strength, and observation ID. |
| Unknowns | Impact and emulation already surface scan health and target-scoped coverage gaps. |
| CLI output | `twin-cli/src/output/impact.rs` and `output/emulate.rs` use shared tree helpers and stable sections. |
| Store graph API | Active edges can be loaded by source/target and decoded to typed `GraphEdge`s. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| CLI flags | `--paths` and `--max-depth` for `impact` and `emulate restart`. |
| Traversal helper | Recursive incoming dependency traversal from affected dependents back to the target. |
| Cycle handling | Per-path visited-node tracking and reportable cycle/depth cap notes. |
| Transitive result model | Structured path reports with terminal node, depth, steps, evidence, and truncation/cycle flags. |
| Scoring inputs | Count direct/transitive dependents, public exposure, runtime evidence, unknown coverage, and criticality hints. |
| Output sections | Human `Impact paths` section for impact and restart emulation. |
| Emulation conversion | Restart reports need path-aware impacts without making `twin-emulate` depend on `twin-store` or `twin-app`. |
| Tests | High-value traversal, cycle, max-depth, scoring, output, JSON, and CLI E2E tests. |

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| CLI request shape | Add `show_paths: bool` and `max_depth: usize` to `ImpactRequest`; add equivalent path options for restart emulation only. |
| Defaults | Use a bounded default depth of `4`. Reject `0`; cap overly large input at a small product limit such as `8` or return a clear validation error. Prefer validation error because it is honest. |
| Service traversal | Traverse incoming active `depends_on` edges recursively for service targets. Direct path direction should read as dependent service -> ... -> target service. |
| Configured/runtime split | Include runtime paths in the main path report. Include configured-only paths only under configured context unless runtime evidence appears on the path. |
| Port targets | Keep port impact behavior direct in this slice. If `--paths` is used on a port target, show direct caller paths only and explain that recursive service blast-radius starts from service targets. |
| Cycle detection | Track node IDs already in the current path. When a next edge would revisit a node, record a cycle-capped path and stop that branch. |
| Max-depth handling | Stop traversal after `max_depth` edges and record the path as depth-capped when more incoming dependents exist beyond the cap. |
| Path evidence | Carry evidence for every edge where available. Missing evidence becomes an unknown that weakens evidence, not a panic. |
| Transitive path result | Add a path report model shared by impact and app-level emulation results, likely `ImpactPath` or `BlastRadiusPath`, containing terminal node, depth, steps, evidence, `is_cycle_capped`, and `is_depth_capped`. |
| Risk scoring | Extend scoring to include direct runtime count, transitive runtime count, public exposure, active connection evidence, unknown process/socket count, and service criticality hints. |
| Public exposure | Conservative MVP: a path is publicly exposed if the terminal or any path node is a service with an owned listener on `0.0.0.0`, `::`, or a non-loopback address discovered in the current graph. Do not inspect firewall rules. |
| Criticality hints | Hard-code a tiny, explicit service-name classifier in app scoring only: database/storage/message-bus names such as `postgres`, `mysql`, `mariadb`, `redis`, `rabbitmq`, `nats`, `kafka`, `etcd`, and `containerd`. Hints add a risk reason; they are not evidence. |
| Runtime evidence | Treat `EdgeClass::Inferred` dependency edges from active connection inference as runtime evidence. Treat `EdgeClass::Observed` systemd dependencies as configured unless existing metadata classifies them otherwise. |
| Emulation integration | `twin-app` loads and passes transitive restart paths into `twin-emulate` as plain input structs. `twin-emulate` formats path-aware impacts but does not query SQLite. |
| Output | Human output uses tree formatting and does not dump debug structs. Paths render as readable chains with edge kind/class and capped/cycle notes. |
| JSON | Add fields without removing existing ones. Existing direct dependent arrays remain. New path arrays are empty when paths are not requested. |
| Docs state | During implementation, update `docs/state/slice-11.md`; on completion, verify each acceptance criterion and note deviations. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Temporal history or snapshots | Slice 12 | Slice 11 reads the current graph only. |
| eBPF runtime evidence | Slices 14-16 | No eBPF event stream exists yet. Use current active connection inference. |
| General `twin-graph` crate | Later | Traversal is still small and service-specific. Extract only if app/emulate duplication becomes real. |
| Rules engine | Later | A deterministic scoring function is clearer than a premature `twin-rules` crate. |
| Weighted graph algorithms | Later | Users need explainable paths, not centrality metrics. |
| File delete transitive blast radius | Later | Delete-file impact is already separated by runtime/restart/persistent buckets. Keep slice 11 centered on service dependency paths. |
| Endpoint blocking | Slice 18+ | Endpoint action model is not present. |
| Service criticality config | Later | A user config field is speculative until users need override behavior. |
| Public internet certainty | Later | Without firewall/routing evidence, public exposure is only a graph hint. Wording must reflect that. |

## 4. Architectural Decisions

### 4.1 Keep traversal in `twin-app` for now

The app layer already resolves targets, loads graph rows, converts evidence, and assembles reports. Add a small traversal module near impact instead of creating a crate:

```text
crates/twin-app/src/commands/
  impact.rs
  impact_paths.rs        # traversal, path assembly, cycle/depth notes
  impact_scoring.rs      # only if impact.rs scoring is becoming too large
```

Only split `impact_scoring.rs` if the existing file becomes hard to read. The best design here is a narrow helper with typed inputs, not a broad abstraction.

The dependency rule stays:

```text
twin-app     -> twin-core, twin-store, twin-emulate
twin-emulate -> twin-core
twin-cli     -> twin-app
```

### 4.2 Traverse incoming runtime dependency edges

For target `service:postgresql.service`, traversal walks incoming active `depends_on` edges:

```text
service:nginx.service -> service:django.service -> service:postgresql.service
```

The path renders from outer dependent toward the target because that is how users reason about blast radius:

```text
service:nginx.service
  -> DEPENDS_ON service:django.service
  -> DEPENDS_ON service:postgresql.service
```

Store internally may query `list_edges_to(current)` repeatedly. Keep the traversal bounded and deterministic:

1. Sort incoming edges by `from` node ID and edge ID before exploring.
2. Skip inactive/stale/gone edges in main paths.
3. Stop branch on cycle or max depth.
4. Deduplicate identical node chains.

### 4.3 Preserve path steps as typed data

Extend the existing path step model rather than replacing it:

```text
ImpactPath {
  terminal: ImpactNodeSummary,
  depth: usize,
  steps: Vec<ImpactPathStep>,
  evidence: Vec<ImpactEvidenceLine>,
  is_cycle_capped: bool,
  is_depth_capped: bool,
}
```

`terminal` is the farthest affected node found for the path, not the target. `steps` should still include edge IDs so later evidence and debugging remain possible.

For emulation, mirror only the domain-neutral parts needed by `twin-emulate`:

```text
EmulationImpactPath {
  terminal id/label
  steps: Vec<EmulationPathStep>
  evidence: Vec<EmulationEvidenceLine>
  is_cycle_capped
  is_depth_capped
}
```

Do not make `twin-emulate` import app models.

### 4.4 `--paths` controls rendering and extra traversal

Do not pay recursive traversal cost unless requested:

```bash
twin impact postgres                 # existing direct report
twin impact postgres --paths          # direct report + path report
twin impact postgres --paths --max-depth 2
```

If `--max-depth` is supplied without `--paths`, accept it but it has no visible effect, or return a concise validation error. Prefer accepting it to keep CLI ergonomics simple.

### 4.5 Avoid fake precision in risk

Risk levels remain coarse:

```text
low, medium, high, critical, unknown
```

Suggested service impact scoring:

| Input | Effect |
|-------|--------|
| No runtime or configured dependents | low unless unknowns dominate |
| 1-2 direct runtime dependents | medium |
| 3+ direct runtime dependents | high |
| Any transitive runtime path beyond depth 1 | raise at most one level |
| 5+ total runtime impacted services | high or critical depending exposure |
| Public exposure on any runtime path | raise one level and add reason |
| Active connection evidence on direct edge | strengthens evidence, not risk by itself |
| Unknown process/socket count or scan degraded | can make risk `unknown` when dependency picture is incomplete |
| Criticality hint on target or terminal service | add reason; raise only if dependents also exist |

Evidence scoring remains separate:

| Evidence input | Effect |
|----------------|--------|
| Inferred active connection dependency | strong |
| Multiple runtime edges in path with evidence | strong or very strong |
| Systemd configured-only edges | moderate |
| Missing edge evidence | weakens and emits unknown |
| Scan gaps | cap evidence strength through existing helper style |

### 4.6 Public exposure is graph-derived, not a security finding

Public exposure wording must stay cautious:

```text
dependency path reaches a service with a non-loopback listener
```

Do not say:

```text
service is internet exposed
```

The graph does not know firewall, routing, NAT, cloud load balancers, or host policy yet.

### 4.7 Cycle behavior should be visible but quiet

Cycles are normal in service graphs. A cycle should produce one concise note:

```text
cycle capped at service:api.service; path already contains this node
```

The path should still render up to the repeated edge. Do not emit every possible cycle permutation.

### 4.8 Emulation keeps overlay semantics separate from path traversal

Restart emulation still overlays only the target service and owned listeners as temporarily unavailable. Transitive path analysis explains who may be affected; it should not mark every transitive dependent as unavailable in the overlay.

This keeps observed facts, inferred blast radius, and hypothetical overlay state separate:

```text
Overlay:
  target unavailable
  target listeners unavailable

Impact paths:
  dependent chain may be affected
```

## 5. Implementation Steps

### Step 1: Add CLI and request fields

Implement:

```text
crates/twin-cli/src/cli/args.rs
crates/twin-cli/src/cli/impact.rs
crates/twin-cli/src/cli/emulate.rs
crates/twin-app/src/lib.rs
```

Add:

```text
--paths
--max-depth <N>
```

to `ImpactArgs` and `EmulateRestartArgs`.

Validation:

1. Default max depth: `4`.
2. Reject `0`.
3. Reject values greater than `8` with a clear error.

Verify with CLI parser tests or E2E tests that requests carry the fields correctly.

### Step 2: Add path result models

Extend app models:

```text
crates/twin-app/src/model/impact_result.rs
crates/twin-app/src/model/emulation_result.rs
```

Recommended fields:

```text
ImpactResult {
  impact_paths: Vec<ImpactPath>,
  paths_requested: bool,
  max_depth: usize,
}

EmulationResult {
  impact_paths: Vec<ImpactPath>,
  paths_requested: bool,
  max_depth: usize,
}
```

Use `Vec::new()` when paths are not requested so JSON shape stays stable.

### Step 3: Implement service path traversal

Add a focused helper:

```text
crates/twin-app/src/commands/impact_paths.rs
```

Suggested public app-internal API:

```text
load_service_impact_paths(store, target, target_label, max_depth) -> ImpactPathAnalysis
```

`ImpactPathAnalysis` should contain:

```text
paths
unknowns
direct_runtime_count
transitive_runtime_count
public_exposure_paths
criticality_hints
```

Keep traversal rules explicit:

1. Start at target service.
2. Query incoming active `depends_on` edges.
3. Classify each edge with existing `classify_dependent_impact_kind`.
4. Recurse through runtime dependents only for main runtime paths.
5. Preserve configured-only edges as context if they are direct; do not let them explode recursive paths.
6. Load evidence using existing evidence helpers.
7. Record missing evidence as unknowns.
8. Track current path node IDs for cycle detection.
9. Sort output by terminal ID, then depth, then edge IDs.

### Step 4: Update impact command assembly

In `commands/impact.rs`:

1. Preserve existing direct report behavior.
2. When `show_paths` is true and target is a service, load path analysis.
3. Merge path unknowns with existing unknowns and deduplicate by kind/detail/source.
4. Score risk using both direct analysis and path analysis.
5. Score evidence with path evidence plus existing direct evidence.

For port/unix targets:

1. Keep direct impact unchanged.
2. If `show_paths` is true, expose one-edge caller paths only.
3. Add a risk reason explaining recursive blast-radius traversal is service-based in this slice if needed.

### Step 5: Update restart emulation input and domain report

Extend `twin-emulate` input structs only enough for path-aware restart reporting:

```text
RestartServiceInput {
  impact_paths: Vec<EmulationImpactPath>,
  paths_requested: bool,
  max_depth: usize,
}
```

`twin-app` converts `ImpactPath` into `EmulationImpactPath` before calling `emulate_restart_service`.

`twin-emulate` should:

1. Keep overlay creation unchanged.
2. Keep direct transient/configured impact arrays unchanged.
3. Include path-aware impacts or path report fields in `EmulationDomainReport`.
4. Avoid any store access.

### Step 6: Render human output

Update:

```text
crates/twin-cli/src/output/impact.rs
crates/twin-cli/src/output/emulate.rs
crates/twin-cli/src/output/sections.rs
```

Add one section title:

```text
Impact paths
```

Path rendering should be compact:

```text
Impact paths
├── nginx.service
│   ├── path: service:nginx.service -> service:django.service -> service:postgresql.service
│   ├── depth: 2
│   └── evidence: active connection inferred from socket ownership
└── worker.service
    ├── path: service:worker.service -> service:postgresql.service
    └── depth: 1
```

For capped paths:

```text
└── note: depth capped at 2
```

or:

```text
└── note: cycle capped at service:api.service
```

Do not add verbose explanations or help text in the CLI output.

### Step 7: Improve scoring reasons

Move scoring into a small helper if needed. Scoring output should be deterministic and inspectable:

```text
Risk:
├── level: high
├── reason: 2 direct runtime dependents
├── reason: 4 transitive runtime dependents
└── reason: dependency path reaches non-loopback listener on nginx.service
```

Evidence output should remain:

```text
Evidence strength: strong
```

Unknowns that weaken evidence should continue to render under `Unknowns (weakens evidence)`.

### Step 8: Update docs state during implementation

As work lands, update `docs/state/slice-11.md` with:

1. status;
2. implemented items;
3. decisions;
4. deviations;
5. acceptance checklist.

Do not mark complete until verification passes.

## 6. Testing Plan

Tests should be small graphs that mirror real service behavior. Avoid bloated matrix tests.

### 6.1 Traversal tests in `twin-app`

Add integration tests under `crates/twin-app/tests/impact_paths.rs` using temporary stores and typed graph inserts.

Cover:

1. Direct only:
   - `api.service -> postgres.service`
   - `--paths` returns one path of depth 1.
2. Transitive chain:
   - `nginx.service -> api.service -> postgres.service`
   - terminal is nginx, depth is 2, ordered steps are preserved.
3. Branching:
   - `api.service -> postgres.service`
   - `worker.service -> postgres.service`
   - both paths render once, sorted deterministically.
4. Cycle:
   - `api.service -> postgres.service`
   - `postgres.service -> api.service`
   - traversal returns a cycle-capped note and terminates.
5. Max depth:
   - `edge.service -> nginx.service -> api.service -> postgres.service`
   - max depth 2 returns capped path and does not include unbounded ancestors.
6. Missing evidence:
   - active edge without observation link returns a path plus an unknown that weakens evidence.

### 6.2 Scoring tests

Keep scoring tests table-driven but narrow:

1. no dependents -> low risk;
2. one direct runtime dependent -> medium risk;
3. transitive path present -> risk reason includes transitive count;
4. non-loopback listener on a path node -> risk reason includes non-loopback listener;
5. scan unknowns can produce `unknown` or cap evidence, depending existing scoring convention.

Do not test every threshold permutation. Test boundaries that encode product behavior.

### 6.3 CLI output tests

Update `crates/twin-cli/tests/output.rs`:

1. impact output includes `Impact paths` only when paths are present/requested;
2. path lines show real node IDs and labels;
3. capped/cycle notes render clearly;
4. restart emulation output still includes safety statement and overlay sections.

### 6.4 CLI E2E tests

Update `crates/twin-cli/tests/cli_integration.rs` with fixture-backed scan data:

1. `twin impact postgresql.service --paths` exits successfully and shows a transitive path.
2. `twin impact postgresql.service --paths --max-depth 1` shows a depth cap.
3. `twin emulate restart postgresql.service --paths` exits successfully, shows paths, and includes `No action was performed.`

Use fake `/proc` and fake systemd fixtures. Do not read real host state in automated tests.

### 6.5 Immutability and safety tests

Extend existing safety coverage to ensure no new mutating strings or command paths appear.

Behavioral checks:

1. restart emulation with paths leaves node/edge rows unchanged;
2. impact paths never write graph rows;
3. no tests require root, Docker, systemd D-Bus, or host `/etc`.

## 7. Acceptance Criteria

- [ ] `twin impact SERVICE --paths` renders direct and transitive service dependency paths.
- [ ] `twin emulate restart SERVICE --paths` renders path-aware restart blast radius and still reports `No action was performed.`
- [ ] `--max-depth N` caps traversal and reports capped paths.
- [ ] Cycles terminate deterministically and render a concise cycle note.
- [ ] Existing direct dependent output remains present and stable.
- [ ] Port/unix impact behavior remains supported; recursive service traversal is not falsely claimed for ports.
- [ ] Risk reasons include direct count, transitive count when present, public-exposure hint when graph evidence supports it, criticality hints when applicable, and unknown coverage when present.
- [ ] Evidence strength remains separate from risk in human and JSON output.
- [ ] JSON includes structured path data with steps, evidence, depth, and cap flags.
- [ ] Missing edge evidence creates an unknown instead of dropping the path or panicking.
- [ ] Restart emulation overlay does not mark transitive dependents as unavailable.
- [ ] No schema migration is added unless implementation proves current tables cannot represent paths.
- [ ] Tests use fixtures and temporary stores only.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace -- -D warnings` passes.

## 8. Ideal User-Facing Outcome

After slice 11, this should feel polished:

```text
twin impact postgresql.service --paths

twin impact
summary: high risk · 1 runtime dependent · 2 transitive dependent(s)

Risk
├── level: high
├── evidence_strength: strong
├── reason: 1 direct runtime dependent
├── reason: 2 transitive runtime dependents
└── reason: dependency path reaches non-loopback listener on nginx.service

Direct runtime dependents
└── api.service
    ├── reason: active connection to postgresql.service
    └── evidence: process socket matched listener port

Impact paths
└── nginx.service
    ├── path: service:nginx.service -> service:api.service -> service:postgresql.service
    ├── depth: 2
    └── evidence: active connection inferred from process ownership
```

The report should be confident where the graph is strong, cautious where inference is incomplete, and explicit about what is unknown.

## 9. Implementation Guardrails

1. Do not add mutation APIs, shell commands, service operations, or filesystem writes beyond normal local state used by existing commands.
2. Do not refactor unrelated scan, graph, or store code.
3. Do not introduce a crate or trait just because traversal sounds reusable. Extract only after duplication appears.
4. Keep all new fields typed and serde-friendly.
5. Preserve existing output sections and tests unless the slice explicitly changes them.
6. Prefer `pub(crate)` for helpers.
7. Sort traversal outputs for deterministic tests and stable CLI output.
8. Keep comments rare; use names and small functions to explain behavior.

## 10. Final Verification and Subagent Instructions

After implementation and normal verification, spawn a subagent for an independent review before marking slice 11 complete.

Suggested subagent prompt:

```text
You are reviewing twin slice 11. Check the implementation against docs/imp-plan/slice-11.md, docs/state/slice-11.md, AGENTS.md, docs/prd.md, docs/tech-document.md, docs/plan-slices.md, and docs/code-layout.md.

Focus on:
1. read-only safety;
2. dependency direction;
3. transitive path correctness;
4. cycle and max-depth behavior;
5. risk/evidence separation;
6. lean, meaningful tests;
7. CLI output quality.

Do not make broad refactors. Report findings with file and line references. If a small fix is obviously required, patch only that fix and rerun the smallest relevant verification command.
```

Then test the CLI on this system with real installed apps, as a manual smoke test only:

```bash
cargo build
./target/debug/twin init
./target/debug/twin doctor
./target/debug/twin scan
./target/debug/twin impact postgresql.service --paths --max-depth 4
./target/debug/twin impact nginx.service --paths --max-depth 4
./target/debug/twin emulate restart postgresql.service --paths --max-depth 4
./target/debug/twin emulate restart nginx.service --paths --max-depth 4
```

Expected manual-test behavior:

1. Commands may report missing services if those apps are not installed; that is acceptable and should be a clean error.
2. If services exist, output should show direct dependents, paths when the graph has them, risk reasons, evidence strength, unknowns, and safety statements.
3. No command may restart, stop, reload, kill, delete, install, or mutate host services/files.
4. Record real-system observations and any clean missing-service outcomes in `docs/state/slice-11.md`.
