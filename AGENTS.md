# AGENTS.md — twin-ops

Agent working on **`twin`**: Rust read-only operational twin for Linux.
Source of truth: `docs/prd.md`, `docs/tech-document.md`, `docs/plan-slices.md`, `docs/code-layout.md` (implemented module/test layout). Read them before changes.

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

`cli` `app` `core` `observation` `collectors` `store` `graph` `rules` `emulate` `test` `ebpf` `container` `k8s` `package` `config` `output` `fixtures` `safety` — all prefixed `twin-`.

**Dependency direction:** inward. `core` depends on almost nothing. `cli→app→{core,store,graph,collectors,emulate,test,output}`. Collectors → `core+observation`. Reasoning → `core+graph+rules`. **Forbidden:** `core→ebpf`, `core→k8s`, `core→container`, `graph→cli`, `emulate→collectors`.

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

### Tests

- Every slice needs tests. No slice ships without tests.
- Prefer integration tests in each crate’s `tests/` directory; use `tests/support/` for shared fixtures (e.g. `IsolatedHome` + `TwinLayout`).
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

- No `dead_code` suppressions. If unused, delete it.
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
- **Read-only adapters.** Only `get/list/watch` methods. K8s: no `create/update/patch/delete`. Docker: no `restart/stop/kill/remove/exec`.
- **Overlays never persist.** Emulation patches hypothetical only.

## Slice State

Implement or change anything from a slice → update `docs/state/slice-NN.md`. Track: status, done, blocked, decisions, deviations. On completion, verify every acceptance criterion.

## Commands

```bash
cargo build && cargo test --workspace && cargo clippy --workspace && cargo fmt --check
```

Add crate → update root `Cargo.toml` `[workspace]`.