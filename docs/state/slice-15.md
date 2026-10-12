# Slice 15: eBPF TCP Runtime Dependency Evidence

## Status: complete

## Implemented

- Userspace TCP decoders in `twin-ebpf` (`tcp.rs`). Fixed little-endian layouts, rejected unless the length matches:
  - connect: 56 bytes → `TcpConnect { pid, cgroup_id, saddr, daddr, dport, timestamp_ns }`
  - accept: 58 bytes → `TcpAccept { pid, cgroup_id, saddr, sport, daddr, dport, timestamp_ns }`
  - bind: 40 bytes → `TcpBind { pid, cgroup_id, addr, port, timestamp_ns }`
  - address family byte `4` or `6`, then a 16-byte address. Anything else, including a short buffer, is `EbpfError::Malformed`.
- Each event becomes one `RawObservation` with `ObservationSource::Ebpf` and collector `ebpf_tcp`:
  - `EbpfConnect` on `process:pid:<pid>` toward `port:tcp:<daddr>:<dport>`
  - `EbpfAccept` on `process:pid:<pid>` toward the local port
  - `EbpfBind` on `process:pid:<pid>` toward the bound port
  - Metadata is `cgroup_id` only. No payload, argv, env, or credentials. No cgroup-to-service mapper.
- `TcpRateLimiter` admits on three dimensions before anything is stored: per PID, per endpoint, and global. Session defaults are 64 / 128 / 512. A dropped event increments one counter. `sync_drops` writes a single `EbpfDroppedEvents` observation (`source=ebpf_tcp`, `count=N`) and replaces the previous dropped row instead of inserting one row per event.
- `TcpIngestor` in `twin-app` persists admitted observations and upserts edges:
  - process `CONNECTS_TO` the remote port (observed)
  - when that process already has a service `OWNS` edge, service `CONNECTS_TO` the listener service if an existing `LISTENS_ON` matches the remote port (same wildcard candidates as the socket-table path), otherwise the remote port
- Evidence stays on the existing labels. Two or more stored eBPF connects → `EvidenceStrength::very_strong`. One eBPF connect, or a socket-table connection with no eBPF connects → `strong`. A config-only dependency → `moderate`. An `ebpf_tcp` dropped-event observation steps an eBPF claim down one label (`very_strong` → `strong`). Socket-table edges with no eBPF connects stay `strong`.
- `twin graph <service> --evidence` adds a runtime dependency section in text and JSON. Without `--evidence` that section is omitted.
- Impact and emulate read an incoming service `CONNECTS_TO` edge in the existing direct-dependent evidence section and print the eBPF reason. Service CONNECTS_TO now participates in bounded transitive path walking and restart overlays.
- `twin watch --ebpf --events tcp` prints `[tcp] process:pid:… connect|accept|bind <endpoint>` beside tick summaries. `--events exec` is unchanged. `--ebpf` with no `--events` still means exec. Any other name is `WatchError::UnsupportedEvent`. Attach failure warns and the polling loop continues. `TcpSession` drops its tracepoint link on every exit path, including Ctrl+C and errors.
- `twin doctor --ebpf` adds a tcp tracing check (`inet_sock_set_state` present). Exec readiness does not require it. `twin doctor` without `--ebpf` does not add the eBPF section.
- Aya 0.13.1 remains the only loader. `attach_tcp` returns `EbpfError::Unavailable` until `attach_tcp_from_bytes` is given an object. Verifier rejection stays `EbpfError::Verifier`. No `EbpfBackend`, no feature flag, no libbpf, no `aya-ebpf` workspace member.
- The safety scan of `crates/twin-ebpf/src` still rejects `bpf_override_return`, `BPF_PROG_TYPE_LSM`, and `bpf_send_signal`.

## Decisions

- TCP programs are tracepoints only (`sock/inet_sock_set_state` when an object is loaded). No LSM, packet rewrite, drop, syscall denial, or signal.
- `twin-core` does not depend on `twin-ebpf`. New observation kinds live in `twin-observation`. eBPF types stay in `twin-ebpf`.
- The on-wire family is one byte shared by both addresses in an event. Tests use one family per event.
- Rate-limit windows are counters for the watch session, not per second. Slice 16 owns recency.
- `N` in “eBPF observed N connect events” is the number of connect observations stored after the rate limit.
- Service-to-service runtime evidence is a `CONNECTS_TO` edge, which is what `twin graph --evidence` prints. The socket-table path’s separate `DEPENDS_ON` edge is not created for these events.
- A dropped-event row is global for `ebpf_tcp`. It weakens eBPF connect claims and does not lower a socket-table edge that has no eBPF connects.
- Doctor reports tcp tracing separately so a missing TCP tracepoint does not change `twin watch --ebpf --events exec`.

## Deviations from Plan

- Tech document §11.1 lists `libbpf_backend.rs`, `file.rs`, and `dns.rs`. Those files are not created.
- Tech document §11.5 also lists per-event-type and per-cgroup limits. This slice limits per PID, per endpoint, and global.
- Tech document §11.3 shows `IpAddr` fields on the event struct. The bytes on the ring are a bounded family plus 16-byte addresses.
- No embedded TCP object and no `aya-ebpf` probe crate. `cargo test --workspace` does not need root, `CAP_BPF`, or `bpf-linker`. Real attach degrades to the existing unavailable error.
- The plan’s “842 connect events in 1h” is the stored count, not a one-hour window.
- `twin doctor --ebpf` shows the tcp tracing check in addition to the slice 14 checks.

## Acceptance Checklist

- [x] Decoder rejects a short buffer and a bad address family, and accepts one valid connect, accept, and bind event
- [x] Connect observation is `EbpfConnect` from `process:pid:<pid>` to `port:tcp:<daddr>:<dport>`, source `ebpf`, with `cgroup_id` and no payload
- [x] Rate limit drops the excess. One `EbpfDroppedEvents` row carries `source=ebpf_tcp` and the count. Storage does not grow one row per dropped event
- [x] An injected connect creates process `CONNECTS_TO`. With an existing owns edge and a listener, it creates service `CONNECTS_TO` the listener service
- [x] Repeated stored connects are `very_strong`. A dropped-event observation weakens that claim to `strong`
- [x] A socket-table connection stays `strong`. A config-only dependency stays `moderate`
- [x] `twin watch --ebpf --events tcp` prints `[tcp] process:pid:…` from an injected event and stops on `--ticks`
- [x] Unavailable eBPF warns and polling ticks still print. `--events exec` still prints `[exec]`
- [x] `--events dns` (and any name other than `exec` or `tcp`) is a typed error. `--ebpf` still defaults to exec
- [x] `twin graph <service> --evidence` text and JSON include the runtime dependency section. Without the flag the section is absent
- [x] Impact and emulate mention the eBPF evidence when the service `CONNECTS_TO` edge exists
- [x] Doctor without `--ebpf` has no eBPF section. Doctor `--ebpf` renders tcp tracing
- [x] Source scan still rejects `bpf_override_return`, `BPF_PROG_TYPE_LSM`, and `bpf_send_signal`
- [x] Aya load of a non-ELF buffer returns a typed unavailable or verifier error
- [x] Existing watch, doctor, graph, impact, and emulate tests pass
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes (383 tests)
- [x] `cargo clippy --workspace -- -D warnings` passes

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All passed on implementation completion (383 tests). No root, `CAP_BPF`, or `bpf-linker`.

## 2026-10-08 follow-up: eBPF history and transitive reasoning

TCP ingestion routes process/port nodes and connect edges through ScanHistorySession without finalizing unrelated host rows. History queries now expose eBPF-only changes. Service CONNECTS_TO participates in default bounded transitive traversal and hypothetical restart overlays. Tests: ebpf_tcp, impact_paths, watch. Per-event-type and per-cgroup limits and bundled probes remain unimplemented.

## Additional 2026-10-08 follow-up

TCP limits now include per-cgroup and per-event-kind dimensions alongside PID, endpoint, and global session counters. Rejected events do not spend other budgets. Scans record cgroup-v2 filesystem identities; admitted events join them to existing cgroup/service ownership and retain observation links/history. Tests cover matching and unknown IDs. Live probe producers are still absent.

## 2026-10-09 completeness integration verification

The host-scan/eBPF lifecycle regression now supplies readable empty FD and TCP/UDP/Unix tables before asserting that a previously observed endpoint is Gone. Missing tables or vanished FD directories represent incomplete coverage and cannot establish removal. The test still proves that retained eBPF edges survive while their source process is observed and that a later event reactivates the endpoint. All seven app eBPF integration tests pass; the complete twin-emulate and twin-ebpf suites pass in `/tmp/twin-depth-emulate-ebpf.log`.

## 2026-10-09 real TCP connect producer

Implemented a bundled TCP connect producer attached to `sys_enter_connect`, `sys_exit_connect`, `sched_process_exit` and `inet_sock_set_state`. Thread-exit cleanup prevents dangling syscall markers from contaminating reused thread IDs. SYN_SENT inside a traced connect syscall captures initiator PID/cgroup, leader start ticks, actual socket namespace and bounded IPv4/IPv6 endpoints. Socket-pointer correlation carries that origin until ESTABLISHED; completion can occur in softirq without attributing the connection to the current completion task. This supports nonblocking completion, suppresses failed attempts and removes closed origins. Pending/origin maps are bounded to 16384 entries; map/ring/read failures increment kernel coverage-drop counters.

The live connect ABI extends the 56-byte fixture prefix to 72 bytes, adding socket namespace inode at 56 and start ticks at 64. Decoding validates namespace identity. App ingestion preserves host aliases only when namespace identity matches the scanner, otherwise uses canonical namespace endpoints. Service listener joins stay in the same namespace. Old-generation events cannot overwrite reused PIDs or borrow current ownership; rejected events count toward coverage losses. Existing process metadata is preserved. Kernel losses combine with admission-limit losses and remain scoped to TCP independently of exec losses.

Default exec/TCP attachment now loads the tracked bundled object. Before TCP attachment, tracepoint offsets/sizes are checked against the compiled context ABI. Doctor's live TCP capability assessment additionally requires connect syscall tracepoints. LLVM artifact rebuild/check instructions and ordinary unprivileged object verification are shared with slice 14. Runtime attachment/verifier success is not claimed on the current host (CapEff=0 and unreadable tracepoint formats).

Remaining producer scope: accept and bind have fixture decoders/admission/storage coverage but are not emitted by these live probes. Safe syscall-result/socket ownership producers are still required before claiming live accept/bind coverage. Connections already established before attachment, kernel-originated connects and sessions beginning before the enter hook are not reconstructed; polling is the separate source for existing sockets. Namespace inode reuse without an observed absence remains an identity limit.

PID namespace safety: a private configured map carries the scanner's namespace device/inode to the read-only `bpf_get_ns_current_pid_tgid` helper. Live event TGIDs use scanner-visible PID aliases; tasks outside that namespace cannot borrow matching numeric aliases. Pending syscall keys remain private global thread identifiers. PPID is emitted only where initial-namespace translation is known; unsupported parent translation is explicitly `ppid_known=false`, never a guessed parent. No namespace transition or host mutation is performed.
