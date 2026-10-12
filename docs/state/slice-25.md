# Slice 25: Airtight and shippable

## Status: done, released as v0.2.0

Plan: [`plan-agent-roadmap.md` § Slice 25](../plan-agent-roadmap.md).

## Implemented

1. **File-edit hook coverage.**
   - `hook::parse_input` returns `HookInput { command, file_changes, cwd }`. File changes come
     from an allowlist of file-*changing* tools: Claude Code `Write`/`Edit`/`MultiEdit`/
     `NotebookEdit` and Gemini `write_file`/`replace`. Read-only tools that carry a path
     (`Read`, `read_file`) are never treated as edits.
   - Codex `apply_patch` envelopes are found in any string field of `tool_input`, and in shell
     heredocs when there is no `tool_input`. They yield Add/Update/Delete/Move entries,
     deduplicated.
   - `PreflightRequest` carries typed `file_changes` (`FileChange { path, operation }`).
     `twin preflight --file PATH --op edit|delete|create` covers direct use.
   - Systemd unit-file edits advise `systemd-analyze verify` plus `daemon-reload` instead of a
     service config validator.
   - `agent-config` matchers now include the file tools. Cursor's lack of a pre-edit hook is
     documented.
2. **Command vocabulary.**

   | Intent | Commands | Assessment |
   |---|---|---|
   | `Unmount` | `umount`, `mount -o remount,ro` | services using paths on the mount (via `scan_mounts::services_on_mount`) → stop; `/` is always stop |
   | `InterfaceDown` | `ip link set … down`, `ip link del`, `ip addr flush/del dev`, `ifdown`, `nmcli dev disconnect` | default route → stop; listeners bound to its addresses → stop; wildcard listeners noted; `lo` → stop |
   | `WipeDevice` | `mkfs*`, `wipefs`, `sgdisk`, `sfdisk`, `blkdiscard`, `mkswap`, `parted` (non-listing), `cryptsetup luksFormat/erase`, `dd of=/dev/…` | backs a mounted filesystem → stop with users; otherwise caution |
   | `Compose` | `docker compose`, `docker-compose` (down/rm/stop/kill/pause/restart, `-p`, `-f`, services) | project resolved from `-p`, else the compose file's directory, else cwd (Compose name normalization) → that project's containers via labels |
   | `Advisory` (fixed verdict, not graph-assessed) | `sysctl -w`, `/proc/sys` writes, `swapoff`, `crontab -r/-e`, `userdel`, `groupdel`, lock/expire flags of `usermod`/`passwd`/`chage`, `systemctl set-default`, `kill -HUP/-USR1/-USR2`, `ip route del/flush default` (stop), `nmcli networking off` (stop) | — |

   - `kill -0` and `kill -CONT` are read-only.
   - `ip` is no longer blanket read-only: before this slice, `ip link set eth0 down` passed as
     harmless.
   - `twin-collectors::parse_interfaces` joins `/proc/net/{route,fib_trie,if_inet6,ipv6_route}`
     into per-interface addresses and default routes, with no netlink and no unsafe code.
   - The Docker adapter projects exactly two labels, `com.docker.compose.project` and
     `com.docker.compose.service`, character-filtered. Container nodes store
     `compose_project`/`compose_service`.
3. **Scripts and make** (`commands/preflight/script.rs`).
   - Applies to `./x`, absolute paths, `bash|sh|… x.sh`, `source`/`.`, and `make|gmake`
     (`-C`, `-f`, `-j N`, targets, defaulting to the first rule).
   - Shell scripts are at most 64 KiB, UTF-8, with a shell shebang or none. Other interpreters
     and binaries stay unrecognized (go).
   - Processing:
     - Backslash continuations are joined; comments and heredoc bodies are skipped.
     - Nested scripts and recipes expand to depth 3; prerequisites are followed;
       `$(MAKE)` lines are skipped.
   - **Only operation lines are kept**, with `name:line:` segments; read-only and unrecognized
     lines never appear in output.
   - Lines with `$` become "uses shell variables" cautions rather than guesses.
   - An unreadable, oversized or missing script is caution.
4. **Output budgets.**
   - `--json` caps every object-held list at 100 items, adding `<field>_omitted`
     (`output::json::cap_lists`). The global `--full` disables the cap.
   - `what-changed --json` without `--verbose` replaces the 10 raw delta lists with
     `raw_change_counts`.
   - `why` lists host-wide coverage gaps once at top level rather than per relationship, and
     shows the 4 newest distinct observations per relationship (scoring still uses all).
5. **CI** (`.github/workflows/ci.yml`):
   - fmt, clippy `-D warnings`, workspace tests (`--locked`);
   - eBPF probe source/object hashes against the manifest;
   - release build with a size summary;
   - a smoke test (`init`/`scan`/`preflight`/`brief`).
6. **Releases** (`.github/workflows/release.yml`, `install.sh`, `packaging/aur/PKGBUILD`,
   `LICENSE`):
   - Static musl x86_64/aarch64 builds via `cross`, `.tar.gz` plus `.sha256`, a combined
     `SHA256SUMS`, build-provenance attestation, and a GitHub release.
   - Smoke runs in Debian 12, Ubuntu 24.04, Fedora 41, Arch and Alpine containers.
   - `.deb` and `.rpm` via cargo-deb and cargo-generate-rpm, installed in CI only.
   - `install.sh` is POSIX sh: it verifies the checksum, uses no sudo, defaults to
     `~/.local/bin`, and supports a `TWIN_DOWNLOAD_BASE` mirror.
7. **First run.**
   - `twin setup <agent>` creates the store, scans, reports how many processes are hidden
     (with the exact `setcap` command) and prints the agent snippets. It writes no agent
     files.
   - `preflight` and `brief` auto-create twin's own store on first use (as `mcp` already did).
     Existing stores still migrate only via `twin init`.
   - The README documents `ssh host twin mcp` for remote hosts.

## Measured (this host, ~480 processes)

| `--json` output | Before | After |
|---|---|---|
| `what-changed --since 1h` | 2.1 MB | 19 KB |
| `scan` | 164 KB | 16 KB |
| `graph --kind process` | 96 KB | 20 KB |
| `impact docker` | 211 KB (slice 24 start) | 15 KB |

- Synthetic busy host (1 000 processes, 40 services, 4 000 connections): every JSON command is
  within its ceiling (`twin-cli/tests/output_budget.rs`). Ceilings: 64 KiB for scan, process
  and port lists, what-changed, impact and emulate; 96 KiB for target graph and why; 16 KiB
  for brief.
- `twin setup` on a fresh home: 2.2 s. The installer was run end to end against a local fake
  release, including the tampered-checksum refusal.

## Bug found by the demo and fixed

On hosts where D-Bus is unreadable, a declared `Requires=` dependent was classified as
"configured only" even while it ran processes and held live connections. As a result,
`restart postgresql` returned GO. `evidence::is_service_running` now treats a service that
owns a live process as running. `Wants=` alone stays configured context. A regression test
covers both declarations.

## Decisions

- Cursor gets no pre-edit coverage because its protocol has none. Its edits are covered by
  `verify` (slice 28).
- Script expansion keeps only operation lines, because other script lines can contain
  credentials.
- `RiskLevel` is not ordered (it has `Unknown`), so Compose aggregates verdicts, not risk.
- CI cannot run `tools/build_ebpf.py --check`, because the manifest pins clang 23.1.1. It
  verifies the source and object hashes instead; byte reproducibility stays a local check.

## Deviations from the plan

- **Moved to slice 26:**
  - Container image and MCP registry entry. Only `/proc` and config discovery support a host
    root today, so inside a container, systemd units, packages and mounts would describe the
    container. They ship with slice 26's host-root mode and daemon.
- **Smaller than planned:**
  - Stress fixture: 1 000 processes / 4 000 sockets instead of 5 000 / 20 000, so the test
    stays under ~30 s in debug CI. It still exceeds every cap.
- **Verified locally before the first tag:**
  - The static musl build (`rust:1-alpine`) produced a static-pie binary that passes the
    five-distro smoke test (Debian 12, Ubuntu 24.04, Fedora 41, Arch, Alpine).
  - `install.sh` plus `twin setup` in clean Alpine (wget) and Fedora (curl) containers: under
    1 s each with a local mirror.
  - This found and fixed a release-workflow bug: the static check expected "statically
    linked", but `file` reports "static-pie linked".
- **Released:** v0.2.0 published by the release workflow: x86_64 and aarch64 musl
  archives, `.deb`, `.rpm`, `SHA256SUMS`, `install.sh`, plus provenance attestations.
  - The first real install from GitHub failed safely: the per-archive `.sha256` files were
    not uploaded, so `install.sh` refused to install.
  - Repaired by uploading checksums derived from the release's own verified `SHA256SUMS`;
    the workflow now uploads `*.tar.gz.sha256`.
  - The one-line install plus `twin setup` from GitHub takes 30 s on a clean Fedora 41
    container (including download), and works on Alpine 3.20.
- **Demo:** `tools/record_demo.py` records `docs/demo.cast` from real twin output on a
  synthetic host (fake `/proc`, units, Docker fixture), so no machine data is published.

## Verification

- `twin-app`: preflight 18 (file changes, unmount/wipe, interfaces, Compose, scripts/make);
  intent unit tests 7; `why` and `scan_graph` updated.
- `twin-collectors`: interface parser 3.
- `twin-container`: Compose label allowlist with a sentinel label.
- `twin-cli`: hook 7, agent_integration 6 (setup, auto-init), output renderers (preflight,
  brief), and the output-budget stress test.
- Full workspace: build, tests, strict clippy and fmt; see the final run below.
