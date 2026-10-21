# AGENTS.md — twin-ops

Agent working on **`twin`**: Rust read-only operational twin for Linux.
Source of truth: `docs/prd.md`, `docs/tech-document.md`, `docs/plan-slices.md`, `docs/code-layout.md` (implemented module/test layout). Read them before changes.
Next work: `docs/plan-agent-roadmap.md` (slices 25–30, in order). Start each slice by creating `docs/state/slice-NN.md`.

## Self-Evolution

This file living. Add crate/dependency/convention/discover better pattern → update this file + relevant `docs/state/slice-NN.md`. Stale instructions worse than no instructions.

## Rules

1. **Read-only forever.** Never mutate host. No restarts, kills, writes, firewall changes, installs, container/K8s mutations.
2. **Evidence first.** Every conclusion cites source + observation + evidence strength. No bare claims.
3. **Observed ≠ Inferred.** Separate facts from conclusions always.
4. **Risk ≠ Evidence strength.** Risk = "how bad". Evidence = "how confident". Never merge.
5. **No panics.** All error paths return `Result`. Missing permissions, vanishing processes, malformed data = normal.
6. **Unsafe isolated.** Only at OS boundaries (eBPF, syscalls, FFI), behind safe APIs. Never in business logic.
7. **Secrets never stored.** No values of secrets, env vars, tokens, private keys. Names/paths/hashes OK.
8. **Emulation = overlay.** Never clone system, start services, or perform action. `base graph + overlay = effective view`.

## Crates

`cli` `app` `core` `observation` `collectors` `store` `graph` `rules` `emulate` `test` `ebpf` `container` `k8s` `package` `config` `output` `fixtures` `safety` — all prefixed `twin-`. `twin-test` parses `twin.yaml` and evaluates graph checks. It does not run a shell. `twin-app` loads the stored graph and persists the report. Agent-facing commands: `brief`, `preflight`, `mcp`, `agent-config` (see `docs/state/slice-24.md`). `twin-config` parses config text (nginx `proxy_pass`) and fingerprints file bytes. It does not read the host; scan passes the bytes in.

**Dependency direction:** inward. `core` depends on almost nothing. `cli→app→{core,store,graph,collectors,emulate,test,output,ebpf}`. Collectors → `core+observation`. `ebpf` → `core+observation`. Reasoning → `core+graph+rules`. **Forbidden:** `core→ebpf`, `core→k8s`, `core→container`, `graph→cli`, `emulate→collectors`. `twin-container` is a read-only Docker adapter (list and inspect only). `twin-k8s` is read-only (get/list only; secret values are never stored).

## Code — Minimal Viable Code Only

### Principle

Write least code that works. No speculative abstractions. No future-proofing. No trait until needed. No config field until needed. No enum variant until needed. Dead code = bug. Scaffold = bug. `TODO` = bug — either do it or don't. Ship thin, extend later.

### Rust Style

- Edition 2021. `cargo fmt`. `cargo clippy -- -D warnings`. Zero warnings.
- No comments unless *why*. Code documents *what*. If comment says what code does, delete comment.
- `pub(crate)` default. Export only what other crates need. No `pub` without reason.
- Struct fields private. Accessor methods if needed. No `pub` fields on domain types unless truly data-only.

### Types

- Domain IDs = newtypes: `NodeId(String)`, `EdgeId(String)`, `ObservationId(Uuid)`, `SnapshotId(String)`.
- Domain kinds = enums: `NodeKind`, `EdgeKind`, `EdgeClass`, `RiskLevel`, `EvidenceStrength`.
- No raw strings in domain core. String only at CLI boundary. Convert to typed ID immediately on entry.
- `#[derive(Debug, Clone)]` on domain types that cross boundaries. Derive only what you use.
- `Display` impl for all IDs. `FromStr` impl for all IDs that come from CLI input.
- Node IDs follow canonical form: `service:nginx.service`, `process:pid:1234`, `port:tcp:127.0.0.1:80`, `file:/etc/nginx/nginx.conf`, `container:redis`, `k8s:deployment:default/api`.

### Error Handling

- `Result<T, E>` everywhere. Never `unwrap()` in non-test code. Never `expect()` in non-test code.
- Each crate owns its error enum. `thiserror::Error` derive. `twin-app` composes crate errors via `From` impls.
- Never `anyhow` in library crates. `anyhow` only in `twin-cli` at top level if needed.
- Error variants carry context: `StoreError::Open { path, source }` not `StoreError::Open(String)`.
- Missing permissions, disappearing processes, malformed proc files = expected errors. Handle gracefully. Never crash user-facing commands.
- Unreachable code paths: use `unreachable!()` with explanation, never `panic!()` with vague message.

### Module layout (implemented)

Follow `docs/code-layout.md`: command code under `twin-app/src/commands/`, results under `model/`, paths via `TwinLayout`, CLI rendering under `twin-cli/src/output/`, integration tests in `crates/*/tests/` with `tests/support/` fixtures. No global mutexes or env-var test hooks in production path code.

### Struct Layout

- One primary type per module file. Module name = type name in snake_case.
- Prefer subdirectories (`commands/init.rs`, `store/open.rs`) over monolithic files once a module has multiple concerns.
- Structs impl their own construction. Constructor `fn new()` validates invariants.
- Builder pattern only if construction has 4+ required + 2+ optional fields. Otherwise plain `fn new()`.
- No `Default` impl on types where default values are meaningless. `Default` only when zero-value is valid.

### Async

- Async only where needed: watch mode, eBPF event streams, Docker API calls, K8s API calls.
- `scan`, `graph`, `impact`, `emulate`, `test run` = sync. No tokio needed at call site.
- Collectors: sync unless they call async external APIs. If async, expose both sync wrapper and async fn.
- Never make entire crate async just because one function needs it.

### Visibility & Coupling

- Default `pub(crate)`. Export only what other crates need.
- No circular crate deps. Check `Cargo.toml` workspace graph if unsure.
- Domain types in `twin-core`. No OS-specific imports in `twin-core`. No eBPF/k8s/container imports in `twin-core`.
- CLI is thin shell: parse args → call app → render result. No business logic in `twin-cli`.
- App layer orchestrates. No direct store access from CLI. CLI talks to `twin-app` only.

### CLI human output

Human-facing command output lives in `twin-cli/src/output/`. **Every command must look polished in the terminal** — not debug dumps or bare key-value lines.

- Use the shared helpers in `output/format.rs` (`Lines`, tree rows, status tags, section headers).
- Prefer a consistent layout: command title with rule line (`═`), short status summary, then tree (`├──` / `└──`) or graph connectors (`│`, `▼`) for structured data.
- Show full paths and labels; do not truncate user-visible names for convenience.
- Use `ok` / `warn` / `fail` status tags for health and outcomes; keep JSON output machine-oriented via `--json` only.
- Graph views should read as trees or neighborhoods, not flat tables.
- New commands ship with a human renderer and tests in `twin-cli/tests/output.rs` that lock in stable sections and formatting.

### Tests

- Every slice needs tests. No slice ships without tests.
- Prefer integration tests in each crate’s `tests/` directory; use `tests/support/` for shared fixtures (e.g. `IsolatedHome` + `TwinLayout`). Shared support modules carry `#![allow(dead_code)]` with a comment; each test binary uses a subset.
- A test function without `#[test]` is dead code that never runs — and a missing `#[test]` must never be fixed by deleting the test.
- Inject dependencies (`TwinLayout`, fake paths) — never global mutexes or hidden env vars in production code for test hooks.
- Unit tests in-source only for small pure helpers; test commands and I/O via integration tests.
- Use `twin-fixtures` for fake `/proc`, fake systemd, fake K8s, fake eBPF streams. No root/Docker/K8s/eBPF needed for tests.
- Never mutate real system in tests. Ever.
- Safety tests in `twin-safety`: scan source for forbidden mutation strings (`systemctl restart`, `docker stop`, `kubectl apply`, etc.).
- Test error paths. Missing file, bad permissions, malformed data = must have tests.
- No `#[ignore]` tests unless gated behind actual runtime capability (eBPF, real Docker). Mark clearly why.

### Naming

- Crates: `twin-<domain>`. Lowercase hyphenated.
- Types: `PascalCase`. Functions/methods: `snake_case`. Constants: `SCREAMING_SNAKE`.
- Files: `snake_case.rs`. Matches primary type or module.
- Booleans: `is_`, `has_`, `should_` prefixes. `is_active`, `has_fd`, `should_retry`.
- Avoid `data`, `info`, `manager`, `handler`, `processor`. Name by what it does: `GraphOverlay`, `ImpactTraversal`, `SocketResolver`.
- Avoid abbreviations except domain-standard: `pid`, `fd`, `ns`, `eBPF`, `K8s`.

### Patterns

#### Observation Pattern
Collector emits `Observation`. Graph builder + inference engine create edges. Collectors never create final conclusions directly. Keeps collectors simple, core extensible.

#### Overlay Pattern
`EffectiveGraphView { base, overlay }`. Read overlay first; fall through to base. Never clone entire graph. Never persist overlay as real state.

#### Adapter Pattern
External systems (Docker, K8s, systemd) behind read-only trait. Trait has only `get/list/watch` methods. No mutation methods exist in trait. Mutations structurally impossible.

#### Fixture Pattern
Fake data in `twin-fixtures`. Tests instantiate fixture → pass to collector/graph/emulate. No real OS interaction needed. Fixtures cover: normal case, missing permissions, malformed data, empty systems, race conditions (process vanished mid-scan).

#### Handling Missing Data
Collector can't run? Return `CoverageGap`. Continue other collectors. Never crash. Report in output: "8 processes hidden (permissions)".

### Dependencies

Use what tech document specifies: `clap` `tokio` `rusqlite` `aya` `kube-rs` `serde` `serde_yaml` `thiserror`.
Check `Cargo.toml` before adding new crate. Prefer std. Minimize deps. New dep = justify in commit message.

### What Not To Do

- No `dead_code` suppressions. If unused, delete it. Exception: `tests/support/mod.rs` modules shared by several integration-test binaries carry `#![allow(dead_code)]` with a comment — each binary uses a subset, so per-binary dead-code warnings are false positives.
- No `#[allow(clippy::...)]` unless proven false positive with comment explaining why.
- No `todo!()` or `unimplemented!()` in merged code. Use proper error return.
- No `println!` or `eprintln!` in library code. Only in `twin-cli` binary.
- No `dbg!` in merged code.
- No stringly-typed domain logic. Parse string → typed at boundary immediately.
- No `Arc<Mutex<...>>` everywhere "just in case." Use only where concurrency requires it.
- No `Box<dyn Trait>` when generic or enum works. Trait objects only when dispatch needed at runtime.
- No feature flags for hypothetical future use. Add when needed.

## Design

- **Vertical slices.** Each slice → runnable CLI command. No horizontal phases.
- **Observations, not conclusions.** Collectors emit `Observation`s. Inference engine creates edges.
- **Fixture testing.** `twin-fixtures` provides fake data. No root/Docker/K8s/eBPF required for tests.
- **Graceful degradation.** Collector can't run → record coverage gap, continue, never crash.
- **One evidence score.** `twin_core::score_evidence` is the only 0-100 formula. It returns `EvidenceStrength`. Coverage gaps lower that score and show up as unknowns. They are not risk.
- **Read-only adapters.** Only `get/list/watch` methods. K8s: no `create/update/patch/delete`. Docker: no `restart/stop/kill/remove/exec`.
- **Overlays never persist.** Emulation patches hypothetical only.

## Slice State

Implement or change anything from a slice → update `docs/state/slice-NN.md`. Track: status, done, blocked, decisions, deviations. On completion, verify every acceptance criterion.

## Commands

```bash
cargo build && cargo test --workspace && cargo clippy --workspace && cargo fmt --check
```

Add crate → update root `Cargo.toml` `[workspace]`.
## 2026-10-08 implementation conventions

- `twin-safety` is the workspace-wide source safety test crate. It scans production source for shell execution and host mutation paths.
- `clap_complete` in twin-cli generates completions from clap. `chrono` (std only) in twin-app parses RFC3339 history timestamps; elapsed durations retain the existing parser.
- Graph exports request raw stored neighborhoods to preserve edge kinds. Historical graph reads use latest retained history at/before the timestamp and never substitute current rows.
- Watch streams every tick but retains only the latest 256 result ticks and 256 distinct warnings. Keep total counts independent of retained history.
- Config overrides select settings only; the database stays under TwinLayout/XDG. Validate configuration and request shape before a refresh scan.

- `twin-package` reads RPM SQLite, NDB v0 and standalone BDB hash v8/v9 databases; headers and binary pages are decoded with strict bounds. NDB slot/blob identity, bounds and Adler checksums must validate. BDB accepts endian-aware hash/overflow pages and rejects unsupported flags, duplicate/subdatabase forms and cache/recovery environments. Never invoke RPM or repair/recover a source database. Active WAL requires a bounded, metadata-stable private snapshot with validated salts/checksums and committed-frame replay; never let SQLite open the source WAL database or create host journal/shm files. Nonempty rollback journals reject immutable reads. Package source/version metadata must match the actual adapter.
- `twin-fixtures` owns shared RPM header, NDB and BDB byte fixtures; migrate other duplicated helpers when changing them. It is a dev dependency only.
- `twin-k8s` uses kube 4.2 + k8s-openapi 0.28 + Tokio for live reads. Rustls must enable the ring provider to avoid provider-selection panics. Reject external credential-command/auth-provider configurations; metadata-only secret/configmap requests, sanitized errors, bounded timeouts, and pagination are required.
- Exec ABI: 44 little-endian bytes, UID appended at offset 40. TCP admission is bounded by PID, endpoint, cgroup, event kind, and global session counters.
- Nginx include expansion is bounded; proxy evidence stores sanitized origins, never URL credentials/query values. Kernel cgroup identities are read only from cgroup-v2 paths and must match uniquely before attribution.

Internet socket collection uses protocol-neutral `InetTableKind`/`InetSocketRecord`/`InetConnectionRecord`. Use core `TransportProtocol` and `NodeId::inet_port` for protocol-sensitive matching; never merge TCP and UDP endpoints. UDP bound/connected socket observations do not prove packet transmission. Missing/malformed UDP tables are coverage gaps and must not reap stored UDP relationships.

Domain history summaries use core ChangeCategory and the shared score_evidence formula. A changed valid systemd InvocationID supports an inferred restart/reactivation, never a claim about its cause; missing/pruned predecessors cannot establish a restart or version change.

Fake proc file construction belongs in twin-fixtures (dev dependencies only); owning test support may wrap it with expect assertions. Systemd templates apply only as fallback for concrete enabled/observed instances; explicit instance files, masks and search-path priority take precedence. Unsupported specifiers must warn and suppress unresolved relationships. Accept=yes template declarations do not prove a running instance; use D-Bus trigger relations for concrete observed instances.

Outbound allowlist DNS belongs in twin-app orchestration through twin-collectors::dns::DnsReader; twin-test stays pure. Tokio is reused for bounded resolver waiting. Current DNS answers are observations, matching historical peers is inference, and failed resolution must never silently pass. Never retain raw resolver errors or malformed endpoint values. Nginx named upstream definitions are scoped per service and ambiguous duplicates must not choose an arbitrary backend.

Lifecycle finalization must consider both active and stale unresolved rows, while active-only current/snapshot queries keep their existing semantics. An incomplete nginx configuration makes unseen nginx evidence stale, never a confirmed removal; independently observed socket evidence stays current. Parse nginx syntax only for nginx configuration sources.

Incident timelines use typed ChangeCategory filters and chronological operational entries. `what-changed --target` traverses bounded explicit semantic links from current/stale rows plus retained window history, never mount/cgroup/parent-process fan-out. Show omitted-entry counts and preserve coverage unknowns under filtering; temporal association is not causation.

Restart reasoning preserves systemd declaration semantics: active Wants alone is configured context, not proof of runtime disruption. Requires is preferred when both declarations exist for a pair. Transitive counts use distinct affected service IDs across path steps, excluding direct dependents and the target; leaves exactly at a depth bound remain visible.

Kubernetes rollout policy is projected as safe resolved counts with documented API defaults/rounding. Availability predictions are conditional on current observed endpoints and policy; never assume new surge pods become ready or mark all rolling-update pods simultaneously unavailable. Missing/stale reads, unsupported policy and unknown endpoint coverage require Unknown availability and shared-core weak evidence. Pod deletion must respect the same coverage/freshness limits.

`why` proof traversal is bounded (4 links, 64 relationships, 32 traversed nodes, 16 paths). Each stored link keeps its observed/inferred class and shared evidence score; do not create a separate chain-scoring formula. Display allowlisted observation facts only, with raw line content excluded. Observation display truncation never truncates the evidence used for scoring.

Network namespace collection uses read-only `/proc/PID/net` tables and `/proc/PID/ns/net` identities, never `setns`. Socket inode ownership joins are scoped to a stable namespace; reread identities after collecting tables/descriptors. Scanner namespace endpoints retain host aliases, while other endpoint IDs carry namespace identity. Wildcard local dependency inference requires loopback or a concrete observed local address in the same namespace; a remote peer sharing the listener port does not prove a local dependency. Preserve relation-specific inference metadata and actual namespace table evidence paths.

Namespace endpoints and inferred/observed runtime edges retain the shared validated boot identity. Changed known boots and gone reappearance reset incarnation boundaries; prior edge observation links cannot count toward new evidence. Missing boot identity is coverage uncertainty, with last known identity retained only for later comparison.

Store schema migrations run explicitly through `twin init`, never implicitly on open. Each migration's changes and version marker commit atomically. Schema 6 removes legacy argv/argv_json recursively from stored metadata, preserves safe argument counts, and sanitizes legacy Docker failures. `secure_delete` applies only to twin's own SQLite store; logical cleanup does not establish forensic erasure of WAL files, backups or exported artifacts. Existing users must run `twin init` to upgrade.

Snapshot edge proof references are captured with graph rows in one transaction and survive current-edge reincarnation/reset and observation retention. Retained reference IDs do not prove retained observations; missing rows remain Unknown. Pre-v6 snapshots can backfill only still-linked observations at or before their capture timestamp; absent older references cannot be reconstructed.

Schema 6's structured argument cleanup includes retained test report JSON. It removes argument keys rather than guessing secret values in arbitrary prose.

Semantic health policies use source-scoped Unknown facts and retained observation counts; snapshot policies read captured snapshot proof references and as-of coverage. Historical disk/DNS/emulation inputs absent from snapshots stay Unknown. Deduplicate routed endpoint pods and reject ambiguous or stale ownership. Future timestamps do not establish freshness. Usage and explicit limits live in docs/operational-workflows.md.

Nginx contextual include parsing preserves enclosing HTTP/stream groups and proxy transport, with depth/file/byte budgets and cycle warnings. twin-config depends inward on twin-core for TransportProtocol. Scan DNS uses injected DnsReader; DNS-derived references are inferred and typed system_dns observations identify current resolver evidence, never nginx's historical/runtime answer. Includes still retain per-file fingerprints and original safe evidence paths.

Systemd relevant values use continued logical lines and quoted/escaped item parsing; malformed relevant values return sanitized parse errors. Type and dash-prefix drop-in specificity precedes load-path priority for equal filenames; masks suppress lower choices. Unit-name-only specifiers can expand without host context; unsupported user/credential-dependent specifiers remain coverage gaps.

Containerd uses tonic/prost metadata-only read projections for Namespaces/Containers/Tasks/Images List RPCs. No OCI specs, labels, task I/O or env bodies are projected. Canonical container/image IDs include runtime plus namespace. Complete source reads may finalize their own absent rows despite unrelated host collector gaps; unavailable sources are preserved and incomplete sources remain uncertain.

Kubernetes resource watches use metadata-only GET streams, bounded event lines and retained results, resource-version/bookmark continuation, relist on 410, and bounded reconnect backoff. Safe full-cluster projections refresh the graph; interruption coverage never finalizes unseen graph rows. `futures-util` supplies bounded stream I/O. Explicit configured-client injection is permitted for embedding and fixtures, without environment hooks. Shared Kubernetes stream fake-server fixtures live in twin-fixtures (dev-only), using serde_json to generate synthetic responses.

Pod log reads are non-following, tail/byte/time bounded and volume/lexical summaries only. Clear transient chunks/windows, never return/store log bodies, snippets or content hashes. Typed phrase matches are observations, not causes or severity conclusions. Capability-gated fake API tests return only on actual PermissionDenied binding.

Bundled read-only eBPF producers are `twin-ebpf/probes/twin.bpf.c` plus its tracked `.o` and SHA-256 manifest; `python3 tools/build_ebpf.py --check` verifies source/object reproducibility with clang's BPF target. Ordinary Rust builds/tests use the bundled object without clang or root. `aya-obj` is a dev dependency solely for unprivileged ELF/BTF/map validation. Live payload extensions are exec 60 bytes and connect 72 bytes, appending namespace inode and process leader start ticks; legacy fixture payloads remain 44/56 bytes. Live attachment checks Linux little-endian 64-bit/USER_HZ=100 and TCP tracepoint field layout. TCP initiator identity is captured inside a connect syscall and retained by socket pointer until ESTABLISHED, never taken from the completion/softirq task. Namespace identity comes from the socket itself. Kernel drop counters and queue drains preserve source-scoped coverage gaps. Exec invalidates previous runtime relationships as stale, not confirmed closed; process generations never borrow an older PID's evidence.

Specialized graph neighborhoods use temporary bounded node/link/observation batches; preserve per-edge proof IDs and historical-store selection. Batch incoming-edge queries bind and deduplicate target IDs in chunks of 500. Missing or malformed specialized-view observation rows retain their existing omission semantics, never fabricate proof. Watch soak cardinality samples read only numeric counts/sizes from Twin's isolated store and explicitly close read-only connections; RSS and database growth correlation does not establish allocator/leak causation.

Systemd main/template/drop-in file reads are bounded to 1 MiB. `[Unit]` is optional; malformed relevant declarations are gaps. Apply a drop-in's dependency and socket changes atomically after all relevant sections validate.


Systemd D-Bus `Socket.Listen` is observed manager configuration, never proof of an open listener or descriptor ownership. Store `socket_config_known` separately from active/substate; failed properties clear current configuration addresses and raise per-unit coverage gaps. Runtime FIFO ownership requires stable descriptor mode/device/inode evidence and namespace-safe path agreement; VSOCK requires kernel socket-diag evidence, never an invented `/proc/net/vsock` table or declaration-only join.

## Agent integration (slice 24)

- Agent surfaces are `twin brief`, `twin preflight`, `twin mcp` and `twin agent-config`. Keep every agent-facing answer bounded: sections ≤12 items, preflight reasons ≤4 and affected ≤8, hook text ≤~1.8 KB, MCP tool text ≤24 KB. Never let host-wide warnings repeat per item; group them by distinct detail.
- Only `twin-app/src/commands/preflight/` may name mutating commands (`systemctl restart`, `iptables -`, ...), and only as text to recognize. Execution primitives stay banned everywhere; `twin-safety` enforces both rules.
- Preflight never says go for something it could not assess on the graph. Unknown targets and failed emulations are caution with `is_assessed=false`. Unmodeled tools (`git`, `cargo`, `ssh`) are go and silent in hooks.
- Hooks fail open and never answer "allow" unless the agent protocol requires an answer (Cursor). Add a new agent dialect in `twin-cli/src/hook.rs` with a test in `twin-cli/tests/hook.rs` that pins its JSON shape.
- The MCP server is hand-written newline JSON-RPC; do not add an SDK without a concrete need. Tools must stay annotated `readOnlyHint: true`. `agent-config` prints snippets and never writes agent configuration files.
- Restart emulation of `docker.service` treats running Docker containers as runtime impacts unless the daemon reports live-restore (`GET /info`, stored as `daemon_live_restore` on container nodes).

## Slice 25 conventions

- `--json` output is bounded by `output::json::cap_lists` (100 items per list plus `<field>_omitted`); `--full` disables it. New commands emit through `emit`/`emit_after_scan` with `global.json_mode()`, never `serde_json` directly. Keep `twin-cli/tests/output_budget.rs` ceilings passing.
- Hook file edits come only from the allowlist of file-changing tools in `hook.rs` (`FILE_EDIT_TOOLS`); read tools that carry a path must never become edits.
- Script expansion (`preflight/script.rs`) surfaces only operation lines; never echo other script lines (they can hold credentials). Lines using shell variables are cautions, not guesses.
- Docker labels: only `com.docker.compose.project`, `com.docker.compose.service` (character-filtered) and `com.docker.compose.project.config_files` (absolute paths, slice 26) are projected. Do not add label projection without a sentinel test.
- Interface data comes from `/proc/net` text via `twin_collectors::parse_interfaces`; no netlink, ioctls or unsafe.
- Release artifacts: musl static builds via `cross`, `.sha256` per artifact plus `SHA256SUMS`, provenance attestation. `install.sh` must never use sudo or write outside its install directory.


## Slice 26 conventions

- Observations are facts with sightings: `timestamp_ns` is the first sighting, and `last_seen_ns`/`seen_count` count repeats. A new row merges into the previous row only via `Store::merge_repeated_observations` (end of every scan pass, and `twin init`). The previous row must have been seen in the same collector's current or previous run, not be sealed by `reset_node_incarnation`, and support exactly the same edges. Recency uses `last_seen_ns` (never newer than the read time); eBPF repeat counts sum `seen_count`. Never add per-sighting keys to observation metadata without adding them to the fingerprint's volatile list.
- History records the net change of a pass (`ScanHistorySession` `PassWrite`). Sighting counters (`evidence_count`, `last_seen_ns`, `runtime_observed_at_ns`) are not changes. Retention keeps each entity's state at the cutoff.
- Collector-run metadata must stay bounded per run (group repeated warnings with counts).
- `[dependencies] window_days` keeps closed inferred connections as runtime dependents with a "last seen … ago (N×)" reason. Timers are `timer → service` `depends_on` edges with `timer_activation`; cron jobs are `cron_job` nodes owned by the cron daemon's service. Cron commands are classified transiently with `preflight::scheduled_facts`, and only program names and typed operations are stored, never command text or environment values (sentinel-tested).
- The daemon (`twin daemon run`) serves `McpServer` in `ServeMode::Daemon` over a 0660 Unix socket: one handler, two transports. In daemon mode handlers never scan and never read a file a client names (`twin_check` refused, `should_read_scripts: false`). Every new handler method goes into `mcp::METHODS` and the read-only sweep in `twin-cli/tests/daemon.rs`. Peer identity comes only from `SO_PEERCRED`/`SO_PEERGROUPS` (`twin_app::daemon`). CLI tests pin `brief`/`preflight`/`mcp` to fixtures with `--no-daemon` via `support::without_host_daemon`.
- `twin daemon install-unit --print` only prints. Keep its `systemd-analyze security` exposure below 3.0; adding a capability, address family or syscall group needs a recorded reason in the slice state.
- Config-declared dependencies: parse values only in `twin-config` and keep only `DeclaredEndpoint::origin()` (scheme/host/port or socket path), the setting name and the declaring file. Every new format needs a secret-sentinel test in `twin-config/tests/declared.rs` and store/output coverage in `twin-app/tests/declared.rs`. Declared edges carry `source: config_declared`; never overwrite an edge another writer already wrote this pass (`ScanHistorySession::has_seen_edge`). Docker labels projected are now project, service and `project.config_files` (absolute paths only).
- Host paths go through `twin_app::paths::host_path` (and `default_proc_root`); never hard-code `/proc`, `/etc`, sockets or package paths at a read site. Node IDs and evidence use `logical_path` (the host's path, not the mount path). Under a host root, host identity (network namespace, mounts, hostname) comes from host PID 1, never from the reader's own `self`. Keep `server.json`, the Dockerfile label and the crate version in step (`twin-cli/tests/packaging.rs`).
- Every live eBPF attach attempt writes one `ebpf` collector run (`success`/`unavailable` + reason); coverage reads it. The daemon's `--ebpf auto` uses the same readiness assessment as `doctor --ebpf`. `tools/build_ebpf.py --check` is byte-exact only with the compiler named in the manifest; do not rebuild the bundled object just for a compiler bump.
