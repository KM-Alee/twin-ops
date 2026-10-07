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
- Impact and emulate read an incoming service `CONNECTS_TO` edge in the existing direct-dependent evidence section and print the eBPF reason. Path walking and overlay actions are unchanged.
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
