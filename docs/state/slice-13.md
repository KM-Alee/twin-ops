# Slice 13: Polling Watch Mode

## Status: implemented with bounded fixture stress coverage

## Implemented

- Synchronous polling with interval, duration, tick limit, stop flag, and injected sink.
- First tick establishes a baseline; subsequent ticks report process/socket changes.
- NDJSON streams every tick. Returned results retain the last 256 ticks and total completed count. Warning retention is deduplicated and bounded to 256.
- Per-tick phase durations: process, systemd units, systemd runtime, graph persistence/adapters, and total scan. Combined adapter timing is explicitly named.
- The 260-tick fixture test checks bounded result retention, complete streaming, and timings. Tests also cover stop, elapsed duration, invalid event modes, and unavailable tracing.

## Decisions

- Bounded history avoids retaining every polling tick in a long-running command.
- Phase timings use monotonic elapsed time and do not change source observation timestamps.

## Remaining

- A two-hour real-host polling measurement is recorded below. Current-binary cause attribution and a verified RSS plateau remain open; bounded Rust result storage, allocator, adapter and kernel memory are separate measurements.
- Individual adapter timings are still grouped in graph persistence/adapters.

## Verification

`cargo test -p twin-app --test watch` and CLI watch integration tests exercise fake proc and injected event streams without root.


## 2026-10-08 live watch measurement

`python3 tools/watch_soak.py --seconds 600 --output /tmp/twin-watch-soak.json` completed with exit 0 using an isolated temporary XDG store and unprivileged read-only live host polling (no eBPF attachment). Observed: 600.382 seconds, 126 scans, 670 RSS samples, peak 59,984 KiB; steady first/last-quarter median RSS 50,616/53,000 KiB. Median process/systemd-runtime/systemd-unit/graph-and-adapters/total durations were 91/359/13/3123.5/3656.5 ms; maximum total 8534 ms. These figures describe this workload, not proof of no leaks.

The initial harness missed the final retained-tick count (null in this report); its final-summary parsing is corrected, and new runs also record binary SHA256. The measured binary predates the named-upstream and DNS follow-ups. A multi-hour soak remains outstanding. The 260-tick fixture test separately checks bounded result retention. The harness never kills the watched process; normal watch duration ends the run.


A two-hour live polling soak of the rebuilt continuation binary is now in progress, with isolated temporary XDG directories: `python3 tools/watch_soak.py --seconds 7200 --output bench-results/watch-soak-2h.json`. Tool session 76374; initial harness/watch PIDs 497454/497460. Summary log: `/tmp/twin-watch-soak-2h.log`. No result is claimed yet; wait for normal duration completion and inspect the artifact before marking this verification complete.

## 2026-10-09 completed two-hour measurement

The persisted bench-results/watch-soak-2h.json reports normal exit 0 after 7200.015 seconds, 1531 scans and 256 retained ticks; 8053 RSS samples, peak 71484 KiB. Steady-quarter medians rose from 54624 to 71152 KiB. Median/max total scan durations were 3687/6384 ms. Binary SHA256 is recorded in the artifact. This supersedes the prior in-progress note; it predates the latest depth fixes and does not establish absence of leaks. RSS growth/plateau analysis remains open.

## 2026-10-09 RSS window analysis

`tools/analyze_watch_soak.py` reports source/binary hashes and observed window medians, with no leak diagnosis. The two-hour artifact's 15-minute medians were 54504, 54752, 55720, 53582, 54140, 68728, 69364 and 71180 KiB. This is not monotonic scan-by-scan growth: a large level change occurs in the 75–90 minute window, followed by smaller increases. That observation does not identify workload changes, allocator retention or a leak as the cause. The raw artifact lacks per-tick graph cardinalities and allocator/live-allocation measurements, so cause attribution and a verified long-term plateau remain open. The analysis output is `bench-results/watch-soak-2h-analysis.json`.

## 2026-10-09 cardinality instrumentation

New watch measurements sample numeric SQLite row/state counts and database/WAL/SHM sizes from their isolated Twin store at startup, every 60 seconds and completion. Reads use a short read-only connection and transaction, closed before the next sample; unavailable stores yield an availability marker without raw errors. No graph metadata or observation bodies are retained. The analyzer attaches first/last available database samples to each RSS window, while older artifacts remain readable with zero database samples. A temporary SQLite fixture verifies numeric counts, missing-store handling and analyzer compatibility. This instrumentation supports comparing RSS changes with retained evidence growth, but does not measure allocator live allocations or establish a leak cause. The completed two-hour artifact predates these samples; a current-binary measured plateau and cause attribution remain open.

## 2026-10-09 instrumented short continuation measurement

The rebuilt continuation binary (SHA256 `91f5b3d3d53232557541a015532bab4f956ab4021b1c302470125e9dbf964c94`) completed 120.011 seconds of unprivileged polling with 89 streamed/retained ticks, peak RSS 40504 KiB and first/last steady-quarter median 40100 KiB. The artifact `bench-results/watch-soak-current-120s.json` contains numerical own-store cardinalities: nodes/edges stabilized at 873/1056, while observations reached 95938 and the database 140668928 bytes. This workload/runtime differs from the earlier two-hour host run, so timing/RSS figures are not a before/after host benchmark or evidence of a long-term plateau.

A separate three-scan own-store audit (`bench-results/watch-history-churn-audit.json`) found 1510 node history transitions changing only `content_hash`, 42 changing only `unit_type`, and 2085 edge history transitions with identical metadata. The fingerprint changes arise from intermediate generic file nodes clearing hashes and fingerprinted nodes restoring them during the same scan. This is measured unnecessary history churn; counter changes and immutable observations have separate temporal/evidence requirements. Fixing false transitions must preserve true configuration changes, snapshot proof references and historical freshness. The benchmark binary predates that correction and further source extensions.
