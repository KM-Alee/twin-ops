# Slice 3 Implementation Plan: Core Domain Model and Observation Pipeline MVP

**Implemented layout and tests:** see `docs/code-layout.md` (canonical module trees, `TwinLayout`, integration-test conventions). This plan extends it with `twin-observation` and the `twin-core` domain vocabulary.

## 1. Overview

Slices 1–2 produced a working binary (`twin init`, `twin doctor`) and a SQLite store whose rows are **stringly typed** on purpose — slice 2 deferred all domain types to "slice 3 when the observation pipeline and graph model arrive."

This slice is that arrival. Its job, per `plan-slices.md`, is to **"define the core language of the system before adding many collectors."** Concretely:

1. Introduce the typed graph vocabulary in `twin-core` (ID newtypes + kind/state/class enums) so nothing in the graph core is a raw string.
2. Create the `twin-observation` crate holding the `Observation` model and the **first end-to-end pipeline shape**: `RawObservation → redact → normalize → Observation`.
3. Bridge the typed `Observation` to the existing stringly-typed `ObservationRow` so observations can be **stored and queried** through the slice-2 store — the conversion functions slice 2 explicitly promised would land here.

There is **no new CLI command** this slice. The user-facing surface stays `twin init` / `twin doctor`. The deliverable is a tested library capability, exactly matching the slice's working demo:

```bash
twin doctor --json     # still healthy; schema unchanged
cargo test core        # exercises IDs, normalization, redaction, pipeline
```

---

## 2. Scope — What This Slice Does

| Area | Detail |
|------|--------|
| **Graph vocabulary (`twin-core`)** | `NodeId`, `EdgeId`, `ObservationId`, `TimestampNs`, `CollectorName`; enums `NodeKind`, `NodeState`, `EdgeKind`, `EdgeClass`, `EdgeState` — all with `Display` + `FromStr` + `serde`. Canonical `NodeId`/`EdgeId` constructors. |
| **Observation domain (`twin-observation`)** | `RawObservation`, `RawIdentity`, `RawEvidenceRef`, `Observation`, `ObservationMetadata`; enums `ObservationSource`, `ObservationKind`, `ConfidenceHint`, `RedactionState`. |
| **Pipeline (`twin-observation`)** | `Redactor` trait + `BasicRedactor`; `Normalizer` (raw identity → canonical `NodeId`); `Pipeline` composing redact → normalize → `Observation`. |
| **Persistence bridge (`twin-store`)** | `From<&Observation> for ObservationRow` and `TryFrom<&ObservationRow> for Observation`. `Store` gains typed `insert_observation_typed` / `get_observation_typed` thin wrappers. No schema change. |
| **Tests** | `twin-core`: ID canonical forms + round-trip. `twin-observation`: redaction, normalization, pipeline. `twin-store`: typed observation round-trip through in-memory DB. |

---

## 3. Scope — What This Slice Does NOT Do

| Excluded | Arrives In | Reason |
|----------|-----------|--------|
| `RiskLevel`, `EvidenceStrength`, `EvidenceLabel` | Slice 8 (risk) / when `Edge` is scored | Slice 1's plan already scheduled these for slice 8. No impact analysis or edge scoring exists yet; they would be unexercised. `plan-slices.md` lists them under slice 3, but the slice-3 pipeline ends at observation persistence — it never scores anything. We honor slice 1's decision. |
| Full `Node` / `Edge` structs | Slice 4 | Slice 3 produces observations, not graph entities. The vocabulary (kinds/states) lands now; the structs land when collectors emit them. |
| Node / Edge row typed conversions | Slice 4 | Nothing creates nodes or edges this slice, so a typed bridge for them would be dead code. Only `Observation ↔ ObservationRow` is needed now. |
| Real collectors / `/proc` reading | Slice 4 | The pipeline is exercised with synthetic `RawObservation`s built in tests. |
| `twin-fixtures` crate | Slice 4 | First fake `/proc` is needed only when a real collector exists. Tests here build raw observations inline. |
| Inference rules, entity resolution joins, observed-edge creation | Slices 4–7 | Pipeline stops at observation persistence per the slice's stated shape. |
| Async / `Collector` trait | Slice 4 (trait) / 13 (async) | No collector implementations yet; the pipeline is sync. |
| `SnapshotId`, `TestRunId`, `EmulationRunId` | Slices 12 / 17 / 9 | No consumers until those features exist. |
| New SQLite migration | — | `metadata_json` (slice-2 catch-all) absorbs `raw_ref`. No `MIGRATION_003`. |
| New CLI command / `doctor` changes | Slice 4 | Working demo is `twin doctor --json` (unchanged) + `cargo test core`. |

---

## 4. Design Decisions

### 4.1 The "core language" tension, resolved

`plan-slices.md` lists a large batch of types for slice 3, but `AGENTS.md` is emphatic that **"Dead code = bug"** and slice 1 explicitly scheduled some of those types (`RiskLevel`, `EvidenceStrength`) for slice 8. These two sources conflict.

Resolution principle: **implement every type with a real consumer in the slice-3 pipeline, plus the graph-core vocabulary the slice is chartered to define; defer analysis-layer types that have no consumer until their slice.**

Applying it:

- **Implemented (graph-core language + pipeline consumers):** the ID newtypes, `NodeKind`/`NodeState`/`EdgeKind`/`EdgeClass`/`EdgeState`, and the observation vocabulary. `NodeKind` is consumed by `NodeId` construction/normalization. The observation enums are consumed by the `Observation` struct and the store bridge (round-tripped through SQLite). The edge vocabulary is consumed by `EdgeId` construction + the conversion/round-trip tests and is the language slice 4 immediately builds on. Acceptance criteria #1 ("all domain-critical concepts use newtypes/enums") and #2 ("no raw stringly-typed graph core") require this typed vocabulary to exist.
- **Deferred (analysis layer, no near-term consumer):** `RiskLevel`, `EvidenceStrength` → slice 8, matching slice 1's table. Defining them now means maintaining unexercised code through five slices.

This is the most defensible line: graph-core vocabulary is the slice's chartered deliverable and is exercised by conversions/tests; risk/evidence are downstream analysis concepts with a real home later.

### 4.2 Domain types split across two crates by concern

- `twin-core` owns the **graph language** shared by every crate: IDs and node/edge kind/state/class enums. It stays a near-leaf (no OS imports, no eBPF/k8s/container — per `AGENTS.md`).
- `twin-observation` owns the **observation domain**: the `Observation`/`RawObservation` structs, the observation-specific enums (`ObservationSource`, `ObservationKind`, `ConfidenceHint`, `RedactionState`), and the pipeline logic (redaction, normalization). It depends on `twin-core`.

Why not put the observation enums in `twin-core`? They are observation-pipeline vocabulary, not graph-core vocabulary. Keeping them in `twin-observation` keeps `twin-core` minimal and gives the observation crate a self-contained schema, matching the tech document's crate table (`twin-observation` = "Observation schema, redaction, normalization contracts").

### 4.3 Dependency direction

```
twin-core         (leaf — std + serde + uuid + thiserror only)
twin-observation  → twin-core
twin-store        → twin-core, twin-observation
twin-app          → twin-core, twin-observation, twin-store
twin-cli          → twin-app
```

`twin-store → twin-observation` is the only new edge. It is intentional and was pre-announced by slice 2 ("the store gains conversion functions that accept/return the newtyped IDs"). Infrastructure (store) depending on a domain crate (observation) for serialization is the standard repository arrangement; the domain never depends on the store. No forbidden edge from `AGENTS.md` is introduced (`core` gains no outward deps; `observation` depends only on `core`).

### 4.4 The pipeline is sync and synthetic this slice

No collector exists, so the pipeline input (`RawObservation`) is constructed directly in tests. The pipeline is plain sync functions — `AGENTS.md` reserves async for collectors that call external APIs and for watch/eBPF, none of which exist yet. The `Collector` trait itself arrives in slice 4.

### 4.5 `raw_ref` is folded into `metadata_json`, not a new column

The tech document's `Observation` carries `raw_ref`, but the slice-2 `observations` table has no such column — slice 2 designated `metadata_json` as the catch-all to avoid schema churn. We honor that: `Observation.raw_ref` is a first-class typed field in the domain, and the `Observation ↔ ObservationRow` bridge serializes it into `metadata_json` under a reserved key (`__raw_ref`) and restores it on read. No `MIGRATION_003`. The round-trip test guarantees fidelity. If a future slice needs to query by `raw_ref`, it can promote the field to a column then.

### 4.6 `ObservationMetadata` is an ordered JSON object, not a free string

Metadata is inherently heterogeneous (env names, endpoints, redaction flags). Modeling it as a newtype over `serde_json::Map<String, Value>` gives: direct serialization to the `metadata_json` column, deterministic key ordering (serde_json's default map is `BTreeMap` → sorted keys → stable test assertions), and typed insert helpers. This avoids both a stringly blob and a premature explosion of per-kind metadata structs. Typed per-kind metadata can arrive when a collector needs it.

### 4.7 IDs are newtypes with `Display` + `FromStr`; construction is canonical

Every ID gets `Display` (for store/CLI output) and `FromStr` (for parsing back from store/CLI), per `AGENTS.md` ("`Display` impl for all IDs. `FromStr` impl for all IDs that come from CLI input."). `NodeId` is never built from a raw string in domain code — it is built through canonical constructors (`NodeId::process`, `NodeId::file`, …) that encode the `NodeKind` prefix and normalize the payload. This makes "no raw stringly-typed graph core" structurally true.

### 4.8 Normalization is lexical and read-only

`NodeId::file` canonicalizes `.`/`..` segments **lexically** (string manipulation only) — it must not touch the filesystem (`std::fs::canonicalize` would hit the host, may fail on non-existent paths, and violates read-only purity). IP normalization (`127.000.000.001` → `127.0.0.1`) parses into `std::net::IpAddr` and re-renders. Service normalization strips the `system.slice/` cgroup prefix and the leading path.

---

## 5. `twin-core` — graph vocabulary

### 5.1 Module layout

```text
crates/twin-core/src/
  lib.rs            # + pub mod id, node, edge; re-exports
  error.rs          # + ParseError (FromStr failures)
  id.rs             # ObservationId, TimestampNs, CollectorName
  node.rs           # NodeId (+ canonical ctors), NodeKind, NodeState
  edge.rs           # EdgeId (+ ctor), EdgeKind, EdgeClass, EdgeState
  config/           # unchanged (mod, retention, template)
crates/twin-core/tests/
  config.rs         # unchanged
  ids.rs            # NEW: TimestampNs, ObservationId, CollectorName round-trips
  node_id.rs        # NEW: canonical NodeId construction + normalization
  enums.rs          # NEW: Display/FromStr round-trip for all kind/state/class enums
```

Enums are grouped by concept (`node.rs`, `edge.rs`) rather than one-file-per-enum — slice 1 explicitly called out `kinds.rs`/`state.rs`/`evidence.rs` over-splitting as a problem to avoid.

### 5.2 `id.rs`

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimestampNs(i64);

impl TimestampNs {
    pub fn new(ns: i64) -> Self {
        Self(ns)
    }

    pub fn now() -> Self {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        Self(ns)
    }

    pub fn as_i64(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObservationId(Uuid);

impl ObservationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl fmt::Display for ObservationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ObservationId {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| ParseError::ObservationId { value: s.to_string() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CollectorName(String);

impl CollectorName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CollectorName { /* writes self.0 */ }
```

`TimestampNs` is `Copy` (it wraps `i64`). `ObservationId::new()` is the UUID v4 factory; `Default` is **not** derived because a "default observation id" is meaningless (`AGENTS.md`: "`Default` only when zero-value is valid").

### 5.3 `node.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Host,
    Process,
    Service,
    Port,
    File,
    Cgroup,
}
```

Exactly the six "initial node kinds" from the slice. The tech document's full 26-variant enum is aspirational; variants are added in the slice that produces that node kind (`Container` in slice 22, `K8sPod` in slice 23, …). `Display`/`FromStr` map to the lowercase prefix tokens (`host`, `process`, …) used in canonical IDs and the store `kind` column.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeState {
    Active,
    Stale,
    Gone,
}
```

`NodeState`/`EdgeState` map to the store `state` column (default `active`).

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(String);
```

**Canonical constructors** (the only blessed way to build a `NodeId` in domain code):

```rust
impl NodeId {
    pub fn host(hostname: &str) -> Self {
        Self(format!("host:{hostname}"))
    }

    pub fn process(pid: u32) -> Self {
        Self(format!("process:pid:{pid}"))
    }

    pub fn service(unit: &str) -> Self {
        Self(format!("service:{}", normalize_unit(unit)))
    }

    pub fn port_tcp(ip: &str, port: u16) -> Result<Self, ParseError> {
        let ip = normalize_ip(ip)?;
        Ok(Self(format!("port:tcp:{ip}:{port}")))
    }

    pub fn file(path: &str) -> Self {
        Self(format!("file:{}", lexical_canonical(path)))
    }

    pub fn cgroup(path: &str) -> Self {
        Self(format!("cgroup:{}", lexical_canonical(path)))
    }

    pub fn kind(&self) -> Option<NodeKind> { /* parse prefix → NodeKind */ }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

Helpers (private to `node.rs`):

- `normalize_unit("system.slice/nginx.service") -> "nginx.service"` — drops a leading `*.slice/` segment and any directory prefix.
- `normalize_ip("127.000.000.001") -> "127.0.0.1"` — `s.parse::<IpAddr>()` then `to_string()`; returns `ParseError::IpAddr` on failure.
- `lexical_canonical("/etc/nginx/../nginx/nginx.conf") -> "/etc/nginx/nginx.conf"` — split on `/`, resolve `.`/`..` without filesystem access, preserve leading `/`.

`FromStr for NodeId` accepts any non-empty string (it is the inverse of `Display` for store reads); the **canonical** constructors are how new IDs are minted. This keeps reads cheap while keeping writes disciplined.

### 5.4 `edge.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    ParentOf,
    Owns,
    InCgroup,
    ListensOn,
    ConnectsTo,
    ConfiguredBy,
    DependsOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeClass {
    Observed,
    Inferred,
    Predicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeState {
    Active,
    Stale,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeId(String);

impl EdgeId {
    pub fn new(from: &NodeId, kind: EdgeKind, to: &NodeId) -> Self {
        Self(format!("{}|{}|{}", from.as_str(), kind, to.as_str()))
    }
}
```

`EdgeKind` is the seven "initial edge kinds" from the slice. `EdgeId::new` makes edge identity deterministic from its endpoints + kind so the same observed relationship upserts to one row (this is what slice 4 will call). Slice 3 itself creates no edges; the edge vocabulary is the language slice 4 builds on, and is exercised here by `Display`/`FromStr` round-trip tests.

### 5.5 `error.rs` (additions)

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid observation id: {value}")]
    ObservationId { value: String },
    #[error("invalid ip address: {value}")]
    IpAddr { value: String },
    #[error("unknown {kind} variant: {value}")]
    Enum { kind: &'static str, value: String },
}
```

`ParseError::Enum` backs every enum's `FromStr` (e.g. unknown `NodeKind`). `ConfigError` is unchanged.

### 5.6 Enum string mapping convention

All graph enums implement `Display`/`FromStr` via a small internal macro **or** explicit match arms (prefer explicit arms — `AGENTS.md` discourages cleverness; a `match` is clearest and greppable). The string forms are the snake/lower tokens already used by the store and canonical IDs:

| Enum | Strings |
|------|---------|
| `NodeKind` | `host`, `process`, `service`, `port`, `file`, `cgroup` |
| `NodeState` / `EdgeState` | `active`, `stale`, `gone` |
| `EdgeKind` | `parent_of`, `owns`, `in_cgroup`, `listens_on`, `connects_to`, `configured_by`, `depends_on` |
| `EdgeClass` | `observed`, `inferred`, `predicted` |

`Display`↔`FromStr` must round-trip; a unit test enumerates every variant and asserts it.

### 5.7 `Cargo.toml` additions

```toml
[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
toml = { workspace = true }
uuid = { workspace = true }
```

`uuid` (features `v4`, `serde`) is added to the workspace dependency table (§9.1).

---

## 6. `twin-observation` — observation domain + pipeline

### 6.1 Purpose

Owns the `Observation` evidence model and the first pipeline: take a `RawObservation` (what a future collector will emit), **redact** secret values, **normalize** raw identities into canonical `NodeId`s, and produce a persistable `Observation`. Depends only on `twin-core`.

### 6.2 Module layout

```text
crates/twin-observation/src/
  lib.rs            # pub mod vocab, raw, observation, redact, normalize, pipeline, error; re-exports
  error.rs          # ObservationError
  vocab.rs          # ObservationSource, ObservationKind, ConfidenceHint, RedactionState
  raw.rs            # RawObservation, RawIdentity, RawEvidenceRef
  observation.rs    # Observation, ObservationMetadata
  redact.rs         # Redactor trait + BasicRedactor
  normalize.rs      # Normalizer (RawIdentity -> NodeId)
  pipeline.rs       # Pipeline (redact -> normalize -> Observation)
crates/twin-observation/tests/
  redaction.rs
  normalization.rs
  pipeline.rs
```

### 6.3 `vocab.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationSource {
    Proc,
    ProcNetTcp,
    ProcCgroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationKind {
    ProcessSeen,
    TcpSocketSeen,
    ProcessBelongsToCgroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfidenceHint {
    Low,
    Moderate,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RedactionState {
    None,
    Partial,
    Redacted,
}
```

`ObservationSource`/`ObservationKind` are seeded with a small set that the slice-3 pipeline tests exercise (process scan, tcp socket, cgroup membership — the tech document's own examples). They grow per collector: slice 4 adds `ProcFd`, slice 6 adds the UDP/unix sources, etc. — the same additive pattern as the store schema. `ConfidenceHint`/`RedactionState` map to the store's `confidence_hint` (default `moderate`) and `redaction_state` (default `none`) columns. All four implement `Display`/`FromStr` (strings: `proc`/`proc_net_tcp`/`proc_cgroup`; `process_seen`/…; `low`/`moderate`/`high`; `none`/`partial`/`redacted`).

### 6.4 `raw.rs`

```rust
use twin_core::{CollectorName, NodeKind, TimestampNs};

use crate::observation::ObservationMetadata;
use crate::vocab::{ConfidenceHint, ObservationKind, ObservationSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawIdentity {
    Host { hostname: String },
    Process { pid: u32 },
    Service { unit: String },
    TcpEndpoint { ip: String, port: u16 },
    File { path: String },
    Cgroup { path: String },
}

impl RawIdentity {
    pub fn node_kind(&self) -> NodeKind { /* maps variant -> NodeKind */ }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvidenceRef(String);

impl RawEvidenceRef {
    pub fn new(reference: impl Into<String>) -> Self { Self(reference.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone)]
pub struct RawObservation {
    pub source: ObservationSource,
    pub kind: ObservationKind,
    pub collector: CollectorName,
    pub subject: Option<RawIdentity>,
    pub object: Option<RawIdentity>,
    pub timestamp: TimestampNs,
    pub raw_ref: Option<RawEvidenceRef>,
    pub confidence_hint: ConfidenceHint,
    pub metadata: ObservationMetadata,
}
```

`RawIdentity` keeps the pipeline typed end to end — there are no raw identity strings floating around; the only strings are *inside* the variants (a pid is a `u32`, a port is a `u16`). `metadata` here may still contain **unredacted** values; redaction runs next.

Public fields on `RawObservation` are acceptable: it is a plain data-transfer record produced and consumed within the pipeline, the carve-out `AGENTS.md` allows ("No `pub` fields on domain types unless truly data-only"). The persisted `Observation` (§6.5) keeps fields private with accessors.

### 6.5 `observation.rs`

```rust
use twin_core::{NodeId, ObservationId, TimestampNs};

use crate::raw::RawEvidenceRef;
use crate::vocab::{ConfidenceHint, ObservationKind, ObservationSource, RedactionState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    id: ObservationId,
    source: ObservationSource,
    kind: ObservationKind,
    subject: Option<NodeId>,
    object: Option<NodeId>,
    timestamp: TimestampNs,
    raw_ref: Option<RawEvidenceRef>,
    confidence_hint: ConfidenceHint,
    redaction_state: RedactionState,
    metadata: ObservationMetadata,
}

impl Observation {
    pub fn id(&self) -> ObservationId { self.id }
    pub fn source(&self) -> ObservationSource { self.source }
    pub fn kind(&self) -> ObservationKind { self.kind }
    pub fn subject(&self) -> Option<&NodeId> { self.subject.as_ref() }
    pub fn object(&self) -> Option<&NodeId> { self.object.as_ref() }
    pub fn timestamp(&self) -> TimestampNs { self.timestamp }
    pub fn raw_ref(&self) -> Option<&RawEvidenceRef> { self.raw_ref.as_ref() }
    pub fn confidence_hint(&self) -> ConfidenceHint { self.confidence_hint }
    pub fn redaction_state(&self) -> RedactionState { self.redaction_state }
    pub fn metadata(&self) -> &ObservationMetadata { &self.metadata }
}
```

`Observation` is constructed only by the pipeline (`pub(crate)` constructor) — collectors and tests go through `Pipeline`, never build one by hand. Fields are private with accessors per `AGENTS.md`.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationMetadata(serde_json::Map<String, serde_json::Value>);

impl ObservationMetadata {
    pub fn new() -> Self { Self::default() }
    pub fn insert_str(&mut self, key: &str, value: &str) { /* ... */ }
    pub fn insert_bool(&mut self, key: &str, value: bool) { /* ... */ }
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> { self.0.get(key) }
    pub fn to_json(&self) -> String { serde_json::Value::Object(self.0.clone()).to_string() }
    pub fn from_json(s: &str) -> Result<Self, ObservationError> { /* parse object */ }
}
```

`Default` **is** derived here (an empty metadata map is a valid zero value). Key ordering is deterministic (serde_json's default `Map` is sorted), so `to_json()` is stable for assertions.

### 6.6 `redact.rs`

```rust
pub trait Redactor {
    fn redact(&self, raw: &mut RawObservation) -> RedactionState;
}

pub struct BasicRedactor;
```

`BasicRedactor::redact` walks `raw.metadata` and:

1. **Sensitive keys** — if a key (case-insensitive) contains any of `password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, `credential`, `authorization`, `private_key` → replace its value with `"<redacted>"`, set companion `value_redacted=true`.
2. **Connection strings** — for any string value matching `scheme://user:pass@host[:port]/...`, replace credentials, keep a derived `maybe_endpoint=host:port` entry, and drop the raw value. (This reproduces the tech document's `DATABASE_URL` example verbatim and is a headline test case.)
3. Returns `RedactionState::Redacted` if anything was redacted, `RedactionState::None` otherwise (`Partial` reserved for future multi-field cases).

It never touches keys/paths/process names/addresses/hashes/timestamps — those are explicitly listed as "store" in the tech document. The trait exists so slice 19 (config parsing) and slice 23 (k8s secrets) can supply richer redactors without touching the pipeline. This satisfies acceptance "redaction hook exists even if simple."

### 6.7 `normalize.rs`

```rust
pub struct Normalizer;

impl Normalizer {
    pub fn normalize(&self, identity: &RawIdentity) -> Result<NodeId, ObservationError> {
        Ok(match identity {
            RawIdentity::Host { hostname } => NodeId::host(hostname),
            RawIdentity::Process { pid } => NodeId::process(*pid),
            RawIdentity::Service { unit } => NodeId::service(unit),
            RawIdentity::TcpEndpoint { ip, port } => NodeId::port_tcp(ip, *port)?,
            RawIdentity::File { path } => NodeId::file(path),
            RawIdentity::Cgroup { path } => NodeId::cgroup(path),
        })
    }
}
```

Normalization is a thin dispatcher onto the `twin-core` canonical constructors — the canonicalization rules (IP, path, unit) live with the IDs in `twin-core` so every future producer normalizes identically. A bad IP surfaces as `ObservationError::Normalize`.

### 6.8 `pipeline.rs`

```rust
pub struct Pipeline<R: Redactor> {
    redactor: R,
    normalizer: Normalizer,
}

impl<R: Redactor> Pipeline<R> {
    pub fn new(redactor: R) -> Self {
        Self { redactor, normalizer: Normalizer }
    }

    pub fn process(&self, mut raw: RawObservation) -> Result<Observation, ObservationError> {
        let redaction_state = self.redactor.redact(&mut raw);
        let subject = raw.subject.as_ref().map(|i| self.normalizer.normalize(i)).transpose()?;
        let object = raw.object.as_ref().map(|i| self.normalizer.normalize(i)).transpose()?;
        Ok(Observation::new(
            ObservationId::new(),
            raw.source,
            raw.kind,
            subject,
            object,
            raw.timestamp,
            raw.raw_ref,
            raw.confidence_hint,
            redaction_state,
            raw.metadata,
        ))
    }
}

impl Default for Pipeline<BasicRedactor> {
    fn default() -> Self { Self::new(BasicRedactor) }
}
```

`Pipeline` is generic over the redactor so tests can inject a no-op or a strict one, and slice 19/23 can compose specialized redactors — without a `Box<dyn>` (a generic suffices; `AGENTS.md`: "Trait objects only when dispatch needed at runtime"). The order is **redact then normalize**: redaction operates on raw metadata before identities become canonical IDs, matching the tech document's pipeline diagram.

### 6.9 `error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("normalization failed: {0}")]
    Normalize(#[from] twin_core::ParseError),
    #[error("metadata is not a json object: {source}")]
    Metadata { source: serde_json::Error },
}
```

### 6.10 `Cargo.toml`

```toml
[package]
name = "twin-observation"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
twin-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

---

## 7. `twin-store` — typed observation bridge

### 7.1 Conversions

Add to `repo/observation.rs` (or a sibling `repo/observation_convert.rs` if it crowds the file):

```rust
use twin_core::{NodeId, ObservationId, TimestampNs};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RedactionState,
};

const RAW_REF_KEY: &str = "__raw_ref";

impl From<&Observation> for ObservationRow {
    fn from(obs: &Observation) -> Self {
        let mut metadata = obs.metadata().clone();
        if let Some(raw_ref) = obs.raw_ref() {
            metadata.insert_str(RAW_REF_KEY, raw_ref.as_str());
        }
        ObservationRow {
            id: obs.id().to_string(),
            source: obs.source().to_string(),
            kind: obs.kind().to_string(),
            subject_node_id: obs.subject().map(|n| n.as_str().to_string()),
            object_node_id: obs.object().map(|n| n.as_str().to_string()),
            timestamp_ns: obs.timestamp().as_i64(),
            confidence_hint: obs.confidence_hint().to_string(),
            redaction_state: obs.redaction_state().to_string(),
            metadata_json: metadata.to_json(),
            collector_run_id: None,
        }
    }
}

impl TryFrom<&ObservationRow> for Observation {
    type Error = StoreError;
    fn try_from(row: &ObservationRow) -> Result<Self, Self::Error> {
        // parse id/source/kind/confidence/redaction via FromStr,
        // node ids via FromStr, lift __raw_ref out of metadata,
        // rebuild Observation through its pub(crate) typed constructor exposed for the store.
    }
}
```

`collector_run_id` stays `None` this slice (no collector runs are created until slice 4 wires a real collector). The `__raw_ref` key is stripped back out of metadata in `TryFrom` and rehydrated into `Observation.raw_ref`, so the round-trip is lossless.

Because `Observation`'s constructor is `pub(crate)` in `twin-observation`, the store reconstructs it through a small dedicated factory the observation crate exposes for persistence: `Observation::from_parts(...)` (a deliberate, documented seam — the only public constructor besides the pipeline). This keeps "observations are built by the pipeline" true for normal code while allowing faithful rehydration from storage.

### 7.2 Typed `Store` methods

Thin ergonomic wrappers in `repo/observation.rs`:

```rust
impl Store {
    pub fn insert_observation_typed(&mut self, obs: &Observation) -> Result<(), StoreError> {
        self.insert_observation(&ObservationRow::from(obs))
    }

    pub fn get_observation_typed(&self, id: ObservationId) -> Result<Option<Observation>, StoreError> {
        match self.get_observation(&id.to_string())? {
            Some(row) => Ok(Some(Observation::try_from(&row)?)),
            None => Ok(None),
        }
    }
}
```

The existing `Row`-level API is untouched. New `StoreError` variant for parse failures during `TryFrom`:

```rust
#[error("cannot decode stored observation: {detail}")]
Decode { detail: String },
```

### 7.3 `Cargo.toml`

```toml
[dependencies]
rusqlite = { workspace = true }
thiserror = { workspace = true }
twin-core = { workspace = true }
twin-observation = { workspace = true }   # NEW
```

---

## 8. Test Plan

### 8.1 `twin-core` (`cargo test -p twin-core`)

| Test (file) | Proves |
|-------------|--------|
| `node_id.rs::process_pid` | `NodeId::process(8841)` == `process:pid:8841` |
| `node_id.rs::file_lexical_canonical` | `/etc/nginx/../nginx/nginx.conf` → `file:/etc/nginx/nginx.conf`; no filesystem access |
| `node_id.rs::port_ip_normalized` | `127.000.000.001`,`5432` → `port:tcp:127.0.0.1:5432` |
| `node_id.rs::port_bad_ip` | invalid IP → `ParseError::IpAddr` |
| `node_id.rs::service_strips_slice` | `system.slice/nginx.service` → `service:nginx.service` |
| `node_id.rs::kind_roundtrip` | `NodeId::process(..).kind() == Some(NodeKind::Process)` |
| `ids.rs::timestamp_roundtrip` | `TimestampNs::new(n).as_i64() == n`; `now()` is non-negative |
| `ids.rs::observation_id_str_roundtrip` | `ObservationId` → `Display` → `FromStr` equal; bad string errors |
| `enums.rs::all_variants_roundtrip` | every `NodeKind`/`NodeState`/`EdgeKind`/`EdgeClass`/`EdgeState` variant: `Display` → `FromStr` equal; unknown string → `ParseError::Enum` |

### 8.2 `twin-observation` (`cargo test -p twin-observation`)

| Test (file) | Proves |
|-------------|--------|
| `redaction.rs::connection_string` | `DATABASE_URL=postgres://user:pass@host:5432/db` → value redacted, `maybe_endpoint=host:5432` kept, state `Redacted` (the tech-doc example) |
| `redaction.rs::sensitive_keys` | keys containing `token`/`secret`/`password` redacted; `value_redacted=true` added |
| `redaction.rs::keeps_safe_fields` | env *names*, paths, addresses untouched; state `None` when nothing matched |
| `normalization.rs::each_identity` | every `RawIdentity` variant normalizes to the expected canonical `NodeId` |
| `normalization.rs::bad_ip_errors` | `RawIdentity::TcpEndpoint` with bad IP → `ObservationError::Normalize` |
| `pipeline.rs::end_to_end` | `RawObservation` (with a secret + a pid subject) → `Observation` with normalized subject, redacted metadata, `RedactionState::Redacted`, fresh `ObservationId` |
| `pipeline.rs::no_identities` | subject/object `None` flow through as `None` |

### 8.3 `twin-store` (`cargo test -p twin-store`)

| Test (file) | Proves |
|-------------|--------|
| `repo.rs::observation_typed_roundtrip` | build `Observation` via `Pipeline`, `insert_observation_typed`, `get_observation_typed`, assert equal (incl. `raw_ref` rehydrated from metadata) |
| `repo.rs::typed_insert_visible_to_row_api` | `insert_observation_typed` then `get_observation` (Row API) shows expected source/kind strings and `__raw_ref` in `metadata_json` |
| `repo.rs::decode_bad_row_errors` | a row with an unparseable `source` → `StoreError::Decode` from `get_observation_typed` |

All tests use `Store::open_in_memory()` + `initialize()` (slice-2 `support::blank_store`). No host mutation, no real `/proc`.

### 8.4 Workspace gates

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo test core        # working-demo command
cargo run -- doctor --json
```

---

## 9. Workspace Changes

### 9.1 Root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/twin-cli",
    "crates/twin-app",
    "crates/twin-core",
    "crates/twin-store",
    "crates/twin-observation",   # NEW
]

[workspace.dependencies]
twin-core = { path = "crates/twin-core" }
twin-store = { path = "crates/twin-store" }
twin-app = { path = "crates/twin-app" }
twin-observation = { path = "crates/twin-observation" }   # NEW
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "1"
dirs = "5"
uuid = { version = "1", features = ["v4", "serde"] }   # NEW
tempfile = "3"
```

`uuid` was named in slice 1's workspace table but never actually added (slice 2 kept `observations.id` as plain TEXT). It lands now because `ObservationId(Uuid)` needs it.

---

## 10. Implementation Sequence

Build bottom-up, verify after each step.

### Step 1 — `twin-core` vocabulary
1. Add `uuid` to workspace + `twin-core` deps.
2. `id.rs` (`TimestampNs`, `ObservationId`, `CollectorName`), `node.rs` (`NodeId` + ctors + `NodeKind`/`NodeState`), `edge.rs` (`EdgeId` + `EdgeKind`/`EdgeClass`/`EdgeState`), extend `error.rs` with `ParseError`.
3. Re-export from `lib.rs`.
4. Tests `tests/ids.rs`, `tests/node_id.rs`, `tests/enums.rs`.
5. `cargo test -p twin-core && cargo clippy -p twin-core -- -D warnings`.

### Step 2 — `twin-observation` crate
1. `cargo new --lib crates/twin-observation`; add to workspace members; set deps.
2. `vocab.rs`, `raw.rs`, `observation.rs` (incl. `ObservationMetadata`, `Observation::from_parts`), `error.rs`.
3. `redact.rs` (`Redactor` + `BasicRedactor`), `normalize.rs` (`Normalizer`), `pipeline.rs` (`Pipeline`).
4. Tests `tests/redaction.rs`, `tests/normalization.rs`, `tests/pipeline.rs`.
5. `cargo test -p twin-observation && cargo clippy -p twin-observation -- -D warnings`.

### Step 3 — `twin-store` bridge
1. Add `twin-observation` dep.
2. `From<&Observation> for ObservationRow`, `TryFrom<&ObservationRow> for Observation`, `StoreError::Decode`.
3. `insert_observation_typed`, `get_observation_typed`.
4. Extend `tests/repo.rs`.
5. `cargo test -p twin-store && cargo clippy -p twin-store -- -D warnings`.

### Step 4 — Workspace verification
Run the §8.4 gates. Confirm `twin doctor --json` is unchanged (schema still v2) and `cargo test core` passes.

### Step 5 — Update `docs/state/slice-03.md`
Status, implemented items, decisions (esp. the §4.1 deferral of risk/evidence and §4.5 `raw_ref` folding), deviations, and the acceptance-criteria checklist.

---

## 11. Acceptance Criteria

From `plan-slices.md` Slice 3:

- [ ] All domain-critical concepts use newtypes/enums (IDs, kinds, states, classes, sources)
- [ ] No raw stringly-typed graph core (canonical `NodeId`/`EdgeId` constructors; enums for every kind/state/class)
- [ ] Observations can be stored and queried (`insert_observation_typed` / `get_observation_typed` round-trip)
- [ ] Redaction hook exists even if simple (`Redactor` trait + `BasicRedactor`)
- [ ] Unit tests cover IDs and normalization

From `AGENTS.md`:

- [ ] No `unwrap()` / `expect()` in non-test code
- [ ] No `todo!()` / `unimplemented!()`
- [ ] No `dead_code` / `clippy` allow attributes (every type has a consumer or a documented near-term one)
- [ ] Newtype IDs with `Display` + `FromStr`; enums for domain kinds
- [ ] Struct fields private with accessors (except the data-only `RawObservation`)
- [ ] Errors via `thiserror`, scoped per crate, no `anyhow` in libraries
- [ ] No OS/eBPF/k8s/container imports in `twin-core`
- [ ] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` all pass

---

## 12. Design Decisions Log

| Decision | Choice | Alternatives | Rationale |
|----------|--------|--------------|-----------|
| Risk/evidence types | Deferred to slice 8 | Implement now (per plan-slices list) | No impact/scoring consumer until slice 8; slice 1 already scheduled them there; avoids dead code. |
| Edge vocabulary now | `EdgeId`/`EdgeKind`/`EdgeClass`/`EdgeState` shipped | Defer to slice 4 | Slice 1 scheduled them for slice 3; they are the graph "language" the slice is chartered to define and slice 4 consumes immediately; exercised via round-trip + `EdgeId` ctor tests. |
| Observation enums home | `twin-observation` | `twin-core` | They are observation-pipeline vocabulary, not graph-core; keeps `twin-core` minimal and the observation crate self-contained. |
| Store→observation dependency | Yes | Conversions in `twin-app` | Slice 2 pre-announced store-side conversions; repository-knows-entity is standard; domain never depends on store. |
| `raw_ref` storage | Fold into `metadata_json` | Add `MIGRATION_003` column | Slice 2 designated `metadata_json` the catch-all; one nullable field doesn't justify a migration; round-trip test guards fidelity. |
| Metadata type | Newtype over `serde_json::Map` | Stringly blob / per-kind structs | Deterministic ordering, direct column serialization, typed helpers, no premature struct explosion. |
| `NodeId` construction | Canonical constructors only | Free `NodeId::new(String)` | Structurally enforces "no raw stringly-typed graph core"; centralizes normalization. |
| Path canonicalization | Lexical (string) | `std::fs::canonicalize` | Read-only purity; paths may not exist; no host touch. |
| Pipeline redactor | Generic `Pipeline<R: Redactor>` | `Box<dyn Redactor>` | Static dispatch suffices; trait objects only when runtime dispatch is needed. |
| Pipeline async | Sync | Async/tokio | No external API calls; async reserved for collectors/watch/eBPF. |
| Initial enum variants | Small seed set, additive | Full tech-doc enums | Matches store's additive schema philosophy; variants land with the slice that produces them; avoids dead variants. |
| `twin-fixtures` | Not yet | Create now | No real collector to feed; tests build `RawObservation` inline. |

---

## 13. File List (after slice 3)

```text
crates/twin-core/src/
  lib.rs            # + pub mod id, node, edge; re-exports
  error.rs          # + ParseError
  id.rs             # NEW: ObservationId, TimestampNs, CollectorName
  node.rs           # NEW: NodeId (+ ctors), NodeKind, NodeState
  edge.rs           # NEW: EdgeId (+ ctor), EdgeKind, EdgeClass, EdgeState
  config/ …         # unchanged
crates/twin-core/tests/
  config.rs         # unchanged
  ids.rs node_id.rs enums.rs   # NEW

crates/twin-observation/        # NEW CRATE
  Cargo.toml
  src/lib.rs error.rs vocab.rs raw.rs observation.rs redact.rs normalize.rs pipeline.rs
  tests/redaction.rs normalization.rs pipeline.rs

crates/twin-store/src/
  repo/observation.rs   # + From/TryFrom, insert_observation_typed, get_observation_typed
  error.rs              # + StoreError::Decode
  Cargo.toml            # + twin-observation
crates/twin-store/tests/
  repo.rs               # + typed observation round-trip tests

Cargo.toml              # + twin-observation member/dep, + uuid
docs/state/slice-03.md  # updated on completion
```

---

## 14. What This Slice Does NOT Ship (vocabulary deferrals)

| Type / variant | Slice | Reason |
|----------------|-------|--------|
| `RiskLevel`, `EvidenceStrength`, `EvidenceLabel` | 8 | No risk/impact/scoring consumer |
| `Node`, `Edge` structs; node/edge typed row conversions | 4 | Slice 3 produces observations, not entities |
| Remaining `NodeKind` variants (`Container`, `Image`, `K8s*`, …) | 20–23 | Added with the producing collector |
| Remaining `EdgeKind` variants (`ProxiesTo`, `RoutesTo`, `Selects`, …) | 7+ | Added with inference/adapters |
| Remaining `ObservationSource`/`ObservationKind` variants | 4+ | Added per collector |
| `Collector` trait, `CollectorBatch`, `CollectorCoverage` | 4 | No collector implementations yet |
| `SnapshotId`, `TestRunId`, `EmulationRunId` | 12 / 17 / 9 | No consumers yet |
| `twin-fixtures` | 4 | First real collector needs fake `/proc` |
