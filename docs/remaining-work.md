# Remaining Work

Snapshot: 2026-10-09. Current producer/adapter implementation status is listed in the capability rows below; the earlier 633-test baseline predates these changes. This checklist now distinguishes implemented follow-up work from remaining or partial capabilities. The original audit references below explain the outstanding scope.
See "Implemented follow-up work" for changes made after the original review. Verification results are recorded separately; live-host coverage is not inferred from fixture tests.

## Earlier verified workflow baseline — 2026-10-09

Implemented the depth-audit trust fixes and six operator workflow slices; see [audit status](high-value-audit.md#implementation-follow-up--2026-10-09) and [usage](operational-workflows.md). Build, full workspace tests, strict workspace/all-target Clippy with `-D warnings`, and formatting passed on the final source tree. Logs: `/tmp/twin-depth-final-build.log`, `/tmp/twin-depth-final-tests.log`, `/tmp/twin-depth-final-clippy.log`, `/tmp/twin-depth-final-fmt.log`.

The test runner reports 633 passes across 127 suites, zero failures/ignored tests. Five fake-server cases explicitly returned after runtime PermissionDenied on TCP/Unix socket binding (one Docker, four Kubernetes); 628 test bodies ran fully. Live Docker/Kubernetes server fixtures need a runtime allowing local socket binding. Namespace/process race fixtures and pure adapters do not require those capabilities. The documented health YAML was also parsed/evaluated by the built CLI against an isolated empty store; it correctly failed with explicit Unknown coverage rather than claiming healthy state.

Run `twin init` to migrate an existing database to schema 6. Logical legacy argument cleanup and snapshot proof retention are tested; physical erasure and prior backup cleanup are not claimed. The platform/producer gaps below remain open and are not superseded by these workflow fixes. Earlier verification notes refer to their earlier source revisions.

## New depth audit — 2026-10-08

The three-agent [operational usefulness audit](high-value-audit.md) identified ten trust defects and six operator workflows. The 2026-10-09 follow-up implements those fixes and useful workflow slices with regression tests. See its implementation table and [operational workflows](operational-workflows.md) for usage and limits. Deferred source/platform breadth below remains open.

## Resume here — handoff, 2026-10-08

The work is **not all complete**. The remaining capability rows below are the current checklist; earlier slice decisions are historical and their latest follow-up sections supersede them. Do not redo completed DNS allowlists, UDP/fdinfo, named nginx upstreams, domain history summaries, typed edge reads, or RPM SQLite/WAL support.

### Next work, in priority order

1. **Verification complete for the continuation changes.** Build, full workspace tests, strict all-targets Clippy and formatting all passed. Logs: `/tmp/twin-continuation-build.log`, `/tmp/twin-continuation-tests.log`, `/tmp/twin-continuation-clippy.log`, `/tmp/twin-continuation-fmt.log`. Re-run after further source changes.
2. **Ship real eBPF producers and their build/artifact pipeline** (3.2, 3.10, 5.3). `attach_from_bytes` loaders and fixture decoders exist; default attach still returns unavailable. Add actual exec/TCP probes, versioned wire-layout agreement, correct attribution and timestamp conversion, drop accounting, capability-gated attachment tests, and a build that keeps ordinary workspace tests independent of root/BPF toolchains. Never attribute softirq TCP state transitions to the current task as though it initiated the connection. Extend safety checks to the actual probe sources. File/DNS tracing and a second backend remain separate missing capabilities; do not add an unused backend trait.
3. **Kubernetes watch and pod logs** (3.1). Live get/list already works. Implement reconnect/relist/resource-version/expired-watch handling and preserve source coverage on interruptions. Define bounded log behavior and secret-safe handling before exposing log bodies; logs can contain credentials. No log reader or resource watch CLI exists yet. Fake API tests must prove GET-only requests, sanitized errors and safe projection.
4. **containerd adapter** (3.9). Implement read-only namespace/container/task/image metadata reads and integrate them with observation vocabulary, graph identity, source-scoped finalization, coverage and fixture tests. Docker IDs and containerd namespace-scoped identities must not collide. Do not execute `ctr`/`crictl` or any shell.
5. **Legacy RPM database formats** (3.4). SQLite, stable checksum-validated WAL snapshots, NDB v0 and standalone BDB hash v8/v9 reads are implemented with package/app fixtures. BDB environments with cache/recovery files and unsupported encrypted/checksummed/partitioned/duplicate/subdatabase variants remain explicit coverage gaps. WAL files with stale trailing frames from reuse are currently conservatively rejected; supporting those safely would improve coverage. Deterministic metadata replacement/change tests and both WAL checksum byte orders are covered; broader concurrent-writer schedules remain unproven. Nonempty rollback journals remain an explicit coverage gap.
6. **systemd runtime depth** (3.5). Runtime FIFO/vsock ownership and remaining supported systemd specifiers are missing. Bounded D-Bus Socket.Listen manager configuration context is observed with explicit per-unit property gaps; it does not establish live listener ownership. Declarations, concrete template instances, precedence/masks, supported specifiers, socket drop-ins, Accept=yes templates and D-Bus trigger relations are implemented. Broader systemd INI syntax (continuations/quoting) and type/prefix-wide drop-in inheritance need an explicit completeness audit before claiming full systemd semantics.
7. **nginx resolution depth** (3.6). DNS hostnames remain unresolved in scan/config inference; outbound test allowlist DNS is a separate implemented feature. Includes inside upstream blocks do not inherit the block context. HTTP/stream group namespaces are not distinguished. Explicit ports on named group references now warn and suppress resolution. Unix upstreams retain file references but do not infer runtime Unix-listener proxy ownership. Missing/malformed/ambiguous nginx configuration now makes unseen nginx relationships and their otherwise-unseen endpoints stale. Uncertainty is conservative across the nginx source; finer file/service granularity remains possible.
8. **Exec lifecycle reasoning** (3.10). Persisted exec/UID/cgroup observations exist; dependency confidence/lifecycle effects and validation against a real producer remain. Malformed/zero InvocationID regression coverage is now implemented and passing.
9. **Watch performance validation** (2.9). The two-hour continuation-binary soak completed: [artifact](../bench-results/watch-soak-2h.json), 7200.015 seconds, exit 0, 1531 scans, 256 retained ticks, peak RSS 71484 KiB. First/last steady-quarter median RSS increased from 54624 to 71152 KiB. This is an observed workload measurement, not proof of no leaks or a soak of the newer depth-audit binary. Investigate growth/plateau before stronger performance claims. Adapter timings remain grouped.
10. **Remaining architecture/performance work** (5.1–5.2). Generic/export and specialized graph neighborhoods now batch peer/evidence queries. A general transaction node cache remains deferred. Benchmark before adding caching, preserve read-your-writes, and extract graph/rules/output crates only when concrete coupling or duplication justifies it. Keep the existing minimal-code rule.
11. **Docker protocol depth** (3.13). The three read paths remain a minimal hand-written Unix HTTP client; no API version negotiation. This is a documented limitation, not a reason to add an SDK without a concrete benefit. Docker list has no general pagination contract; do not invent unsupported pagination.

### Current implementation and verification notes

- Latest follow-ups: app-layer DNS allowlists via `twin-collectors::dns::DnsReader`; nginx cross-file named upstreams; socket-only systemd drop-in overrides/resets; batched graph/export peers/evidence; valid InvocationID guard; private RPM WAL snapshot reconstruction and rollback-journal rejection.
- **Passed:** `cargo build` (`/tmp/twin-final-build.log`), strict `cargo clippy --workspace --all-targets -- -D warnings` (`/tmp/twin-final-clippy.log`), and `cargo fmt --check` (`/tmp/twin-final-fmt.log`). These are the pre-continuation checks; the newer full continuation checks above also pass.
- Focused tests passed: DNS app checks (6), DNS collector checks (2), systemd unit collection (10), store typed/batched reads (6), app scan graph (45), temporal summaries (12), nginx app checks (5), and RPM adapter tests before the final rollback-journal test was added (5). The completed full workspace rerun covered the additional rollback-journal RPM test.
- Re-run after further changes: `cargo build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`.
- This checkout was already substantially dirty before this work; preserve unrelated modifications. No commit was made. Untracked source crates/files and `tools/watch_soak.py` are part of the working tree and must be included when eventually committing.
- No sudo or host package installation was used. Local build facts: clang and bpftool are available; bpf-linker and Rust source are absent, and only the host Rust target is installed. Do not mistake build-tool availability for live probe validation.

Sources for this list:

1. Full audit of every `docs/state/slice-*.md` + `docs/plan-slices.md` + `docs/code-layout.md` + `docs/tech-document.md` + `docs/prd.md` for deferred/limited/missing capabilities (explore agent).
2. Static + live audit of the `twin` CLI surface (commands, flags, renderers, request fields) (explore agent).
3. Data-source lifecycle audit across host / docker / k8s / eBPF writers (explore agent).
4. Findings from hands-on CLI use during the review session.

Priority legend: **P1** = correctness or user-visible defect, **P2** = real capability gap, **P3** = depth/polish.
Size: S (hours), M (days), L (multi-day/feature-sized).

---

## 1. Correctness / lifecycle (P1)

| # | Item | Refs | Why it matters | Size |
|---|------|------|----------------|------|

---

## 2. CLI / UX gaps (P2)

| # | Item | Refs | Why it matters | Size |
|---|------|------|----------------|------|
| 2.9 | Partial: collector timings, bounded 260-tick fixture stress and two-hour host soak are measured. Fifteen-minute RSS analysis identifies a 75–90-minute step; cause and current-binary plateau remain unverified | `docs/state/slice-13.md`, `tools/analyze_watch_soak.py` | New harness runs record numeric store cardinalities and byte sizes; the completed artifact predates them. Retained evidence/workload growth and allocator growth must be separated before claiming a leak or steady state. | M |

---

## 3. Data sources / collectors (P2–P3)

| # | Item | Refs | Why it matters | Size |
|---|------|------|----------------|------|
| 3.1 | Implemented: bounded Kubernetes metadata watches with resource-version continuation, bookmarks, 410 relist/backoff; bounded pod-log summaries and typed phrase counts without log bodies | `docs/state/slice-23.md`, `crates/twin-k8s/src/watch.rs`, `crates/twin-k8s/src/logs.rs` | Fake API tests validate GET-only behavior; current runtime socket gates and live cluster validation remain explicit limits. | L |
| 3.2 | Implemented bundled exec/connect BPF objects, reproducible clang pipeline, Aya tracepoint loader and bounded ring reads. Live kernel verifier/attachment remains unverified here | `docs/state/slice-14.md`, `docs/state/slice-15.md`, `tools/build_ebpf.py` | Object/ABI tests run without privileges; this runtime lacks effective capabilities and readable tracepoint formats. Live accept/bind producers remain absent. | L |
| 3.4 | Partial: SQLite/private WAL, validated NDB v0 and standalone endian-aware BDB hash v8/v9 ownership/version reads are implemented | `docs/state/slice-21.md` | NDB validates slots/blob checksums; BDB follows active buckets/overflow chains. Active BDB cache/recovery environments, unsupported BDB features and reused WAL stale tails remain coverage gaps. | M–L |
| 3.5 | Partial: instances/templates, quoted/continued syntax, C escapes, name/type/prefix drop-in precedence and pure unit/instance/path specifiers are implemented. Runtime FIFO/vsock ownership and host/user-context specifiers remain | `docs/state/slice-08.7.md` | Unsupported identities stay unknown; configured FIFO/vsock declarations do not prove runtime ownership. | M |
| 3.6 | Implemented bounded contextual includes, scoped HTTP/stream upstreams, TCP/UDP/Unix backends and concrete-hostname system DNS evidence | `docs/state/slice-19.md` | Current scanner DNS mappings are inferred; nginx resolver/cache/namespace state and variable-expanded targets remain unknown. | S |
| 3.9 | Implemented metadata-only containerd namespace/container/task/image List adapter, fixtures, namespace-qualified graph IDs, incarnation-safe process joins, source-scoped coverage and doctor output | `docs/state/slice-22.md` | Live daemon interoperability remains unverified; OCI port/volume/env/specification contents are excluded. | M |
| 3.10 | Implemented exec lifecycle invalidation and bundled producer identity extensions, UID/cgroup attribution, per-event/per-cgroup bounds and separate kernel drop counters | `docs/state/slice-14.md`, `docs/state/slice-15.md` | Fixture regressions cover incarnation and stale dependency proof; live producer behavior still needs a capable kernel. | M |
| 3.13 | Implemented Docker API version negotiation against server min/max before the three bounded GET-only resource paths | `docs/state/slice-22.md` | Compatible API range is 1.24–1.51. No SDK or speculative mutation-capable interface is added. | S |

---

## 4. Reasoning / feature depth (P2–P3)

| # | Item | Refs | Why it matters | Size |
|---|------|------|----------------|------|

---

## 5. Architecture / planned crates (P3)

| # | Item | Refs | Why it matters | Size |
|---|------|------|----------------|------|
| 5.1 | Partial/architecture decision: twin-safety now scans all production crates. twin-package and twin-fixtures are implemented for RPM reads and shared fixtures. Graph/rules/output remain in existing owning modules; extract only with concrete coupling/duplication justification | `docs/tech-document.md`, `docs/code-layout.md`, `docs/state/slice-01/04/09.md` | Remaining graph/rules/output logic lives in `twin-app`/`twin-cli`. Cross-workspace mutation scanning is now implemented in twin-safety. The RPM fixture is shared in twin-fixtures; other fixtures still live in per-crate tests/support. | M |
| 5.2 | Store/perf deferrals: an existing positive edge cache is used during scans; no general transaction node cache. Generic/export and specialized neighborhoods now batch peers and evidence. Typed edge lookup and shared proc fixture writer are now implemented; broader transaction caching remains deferred pending measured need | `docs/state/slice-05.md` | Fine at current scale; relevant on hosts with 10k+ processes. | M |
| 5.3 | eBPF file/DNS tracing and second backend remain absent. Independent rate_limit.rs exists; an unused second-backend trait would violate the minimal-code rule | `docs/state/slice-14.md`, `docs/state/slice-15.md` | Same area as 3.2/3.10. | M |

---

## 6. Explicitly out of scope (by design — do not "fix")

- Emulation never performs an action, never persists overlays; all safety statements stay.
- K8s secret `data`/`stringData`, configmap bodies, and event messages are never stored.
- No shell execution anywhere; `twin.yaml` has no shell keys.
- Read-only adapters expose only get/list (no Docker restart/stop/kubectl apply paths).
- eBPF programs use tracepoints only (no LSM, no packet rewrite, no signal).

---

## Completed this session (for context)

- Host-scan finalization scoped by source (`FinalizeScope`, `skip_source`), fixing K8s rows, Docker-outage reaping, shared-image edge reaping, package-DB outages, and eBPF evidence reaping; node reactivation on re-observation.
- `twin k8s scan` now finalizes vanished objects (gone/stale), records history, and writes a collector-run row.
- Retention implemented (`Store::prune_retention`) and wired into every scan; `twin doctor` validates config content.
- New `twin test list`; `graph --kind file|unix_socket|cgroup` listing; mount/directory rendering; target+kind conflict error; supported-kind errors.
- `emulate --evidence` for fill-disk/container/rollout/pod-delete; container `--paths`/`--max-depth` transitives.
- pacman package support; endpoint-fact dedup; error-message dedup; scan flag validation; watch `--json` NDJSON streaming; k8s scan source row.
- Zero warnings: dead-code cleanup with documented shared-test-support exception (AGENTS.md), clippy lints, fmt; two never-running tests restored (`#[test]`).
- Docs updated: `AGENTS.md`, `docs/state/slice-12/17/21/23.md`.

## Implemented follow-up work

| Audit items | Implementation and evidence |
|---|---|
| 1.1, 1.4 | Entity predecessor queries, field-level history summaries, eBPF node/edge history; store history and app temporal/ebpf_tcp tests. |
| 1.2, 1.3, 1.7 | Config validation on read paths and before refresh; settings-only XDG semantics; config_commands and invalid-request CLI regression tests. |
| 1.5 | Independent retention cutoffs, boundary behavior, edge-link cleanup, snapshot/current graph preservation, configured scan pruning; store retention and app config_commands tests. |
| 1.6 | Kubernetes labels print kind once; k8s_output tests. |
| 2.1, 2.2 | Explicit fixture flags and completion generation; CLI integration tests. |
| 2.3–2.6 | DOT/Mermaid export, retained-history graph --at, image/cgroup neighborhoods, state/label-substring/limit filters; output, history, temporal, K8s, and CLI tests. Label matching is a substring, not regex. |
| 2.7 | Existing generic assert.node already supports canonical Kubernetes IDs; app K8s test now proves deployment/service assertions against persisted fixture resources. |
| 3.7, 3.8, 3.12 | PostgreSQL config discovery, common service directories, .so/.so.* mapped library filtering; mounts/maps/libraries tests. |
| 4.1, 4.3, 4.5, 4.6 | Stored-evidence why command, upstream port chains, sourced persisted warning details, topology-derived storage criticality; why/impact_paths/CLI renderer tests. |
| 4.8, 4.9 | Availability follows collector read status; failed K8s reads preserve rows and record gaps; all process warning details persist. |
| 5.4, 5.5 | Slice records updated for actual polling watch, lifecycle/history semantics, fixture layout, endpoint dedup, and follow-up limits. |

No root or host mutation is used for these checks. Read-only live Kubernetes support uses kube 4.2, k8s-openapi 0.28, Tokio, Rustls with ring; external credential-command providers are rejected. A fake HTTP API tests GET-only calls, pagination, metadata-only reads, denied access, sanitized errors, and timeouts. Bundled probes are now implemented; successful live kernel attachment remains unverified in this runtime.


Further completed work: default container transitive overlays (4.2), competing-ownership confidence adjustment (4.7), RPM SQLite + shared fixtures (part of 3.4/5.1), nginx include discovery + secret-safe proxy origins (part of 3.6), and unambiguous TCP shorthand (part of 3.11). See the latest slice follow-up sections for exact limits and tests.


Socket follow-up completed (3.3, 3.11): the shared internet-socket parser now reads TCP/TCP6 and UDP/UDP6. UDP bound sockets and connected peers have distinct canonical UDP IDs, source/kind vocabulary, ownership observations, graph/impact evidence, protocol-separated listener matching, counters, and coverage gaps. Unconnected UDP sockets do not imply remote traffic. Missing/malformed UDP tables preserve stored UDP nodes and dependencies. fdinfo inode validation rejects descriptor reuse, malformed inode fields, and vanished descriptors; inaccessible fdinfo falls back to the observed FD symlink with an explicit unverified flag. Collector socket and app scan_graph regressions cover these cases. Packet-level DNS/QUIC activity still requires tracing.

Domain-level what-changed summaries (4.4) are implemented alongside generic node/edge lifecycle sections. They cover retained process transitions, likely service restarts/reactivations, config fingerprints, listening port availability, connected endpoints, containers/image references, package versions, and Kubernetes objects. Each entry names its history row, observation, observed/inferred class, timestamp and core evidence score. Restart/reactivation is inferred only from a changed nonempty systemd invocation identity; cause is unknown. Missing/pruned predecessors never manufacture version/config/restart comparisons. Multiple changes inside the window remain visible, including upgrades followed by rollbacks. Listening-port changes replay owner history so losing one of multiple owners does not imply the port disappeared. Outbound direction from proc peer observations is inferred. Tests cover all categories, rollback, secret-safe summaries, and terminal rendering.


Outbound hostname allowlists (2.8) now resolve through the system resolver in the app layer; twin-test remains a pure graph evaluator. Queries are limited to 32 unique hostnames and a five-second command-wide resolver deadline. IPv4/IPv6 answers expand to exact port endpoints, including UDP graph peers. Reports retain safe current DNS observations with timestamps and explicitly label historical hostname matching as inferred/unknown. Resolution failures warn or fail according to fail_on_unknown even when no endpoints are stored. The OS resolver task cannot be cancelled; a timed-out task may finish in the background. Fixture tests cover address families, deduplication, malformed/secret-bearing inputs and all error statuses without network access.


Continuation verification: `/tmp/twin-nginx-lifecycle.log` covers nginx app checks (7) and temporal checks (13); `/tmp/twin-stale-store.log` covers store history checks (8); `/tmp/twin-nginx-parser.log` covers nginx parser checks (9); `/tmp/twin-lifecycle-clippy.log` records strict all-targets Clippy. The latest whole-workspace verification completed successfully in `/tmp/twin-continuation-*.log`.

Operator workflow follow-up: target-scoped chronological `what-changed` is implemented with typed repeatable category filters, bounded neighborhood depth and newest-entry limits. Historical ownership links in the requested window retain attribution after disappearance. Human output includes RFC3339 UTC timestamps and omission counts; filters never hide coverage unknowns. The command correlates retained changes with a workload, without claiming root cause.

Operator workflow follow-up: `test run --refresh --max-age 5m --baseline before` and `--snapshot before` expose explicit graph evaluation context and baseline regressions. `why` now includes bounded supporting chains across config, endpoints, listeners and ownership; safe allowlisted facts and human timestamps accompany evidence IDs. Chains retain per-link direction/class/score and do not assert causation.
