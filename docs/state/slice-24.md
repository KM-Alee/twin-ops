# Slice 24: Agent Integration — brief, preflight, MCP, hooks

## Status: done

## Goal

Make twin useful to AI coding agents and to people who delegate operations to them. The goal
is one call for context, and one call (or an automatic hook) before every mutating command.
Read-only guarantees are unchanged.

## Implemented

- `twin preflight "<cmd>"` (`twin-app/src/commands/preflight/`). `shell.rs` tokenizes quoting,
  escapes, `&&`/`||`/`;`/`|`/`&`, comments and output redirection (descriptor duplication such
  as `2>&1` is not a file). `intent.rs` maps each simple command to a typed `Intent` and sees
  through `sudo`/`doas`/`run0`, env assignments, `env`/`nohup`/`nice`/`timeout`/`exec`, and
  nested `sh|bash -c` (depth 3). `mod.rs` evaluates intents through the existing emulations
  and graph and returns go/caution/stop steps with bounded reasons (≤4) and affected nodes (≤8).
- Exit codes are 0 go, 3 caution, 4 stop, 1 error. `--json` returns `PreflightResult`.
- Hook dialects (`twin-cli/src/hook.rs`, `--hook claude|codex|cursor|gemini`,
  `--on-stop ask|deny|warn`):
  - Input is `tool_input.command` (string or argv array) or top-level `command`, plus `cwd`.
  - Go is silent; Cursor receives `{"permission":"allow"}` because its protocol requires an answer.
  - Caution becomes context: `additionalContext` for Claude/Codex, `agent_message` for Cursor,
    `systemMessage` for Gemini.
  - Stop asks where the protocol supports it (Claude, Cursor) and denies with an explanation
    where it does not (Codex, Gemini).
  - Errors and unparseable input fail open.
- `twin brief [target]` (`commands/brief.rs`):
  - The host overview has hostname/kernel facts, active/failed services, network-exposed
    listeners grouped by owner, the most depended-on services, running containers, and
    significant changes. Process/outbound churn is counted, not listed.
  - Target briefs cover services, containers (a bare name resolves after services), ports,
    files, packages and generic nodes. Each includes state facts, sections capped at 12 items,
    a restart verdict, recent changes and next steps.
- `twin mcp` (`twin-cli/src/mcp/`) is a stdio JSON-RPC 2.0 server written without new
  dependencies. It provides `initialize`, `ping`, `tools/list` and `tools/call`; notifications
  get no response.
  - Seven tools, all annotated `readOnlyHint`: `twin_overview`, `twin_brief`, `twin_preflight`,
    `twin_changes`, `twin_why`, `twin_find`, `twin_check`.
  - Tool text is capped at 24 000 bytes. Server `instructions` explain when to use each tool.
  - The graph is rescanned when older than `--max-age` (default 60 s).
  - On first use the server creates twin's own store when no database exists. Existing
    databases are never migrated implicitly.
- `twin agent-config <claude-code|codex|cursor|gemini|vscode|generic>` prints MCP, hook,
  `SKILL.md` and AGENTS.md snippets. It never writes agent configuration. The skill text lives
  in `twin-cli/assets/SKILL.md` and is embedded with `include_str!`.
- Docker: the adapter adds a read-only `GET /info` (`live_restore`). Container node metadata
  records `daemon_live_restore`. Restart emulation of `docker.service` adds every running
  Docker container as a runtime impact (risk at least high) unless live-restore is enabled;
  when the daemon does not report the state, survival is stated as unknown.
- Output budgets:
  - Impact and emulation unknowns group host-wide scan warnings by pattern: numbers, paths,
    abstract socket names and IPv6 literals are masked, giving one entry per pattern with a
    "(+N similar)" count. On this ~480-process host, `impact docker --json` dropped from
    211 KB (1196 unknowns) to 12.8 KB (13 unknowns), and `emulate restart NetworkManager
    --json` dropped to 5.8 KB.
  - `why` collapses identical observations re-seen on every scan into one entry with
    `repeat_count` and `first_timestamp_ns`.

## Decisions

- Verdict mapping: emulated risk low → go, medium/unknown → caution, high/critical → stop.
  Stopping, killing or removing a target with affected dependents is always stop.
- A service listening on a non-loopback port raises restart/stop to at least caution, because
  external clients are not visible as graph dependents.
- Stopping sshd, network managers, systemd-resolved or the system bus is always stop.
- Commands twin does not model (`cargo`, `git`, remote `ssh`) are go with `is_assessed=false`;
  they never produce hook noise.
- An unknown service or failed emulation is caution with `is_assessed=false`, never go.
- File operations outside system roots (`/etc`, `/usr`, `/boot`, `/var/lib`, `/var/spool`,
  `/lib*`, `/bin`, `/sbin`, `/opt`, `/srv`, `/root`, `/var/www`) are go unless a tracked node
  under the path has a service/container reader. They also work without an initialized store.
  `rm -r` of `/`, a system root, `/var`, `/home`, `/dev`, `/proc` or `/sys` is always stop.
- System-wide upgrades are caution and suggest preflighting specific packages; twin cannot
  see the package manager's transaction plan.
- Safety scanning: execution primitives (`Command::new`, `std::process::Command`,
  `std::os::unix::process`, `libc::kill(`, Kubernetes `patch`/`delete_collection`/`DeleteParams`,
  `portforward`) are banned in every production file. Command phrases such as
  `systemctl restart` are allowed only under `twin-app/src/commands/preflight/`, as recognition
  vocabulary. A test pins that exception to that directory.
- MCP is implemented by hand: newline JSON-RPC over stdio plus four methods needs no SDK, so no
  new dependency was added.

## Deviations

- `brief` changes default to 24 h for targets (CLI) and 1 h for the MCP overview.
- Hook timeouts are left to the agent configuration (30 s suggested). Preflight on recognized
  operations rescans only when the graph is older than `--max-age` (default 120 s); on this
  host a full scan takes about 1 s, and unrecognized commands never touch the store.

## Verification

- `twin-app` tests: `preflight` (12), `brief` (4), `why` collapse, grouped scan-warning
  unknowns, and intent unit tests (4).
- `twin-cli` tests: `hook` dialects (5) and `agent_integration` (5, spawning the binary for the
  MCP handshake/tools, hook stdin/stdout, exit codes, first-use init and agent-config validity).
- `twin-container` live-restore fixture test; `twin-safety` vocabulary exception test.
- Pre-existing isolation leak fixed: 45 `scan_graph` scans used `ScanRequest::default()` and
  read the host's real Docker daemon, so 7 tests failed on any machine running containers.
  They now use `support::host_isolated_scan` (empty Docker fixture).
- Known remaining: plain `what-changed --json` stays unbounded (3.3 MB for 1 h here). The MCP
  `twin_changes` tool and `brief` bound it.
- Live host checks: `preflight "systemctl restart docker"` named the running container and
  "live-restore disabled" from the real daemon. An MCP stdio handshake listed 7 tools with
  about 1 KB answers. Target briefs took 0.25 s and the host overview 0.7 s, including the
  freshness check.
