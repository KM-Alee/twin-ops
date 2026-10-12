# Slice 14: eBPF Capability Checks and Exec Events

## Status: complete

## Implemented

- Crate `twin-ebpf` (`twin-ebpf` → `twin-core` + `twin-observation` only). `twin-app` calls it. `twin-core` does not depend on it.
- Read-only capability checks, all injectable via `EbpfFacts` / `EbpfProbePaths`:
  - kernel release parsed from text; ring buffer requires 5.8 or newer; empty or unparseable text is unsupported
  - BTF present when the path is a non-empty file (host default `/sys/kernel/btf/vmlinux`)
  - tracing privileges: `CAP_SYS_ADMIN`, or both `CAP_BPF` and `CAP_PERFMON`, from `CapEff`
  - exec tracing: `sched_process_exec` tracepoint path exists
- Missing files, permission errors, and parse failures become unsupported/unavailable checks. They do not panic.
- Userspace exec decoder: exactly 44 little-endian bytes (`pid`, `ppid`, `cgroup_id`, `timestamp_ns`, `comm[16]`, `uid`) → `EbpfExec` → `RawObservation` (`ObservationSource::Ebpf`, `ObservationKind::EbpfExecObserved`, subject `process:pid:<pid>`). Comm stops at the first NUL and is the task name only. Metadata is `ppid`, `comm`, `cgroup_id`, and `uid`. No argv, environment values, or credentials.
- Aya 0.13.1 loader (`attach` / `attach_from_bytes` / `ExecSession::poll_exec` / `detach`). `Drop` drops the tracepoint link before the loader so the hook is released on every exit path. Empty or non-ELF objects return `EbpfError::Unavailable`. `ProgramError::LoadError` returns `EbpfError::Verifier`. No second backend, no feature flag, no libbpf.
- `twin doctor --ebpf` adds an eBPF section with the four checks. Failures use `warn`. The command still exits 0. `twin doctor` without `--ebpf` omits the section (JSON omits `ebpf`).
- Polling `twin watch` (`--interval`, `--duration`, `--ticks`, Ctrl+C). Exec lines are an extra stream beside tick summaries. `--events` accepts `exec` and `tcp`; other names are a typed error. `--ebpf` defaults to exec. If attach or the checks fail, watch prints the warning and keeps scanning. Exec events are not stored on `WatchResult`.
- Human renderers in `twin-cli/src/output/` (`doctor.rs`, `watch.rs`) using `output/format.rs`. JSON via `--json`.
- Safety scan of `crates/twin-ebpf/src` rejects `bpf_override_return`, `BPF_PROG_TYPE_LSM`, and `bpf_send_signal`.

## Decisions

- Aya is the only loader. There is no `EbpfBackend` trait because a second backend does not exist.
- No `aya-ebpf` probe crate and no embedded object. `cargo test --workspace` must pass without root, `CAP_BPF`, or `bpf-linker`. A workspace member probe crate would be built by that command. `attach()` returns unavailable until a program object is passed to `attach_from_bytes`.
- Kernel cutoff is 5.8 because that is when the BPF ring buffer exists.
- Doctor does not load or attach. It only reports the four checks. Verifier rejection is a watch-time warning from Aya.
- Watch is synchronous. Ctrl+C is an atomic flag installed in `twin-cli`; the library loop polls it and drops the session on the way out.
- `cgroup_id` is recorded on the observation and is not resolved to a service or cgroup node.
- Exec observations go through the existing pipeline in the app process and are rendered immediately. They are persisted with process history; they do not yet change dependency evidence scores, impact, or emulate.
- First watch tick is a baseline of process, TCP connection, and listening-port counts. Later ticks diff active process ids and net connection/listener counts.
- Slice 13 was not in the tree. This slice adds the polling loop the exec stream sits beside. Polling timing metrics and a bounded fixture stress test are now recorded in slice 13.

## Deviations from Plan

- Tech document §11.1 lists `libbpf_backend.rs`, `tcp.rs`, `file.rs`, `dns.rs`, and `rate_limit.rs`. Those files are not created.
- ExecEvent includes UID in the 44-byte wire ABI.
- Tech document §5.4 `EbpfBackend` trait is not implemented. One Aya loader is enough.
- No rate limiting, dropped-event observations, TCP connect/accept/bind, evidence-score changes, or service mapping from `cgroup_id` (slice 15).
- `twin-store` `with_transaction` and `scan_cgroup_validate` sort were adjusted so `cargo clippy --workspace -- -D warnings` passes on rustc 1.99. Behavior is unchanged: errors still return before commit, and cgroup prefixes are still longest-first.

## Acceptance Checklist

- [x] `twin doctor --ebpf` text and JSON include kernel, BTF, capabilities, and exec tracing
- [x] `twin doctor` without `--ebpf` does not add the eBPF section
- [x] Each check can fail alone; the report names it and the human line uses `warn`
- [x] Missing files and unparseable kernel text are unsupported, not panics
- [x] Decoder rejects a short or over-long buffer and accepts a 40-byte exec event
- [x] Valid event becomes `EbpfExecObserved` on `process:pid:<pid>` with task-name comm only
- [x] `twin watch --ebpf --events exec` prints `[exec] process:pid:…` from an injected event and stops on `--ticks`
- [x] Unavailable eBPF warns and polling ticks still print
- [x] `--events tcp` is supported by slice 15; unknown event names remain typed errors
- [x] Watch without `--ebpf` stays a polling loop and still stops on `--ticks`
- [x] Ctrl+C is a stop flag; the loop returns and drops the session (Aya link drop detaches)
- [x] Exec payloads are not retained on `WatchResult`
- [x] Source scan rejects enforcement strings
- [x] Aya load of a non-ELF buffer returns a typed unavailable/verifier error and does not panic
- [x] No root, `CAP_BPF`, or `bpf-linker` required for `cargo test --workspace`
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes (369 tests)
- [x] `cargo clippy --workspace -- -D warnings` passes

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All passed on implementation completion (369 tests).

## Manual Smoke Test

On this host, without root:

```text
twin doctor --ebpf
  kernel          ok  supported
  BTF             warn  unavailable (BTF file not present)
  capabilities    warn  unavailable (CAP_BPF and CAP_PERFMON or CAP_SYS_ADMIN required)
  exec tracing    warn  unavailable (sched_process_exec tracepoint not present)
```

Doctor still exits 0. `twin watch --ebpf --events exec --ticks 1` prints the warning and a polling tick, then stops.

## 2026-10-08 follow-up: Watch and exec persistence

Polling watch has collector phase timings, a 256-tick result window, total tick count, and deduplicated bounded warnings. Every tick still streams to the sink. A 260-tick fixture stress test verifies the bound and timing presence. Injected exec observations and process nodes now persist transactionally with history. UID and cgroup attribution are implemented; exec-driven dependency lifecycle reasoning remains unimplemented. No embedded probe object is shipped, so live attach still reports unavailable.

## Additional 2026-10-08 follow-up

Exec wire ABI is now 44 bytes: the original 40 bytes plus UID at offset 40. Encoder/decoder and fixtures agree; old-length payloads fail gracefully. UID is safe identity metadata; argv/environment/credential values remain excluded. Exec observations persist and can attribute newly seen processes through a uniquely matched stored cgroup-v2 kernel ID. Unresolved or ambiguous IDs do not borrow an old PID ownership claim.

## 2026-10-09 bundled producer and exec lifecycle

Implemented a bundled read-only C/BPF `sched_process_exec` tracepoint producer. It emits bounded comm, UID, PID/PPID, cgroup ID, monotonic capture timestamp, process-leader start ticks and network namespace inode. It never reads argv, environment, filenames or payloads. CO-RE reads isolate kernel layout differences; unknown fields, unavailable capabilities, missing BTF or failed verifier/attachment return typed errors and retain polling.

The live exec ABI is 60 bytes: the compatible 44-byte prefix plus namespace inode at offset 44 and start ticks at 52. Legacy 44-byte fixture decoding remains supported. Session clocks translate monotonic capture timestamps to epoch nanoseconds with an isolated `clock_gettime` boundary; attachment validates little-endian 64-bit Linux and USER_HZ=100. Translation uses a bracketed attachment-time clock sample; subsequent wall-clock adjustments are an explicit timestamp limitation.

Exec ingestion preserves boot/start metadata, rejects old-generation events, resets changed process generations and retains source observations. Same-generation exec makes previously observed process runtime sockets/libraries and exclusively sourced service runtime connections stale, because descriptor survival is not known. It does not claim all sockets closed. Other processes' supporting evidence prevents broad service invalidation. The next complete poll can establish current relationships. Kernel losses become source-scoped `ebpf_exec` drop observations; TCP drop cleanup never removes them. Final shutdown removes hooks before draining the remaining bounded ring.

Artifact build: `python3 tools/build_ebpf.py`; verification: `python3 tools/build_ebpf.py --check`. The source/object SHA-256 manifest accompanies the tracked ELF. No build script executes a compiler during normal Rust builds. `aya-obj` dev tests parse ELF/BTF, validate ring/drop map sizes and bound all telemetry maps; C helper safety tests reject mutation helpers. Ordinary fixture tests require no root or host attachment.

Live verification limitation observed on this session: `/proc/self/status` reports CapEff=0; BTF exists, but tracepoint format files are unavailable. No successful live attachment or verifier acceptance is claimed. Tests verify object parsing, ABI, graceful errors, generation safety, lifecycle invalidation and default attachment code paths. Kernel attach validation remains a capability-gated release check.

PID namespace safety: a private configured map carries the scanner's namespace device/inode to the read-only `bpf_get_ns_current_pid_tgid` helper. Live event TGIDs use scanner-visible PID aliases; tasks outside that namespace cannot borrow matching numeric aliases. Pending syscall keys remain private global thread identifiers. PPID is emitted only where initial-namespace translation is known; unsupported parent translation is explicitly `ppid_known=false`, never a guessed parent. No namespace transition or host mutation is performed.
