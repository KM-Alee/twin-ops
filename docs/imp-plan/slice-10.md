# Slice 10 Implementation Plan: Runtime vs Restart Impact and File Deletion Emulation

**Implemented layout and tests:** see `docs/code-layout.md` for current crate boundaries, `TwinLayout`, CLI rendering, and integration-test conventions. Slice 10 builds on the completed slice 9 overlay engine. Keep the implementation narrow: file nodes, service-config edges for a small set of common config locations, delete-file emulation, and explicit runtime/restart impact sections. Do not build a general config parser yet.

## 1. Overview

Slice 10 makes `twin` answer:

```text
If this config file disappeared, what changes immediately, what changes after restart or reload, what evidence supports that, and what did twin leave untouched?
```

Working demo:

```bash
twin scan
twin emulate delete /etc/nginx/nginx.conf
```

The ideal outcome is:

1. `twin scan` materializes file nodes for known service configuration files and observed `service configured_by file` edges.
2. `twin graph service:nginx.service` shows the service's known configuration files with evidence.
3. `twin graph file:/etc/nginx/nginx.conf` shows the services configured by that file.
4. `twin emulate delete /etc/nginx/nginx.conf` canonicalizes the target to `file:/etc/nginx/nginx.conf`, never deletes it, and never shells out.
5. Delete-file emulation reports separate impact categories:
   - runtime impact;
   - restart impact;
   - transient impact;
   - persistent impact;
   - unknown impact.
6. Runtime impact is usually low for an already-running service whose config file was discovered statically.
7. Restart impact is high or critical when a service is configured by the deleted file and may fail to reload, restart, or start cleanly without it.
8. Risk and evidence strength remain separate report fields.
9. Human output is polished and explicit: `No file was deleted.` and `No action was performed.`
10. Tests prove the target file remains on disk and the stored base graph remains unchanged after emulation.

This slice should not become slice 19 early. It should discover and connect files; it should not parse nginx `proxy_pass`, infer config-driven upstream dependencies, hash file contents for temporal change detection, or introduce a `twin-config` crate.

## 2. Current Starting Point

Already available:

| Area | Existing support |
|------|------------------|
| Graph vocabulary | `NodeKind::File`, `NodeId::file`, `GraphNode::file`, and `EdgeKind::ConfiguredBy` already exist. |
| Store | Typed node/edge upsert, evidence links, batch node/observation loads, and active-edge queries exist. |
| systemd unit scan | Unit files, drop-ins, D-Bus state, enable symlinks, and socket activation are already collected. |
| Service graph | Services own processes/cgroups and listen on TCP/Unix sockets; declared and runtime dependents are modeled. |
| Emulation | `twin-emulate` owns `EmulationAction`, `GraphOverlay`, `EffectiveGraphView`, restart-service input/report/scoring, and safety constants. |
| App orchestration | `twin-app/src/commands/emulate.rs` resolves restart targets, loads service dependents, and converts domain reports to app results. |
| CLI pattern | `twin-cli/src/cli/emulate.rs` parses emulate commands; `twin-cli/src/output/emulate.rs` renders human reports and JSON. |
| Unknowns | Impact/emulate already carry structured unknowns from scan quality and target-scoped coverage gaps. |

Missing:

| Area | Needed in this slice |
|------|----------------------|
| File discovery | A small read-only collector path for known service config files. |
| Configured-by persistence | `service configured_by file` edges with evidence links and source metadata. |
| File graph view | Graph output for file targets and service config sections. |
| Delete action model | `EmulationAction::DeleteFile { target }` and CLI parsing for `twin emulate delete PATH_OR_FILE_ID`. |
| Overlay state | A node override representing hypothetical deletion or unavailability of a file. |
| Impact categories | Domain/app/CLI result sections for runtime, restart, transient, persistent, and unknown impact. |
| Delete-file scoring | Action-specific risk/evidence scoring that does not reuse restart-service assumptions blindly. |
| Safety tests | Behavioral and static checks that no file removal or host mutation path exists. |

## 3. Scope

### 3.1 This Slice Does

| Area | Detail |
|------|--------|
| Observation vocabulary | Add the minimum observation kinds and source needed for config-file discovery, likely `ConfigFileSeen` and `ServiceConfiguredByFile`, with source `ConfigFileDiscovery` or a narrowly named equivalent. |
| Collector behavior | Extend existing systemd/service scan code to discover service config files from systemd unit metadata and a tiny list of well-known config paths. |
| File nodes | Upsert `GraphNode::file(path, seen_at, existing)` for each discovered config file that can be safely identified. |
| Configured-by edges | Add active `service configured_by file` edges with `EdgeClass::Observed` when direct source evidence exists. |
| Evidence | Link edges to observations and render evidence lines that cite the discovery source, for example unit path, drop-in path, or known-service config mapping. |
| Graph output | Show configured files in service graph output and add a file neighborhood view listing configured services and evidence. |
| Delete target parsing | Accept both `file:/absolute/path` and plain absolute paths for `twin emulate delete`; convert to `NodeId::file` at the CLI/app boundary. |
| Delete action | Add `EmulationAction::DeleteFile { target: NodeId }` in `twin-emulate`. Reject non-file targets for delete-file emulation. |
| Overlay model | Add the smallest new overlay node state needed for a hypothetical deleted file. Do not persist it. |
| Delete-file input | Add a delete-file input model containing target file, configured services, evidence lines, unknowns, and file-existence context. |
| Impact sections | Add typed delete-file impact buckets for runtime, restart, transient, persistent, and unknown impacts. |
| Risk/evidence scoring | Score delete-file risk from configured service count, service state evidence, file existence, and coverage unknowns; compute evidence strength separately from risk. |
| CLI output | Render stable sections for action, target, risk, evidence strength, overlay summary, runtime impact, restart impact, persistent impact, transient impact if any, unknown impact, evidence, and safety statement. |
| JSON output | Keep snake_case fields stable; include `action_performed: false` and a delete-specific safety statement. |
| Tests | Focused collector, app, emulate-domain, output, CLI E2E, immutability, and safety tests described below. |

### 3.2 This Slice Does Not Do

| Excluded | Arrives In | Reason |
|----------|------------|--------|
| Full nginx parser | Slice 19 | Slice 10 only needs service-to-config relationships. |
| `proxy_pass` / endpoint inference | Slice 19 | Config content dependency inference is a separate feature. |
| File hash tracking | Slice 12 or 19 | Temporal change detection is not required for delete-file emulation. |
| Generic config crate | Slice 19 | A new `twin-config` crate would be mostly empty in this slice. |
| Recursive/transitive blast-radius paths | Slice 11 | Slice 10 classifies direct services configured by the deleted file. |
| Package ownership | Slice 21 | Do not infer config ownership from dpkg/rpm/pacman yet. |
| Mutating filesystem checks | Never | No deleting, renaming, chmod, writing temp markers next to user files, or shelling out. |
| Service reload semantics per daemon | Later | Do not pretend to know whether a specific daemon rereads config live unless evidence exists. |
| New SQLite migration | Prefer no | Existing node/edge/observation metadata is enough unless implementation proves otherwise. |

## 4. Design Decisions

### 4.1 Use the existing scan pipeline, not a new crate

The current collectors already know about processes and systemd unit files. Slice 10 should add a small config-file discovery step near the systemd scan path rather than introducing `twin-config`.

Recommended placement:

```text
crates/twin-collectors/src/systemd/
  config_files.rs       # pure helpers for unit/drop-in/known-path discovery

crates/twin-app/src/commands/
  scan_config_files.rs  # app persistence of file nodes and configured_by edges
```

Only split files if the existing scan modules would become difficult to read. Keep discovery pure and persistence in `twin-app`, matching the current observation-to-graph pattern.

### 4.2 Discovery should be conservative and evidence-backed

Start with sources that are defensible:

| Source | Relationship |
|--------|--------------|
| The service's main unit file path | `service configured_by file:<unit-file>` |
| Unit drop-in files | `service configured_by file:<drop-in>` |
| Known config paths for common services | `service configured_by file:<known-config>` only when the service node exists and the file exists or can be read as metadata |

Initial known config mapping should stay tiny:

```text
nginx.service      -> /etc/nginx/nginx.conf
postgresql.service -> /etc/postgresql
```

Do not glob broad trees such as `/etc/postgresql/*` unless the implementation can keep it read-only, deterministic, and testable without host leakage. Prefer one directory node only if directory nodes already exist; otherwise use discovered regular files under fixture-controlled paths and defer recursive filesystem modeling to slice 20.

For systemd unit files, there is already strong evidence from the unit collector. For known service mappings, evidence must say it is a known-path convention, not direct proof from file contents.

### 4.3 `configured_by` direction stays service to file

Use the existing edge kind exactly as designed:

```text
service:nginx.service configured_by file:/etc/nginx/nginx.conf
```

For file-target graph and emulation, query incoming `configured_by` edges to find services affected by the file. Do not add a reverse edge.

### 4.4 Add only the overlay state the action needs

Slice 9 has `OverlayNodeState::TemporarilyUnavailable`. Delete-file emulation needs a distinct hypothetical state because deletion is persistent until restored, not transient downtime.

Add one state:

```text
HypotheticallyDeleted
```

Use it only in the overlay. Do not add `NodeState::Deleted` to the persisted graph unless the temporal-history slice needs it. The base graph must remain active.

### 4.5 Keep delete-file emulation in `twin-emulate`

`twin-emulate` should own the domain semantics:

```text
DeleteFileInput
  target file
  configured services
  evidence
  unknowns
  file existence/readability context

emulate_delete_file(input) -> EmulationDomainReport
```

The app layer should only:

1. canonicalize and validate the target;
2. load current graph rows;
3. load evidence and unknowns;
4. pass plain input structs into `twin-emulate`;
5. convert the domain report into the app model.

Do not make `twin-emulate` depend on `twin-store`, collectors, `twin-app`, or `twin-cli`.

### 4.6 Use typed impact buckets, not ad hoc strings

The report should make impact categories structural. Extend the domain/app result models so each bucket is a field, not a heading inferred from a string:

```text
runtime_impacts
restart_impacts
transient_impacts
persistent_impacts
unknown_impacts
```

Restart-service reports can keep their existing transient/configured fields for compatibility in this slice, but delete-file emulation should use the richer buckets. If unifying the model is simple, do it once; if it causes broad churn, add delete-specific fields without refactoring restart behavior.

### 4.7 Runtime vs restart semantics

For a file configured by a service:

| Impact bucket | Default classification |
|---------------|------------------------|
| Runtime | Low risk statement: running service is not assumed to reread this file immediately. |
| Restart | High or critical statement: service may fail to reload/restart/start if required config is missing. |
| Persistent | The missing file remains a problem until restored. |
| Transient | Empty by default for delete-file. Use only if evidence supports temporary reload/start interruption. |
| Unknown | Use when `twin` has file evidence but cannot link it to any active service, cannot stat/read metadata due to permissions, or scan quality weakens confidence. |

Avoid absolute claims like "nginx will fail." Prefer "may fail to reload or restart without this file" unless a later parser proves required semantics.

### 4.8 Risk scoring should be deterministic and explainable

Suggested delete-file risk:

| Condition | Risk |
|-----------|------|
| No configured services found, file exists only as target | low or unknown depending on evidence |
| One configured service, no active runtime/service-state evidence | high |
| One active service configured by file | critical for restart impact |
| Multiple configured services | critical |
| Target file not in graph and cannot be found safely | unknown |

Evidence strength should come from the source:

| Evidence | Strength |
|----------|----------|
| Direct systemd unit/drop-in file path | strong |
| Known-path service mapping with file existence | moderate |
| Known-path mapping without file existence | weak |
| Coverage gaps or permission gaps | cap or weaken using existing evidence cap helpers |

Risk must not increase just because evidence is strong. Evidence must not be strong just because risk is high.

### 4.9 Path handling must be exact and safe

Accept:

```text
twin emulate delete /etc/nginx/nginx.conf
twin emulate delete file:/etc/nginx/nginx.conf
```

Reject:

```text
relative/path.conf
file:
service:nginx.service
```

Use `NodeId::file` for canonical IDs. Do not call `std::fs::canonicalize` on user paths because it fails for missing files and may follow symlinks in ways that obscure the user's target. The existing lexical canonicalization is the right behavior for a read-only graph ID.

### 4.10 Safety statement should be action-specific

Keep the existing general safety statement, and add a delete-specific statement:

```text
No file was deleted.
No action was performed.
```

Both human and JSON output should carry this meaning. If the app model keeps a single `safety_statement`, use the delete-specific line there and render the general line as an additional fixed footer. If multiple safety statements are a small change, use a list.

## 5. Implementation Steps

### Step 1: Add observation vocabulary

Add the minimum source/kinds for config discovery in `twin-observation`.

Recommended names:

```text
ObservationSource::ConfigFileDiscovery
ObservationKind::ConfigFileSeen
ObservationKind::ServiceConfiguredByFile
```

Verify:

```text
cargo test -p twin-observation vocab
```

### Step 2: Discover service config files during scan

Add pure discovery helpers that produce service/file pairs with a source label and evidence reference. Keep discovery deterministic and fixture-friendly.

Implementation choices:

1. Reuse systemd unit scan results to emit unit-file and drop-in configured-by relationships.
2. Add tiny known-service mappings for `nginx.service` and PostgreSQL only when the matching service exists.
3. Avoid broad recursive scanning. If PostgreSQL config cannot be modeled cleanly as files without directory nodes, defer recursive discovery and document that the initial demo focuses on nginx.

Verify:

```text
twin-collectors tests with fixture unit/drop-in paths
twin-app scan test that persists file nodes and configured_by edges
```

### Step 3: Persist file nodes and configured-by edges

In `twin-app`, upsert file nodes and `service configured_by file` edges in the same scan transaction as related service graph updates.

Rules:

1. Preserve `first_seen` on repeated scans.
2. Use active observed edges when discovery is direct.
3. Link each edge to its supporting observation.
4. Use metadata to distinguish `systemd_unit_file`, `systemd_drop_in`, and `known_service_config_path`.

Verify:

```text
scan_in_persists_config_file_edges
scan_in_repeated_config_scan_preserves_first_seen
```

### Step 4: Update graph output for files

Extend `graph` so users can inspect both sides of the relationship:

```text
twin graph service:nginx.service
twin graph file:/etc/nginx/nginx.conf
twin graph /etc/nginx/nginx.conf
```

Expected service view section:

```text
configured by
└── file:/etc/nginx/nginx.conf
```

Expected file view section:

```text
configures
└── service:nginx.service
```

Verify:

```text
twin-app graph tests for service and file neighborhoods
twin-cli output tests for stable sections and evidence
```

### Step 5: Extend CLI parsing for delete-file emulation

Update `twin-cli/src/cli/emulate.rs` so the grammar is explicit:

```text
twin emulate restart TARGET
twin emulate delete PATH_OR_FILE_ID
```

Parse delete targets as raw strings in the CLI and canonicalize in `twin-app`. This avoids making CLI parsing responsible for filesystem semantics.

Verify:

```text
CLI parser tests or E2E tests for absolute path, file ID, relative path rejection, and non-file target rejection
```

### Step 6: Add delete-file action and overlay

In `twin-emulate`:

1. Add `EmulationAction::DeleteFile { target: NodeId }`.
2. Add `OverlayNodeState::HypotheticallyDeleted`.
3. Add `DeleteFileInput`.
4. Add `emulate_delete_file`.
5. Add action-specific scoring and evidence helpers.

The overlay should include only the target file node override. Do not mark services unavailable in the overlay; the impact buckets explain runtime/restart consequences without pretending the running service has already failed.

Verify:

```text
twin-emulate tests for overlay state, impact buckets, scoring, evidence strength, and empty-configured-service cases
```

### Step 7: Load delete-file input in the app layer

In `twin-app/src/commands/emulate.rs` or a small submodule if it grows:

1. Resolve the delete target to a file `NodeId`.
2. Load the file node if present.
3. Query incoming active `configured_by` edges.
4. Load configured service nodes.
5. Load edge evidence lines with existing evidence helpers.
6. Add unknowns when the file is not in the graph, no configured services are known, or scan quality weakens confidence.
7. Pass a plain `DeleteFileInput` to `twin-emulate`.

Do not scan or read arbitrary file contents during emulation. Emulation analyzes the stored graph.

Verify:

```text
emulate_delete_file_reports_runtime_and_restart_impacts
emulate_delete_file_unknown_when_file_not_in_graph
emulate_delete_file_rejects_service_target
```

### Step 8: Render delete-file output

Update `twin-cli/src/output/emulate.rs` without introducing a new output crate.

Human output should include:

```text
Emulation: delete file:/etc/nginx/nginx.conf

Risk: CRITICAL
Evidence strength: MODERATE

Runtime impact:
├── service:nginx.service
│   └── Low immediate impact: running service is not assumed to reread this file immediately

Restart impact:
├── service:nginx.service
│   └── May fail to reload or restart without /etc/nginx/nginx.conf

Persistent impact:
└── Missing file remains a risk until restored

Evidence:
└── known nginx config path exists at /etc/nginx/nginx.conf

No file was deleted.
No action was performed.
```

Keep text concise. Do not add usage instructions inside the app output.

Verify:

```text
twin-cli output tests for non-empty and empty sections
JSON output test for stable snake_case fields
```

### Step 9: Prove immutability and read-only behavior

Tests must prove:

1. The target fixture file still exists after delete emulation.
2. Store node/edge rows are unchanged after delete emulation.
3. Static safety scan rejects mutation strings in emulate paths.
4. No code path shells out to `rm`, `unlink`, `systemctl`, `docker`, or `kubectl`.

Keep safety tests scoped to relevant source directories so they catch regressions without becoming noisy.

Verify:

```text
cargo test -p twin-emulate
cargo test -p twin-app emulate_delete
cargo test -p twin-cli emulate_delete
```

### Step 10: Update docs state during implementation

When implementing the slice, update `docs/state/slice-10.md` with:

1. status;
2. implemented items;
3. decisions;
4. deviations;
5. completed acceptance checklist;
6. verification commands and outcomes.

## 6. Data Model Details

### 6.1 Node and edge shape

Use existing graph primitives:

```text
file:/etc/nginx/nginx.conf
service:nginx.service configured_by file:/etc/nginx/nginx.conf
```

No new edge kind is needed.

### 6.2 Observation metadata

Keep metadata compact and source-specific:

| Key | Example |
|-----|---------|
| `source` | `systemd_unit_file`, `systemd_drop_in`, `known_service_config_path` |
| `path` | `/etc/nginx/nginx.conf` |
| `service` | `nginx.service` |
| `discovery` | `known_config_mapping` |

Do not store file contents, environment values, secrets, tokens, or large snippets. Paths and labels are acceptable.

### 6.3 Evidence wording

Evidence lines should distinguish facts from conventions:

| Source | Wording |
|--------|---------|
| Unit file | `systemd unit file /usr/lib/systemd/system/nginx.service configures nginx.service` |
| Drop-in | `systemd drop-in /etc/systemd/system/nginx.service.d/override.conf configures nginx.service` |
| Known path | `known nginx config path /etc/nginx/nginx.conf was discovered for nginx.service` |

Do not say a file is required unless that is directly proven.

## 7. Test Plan

The tests should be high-signal and fixture-driven. Avoid broad matrix tests that only repeat the same behavior.

### 7.1 `twin-collectors`

Add pure tests for config discovery helpers:

1. `discovers_unit_and_drop_in_files_for_service`
2. `discovers_nginx_known_config_when_service_exists`
3. `does_not_discover_known_config_for_unrelated_service`

These tests should use fixture paths under temp dirs or existing fixture directories. They should not read host `/etc`.

### 7.2 `twin-app`

Add integration tests:

1. `scan_in_persists_service_config_file_edges`
   - fixture has `nginx.service` and `/etc/nginx/nginx.conf`;
   - scan creates file node and `configured_by` edge;
   - edge has evidence.
2. `graph_in_service_shows_configured_files`
   - service graph contains config file section.
3. `graph_in_file_shows_configured_services`
   - file graph lists `nginx.service` and evidence.
4. `emulate_delete_file_reports_runtime_restart_and_persistent_impacts`
   - delete emulation for nginx config returns separate impact buckets.
5. `emulate_delete_file_does_not_mutate_file_or_graph`
   - target fixture file still exists;
   - relevant node and edge rows are unchanged after emulation.
6. `emulate_delete_file_unknown_when_target_not_in_graph`
   - clear unknown, weak evidence, no panic.

### 7.3 `twin-emulate`

Add domain tests:

1. `delete_file_overlay_marks_only_file_hypothetically_deleted`
2. `delete_file_classifies_configured_service_as_low_runtime_critical_restart`
3. `delete_file_empty_configured_services_returns_unknown_impact`
4. `delete_file_scoring_keeps_risk_and_evidence_separate`

These should instantiate plain input structs. No store, no filesystem.

### 7.4 `twin-cli`

Add output and E2E tests:

1. Human output contains `Runtime impact`, `Restart impact`, `Persistent impact`, `No file was deleted.`
2. JSON output includes `action: "delete"`, `action_performed: false`, and separate impact arrays.
3. CLI accepts `/etc/nginx/nginx.conf` and `file:/etc/nginx/nginx.conf`.
4. CLI rejects relative paths and service targets for delete.

### 7.5 Safety

Extend the existing safety scan to cover delete-file emulation paths. Forbid mutation strings such as:

```text
remove_file
remove_dir
unlink
rename
set_permissions
rm 
systemctl
docker stop
kubectl delete
```

Keep this as a source scan plus behavioral immutability tests. The behavioral tests matter more.

## 8. Acceptance Criteria

- [ ] `twin scan` creates file nodes for supported service config sources.
- [ ] `twin scan` creates active `service configured_by file` edges with linked evidence.
- [ ] `twin graph service:nginx.service` shows configured files.
- [ ] `twin graph file:/etc/nginx/nginx.conf` shows services configured by that file.
- [ ] `twin emulate delete /etc/nginx/nginx.conf` works after scan.
- [ ] `twin emulate delete file:/etc/nginx/nginx.conf` works after scan.
- [ ] Relative paths are rejected with a clear error.
- [ ] Non-file delete targets are rejected with a clear error.
- [ ] Delete emulation never deletes, renames, writes, chmods, or shells out.
- [ ] Overlay is not persisted as real graph state.
- [ ] Report includes runtime and restart impact as separate fields and sections.
- [ ] Report includes persistent impact when a configured file is hypothetically deleted.
- [ ] Unknown impact appears when the graph cannot connect the file to services.
- [ ] Risk and evidence strength are separate fields.
- [ ] Evidence cites config discovery source.
- [ ] Human output says `No file was deleted.` and `No action was performed.`
- [ ] JSON output includes `action_performed: false`.
- [ ] Tests use fixtures/temp dirs and do not read or mutate host config.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace -- -D warnings` passes.

## 9. Quality Bar

The best slice-10 implementation is small and convincing:

1. The graph gains exactly one new relationship family that already exists in the domain language: `configured_by`.
2. The delete-file emulator explains operational semantics better than direct dependency traversal without claiming certainty it does not have.
3. Evidence is visible everywhere the user would doubt the result.
4. The overlay remains a patch, not a cloned graph.
5. Tests cover realistic nginx/systemd cases and the safety promise, not every permutation of path spelling.
6. No speculative crates, parser engines, rule DSLs, or config schemas are introduced.

