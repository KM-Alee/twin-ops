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
- Userspace exec decoder: exactly 40 little-endian bytes (`pid`, `ppid`, `cgroup_id`, `timestamp_ns`, `comm[16]`) → `EbpfExec` → `RawObservation` (`ObservationSource::Ebpf`, `ObservationKind::EbpfExecObserved`, subject `process:pid:<pid>`). Comm stops at the first NUL and is the task name only. Metadata is `ppid`, `comm`, and `cgroup_id`. No argv, env, uid, or credentials.
- Aya 0.13.1 loader (`attach` / `attach_from_bytes` / `ExecSession::poll_exec` / `detach`). `Drop` drops the tracepoint link before the loader so the hook is released on every exit path. Empty or non-ELF objects return `EbpfError::Unavailable`. `ProgramError::LoadError` returns `EbpfError::Verifier`. No second backend, no feature flag, no libbpf.
- `twin doctor --ebpf` adds an eBPF section with the four checks. Failures use `warn`. The command still exits 0. `twin doctor` without `--ebpf` omits the section (JSON omits `ebpf`).
- Polling `twin watch` (`--interval`, `--duration`, `--ticks`, Ctrl+C). Exec lines are an extra stream beside tick summaries. `--events` accepts `exec` only; `tcp` and any other name are a typed error. `--ebpf` defaults to exec. If attach or the checks fail, watch prints the warning and keeps scanning. Exec events are not stored on `WatchResult`.
- Human renderers in `twin-cli/src/output/` (`doctor.rs`, `watch.rs`) using `output/format.rs`. JSON via `--json`.
- Safety scan of `crates/twin-ebpf/src` rejects `bpf_override_return`, `BPF_PROG_TYPE_LSM`, and `bpf_send_signal`.

## Decisions

- Aya is the only loader. There is no `EbpfBackend` trait because a second backend does not exist.
- No `aya-ebpf` probe crate and no embedded object. `cargo test --workspace` must pass without root, `CAP_BPF`, or `bpf-linker`. A workspace member probe crate would be built by that command. `attach()` returns unavailable until a program object is passed to `attach_from_bytes`.
- Kernel cutoff is 5.8 because that is when the BPF ring buffer exists.
- Doctor does not load or attach. It only reports the four checks. Verifier rejection is a watch-time warning from Aya.
- Watch is synchronous. Ctrl+C is an atomic flag installed in `twin-cli`; the library loop polls it and drops the session on the way out.
- `cgroup_id` is recorded on the observation and is not resolved to a service or cgroup node.
- Exec observations go through the existing pipeline in the app process and are rendered immediately. They are not written to the store and do not change evidence scores, impact, or emulate.
- First watch tick is a baseline of process, TCP connection, and listening-port counts. Later ticks diff active process ids and net connection/listener counts.
- Slice 13 was not in the tree. This slice adds the polling loop the exec stream sits beside. It does not add collector-timing metrics or a memory-leak harness.

## Deviations from Plan

- Tech document §11.1 lists `libbpf_backend.rs`, `tcp.rs`, `file.rs`, `dns.rs`, and `rate_limit.rs`. Those files are not created.
- Tech document §11.3 `ExecEvent` includes `uid`. This slice omits it. The event is `EbpfExec { pid, ppid, comm, cgroup_id, timestamp }`.
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
- [x] `--events tcp` is a typed error
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
