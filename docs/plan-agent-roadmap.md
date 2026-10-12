# `twin` Roadmap: from useful to indispensable

Six slices, 25–30. This continues [`plan-slices.md`](plan-slices.md) (slices 1–23) and
[`state/slice-24.md`](state/slice-24.md) (agent integration: `brief`, `preflight`, `mcp`, hooks).

Starting point (2026-10-12): 725 tests pass and the agent surfaces work on a real host. The
problems:

- Predictions have never been measured against reality.
- About 75% of processes are hidden without root.
- Installation requires `cargo build`, and there is no CI.
- Twin never checks what happened after an action.

---

# 0. Rules that do not change

Every slice ends with runnable commands, real output, tests and acceptance criteria, and **no
host mutation**. In addition:

1. **Agent answers stay bounded.** Every new result has item and byte caps plus `omitted`
   counts.
2. **Unknown is never green.** Unreadable data is a coverage gap that lowers confidence. It is
   never "safe".
3. **Twin predicts and verifies; it never acts.** Only the accuracy-lab harness (outside the
   production crates) performs actions, and only inside disposable machines.
4. **No secret values.** That covers logs, env files, compose files, commands in the audit log
   and journal entries. Text is parsed transiently into typed facts (endpoint, exit code,
   signal) and the raw text is dropped. Every new parser gets a sentinel test.
5. **Measure before claiming.** Each slice records its numbers in `bench-results/`.

---

# 1. Order at a glance

| # | Slice | Contains | Exit target (measured) | Effort |
|---|---|---|---|---|
| 25 | **Airtight and shippable** | file-edit hooks, full command vocabulary, scripts/make, output budgets, CI, releases, one-line install | fresh VM → working agent setup in < 2 min; no known bypass | ~2 weeks |
| 26 | **Complete graph** | store dedup, privileged daemon, idle/periodic/config-declared dependencies, live eBPF | ≥ 95% process coverage; DB growth < 10% over 100 unchanged scans | ~3 weeks |
| 27 | **Accuracy lab** | disposable-VM harness, 15 scenarios, precision/recall, nightly CI | precision ≥ 0.90, recall ≥ 0.80 | ~2 weeks |
| 28 | **Close the loop** | `twin verify`, house rules, audit trail | every lab scenario passes predict → act → verify | ~1.5 weeks |
| 29 | **One-step operations** | unit health and journal signals, `twin diagnose`, multi-step plan simulation | right cause ranked #1 in ≥ 80% of injected incidents | ~3 weeks |
| 30 | **Reach and polish** | fleet (cross-host) impact, visual map, codebase slimming | cross-host impact proven; default binary < 15 MB → **v1.0** | ~3 weeks |

Why this order:

- **25 first.** Nobody evaluates a tool they cannot install, and a guard with a bypass is not a
  guard.
- **26 before 27.** Measure the graph after its known blind spots are fixed. Otherwise the lab
  only confirms known gaps.
- **27 before 28–30.** The lab is the regression gate for everything after it, and `verify` and
  `diagnose` are scored on it.
- **28 before 29.** `diagnose` reuses the before/after capture from `verify`, and plan
  simulation reuses house rules.
- **30 last.** Fleet and map multiply whatever quality exists.

Effort assumes one developer working with an agent.

---

# Slice 25: Airtight and shippable

## Goal

Close every bypass, recognize every common destructive command, cap every output, and make
twin installable in one line. Tag **v0.2**.

## Working demo

```bash
curl -fsSL https://…/install.sh | sh
twin setup claude-code                     # agent-config + init + scan + doctor summary
twin preflight "sudo umount /var/lib/postgresql"
twin preflight "./deploy.sh"               # evaluates every command inside the script
echo '{"tool_name":"Edit","tool_input":{"file_path":"/etc/nginx/nginx.conf"}}' | twin preflight --hook claude
```

## Work, in order

1. **File-edit hook coverage (the bypass).** Agents edit files with their own tools, not the
   shell. `hook.rs` parses these into `HookInput::FileChange { path, op, cwd }`, which reuses
   the existing `EditFile`/`DeleteFile`/`DeleteTree` intents:
   - Claude: `Write`/`Edit`/`MultiEdit` (`file_path`), `NotebookEdit` (`notebook_path`).
   - Codex: `apply_patch` headers (`*** Add/Update/Delete File:`, `*** Move to:`).
   - Gemini: `write_file`, `replace`.

   Cursor only offers `afterFileEdit`, so it is covered by verify (slice 28). `agent-config`
   matchers add the file tools. CLI: `twin preflight --file PATH --op edit|delete|create`.

2. **Complete the command vocabulary.** Every entry gets a graph assessment, not just a label.

   | Command | Assessment |
   |---|---|
   | `umount`, `mount -o remount,ro` | services/containers using paths under the mount; stop if any |
   | `ip link … down`, `ip addr del`, `nmcli con down`, `ifdown` | listeners and connections on that interface; stop if it carries the SSH session |
   | `mkfs*`, `wipefs`, `dd of=/dev/…`, `parted` | stop if the device backs a mounted filesystem |
   | `swapoff`, `sysctl -w`, writes to `/proc/sys` | caution with the specific setting |
   | `crontab -r/-e`, `userdel`, `usermod -L` | caution when jobs or active services run as that user |
   | `systemctl isolate/set-default` | isolate is stop; set-default is caution |
   | `docker compose down/stop/restart/rm` | project → containers via the single allowlisted `com.docker.compose.project` label |
   | `kill -STOP` / `-CONT` | stop / go |

3. **Look inside scripts.** `./x.sh`, `bash x.sh` and `source x.sh` read a regular file of at
   most 64 KiB under cwd, with no symlink escape, and classify every line. `make TARGET` reads
   the recipe (prerequisites to depth 3, no variable expansion). Unexpanded variables or
   unreadable files produce caution "not inspected", never go. Each step cites its script line.

4. **Output budgets everywhere.**
   - A shared `twin_app::budget` helper provides caps and `omitted` counts.
   - `what-changed --json` drops raw delta lists unless `--verbose`. Today it emits 3.3 MB per
     hour.
   - `graph --kind --json` defaults to a limit of 200; `scan` and `doctor` warning lists are
     capped.
   - A regression test on a synthetic 5 000-process / 20 000-socket fixture keeps every
     `--json` command under a documented ceiling.

5. **CI.** `.github/workflows/ci.yml` runs build, tests, clippy `-D warnings`, fmt, the
   `build_ebpf.py --check` script and a binary-size report. All tests already run without root,
   Docker or a cluster.

6. **Releases and install.**
   - Static musl builds for x86_64 and aarch64 with SHA-256 sums and provenance attestation.
   - A POSIX `install.sh` that verifies the checksum, installs to `~/.local/bin` and never uses
     sudo.
   - An AUR `twin-bin` package, `.deb` and `.rpm` (packaging tools in CI only, not workspace
     dependencies), a container image for read-only `--pid=host` use, and an MCP registry
     `server.json`.

7. **First run.**
   - `twin setup <agent>` combines agent-config, init, scan and a doctor summary. It prints
     config and never writes it.
   - Agent-facing commands auto-create twin's own store on first use. Existing stores still
     migrate only through `twin init`.
   - Document `ssh host twin mcp` for remote hosts. Add a 30-second asciinema demo to the
     README.

## Acceptance criteria

- Edits to tracked configs through any supported agent file tool produce caution naming the
  readers and the validator. Project-file edits stay silent. Multi-file patches take the worst
  verdict.
- Every new command has an intent unit test plus a fixture verdict test. A script containing
  `systemctl stop postgresql` is a stop that cites the line.
- Every `--json` command stays within its ceiling on the stress fixture, and nothing is
  silently dropped.
- CI is green and blocks merges. The musl binary is smoke-tested on Debian 12, Ubuntu 24.04,
  Fedora 41, Arch and Alpine.
- Fresh VM → first MCP answer in under 2 minutes, recorded in `bench-results/install.json`.
- `twin-safety` passes: all vocabulary stays in `commands/preflight/`.

---

# Slice 26: Complete graph

## Goal

See everything, keep it fresh, and catch the dependencies a single scan misses. Fix the store
growth along the way.

## Working demo

```bash
sudo twin daemon install-unit --print > twin.service   # the user installs it; twin never does
twin brief                                             # unprivileged, served by the daemon, ~50 ms
twin brief redis                                       # "used by backup.timer (last 3 d ago, 14×)"
```

## Work, in order

1. **Stop storing unchanged facts on every scan.** This is the root cause of the 109 MB
   database, the 84 000 observations and the `why` repetition.
   - Fingerprint (source, kind, subject, normalized safe facts). Matching observations update
     `last_seen_ns` and `seen_count` instead of inserting a row.
   - Migration 7 runs only through `twin init` and collapses existing duplicates in batched
     transactions.
   - Recency comes from `last_seen_ns` and still flows through `score_evidence`; there is no
     new formula.
   - Remove the in-app `why` collapse from slice 24.
   - Disappearance and reappearance still create a new incarnation.

2. **Privileged daemon, unprivileged queries.**
   - `twin daemon` runs the watch loop (15 s) against `/var/lib/twin/twin.db` and serves
     `/run/twin/twin.sock` (0660, group `twin`).
   - The socket speaks **the same JSON-RPC handler as `twin mcp`**: one read-only
     implementation, two transports.
   - Clients use the socket when it exists and is fresh, otherwise they scan locally as today.
   - Peer authorization via `SO_PEERCRED`, with unsafe code isolated behind a safe function.
   - The printed unit is hardened: `ProtectSystem=strict`, `NoNewPrivileges`, a minimal
     capability set (`CAP_DAC_READ_SEARCH`, `CAP_SYS_PTRACE`, `CAP_BPF`, `CAP_PERFMON`), a
     syscall filter and restricted address families.
   - A test enumerates every handler and asserts each one is read-only.

3. **Idle and periodic dependencies.**
   - Edges seen within `dependency_window` (default 7 days) count as runtime-likely, labeled
     "last seen 3 d ago (14×)". This catches idle connection pools.
   - systemd timers (trigger relations, last and next run via D-Bus).
   - cron files (`/etc/crontab`, `/etc/cron.d`, spool files readable by the daemon), with
     command lines parsed by the preflight shell parser into typed targets.

4. **Config-declared dependencies.** Parse values transiently and store **only sanitized
   endpoints** (scheme/host/port), as already done for nginx origins:
   - systemd `Environment=`/`EnvironmentFile=` URL-like and `host:port` values.
   - `docker-compose.yml` (`depends_on`, ports, links, env URLs).
   - HAProxy backends, Caddy `reverse_proxy`, PHP-FPM pools, and database replication
     targets.

   Each parser gets a fixture with secret-bearing values and a sentinel test across the store,
   output and logs.

5. **Host-root mode, container image and MCP registry** (moved from slice 25).
   - Every collector reads through one configurable host root (`/proc`, systemd units,
     package databases, mounts, Docker socket), so twin can run in a container with the host
     mounted read-only.
   - Publish `ghcr.io/km-alee/twin` and list it in the MCP registry as an `oci` package with
     the required runtime arguments.
   - This shares the daemon's packaging and privilege model.

6. **Live eBPF.**
   - Verify attachment on 6.1/6.6 LTS, current stable and this host's 7.2 kernel, recording
     verifier logs and drop rates.
   - Add the missing accept and bind producers.
   - The daemon enables eBPF when `doctor --ebpf` passes; otherwise it records an explicit
     coverage gap.

## Acceptance criteria

- With the daemon running, at least 95% of processes are visible, and an unprivileged `brief`
  through the socket has p95 under 150 ms. Both numbers are recorded.
- Non-member uids are rejected. An absent or stale daemon falls back cleanly with a coverage
  note.
- `systemd-analyze security` exposure is below 3.0 for the printed unit, and the score is
  recorded.
- 100 unchanged scans grow the database by less than 10% over a single scan.
- An idle-pool consumer and a cron-only consumer appear as dependents.
- Secret sentinel tests pass for every new parser.
- eBPF drop rate is below 0.1% at 5 000 connections per second on the soak rig.

---

# Slice 27: Accuracy lab

## Goal

Publish how often twin is right. The lab becomes the regression gate for every later slice.
Tag **v0.3** when the targets are met.

## Working demo

```bash
python3 tools/lab/run.py --all          # creates disposable machines, runs scenarios, destroys them
cat bench-results/accuracy.json         # precision, recall, verdict accuracy per scenario
```

## Work, in order

1. **Harness.** `tools/lab/` is Python outside the production crates and is the **only** code
   in the repository that performs actions.
   - incus/LXD system containers or QEMU VMs with real systemd, created and destroyed per
     scenario.
   - The harness refuses to run unless the target is a lab instance it created.

2. **Scenario protocol.** For each scenario:
   - Install twin and deploy the topology.
   - Run `twin scan` and `twin preflight "<action>"`, recording the predicted affected set and
     verdict.
   - **Actually perform** the action in the disposable machine.
   - Probe every service for 30 s (TCP connect, HTTP health, `systemctl is-active`, container
     state) and record the actually broken set.

3. **Fifteen scenarios.**
   1. nginx → gunicorn → postgres
   2. Postgres behind an idle connection pool
   3. Redis used only by a cron job
   4. Compose stack down
   5. Docker restart without and with live-restore
   6. Config delete
   7. Invalid config edit then reload
   8. `umount` of a data volume
   9. `ufw deny` on a listener
   10. `apt upgrade openssl` with libssl-mapping services
   11. `kill` under `Restart=always`
   12. Socket-activated service
   13. Network-namespace container peer
   14. `Wants=` versus `Requires=` chain
   15. Stop sshd

4. **Metrics.** Precision, recall, verdict correctness against ground-truth labels, evidence
   calibration (does "strong" mean right more often?) and the false-caution rate. Too many
   cautions train people to ignore twin, so a high rate counts as a bug.

5. **Nightly CI** on a runner with nested virtualization. Every misprediction becomes an issue
   with the scenario attached.

## Acceptance criteria

- The baseline is recorded honestly first, then improved to precision ≥ 0.90 and recall
  ≥ 0.80 across all scenarios.
- The false-caution rate is reported and has a target set from the baseline.
- The README publishes the numbers and links to the methodology.

---

# Slice 28: Close the loop

## Goal

Predict → act → **verify**, enforce team rules, and record what happened. This makes
autonomous agent work safe. Tag **v0.4**.

## Working demo

```bash
twin preflight "systemctl restart nginx"   # also records an expectation
sudo systemctl restart nginx               # run by the user or agent, never by twin
twin verify --wait 30s                     # "recovered: nginx active, :443 back, 3/3 clients reconnected"
twin rules explain "systemctl restart postgresql"
twin audit --since 24h --verdict stop
```

## Work, in order

1. **Before-capture at preflight time.** A bounded record of the target neighborhood, kept in
   twin's own database with a 1 h TTL:
   - active and sub states, listeners, client counts per listener,
   - running containers, dependents' connections,
   - mount usage where relevant.

   It is keyed by session and normalized intent, **never the raw command**.

2. **`twin verify`.**
   - Compares the current state against the capture and reports recovered, degraded or new
     issue (outside the predicted set).
   - `--wait` re-scans every 2 s with a hard cap of 120 s. File edits compare fingerprints.
   - Post-action hooks: Claude/Codex `PostToolUse`, Gemini `AfterTool`, Cursor
     `afterShellExecution`/`afterFileEdit`. The result is injected as agent context.
   - MCP tool `twin_verify`.

3. **House rules** in `config.toml`:

   ```toml
   [[rules]]
   name = "business hours"
   targets = ["service:postgresql.service"]
   operations = ["restart", "stop"]
   when = { weekdays = "mon-fri", hours = "09:00-17:00", tz = "UTC" }
   verdict = "stop"
   reason = "DB restarts only outside business hours"

   [[rules]]
   name = "protected data"
   paths = ["/srv/data/**"]
   operations = ["delete", "edit"]
   verdict = "stop"
   ```

   - Rules can only **raise** a verdict, so they never hide real impact.
   - `twin rules check` and `twin rules explain` show matches; malformed rules show in
     `doctor` and as a coverage note.

4. **Audit trail.**
   - Each record holds the time, agent dialect, session, normalized intents and targets,
     verdict, rule matches and the joined verify outcome. Never raw command text.
   - `twin audit` (filters, JSON Lines export), 90-day default retention, MCP `twin_audit`.

## Acceptance criteria

- In the lab, every scenario's verify verdict matches the probe outcome. A failed restart (bad
  config) is reported as degraded within the wait window, naming the listener and unit state.
- Rule matchers (time window with DST and timezone edges, globs, targets, operations) are
  tested.
- Sentinel test: preflighting `mysql -pSENTINEL …` never puts the sentinel in the
  expectations, the audit store or any output.
- Preflight → verify joins are correct when several sessions interleave.

---

# Slice 29: One-step operations

## Goal

Make the most tedious multi-step jobs a single call: "why is it broken?" and "is this plan
safe?". Tag **v0.5**.

## Working demo

```text
$ twin diagnose api
twin diagnose · api.service   state failed (exit 1, 4 restarts in 5 min)

likely causes (ranked)
  1. port conflict — strong
     ├── observed  api.service log: "address already in use" ×4 since 10:02
     ├── observed  :8080 is held by process 4411 (old-api, started 10:01)
     └── check     twin brief :8080
  2. config changed before failure — moderate
     ├── observed  /etc/api/config.yaml changed 10:00 (restart at 10:02)
     └── note      temporal association, not proven cause
ruled out
  ├── dependency down    postgresql.service active, accepting
  └── disk               / 41% used
suggested next steps (pre-checked)
  ├── kill 4411                     preflight: CAUTION — no dependents
  └── systemctl restart api         preflight: GO

$ twin preflight --plan plan.sh
plan · 4 steps          verdict STOP at step 3
  3  systemctl restart postgresql   STOP   worker loses DB while api is still down → 2 services offline at once
net effect: all restored · peak outage: 2 services · suggested reorder: 3 before 1
```

## Work, in order

1. **Unit health** from D-Bus: `Result`, `ExecMainStatus`/`Code`, `NRestarts` and state
   timestamps. These produce typed facts: restart loop, failed exit, killed by signal, OOM
   killed.

2. **Journal signals** through `sd-journal` via dlopen of libsystemd at runtime. Unsafe code is
   isolated in `twin-collectors/src/journal/ffi.rs`. Reads are bounded by unit, window, entries
   and bytes.
   - **Typed phrase counts only**, the same model as Kubernetes logs; message bodies are never
     stored or returned.
   - Phrases: address in use, permission denied, connection refused, no space left, too many
     open files, OOM, segfault, TLS errors, configuration test failed.
   - Also record the expiry date of TLS certificates referenced by proxy configs (public
     material).

3. **`twin diagnose <target>`.** Typed hypothesis rules, each with observed inputs, an
   inferred conclusion and the shared evidence score:
   - unit failed or restart loop
   - dependency down
   - expected listener missing
   - port conflict
   - disk full on a used mount
   - OOM or killed by signal
   - config changed since last healthy
   - package changed since last healthy
   - upstream unreachable
   - certificate expired
   - permission change

   "Last healthy" is a rolling baseline kept by the daemon. Ranking is evidence strength, then
   specificity, then recency. Each hypothesis carries a read-only "check" and, where useful, a
   **pre-checked** fix command that twin never runs. "Ruled out" is kept separate from
   "unknown". MCP tool `twin_diagnose`, capped at 5 hypotheses with 3 evidence lines each.

4. **Multi-step plan simulation.**
   - `twin preflight --plan` (file, stdin or MCP list) stacks one emulated step on top of the
     previous ones in `twin-emulate`, with an explicit "restored" step for start/enable.
     Nothing is ever persisted.
   - Reports per-step verdicts against the cumulative state, peak simultaneous outage, steps
     broken by earlier steps, and the net end state (anything left down).
   - House rules are applied per step.
   - `twin plan restart a b c` suggests a dependency order that minimizes outage and reports
     cycles.

## Acceptance criteria

- Fixture journals produce correct counts, and sentinel message bodies are never persisted.
  Restart loops and OOM kills are detected in lab scenarios built for them.
- A lab incident suite of 12 injected faults ranks the true cause #1 in ≥ 80% and in the top 2
  in ≥ 95%, recorded in `bench-results/diagnose.json`. Wording never claims causation, which is
  tested.
- Plans with seeded bad orderings are flagged at the right step. The suggested reorders avoid
  the outage when executed in the lab, and a plan that forgets a restart is caught.

---

# Slice 30: Reach and polish

## Goal

Impact across hosts, a visual map, and a codebase sized to the product. Tag **v1.0**.

## Working demo

```bash
twin fleet add prod-db prod-app-1 prod-app-2
twin preflight --fleet "systemctl restart postgresql"   # names services on the app hosts
twin graph --format html api > api-map.html             # self-contained, offline
```

## Work, in order

1. **Fleet.**
   - `twin export --json` gives a bounded, versioned graph export with no raw observation
     facts.
   - Prefer a transport where the user forwards remote daemon sockets over their own SSH, so
     twin starts no processes.
   - Only if that is impractical: one audited module that runs a fixed-argv
     `ssh <host> twin export --json`. It needs a narrowly scoped `twin-safety` exception and
     tests proving no other argv can be built.
   - Node IDs are host-qualified. Cross-host edges are inferred when a remote `ip:port` matches
     another fleet host's listener, labeled "inferred across hosts".
   - `--fleet` on `impact`, `brief` and `preflight`.

2. **Visual map.** `twin graph --format html` and `twin preflight … --format html` write one
   self-contained offline file with bounded data and a vendored, pinned layout library.
   Observed and inferred edges are styled differently, coverage gaps are shown, and preflight
   overlays are colored.

3. **Slimming.**
   - Measure compile time, binary size and lines per crate.
   - Add cargo features for `k8s`, `containerd` and `rpm-legacy`. These are real
     capabilities, so the flags are allowed.
   - Ship two flavors, `twin` (lean default) and `twin-full`. Doctor reports compiled-out
     collectors as coverage gaps.
   - Delete code with no callers.

4. **Launch.**
   - Publish the lab numbers.
   - Run recorded end-to-end hook and MCP tests against the current releases of Claude Code,
     Codex, Cursor, Gemini CLI and VS Code.
   - Write security notes covering the threat model, daemon hardening, secrets policy and what
     twin can never do.
   - Publish a comparison against the manual 15-command investigation, measured in the lab.

## Acceptance criteria

- On a 3-host lab, restarting postgres on the DB host predicts the app hosts' services as
  affected, and the lab confirms it. Secret sentinels never appear in exports.
- The map is under 2 MB for a 500-node neighborhood, opens offline, and has snapshot tests on
  its data shape.
- The default binary is under 15 MB (stripped, musl) with a measured build-time reduction; both
  flavors pass CI.

---

# 2. Success metrics for "10/10"

| Metric | Target | Slice |
|---|---|---|
| Install → first agent answer | < 2 min | 25 |
| Process coverage (daemon) | ≥ 95% | 26 |
| Preflight p95 latency (daemon) | < 300 ms | 26 |
| Preflight precision / recall | ≥ 0.90 / ≥ 0.80 | 27 |
| Verify matches reality | 100% of lab scenarios | 28 |
| Diagnose top-1 / top-2 | ≥ 80% / ≥ 95% | 29 |
| MCP answer size p95 | ≤ 4 KB | 25 |
| Tool calls to answer "is it safe to restart X?" | 1 (today an agent needs 10–20) | 27 |
| Host mutations by twin | 0, enforced by `twin-safety` in CI | all |

# 3. Risks and non-goals

- **Lab cost.** Nested virtualization may need a self-hosted runner; start with nightly runs.
- **libsystemd ABI.** dlopen plus symbol checks; a missing or old library becomes a coverage
  gap, never a crash.
- **Over-caution.** Track the false-caution rate in the lab and treat a high rate as a bug.
- **Fleet transport.** The only place twin might run another program; prefer socket
  forwarding.
- **Non-goals.**
  - Remediation: twin suggests and pre-checks, it never runs.
  - Monitoring dashboards.
  - Non-Linux hosts.
  - Storing logs, env values or secret material.

# 4. How to work through it

1. Start each slice by creating `docs/state/slice-NN.md` from its section, with status
   "in progress".
2. Do the numbered work items in order; each one is independently testable.
3. From slice 28 onward, run the lab before declaring done and record the delta.
4. Update AGENTS.md when a slice introduces a convention.
