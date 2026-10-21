# Slice 26: Complete graph

## Status: implemented; root-only measurements pending (items 2 and 6)

Plan: [`plan-agent-roadmap.md` § Slice 26](../plan-agent-roadmap.md).

## Work items

| # | Item | Status |
|---|---|---|
| 1 | Stop storing unchanged facts on every scan (migration 7) | done |
| 2 | Privileged daemon, unprivileged queries | done (root-only acceptance numbers pending) |
| 3 | Idle and periodic dependencies (dependency window, timers, cron) | done |
| 4 | Config-declared dependencies | done |
| 5 | Host-root mode, container image, MCP registry | done (publishing runs on the next release tag) |
| 6 | Live eBPF | daemon integration done; kernel verification needs root (commands below) |

## 1. Repeated observations

- Observations gain `last_seen_ns`, `seen_count`, `fingerprint` and `merge_pending`.
  `timestamp_ns` stays the first sighting.
- The fingerprint covers source, kind, subject, object, confidence, redaction and the
  metadata. It leaves out the per-sighting keys `sample_at_ns`, `sample_index` and
  `raw_line`, plus the line suffix of `__raw_ref`. It is a 128-bit FNV-1a hash of that
  canonical key; at this width equal fingerprints are treated as the same fact, so the
  merge never re-parses metadata.
- `Store::merge_repeated_observations` runs at the end of every scan (and so every watch
  tick) and in `twin init`, in batches of 5 000 rows per transaction. A new row folds into
  the newest row holding the same fact only if all of these hold:
  - that row was seen by the same collector's current or immediately previous run. Event
    rows without a run merge only with other run-less rows; eBPF event metadata carries
    process start ticks, so process generations stay apart;
  - no node incarnation reset sealed it (`reset_node_incarnation` clears the fingerprints of
    the node's settled rows);
  - it supports exactly the same relationships (`edge_observations` set equality). An edge
    incarnation reset deletes links, so an earlier incarnation's evidence never counts for
    the new one, and a fact re-seen for one edge never refreshes another edge's proof.
- Otherwise the new row stands, so a fact that disappears for a run and comes back starts a
  new incarnation.
- Recency for current reads comes from `last_seen_ns` through the existing `score_evidence`.
  eBPF repeat counts sum `seen_count`. Historical reads use `last_seen_ns` only when it is at
  or before the read time, and otherwise the first sighting (they never look fresher than
  they were).
- Snapshot proof references capture `last_seen_ns` at capture time
  (`SnapshotProofRef.last_seen_ns`). Merges keep the larger captured value.
- Retention prunes observations by `last_seen_ns`, so a fact that is still being seen is
  never pruned for being old.
- `why` shows each stored fact once, with `seen N×` and first/last times from the row. The
  slice 24 display collapse is removed.
- Migration 7 runs only through `twin init`. It adds the columns and indexes and computes
  fingerprints for every existing row, all in one transaction with the version marker. Init
  then collapses the legacy duplicates through the same merge in batched transactions; if it
  is interrupted, the next scan or init continues.

## Found while measuring: history churn

- On this host `edge_history` held 571 336 rows (≈200 MB with indexes). 531 194 of them
  were `changed` rows written only because `evidence_count`, a per-sighting counter, went up
  on every scan.
- `classify_edge_change` now ignores `evidence_count` (and, as before, `last_seen_ns`).
- Historical reads used to depend on that churn to keep long-lived edges visible past the
  history retention cutoff. Retention now drops a pre-cutoff history row only when a newer
  row at or before the cutoff supersedes it, or when it records `gone`. Each entity's state
  at the cutoff therefore survives for `graph --at` inside the retention window.

## 2. Privileged daemon

- `twin daemon run` binds the socket (default `/run/twin/twin.sock`, mode 0660), scans
  once, then runs the watch loop (default 15 s) against its own store
  (`TwinLayout::flat(--state-dir)`, default `/var/lib/twin`, holding `twin.db` and
  `config.toml`) and serves the socket until SIGINT/SIGTERM. The socket file is removed on
  shutdown. A leftover socket from a stopped daemon is replaced; a live daemon or a
  non-socket file at the path is an error.
- **One handler, two transports.** The socket serves `twin_cli::mcp::McpServer` in
  `ServeMode::Daemon`, the same JSON-RPC handler as `twin mcp`. `METHODS` lists every method:
  the MCP ones plus `twin/status`, `twin/brief` and `twin/preflight`, which return typed
  results for the CLI (`BriefResult`/`PreflightResult` now derive `Deserialize`).
- In daemon mode the handler never scans (the watch loop owns freshness) and never reads a
  file a client names: `twin_check` is not listed and refuses, and preflight runs with
  `should_read_scripts: false`, so a script or makefile step is a caution
  (`PreflightRequest.should_read_scripts`).
- **Peer authorization.** `twin_app::daemon::peer_credentials` reads `SO_PEERCRED` and
  `SO_PEERGROUPS` (the only new unsafe code, two `getsockopt` calls). Both are captured by
  the kernel at connect time, so no `/proc` lookup can race PID reuse. `AccessPolicy` admits
  root, the daemon's effective user, and members of its effective group (primary or
  supplementary). Others get JSON-RPC error -32001 and the connection closes. At most 32
  sessions run at once; requests are capped at 256 KiB and idle sessions end after 5 min.
- **Clients.** `twin brief`, `twin preflight` (including `--hook`) and `twin mcp` ask the
  daemon first (`--daemon-socket`, `--no-daemon`). They fall back to a local scan when the
  socket is missing, refused, slow (10 s), or the daemon's graph is older than the caller's
  `--max-age`. A CLI fallback adds the coverage note `twin daemon not used (<reason>);
  answered from a local scan, which cannot see processes other users own`. A command that
  runs a script is always evaluated locally, so the script is read with the caller's own
  permissions. `twin mcp` forwards every tool except `twin_check` and script preflights.
- `twin daemon status` reports whether the daemon answers and its graph age (exit 3 when
  not available). `twin daemon install-unit --print` prints the unit; `--print` is required
  and twin never installs it.
- **The unit.** Runs as root with `Group=twin`, `UMask=0027`, `StateDirectory=twin`,
  `RuntimeDirectory=twin` (0750; the state directory is 0700, so group twin reaches the graph only through the socket, never the raw store). Bounding and ambient capabilities are exactly
  `CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_BPF CAP_PERFMON`. `NoNewPrivileges`,
  `ProtectSystem=strict`, `ProtectHome=read-only`, private tmp and devices, kernel
  tunables/modules/logs/cgroups/clock/hostname protection, restricted namespaces, realtime,
  SUID, personality and W^X. Address families `AF_UNIX AF_NETLINK AF_INET AF_INET6` (Docker
  and containerd sockets, socket diagnostics, Kubernetes API and DNS).
  `SystemCallFilter=@system-service bpf perf_event_open` minus `@resources @mount @reboot
  @swap @module @raw-io @clock @cpu-emulation @obsolete`. `@privileged` and `@debug` cannot
  be denied because they contain `bpf` and `perf_event_open`.

### Measured (item 2)

- `systemd-analyze security --offline=true` exposure for the printed unit: **2.7 OK**
  (target < 3.0). The test `install_unit_only_prints_a_hardened_unit` re-scores it wherever
  `systemd-analyze` can analyze offline. The remaining points are inherent: root user,
  read-everything capabilities, full `/proc`, host network.
- Unprivileged daemon on this host (uid 1000, so it sees only its own processes), 200
  sequential client invocations each, release build:

  | command | p50 | p95 | max |
  |---|---|---|---|
  | `twin brief` (host overview) | 47.8 ms | 53.7 ms | 61.3 ms |
  | `twin brief sshd.service` | 26.1 ms | 27.8 ms | 37.7 ms |
  | `twin preflight -- systemctl restart docker` | 10.4 ms | 11.2 ms | 12.0 ms |

  These are whole-process times (exec, connect, query, render). The p95 target (< 150 ms)
  holds. No local store was created by any of these calls.
- SIGTERM stops the daemon within one poll interval and removes the socket.

### Still to record (needs root)

The 95% process visibility number needs the daemon running as root with its capabilities,
which only the user can start. Commands:

```bash
sudo groupadd --system twin
sudo usermod -aG twin "$USER"          # log in again for the group to apply
twin daemon install-unit --print | sudo tee /etc/systemd/system/twin.service
sudo systemctl daemon-reload && sudo systemctl start twin.service
twin daemon status
twin brief --json | jq '.coverage_notes'
ls -d /proc/[0-9]* | wc -l              # compare with the brief's process count
```

## 4. Config-declared dependencies

- **Parsers (`twin-config`, pure).** `endpoints_in_value`/`endpoints_in_environment`
  (URLs of any scheme with credentials, paths and queries dropped; `host:port` lists;
  `X_HOST`+`X_PORT` pairs), `parse_env_file`, `parse_compose` (serde_yaml: `depends_on`,
  `links`, published `ports`, `environment`, `env_file`), `parse_haproxy` (`server` lines of
  `backend`/`listen`), `parse_caddy` (`reverse_proxy`, `php_fastcgi`, `to`),
  `parse_fpm_pools` (`listen`), `postgres_primary` (`primary_conninfo`: only `host`,
  `hostaddr`, `port`), `redis_primary` (`replicaof`/`slaveof`) and `mysql_master_info`
  (lines 4 and 7 of MariaDB's `master.info`; user and password lines are never read). The
  nginx parser also follows `fastcgi_pass`, `uwsgi_pass`, `scgi_pass` and `grpc_pass`.
- **Secret handling.** Values of names containing PASSWORD/PASSWD/SECRET/TOKEN/CREDENTIAL/
  PRIVATE, or with a segment KEY/APIKEY/AUTH/SALT/COOKIE or ending in PASS/PWD, are never
  parsed. A URL whose credentials contain an unescaped `/`, `?` or `#` (so the `@` count
  of the cut authority differs) is rejected rather than misread as `user:1234`. Bare
  single-label hosts count only where the setting's name says it holds an endpoint
  (`…_HOST`, `…_URL`, `…_ADDR`, …), so `12:30` is not an endpoint. `EnvAssignment` and
  `ServiceEnvironment` redact values in `Debug`. YAML errors report only a location.
- **Sources.** systemd `Environment=`/`EnvironmentFile=` from unit files and drop-ins
  (`EffectiveUnit.environment`; an empty assignment resets, `-` marks optional files),
  known configs `/etc/haproxy/haproxy.cfg`, `/etc/caddy/Caddyfile`, Redis/Valkey configs,
  PostgreSQL `postgresql.conf`/`postgresql.auto.conf` (also data directories),
  PHP-FPM pool directories, MariaDB `master.info`, and Compose files named by the newly
  projected `com.docker.compose.project.config_files` label (absolute paths only, ≤8,
  sentinel-tested). Files are read through the host root, bounded to 256 KiB.
- **Graph** (`twin-app/src/commands/scan_declared.rs`, after every other writer and before
  finalization). Each endpoint becomes a `config_declared_endpoint` observation
  (`declared_in`, `key`, `endpoint`, `format`), an observed `file references port`, and an
  inferred `owner references port`. A local provider (observed listener service on the
  exact or, for loopback clients, wildcard address; a container mapping the port; or a
  declared PHP-FPM/Compose listener) gets `owner depends_on provider` (`proxies_to` for
  HAProxy/Caddy). Compose single-label hosts that name a sibling service link the
  containers directly; `depends_on`/`links` become observed container `depends_on` edges.
  DNS names use the scan's bounded `DnsReader` and are marked `resolved_by: current_dns`.
- **One edge, one writer per pass.** If a connection, unit `Requires=` or D-Bus writer
  already wrote the same edge in this pass, the declaration only links its observation.
  An edge nobody else saw this pass becomes declared, so a closed connection with a
  declared URL keeps a truthful reason.
- **Impact.** Declared dependents read `declared in /etc/app.env (DATABASE_URL →
  postgres://127.0.0.1:5432)`; they are runtime while the client runs and configured
  otherwise. `why` shows `declared_in`, `key`, `endpoint` and `format`.
- **Lifecycle.** A removed declaration's edges go `gone`. An unreadable, missing
  (non-optional) or oversized source marks `config_declared` uncertain, so its edges go
  `stale`. Unresolvable hosts are grouped warnings.
- Tests: `twin-config/tests/declared.rs` (every parser with secret sentinels),
  `twin-app/tests/declared.rs` (env URL dependent and reason, HAProxy backend, Compose
  containers, declared PHP-FPM pool behind Caddy, removal/stale lifecycle, sentinel across
  store bytes, scan JSON and `why`), `twin-container/tests/read.rs` (label projection).

### Limitation

- nginx `fastcgi_pass` to a PHP-FPM pool links only to an *observed* listener (the
  existing nginx path); the declared-listener match applies to the new declaration
  sources. A root daemon observes FPM sockets, so this matters only for unprivileged scans.

## 5. Host-root mode, container image, MCP registry

- **One host root.** `twin_app::paths::{host_root, host_path, default_proc_root,
  is_host_proc_root, logical_path, host_dbus_socket}`. `TWIN_HOST_ROOT` (or the global
  `--host-root DIR`, which checks for `DIR/proc` and sets it once at startup) moves every
  host read under one directory: `/proc`, systemd unit search paths, package databases
  (dpkg, pacman, rpm), mounts, crontabs, service configuration, declared files, the
  cgroup tree, and the Docker, containerd and D-Bus sockets
  (`SystemdRuntimeCollector::with_dbus_socket`, `try_connect_dbus(Some(path))`). Every
  former `/proc` default (`scan`, `brief`, `preflight`, `test`, `doctor`, `watch`) now
  resolves through it.
- **Host identity, not the reader's.** Inside a container the reader's namespaces are
  not the host's, so under a host root:
  - the network namespace and socket tables of host PID 1 stand for the host
    (`ProcessCollector::with_host_process(Some(1))`), applied only when the scan reads the
    host's `/proc`;
  - mounts come from `/proc/1/mounts` (always, falling back to `/proc/mounts`; this also
    corrects the sandboxed daemon, whose own mount namespace differs);
  - the hostname comes from the host's `/etc/hostname`;
  - unit and configuration files are named by their host path (`/etc/systemd/system/x`),
    not the mount path (`/host/etc/...`), in node IDs and evidence (`logical_path`).
- **Image.** `packaging/container/Dockerfile`: `scratch` plus CA certificates and the static
  musl binary, `TWIN_HOST_ROOT=/host`, store in `/data`, entrypoint `twin`, default `mcp`,
  label `io.modelcontextprotocol.server.name="io.github.KM-Alee/twin"`. The release
  workflow's `image` job builds `linux/amd64,linux/arm64` from the release binaries, pushes
  `ghcr.io/km-alee/twin:<version>` and `:latest`, and attests provenance.
- **MCP registry.** `server.json` (schema 2025-12-11) lists the OCI package with its runtime
  arguments: `--volume /:/host:ro` (required), `--volume twin-data:/data`, `--cap-drop ALL
  --cap-add DAC_READ_SEARCH --cap-add SYS_PTRACE`, `--security-opt no-new-privileges`, and
  package argument `mcp`. The `mcp-registry` job checks the version against the tag and
  publishes with `mcp-publisher` via GitHub OIDC. `twin-cli/tests/packaging.rs` pins the
  version, image identifier, read-only host mount and image label.
- Tests: `twin-app/tests/host_root.rs` (a whole host tree under one root: processes, unit
  file named by its host path, nginx config, crontab, PID 1 mounts over the reader's;
  resolver rules), `twin-collectors/tests/network_namespaces.rs` (host process namespace).

### Measured (item 5)

A throwaway image with this build (Debian base, since the local build is not musl) was run
on this host with `-v /:/host:ro --cap-drop ALL --cap-add DAC_READ_SEARCH --cap-add
SYS_PTRACE --security-opt no-new-privileges`, then removed:

| | unprivileged host scan | container, host root |
|---|---|---|
| processes | 456 (357 hidden) | 512 of 512 |
| services | 263 | 272 |
| listening endpoints | 3 | 21 |
| containers | 1 | 2 |

The first container run, before the namespace fixes, showed the container's hostname and a
`netns:` suffix on every host endpoint; both are fixed and covered by tests.

Nothing was published: the image and registry entry appear on the next `v*` tag.

## 6. Live eBPF

- **Producers.** The accept (`sys_enter/exit_accept`, `accept4`) and bind
  (`sys_enter/exit_bind`) producers the roadmap lists as missing already exist in
  `twin.bpf.c`, are attached by `attach_tcp`, and are decoded as `TcpEvent::Accept` /
  `TcpEvent::Bind` (`EbpfAccept`/`EbpfBind` observations). Nothing new was needed there.
- **Daemon.** `twin daemon run --ebpf auto` (default) runs the same assessment as
  `twin doctor --ebpf` (`twin_app::daemon::ebpf_readiness`) and, when it passes, watches with
  live TCP tracing (connect, accept, bind). Otherwise, or with `--ebpf off`, it scans without
  eBPF.
- **Explicit coverage gap.** Every live or unavailable attach attempt by the watch loop now
  writes one `ebpf` collector run (`success`, or `unavailable` with the reason), and
  coverage reports `ebpf collector unavailable` until a later attach succeeds. A refused
  attach at runtime is recorded the same way. Test: `twin-app/tests/watch.rs`
  (`unavailable_ebpf_warns_and_still_ticks`).
- **Reproducibility check.** This host's clang moved from 23.1.1 (which built the bundled
  object) to 23.1.3, which emits different bytes. `tools/build_ebpf.py --check` now says
  exactly that instead of "stale". The source still matches the manifest, so the object was
  not rebuilt: rebuilding just for a compiler bump would change the shipped program without
  a source change.

### Still to verify (needs root, and VMs for the other kernels)

Loading BPF programs needs `CAP_BPF`/`CAP_PERFMON`; twin never uses sudo, so these are for
the user to run. On this host (7.2.11) and on 6.1 LTS, 6.6 LTS and current stable:

```bash
sudo twin doctor --ebpf
sudo twin watch --ebpf --events tcp --ticks 20 --interval 15s --json > ebpf-run.jsonl
grep -c '"event":"tcp"' ebpf-run.jsonl
grep '"event":"warning"' ebpf-run.jsonl
sudo bpftool prog show | grep -i twin
```

`doctor --ebpf` shows each readiness check (kernel, BTF, capabilities, exec and TCP
tracing); an attach the verifier rejects surfaces as a watch warning with the error, and
dropped events surface as watch warnings too. Record the kernel, attach outcome, event count
and drops per kernel here.

## 3. Idle and periodic dependencies

- **Dependency window.** `[dependencies] window_days` (default 7, the same as observation
  retention) in `config.toml`. An inferred `depends_on`/`connects_to` edge that is no longer
  active but was last seen inside the window keeps its dependent as a runtime dependent in
  `impact`, `emulate restart` and everything built on them (preflight, brief, MCP). The
  reason reads `no open connection to postgresql.service now; last seen 3 d ago (14×)`. A
  dependent that is itself gone, or that also has an open connection, is not repeated.
- **systemd timers.**
  - Unit files: `.timer` units are discovered (with drop-ins). `[Timer] Unit=`, or the
    same-named service, becomes a `systemd_timer_activates` observation and an observed
    `timer → service` `depends_on` edge with `timer_activation: true`, alongside socket
    activation. Unexpandable specifiers and template targets (`job@.service`) are parse
    warnings, not guesses.
  - D-Bus: `TriggeredBy` timers give the same relation from the manager. The
    `org.freedesktop.systemd1.Timer` properties `LastTriggerUSec` and
    `NextElapseUSecRealtime` become `timer_last_trigger_ns`/`timer_next_elapse_ns` on the
    timer node; 0 and `u64::MAX` mean none, and an unreadable timer records
    `timer_schedule_known: false`.
  - `normalize_unit_name` now accepts `.timer`. Before, every `Wants=x.timer` (for example
    from `timers.target`) was dropped as an invalid dependency.
  - A recent dependent started by a timer adds `started by backup.timer (last ran 3 d ago,
    next in 4 h)`. Graph and brief service views list `timer_activation`.
- **cron** (`twin-collectors::cron`, `twin-app/src/commands/scan_cron.rs`).
  - Reads `/etc/crontab`, `/etc/cron.d/*`, `/var/spool/cron/crontabs/*`,
    `/var/spool/cron/*` and the `cron.{hourly,daily,weekly,monthly}` run-parts
    directories. Bounds: 64 KiB per file, 256 files, 1 024 jobs. Run-parts naming rules
    apply (names with dots or `~` are skipped).
  - Environment lines are skipped without reading their values, and text after an
    unescaped `%` (stdin) is dropped.
  - Commands are classified transiently with the preflight parser
    (`preflight::scheduled_facts`). Only program names (a bare name or absolute path),
    typed operations (`restart nginx.service`, `delete /path`, …), the schedule and the
    user are stored. `CronEntry`'s `Debug` redacts the command.
  - Each job is a `cron_job` node `cron:/etc/cron.d/backup:3`, owned (inferred,
    `cron_runner`) by the first of `cron.service`/`cronie.service`/`crond.service` that
    exists, preferring a running one.
  - A recent dependent that is a cron daemon adds `runs cron jobs: backup:3 (@daily) runs
    pg_dump, …` (3 named, then a count). Which job held the connection is not observed, so
    none is singled out.
  - Unreadable or oversized sources and truncation mark `cron_file` uncertain, so unseen
    jobs go stale rather than gone. A plain fixture scan (not `/proc`) skips cron unless
    `ScanRequest.cron_root` is set.

## Measured

See `bench-results/observation-dedup.json` (all runs: release build, this host).

| 100 scans | v0.2.0 | slice 26 |
|---|---|---|
| Fixture host (1 000 processes, unchanged): used bytes | 8.8 MB → 718 MB | 9.4 MB → 11.5 MB |
| Fixture host: observation rows | 8 040 → 804 000 | 8 040 → 8 040 |
| Fixture host: edge history rows | 3 085 → 304 540 | 3 045 → 3 045 |
| This host (changing): used bytes | 14.9 MB → 892 MB | 17.9 MB → 31.4 MB |
| This host: median scan | 1.44 s | 1.51 s |

- The fixture's byte growth is almost all a one-time step at scan 2 (B-tree pages half
  filled by the first merge). Scans 2–100 add 4.7%, mostly collector-run rows.
- Migration 7 on this host's 448 MB schema-5 store: 29 s, 278 964 → 46 068 observations,
  with the sum of `seen_count` exactly 278 964.
- Merge cost is about 27 µs per merged row (≈220 ms per scan on the fixture). It is
  dominated by insert-then-delete index maintenance and by `secure_delete`, which is kept.
- On this host the scan finds 20 timer activations. Cron is not installed here, which
  correctly produced no jobs and no warnings.

## Also fixed while measuring

- **Bounded warning metadata.** `proc_process` collector runs stored every per-process
  warning (≈129 KB per scan on this host, forever). They now store at most 64 groups of
  `(kind, detail pattern)` with an occurrence count and `warning_details_omitted`;
  `warning_pattern` moved to `scan_quality.rs` and `impact` sums the stored counts.
- **Collector-run retention.** Runs older than the observation and history cutoffs are
  pruned unless an observation or history row still cites them or they are a collector's
  newest run.
- **History netting per pass.** Several writers can touch one entity in a pass (unit file
  and D-Bus for `depends_on`; cgroup inference and the D-Bus correction for user-session
  `owns`). `ScanHistorySession` now records one row per entity per pass, comparing the
  state before the pass with the final state, and nothing when the two match. This also
  covers an edge created and deleted in the same pass.
- **Per-sighting node keys.** `runtime_observed_at_ns` no longer counts as a node change,
  and a plain file sighting keeps the stored `content_hash` instead of erasing it.

## Open finding

- For user-session units, the system D-Bus prefix lookup resolves
  `/user.slice/user-1000.slice/user@1000.service/app.slice/app-x.service` to
  `user@1000.service`, so the cgroup correction replaces the more precise path-derived
  owner. The churn is gone (history netting), but the attribution question is open: the
  system manager only knows the delegating unit.

## Decisions

- The daemon's socket server lives with the MCP handler in `twin-cli` (it is a transport for
  that handler). Peer credentials, the access policy and the unit text are in
  `twin_app::daemon`, which already owns twin's other libc boundary (`statvfs`).
- Preflight of a script-running command is not forwarded to the daemon: root would read
  files the caller may not be allowed to read. The caller's local preflight reads it with
  the caller's own permissions instead.
- Clients identify themselves only through the kernel's socket credentials; no token or
  password exists, so nothing secret is stored.

- Merging happens after the scan's writes, not at insert time. Incarnation resets and edge
  links are written after observations are inserted, so only an end-of-pass merge can tell
  whether the earlier row is still the same incarnation.
- The pending flag is a column rather than a rowid watermark, because the observations table
  has a text primary key and its rowids are not stable across `VACUUM`.
- Timers reuse the socket-activation path (`Activation::{Socket, Timer}`) and the edge
  constructor is now `GraphEdge::observed_activates_service`. Timer and cron nodes stay
  service-kind/`cron_job` nodes; no separate timer node kind was needed.
- No `VACUUM` is run. SQLite reuses freed pages, so a collapsed store stops growing; a user
  who wants the file smaller can vacuum twin's own database themselves.
