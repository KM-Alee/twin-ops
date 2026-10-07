# Slice 16: Evidence Scoring, Unknowns, and Coverage Model

## Status: complete

## Implemented

- `twin_core::score_evidence` is the 0–100 formula behind `EvidenceStrength`. Inputs are source type, runtime confirmation, static confirmation, recency, repeat count, independent source count, permission gaps, conflicting evidence, and dropped eBPF events. The same function explains the score with stable why-lines.
- `connect_evidence_strength` uses that formula. Repeated eBPF connects score 86 (very strong). One eBPF connect scores 70 (strong). A socket-table connection scores 75 (strong). A config-only dependency scores 45 (moderate). An `ebpf_tcp` dropped-event observation subtracts 16 from an eBPF claim only (86 → 70 strong, 70 → 54 moderate). A socket-table claim is unchanged when the drop flag is set.
- Report scores run the formula and then the existing caps: no linked observations min 30, weakening unknowns min 60, inferred-only min 85. The inferred-only cap does not apply when the formula source is eBPF, so a repeated eBPF connect can stay very strong.
- Recency is `EvidenceRecency::from_timestamps(observed_ns, reference_ns)`. Fresh is an age of at most 10 minutes (+4). Recent is at most 60 minutes (+2). Older is stale (+0). A missing timestamp is unknown (+0). The reference is the latest `proc_process` collector run `ended_at_ns`. Nothing reads the wall clock.
- Coverage on `ScanResult` and reloaded for impact and emulate:
  - readable processes
  - restricted processes (process, fd, and cgroup permission denials)
  - unmapped sockets (TCP listener, active TCP, unix listener, unix connection)
  - unavailable collectors (`proc_net_tcp`, `proc_net_unix`, `systemd_dbus` when that input is missing)
  - eBPF availability (kernel, BTF, and capabilities)
  - Docker availability (`/var/run/docker.sock` exists)
  - Kubernetes availability (service-account token path or kubeconfig path exists)
- Docker and Kubernetes are existence checks only. No collector, adapter, or API call.
- `twin scan` prints a coverage section and unknown lines for hidden processes, unmapped sockets, and unavailable collectors.
- `twin impact <target> --evidence` and `twin emulate restart|delete <target> --evidence` print `N/100, <words>` plus the why-lines. Unknowns are on the report either way.
- Coverage unknowns use `weakens_evidence: false`. They lower the formula through `permission_gaps` and do not change risk traversal or the overlay. Edge-level missing evidence still uses the 60 cap.

## Decisions

Weights, then clamp to 0–100:

| Input | Points |
| --- | --- |
| source none | 15 |
| source config | 45 |
| source socket table | 75 |
| source eBPF | 70 |
| runtime confirmation on an eBPF claim (socket inode mapped) | +5 |
| static confirmation when the source is not already config (process mapped to service) | +4 |
| recency fresh / recent | +4 / +2 |
| repeat count ≥ 2 | +16 |
| each independent source beyond the primary, runtime, and static confirmations already counted | +3, at most +6 |
| each permission gap | −1, at most −8 |
| conflicting evidence | −20 |
| dropped eBPF events on an eBPF claim | −16 |

The plan's example is this row: eBPF 70 + socket inode 5 + process mapped to service 4 + repeated 16 − 8 permission gaps = **87, very strong**. The three independent sources are already counted by the source, the runtime bonus, and the static bonus, so they add nothing further. Why-lines are `eBPF connect observed`, `socket inode mapped to process`, `process mapped to service`, `relationship observed repeatedly`, and `permission gaps reduce confidence`.

Restricted processes are the sum of `permission_denied`, `fd_permission_denied`, and `cgroup_permission_denied` warnings. Unmapped sockets are the four unmapped counters already stored on the process collector run.

## Deviations from Plan

- No second score type. Labels stay on `EvidenceStrength` (0–30 weak, 31–60 moderate, 61–85 strong, 86–100 very strong).
- The inferred-only cap of 85 still applies to socket and config claims. It does not apply when the formula source is eBPF.
- Coverage summary lines do not set `weakens_evidence`. Edge-level unmapped sockets on a target still do, and still cap that report at 60.
- Conflicting evidence is a formula input. No stored observation currently sets it.
- Docker and Kubernetes are path checks. Collectors for them are later slices.
- Recency is measured against the stored scan end, not `now()`.
- Slice 17 is not implemented.

## Acceptance Checklist

- [x] Evidence scores are deterministic for the same factors and the same stored timestamps
- [x] Repeated eBPF connects are very strong (86). One eBPF connect is strong (70). A socket-table connection is strong (75). A config-only dependency is moderate (45)
- [x] A dropped eBPF event weakens an eBPF claim by one band and leaves a socket-table claim strong
- [x] The documented full confirmation with 8 permission gaps scores 87 / very strong and explains why
- [x] `--evidence` on impact and emulate shows the score and the why-lines
- [x] Unknowns and coverage appear on scan. The same unknown lines appear on impact and emulate
- [x] Missing permissions reduce the score and do not crash
- [x] Risk traversal and overlay actions are unchanged
- [x] `cargo fmt --check` passes
- [x] `cargo test --workspace` passes (401 tests)
- [x] `cargo clippy --workspace -- -D warnings` passes

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All passed on implementation completion (401 tests). No root, `CAP_BPF`, or `bpf-linker`.
