# Operational usefulness audit — 2026-10-08

Three agents reviewed collection/lifecycle, reasoning/emulation, and operator workflows. The primary agent cross-checked the highest-impact paths and reran two isolated reasoning reproductions. This report describes the current dirty working tree, not a released version. The findings below describe the original audit; the follow-up implementation status is recorded next.

**Priority describes potential harm; confidence describes evidence for the finding.** Reproduced means an isolated executable exercised real library APIs. Source-inspected findings have high confidence in the identified implementation, but their full operational scenarios have not all been reproduced. The original proposals are retained below for traceability.

## Implementation follow-up — 2026-10-09

All ten trust findings have implementation changes and regression coverage in this working tree. Operational/platform limits are documented in [operational workflows](operational-workflows.md) and [remaining work](remaining-work.md); this is not a claim that every original platform capability is complete.

| Finding | Implemented behavior | Regression evidence |
|---|---|---|
| 1 | Argument values omitted at ingestion; migration 6 scrubs legacy structured fields and Docker errors | observation redaction, store migration tests |
| 2 | Required evidence counts retained observations rather than explanatory text | pure trust and app policy-context tests |
| 3 | Relevant missing/incomplete coverage prevents negative assertions and outbound green results | trust, health-contract and test-run tests |
| 4 | Complete leaves at the depth boundary retained; counts union distinct affected services | impact-path tests |
| 5 | Partial/malformed reads retain stale uncertainty rather than confirmed removal | scan-graph, container and K8s trust tests |
| 6 | Boot/start, immutable container IDs, K8s UIDs and cluster identity guard incarnations | scan-graph, namespace, container and K8s tests |
| 7 | Conditional ready-endpoint and rollout-budget reasoning replaces constant rollout risk | emulate K8s and app rollout tests |
| 8 | Typed shared evidence assessment, wall-clock recency, source coverage, age limits and snapshot context | evidence scoring, why, policy-context and health-contract tests |
| 9 | Wants remains configured context; Requires and observed runtime relationships remain distinct | restart and impact tests |
| 10 | Protocol-sensitive endpoints, workload selectors and direction uncertainty retained | trust and outbound tests |

The six operator workflows now have usable slices: active/listener/container/K8s health contracts; bounded target timelines; namespace-scoped topology; snapshot policy baselines with captured proof references; safe bounded proof chains; and conditional availability rehearsal. Historical disk/DNS/emulation inputs are not reconstructed from graph snapshots and explicitly remain Unknown. Legacy snapshot backfill cannot recreate proof links already lost before migration. Same-boot namespace inode reuse without an observed absence remains unprovable. Migration cleanup is logical, not forensic erasure.

The original source line references and reproductions below are historical; use the regression tests and current module layout to assess the implementation.

## Fix trust defects first

### 1. Credential arguments survive redaction and reach SQLite — P1, reproduced

Observed: process collection copies arguments into `argv_json` (`crates/twin-collectors/src/process/collector.rs:588`). The redactor recognizes selected flags and URI userinfo (`crates/twin-observation/src/redact.rs:91–137`), leaving other secret-bearing arguments intact.

A synthetic test through `Pipeline<BasicRedactor>` and `Store::insert_observation_typed`, followed by SQLite readback, retained `SENTINEL` for all three inputs:

- `curl -H 'Authorization: Bearer SENTINEL'`
- `redis-cli -a SENTINEL`
- `mysql -pSENTINEL`

The control `tool --password SENTINEL` was redacted. Artifacts: `/tmp/twin-argv-audit-repro/`, including source, `evidence.db`, and `result.log`. These contain synthetic values only. Collection persistence is wired at `crates/twin-app/src/commands/scan.rs:369`.

Consequence: normal read-only scanning can retain credentials, violating the product's secrets guarantee. Prefer retaining executable identity, argument count, and explicitly safe structural facts rather than arbitrary argument values. A flag blacklist cannot cover arbitrary positional secrets. Define how existing retained records will be handled; fixing future ingestion does not remove earlier values.

Acceptance: sentinel-bearing headers, short flags, concatenated flags, positional arguments, URI forms, and external errors leave no sentinel in persisted observations, graph attributes, reports, or exports.

Related source-inspected risk: Docker non-success response bodies are copied into errors (`crates/twin-container/src/live.rs:216–221`) that can be stored (`crates/twin-app/src/commands/scan.rs:1107`). Store operation/status/error category instead of arbitrary server text. This external-error path was not reproduced.

### 2. Evidence-required tests pass with zero evidence — P1, reproduced

Observed: `crates/twin-app/src/commands/test_cmd.rs:256` appends explanatory `evidence_reasons`; `crates/twin-test/src/evaluate.rs:271` tests whether rendered evidence text exists. An isolated store containing service nodes but zero edges and observations returned:

```text
Pass — Risk: LOW — Evidence: WEAK; no supporting observations
```

Reproduction: `/tmp/twin-reasoning-audit-c9a926xu/evidence.rs`, exercised through `twin_app::run_file`.

Consequence: a deployment gate can claim its evidence requirement was satisfied by a sentence saying evidence is absent. Carry supporting observation IDs/count and assessed strength separately from explanation text. Acceptance: `require_evidence: true` cannot pass with zero supporting observations, including successful emulation and error paths.

### 3. Missing coverage can produce successful absence/outbound checks — P1, source-inspected

Observed: `crates/twin-test/src/evaluate.rs:306` passes outbound checks when there are no unexpected endpoints without checking `GraphFacts.unknowns`; negative node/dependency checks also use absence (`:176`, `:194`).

Consequence: unavailable socket collection can look like a compliant machine with no prohibited traffic. Make source completeness relevant to each assertion. Missing relevant evidence should produce Unknown, with strict policy converting Unknown to failure. Avoid requiring users to remember a separate coverage assertion.

Acceptance: empty initialized store, missing TCP/UDP tables, denied FD directories, and pruned observations cannot establish absence. Complete collection of genuinely empty state must remain distinguishable and usable.

### 4. Depth-limited impact drops complete paths — P1, reproduced

Observed: `crates/twin-app/src/commands/impact_paths.rs:142–146` handles the depth boundary before leaf emission. For `front → api → db`, querying db returned:

| Maximum depth | Result |
|---|---|
| 1 | api path, capped |
| 2 | **no paths** |
| 3 | front → api → db |

Reproduction: `/tmp/twin-reasoning-audit-c9a926xu/main.rs`. Consequence: blast radius and dependent restart overlays can omit affected services at an ordinary depth setting. Always emit accumulated nonempty paths at the boundary; mark capped only when unexplored incoming edges remain.

Related counting defect: `impact_paths.rs:333–339` counts only terminal services, omitting intermediate transitive dependents; `impact_scoring.rs:36` uses that count in risk thresholds. Acceptance: chain, branch, converging branch, cycle, and exact-depth fixtures preserve affected service membership and count each service once.

### 5. Incomplete reads can become false disappearance — P1, source-inspected

Observed: host finalization has explicit incomplete UDP handling (`crates/twin-app/src/commands/scan.rs:365–366`), while stale selection in `scan_history.rs:53–63` relies on process permission warnings or an FD denial threshold. Missing TCP/Unix data and smaller ownership gaps are not equivalent completeness guards. Missing tcp6 can be silent (`crates/twin-collectors/src/process/procfs.rs:65–80`). Docker inspect failures fall back to list summaries (`crates/twin-app/src/commands/scan_containers.rs:150–160`), losing inspect-only facts while the scan can still finalize the scope.

Consequence: a collector outage can be reported as a real dependency/container relationship disappearing. Track completeness per source, address family, namespace, and object where needed. Successful membership enumeration does not imply successful detail collection.

Acceptance: complete → partial/denied/malformed → complete fixtures retain uncertain prior relationships as stale/unknown, then report Gone only after authoritative absence. Cover a single failed Docker inspect alongside successful peers.

### 6. Mutable identifiers merge distinct lifetimes — P1, source-inspected

Observed:

- Process stat parsing drops start time (`crates/twin-collectors/src/process/procfs.rs:455–470`); graph identity is PID-only (`crates/twin-app/src/commands/scan.rs:373–377`). Existing lifetime is preserved (`crates/twin-core/src/graph_node.rs:53–55`), and retained eBPF edges use node presence (`scan_history.rs:168–170`).
- Kubernetes `ClusterView` lacks cluster identity/UID (`crates/twin-k8s/src/model.rs:66–79`), IDs use namespace/name, and finalization covers the K8s scope (`crates/twin-app/src/commands/k8s/scan.rs:170`, `:185`, `:208`).
- Docker graph construction uses names and tags rather than preserving immutable container/image identity (`crates/twin-app/src/commands/scan_containers.rs:176–190`).

Consequence: PID reuse, reboot, same-name recreation, mutable image tags, or Kubernetes context switching can mix unrelated history and dependencies. Preserve human-friendly aliases while tracking boot/start ticks, object UID, cluster identity, container ID, and image digest. Re-read process identity around multi-file reads to detect reuse during collection.

Acceptance: reused PID, new boot, recreated same-name workload, moved image tag, and two clusters with identical names produce distinct incarnations without inheriting old runtime edges. Scanning one cluster must not finalize another cluster's records.

### 7. Rollout risk is constant despite modeled unavailability — P1, source-inspected

Observed: `crates/twin-emulate/src/rollout_k8s.rs:128` hard-codes Low risk and Moderate evidence while its overlay marks owned pods unavailable. Replica availability, routing, rollout strategy, and source coverage do not determine that result.

Consequence: a single-replica workload can receive reassurance unsupported by its availability. Model bounded scenarios from ready routed endpoints and collected strategy; use unknowns when evidence is missing. Do not label simultaneous removal of every pod as a faithful rolling update.

Related routing uncertainty: `crates/twin-app/src/commands/k8s/scan.rs:425–448` falls back to ready selector-matching pods when endpoint objects are absent. Selector candidates must remain inferred; missing endpoint collection cannot establish actual routing. Kubernetes persistence also needs a transaction around the whole scan (`k8s/scan.rs:158`) to prevent partial graph/history publication.

Acceptance: single replica, redundant replicas, unavailable backend, missing endpoint permission, and different rollout capacities yield justified distinctions between risk and confidence; injected storage failure rolls back the scan.

### 8. Coverage, freshness, and explanation disagree — P1/P2, source-inspected

Observed:

- `crates/twin-app/src/commands/scan_quality.rs:23–45` omits recorded missing socket-table/permission counters; classification at `:92–106` can return Good and `impact_reliable=true` with unavailable tables and zero unmapped rows.
- `commands/coverage.rs:146–149` uses the last process scan's end time as the age reference. Stored evidence can remain fresh relative to an old scan. CLI impact/emulate refresh first, limiting their exposure; library/stored workflows still need an explicit evaluation time.
- `commands/why.rs:124–135` uses fixed Observed/Inferred line scores and default adjustments, unlike impact's coverage/recency handling (`impact.rs:913`).
- Tests load stored graph state without an age bound (`test_cmd.rs:77–120`) while disk facts (`:191`) and DNS answers are current.

Consequence: users can get incompatible confidence and mixed-time answers about the same system. Share a structured evidence assessment, retain core's single scoring formula, and report evaluation time, observation window, source coverage, and scan identity. Support explicit stored evaluation and refresh, with a maximum acceptable evidence age.

Acceptance: advance an injected clock without rescanning; all relevant reports age consistently. Missing source data degrades quality and relevant conclusions. Reproducible stored evaluation must use a documented as-of time rather than implicitly pretending to be current.

### 9. Systemd declarations are overpromoted to runtime dependencies — P1/P2, source-inspected

Observed: scan retains the dependency key (`crates/twin-app/src/commands/scan_systemd.rs:79`), but `commands/evidence/systemd.rs:84–95` treats observed dependencies of active services as Runtime without distinguishing that key.

Consequence: an optional `Wants=metrics.service` can propagate an outage prediction without observed runtime use. Retain startup ordering/pull-in, lifecycle propagation, and observed communication as distinct evidence. Optional Wants alone does not establish runtime necessity; see the [official unit dependency semantics](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#Wants=).

Acceptance: optional Wants without runtime communication does not become a proven outage chain; explicit runtime evidence and relevant Requires/BindsTo/PartOf semantics remain distinguishable. Do not infer all restart behavior from one declaration type.

### 10. Outbound policy loses transport and workload identity — P1/P2, source-inspected

Observed: `crates/twin-app/src/commands/test_cmd.rs:176` strips both TCP and UDP prefixes into the same address/port key (`:130`). `crates/twin-test/src/document.rs:41` provides no outbound workload selector. Proc peer evidence is rendered as connecting (`test_cmd.rs:183`) although a peer row alone does not establish initiation direction.

Consequence: transport-specific restrictions cannot be expressed reliably, and one service's policy includes unrelated machine traffic. Keep canonical protocol-sensitive endpoint IDs and add service/container scope. Label peer observations separately from observed outbound connect events. If preserving address-only allowlists, document them explicitly as any transport.

Acceptance: TCP/443 allowance does not implicitly allow UDP/443 under a transport-specific policy; two workloads with different allowlists remain independent; direction uncertainty is visible.

## Highest-value capabilities after the trust fixes

These build on existing observations, history, overlays, and tests. They require no active probes, service actions, or shell execution.

| Order | Capability | Operator question it answers | Smallest useful slice and acceptance |
|---|---|---|---|
| 1 | Semantic health contracts | “Is the intended workload actually ready?” | Add active service state and local listener with expected owner. Missing runtime evidence is Unknown. Extend to container health and ready/routed K8s replicas. |
| 2 | Target-scoped incident timeline | “What changed around this failing service?” | Target plus dependency neighborhood, chronological timestamps, bounded results, configuration/package/restart/connectivity filters. Show correlated changes with evidence, not asserted causation. |
| 3 | Namespace-aware topology | “What connects my host, containers, and backends?” | Discover unique `/proc/PID/ns/net` identities, read representative `/proc/PID/net/*` without entering namespaces, and scope endpoint IDs. Test two namespaces with identical loopback addresses and inaccessible namespaces. |
| 4 | Policy regression against a known-good snapshot | “What operational assumption broke after this deployment?” | Evaluate identical health/outbound contracts against a named snapshot and current state; show newly failing checks and changed supporting paths. Preserve observation timestamps and coverage for both sides. |
| 5 | Complete, safe proof chains | “Why should I believe this dependency or predicted outage?” | Render config → endpoint → listener → process → service with safe typed facts, human timestamps, missing links, and shared confidence. Never dump arbitrary metadata. |
| 6 | Availability-aware change rehearsal | “Can this workload tolerate a restart or rollout?” | Begin with ready routed endpoint counts, replica/strategy constraints, and explicit uncertainty. Keep hypothetical effects in overlays and distinguish scenario assumptions from observed facts. |

Evidence for these capability gaps:

- Existing service/port assertions are generic node existence (`crates/twin-test/src/evaluate.rs:122–124`). Services can originate from declarations (`crates/twin-app/src/commands/scan_systemd.rs:47–69`); ports can be remote peers (`commands/scan.rs:613–633`). Existence is useful inventory, but does not establish active service health or a local listener.
- `what-changed` lacks a target selector (`crates/twin-cli/src/cli/args.rs:326`); human operational rows omit timestamps and bypass concise limits (`src/output/what_changed.rs:31`).
- Process network collection reads scanner-level `/proc/net/*` (`crates/twin-collectors/src/process/procfs.rs:56`), while examining other processes' FDs (`collector.rs:237`), without network namespace identity. Container-internal connectivity can therefore be absent from host topology.
- Snapshot diff is structural (`crates/twin-app/src/commands/diff.rs:39`); current tests do not select a baseline snapshot.
- Why discards most safe recorded proof detail (`crates/twin-app/src/commands/why.rs:70`) and renders kinds/IDs/raw timestamps (`crates/twin-cli/src/output/why.rs:23`).

## Recommended implementation sequence

1. Close credential retention and evidence-required false passes; add permanent regression fixtures for the three reproduced defects.
2. Establish source-specific completeness, incarnation identity, evaluation time, and shared evidence assessment. These are prerequisites for reliable negative assertions, history, and baselines.
3. Correct impact traversal/counts, systemd semantics, and rollout risk. Preserve explicit unknowns until enough facts exist.
4. Ship semantic health contracts and a target-scoped timeline as runnable vertical slices with polished human output and JSON.
5. Add namespace topology, snapshot policy comparison, and availability rehearsal incrementally. Update the owning slice state and acceptance criteria as each is implemented.

The original `remaining-work.md` still tracks deferred source/platform breadth, including a real eBPF producer and containerd. This audit raises correctness and operator-workflow work above additional breadth. At audit time the existing passing suite did not cover the reproduced cases; permanent regressions now exercise them. The original audit added documentation only; the implementation follow-up above records the subsequent fixes.
