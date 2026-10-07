# Slice 13: Polling Watch Mode

## Status: complete

## Implemented

- `twin watch` rescans on an interval and prints an operational change stream
- Flags: `--interval` (default `5s`, `0s` allowed), `--duration`, `--ticks`
- Stop reasons: Ctrl+C (`ctrlc` in `twin-cli` only), duration elapsed, tick limit
- Each tick runs the existing sync scan (1 sample) and reads `node_history` / `edge_history` recorded during that scan
- Significant stream: processes, process `connects_to` connections, listening ports, services, plus stale counts for those kinds
- Quiet ticks print `no significant changes`
- Event counter is the sum of those deltas across the session
- Collector timings come from `collector_runs` rows started during the tick (`list_collector_runs_since`); the summary shows totals
- Human output: title, stop plan, tick lines, stopped summary. JSON is one object per line (`record: tick|summary`)
- Library loop keeps only counters and cumulative collector timings, not every scan result
- Tests: `twin-app/tests/watch.rs`, `twin-store` collector-run since query, `twin-cli` output + CLI integration including SIGINT

## Decisions

- Polling watch stays sync. `thread::sleep` plus an `AtomicBool`. No tokio. eBPF watch remains the async case
- `ctrlc` is installed only in the `twin` binary so library tests can stop the loop without a process-wide handler
- The stop flag is polled every 100ms during the interval so Ctrl+C does not wait out the full sleep
- An in-flight scan finishes before the loop exits
- `--ticks` is a deterministic bound in addition to `--duration`
- File, cgroup, and parent-edge churn stays in history and is omitted from the tick line
- Connection counts use process `connects_to` edges so an inferred service edge is not a second connection
- `0s` is a valid interval (back-to-back scans). A zero duration is rejected

## Deviations from Plan

- No eBPF (`twin watch --ebpf` is slice 14)
- `--ticks` is extra relative to the plan's interval/duration demo
- JSON is newline-delimited tick and summary records rather than one document, so a long watch does not buffer every tick
- Tokio was not added; the tech-doc dependency list names it for later async streams

## Acceptance Checklist

- [x] `twin watch --interval 5s` runs a scan loop
- [x] Watch stops cleanly on Ctrl+C, `--duration`, and `--ticks`
- [x] Repeated scans update the graph without duplicating active nodes or flooding history on identical rescans
- [x] Quiet rescans do not grow retained tick state; RSS across quiet scans stays bounded in the fixture test
- [x] The user sees a change stream (processes, connections, listening ports) plus collector timing
- [x] Missing database and bad interval/duration/tick count return typed errors
- [x] No host mutation paths in the watch command
- [x] Tests use isolated HOME, temp stores, and fake `/proc` fixtures
