# twin

**Know what breaks before you touch a Linux box. Built for the AI agents doing the touching.**

`twin` keeps a read-only, evidence-backed map of a Linux host: services, processes, listening
ports, live connections, containers, config files, packages, Kubernetes objects, and what
depends on what. One call tells you what is going on, or what a command would break, before you
run it.

```console
$ twin preflight "sudo systemctl restart docker"
twin preflight
══════════════

  fail          STOP  restart docker — 1 dependent(s) interrupted: shop-web-1
  —             command  sudo systemctl restart docker

STOP · restart docker
  ├──  target          service:docker.service
  ├──  risk            high · evidence weak (15/100)
  ├──  reason          1 running container(s) stop when the Docker daemon restarts
  ├──  affected        shop-web-1 — shop-web-1 stops while the Docker daemon restarts (live-restore disabled)
  └──  advice          validate configuration first: `dockerd --validate`
```

A 20-second recording of real output on a synthetic host is in [`docs/demo.cast`](docs/demo.cast)
(`asciinema play docs/demo.cast`).

Nothing was restarted. `twin` cannot change the host. That guarantee is enforced in code and by the
test suite, which is why it is safe to hand to an agent.

---

## Why

Agents (Claude Code, Codex, Cursor, Gemini CLI, Copilot) are now routinely asked to "fix nginx",
"free some disk" or "upgrade openssl" on real machines. To do that they run 10–20 shell commands
(`systemctl status`, `ss -tlnp`, `lsof`, `docker ps`, `cat` on unit files) to rebuild a picture
of the host, then run mutating commands without knowing who depends on what.

`twin` collapses the investigation into one call and adds the missing step: **a blast-radius
check before every mutating command.**

| Without twin | With twin |
|---|---|
| `systemctl status` + `ss -tlnp` + `lsof -i` + `cat /etc/...` + `docker ps` + `journalctl` | `twin brief nginx` |
| "restarting docker should be fine" | `twin preflight "systemctl restart docker"` → STOP, names every container that goes down |
| "what changed since it broke?" across logs and package history | `twin what-changed --since 1h --target api` |
| Hope the agent doesn't `rm -rf` a data dir | hook auto-checks every agent shell command |

---

## Quick start

```bash
# Static binary for x86_64/aarch64 Linux, checksum-verified, no sudo, into ~/.local/bin
curl -fsSL https://github.com/KM-Alee/twin-ops/releases/latest/download/install.sh | sh

twin setup claude-code         # create twin's store, scan this host, print agent config
twin brief                     # what runs here, what's exposed, what's fragile
twin brief postgresql          # everything about one service/container/port/file
twin preflight "apt remove openssl"
```

Releases also ship `.deb` and `.rpm` packages, and `packaging/aur/PKGBUILD` builds `twin-bin`.
From source: `cargo build --release`. Commands create twin's own store on first use; an
existing store is only migrated by `twin init`.

For full process and socket coverage, run as root or give the binary read capabilities:
`sudo setcap cap_dac_read_search,cap_sys_ptrace+ep "$(command -v twin)"`. Without them,
`twin` still works and reports hidden processes as **unknown**, never as absent. `twin setup`
tells you how many are hidden.

On a remote machine, any MCP client can run `ssh host twin mcp` as its server command.

---

## Plug it into your agent

`twin setup <agent>` prepares twin and prints ready-to-paste config; `twin agent-config
<agent>` prints only the config. Neither writes your agent's files, so you review and merge the
snippets yourself.

```bash
twin agent-config claude-code   # MCP server + PreToolUse hook (shell and file tools) + SKILL.md
twin agent-config codex         # MCP server + PreToolUse hook (shell and apply_patch) + AGENTS.md
twin agent-config cursor        # MCP server + beforeShellExecution hook
twin agent-config gemini        # MCP server + BeforeTool hook
twin agent-config vscode        # MCP server for Copilot agent mode
twin agent-config generic       # AGENTS.md instructions for any shell-capable agent
```

There are three integration layers. Use any or all of them:

1. **MCP server** (`twin mcp`). Works with every MCP client and provides seven read-only tools:

   | Tool | Use it to |
   |---|---|
   | `twin_overview` | orient on a machine: failed units, network exposure, hub services, containers, recent changes |
   | `twin_brief` | get everything about one target in a single call |
   | `twin_preflight` | predict a command's blast radius (go / caution / stop) |
   | `twin_changes` | see restarts, config edits, listener and package changes in a window |
   | `twin_why` | see the evidence chain behind a dependency |
   | `twin_find` | look up exact ids by name |
   | `twin_check` | run `twin.yaml` health contracts |

   Every tool is annotated `readOnlyHint: true`. Answers are bounded, typically 1–3 KB.

2. **Pre-command hook** (`twin preflight --hook <agent>`). The agent's own hook system sends
   each shell command **and each file edit made with its own tools** (Claude Code
   Write/Edit/MultiEdit, Codex `apply_patch`, Gemini `write_file`/`replace`) to twin before
   it runs. Cursor has no pre-edit hook, so its file edits are not checked in advance.
   - **go**: silent.
   - **caution**: the reasons are added to the agent's context.
   - **stop**: the human is asked to confirm, or the command is blocked where the agent can't
     ask. Pass `--on-stop deny|ask|warn` to choose.

   The hook fails open: if twin is unavailable, your agent keeps working.

3. **Skill / AGENTS.md**. Plain instructions that teach an agent when to reach for twin, for
   agents without MCP or hooks.

---

## What `preflight` understands

| Area | Commands |
|---|---|
| Services | `systemctl`, `service`, `nginx -s`, `apachectl` |
| Containers | `docker`/`podman`/`nerdctl`, `docker compose`/`docker-compose` (resolved to the project's containers) |
| Files | `rm`, `unlink`, `shred`, `sed -i`, `perl -i`, `tee`, `truncate`, `mv`, `cp`, `chmod`, `chown`, `>`/`>>` |
| Packages | `apt`, `apt-get`, `dnf`, `yum`, `zypper`, `apk`, `pacman`, `dpkg`, `rpm` |
| Processes | `kill` (including `-0`, `-STOP`, `-HUP`), `pkill`, `killall` |
| Storage | `umount`, `mount -o remount,ro`, `mkfs*`, `wipefs`, `dd of=/dev/…`, `parted`, `sgdisk` |
| Network | `ip link/addr/route`, `ifdown`, `nmcli`, `ufw`, `iptables`, `nft`, `firewall-cmd` |
| System | `sysctl -w`, `/proc/sys` writes, `swapoff`, `crontab`, `userdel`, `usermod -L`, `reboot`, `shutdown` |
| Kubernetes | `kubectl`/`oc` rollout, delete, scale, apply, drain |
| Scripts | `./deploy.sh`, `bash x.sh`, `source x.sh`, `make TARGET`: every line is evaluated |

It sees through `sudo`/`doas`, env assignments, `nohup`/`timeout`/`env`, `&&`/`||`/`;`/pipes,
nested `bash -c '…'` and heredocs. In scripts, only lines that are operations are shown; lines
that depend on shell variables are flagged as not assessable rather than guessed. Direct file
changes can be checked with `twin preflight --file PATH --op edit|delete|create`.

Examples of what it catches:

- **Killing a pid** is resolved to its owning service, with that service's dependents.
- **Restarting Docker** lists every running container that stops (it reads `live-restore`).
- **`rm -rf /var/lib/docker`** is a stop: a running service reads data under that path.
- **Editing `/etc/nginx/nginx.conf`** reports that nginx reads the file and suggests `nginx -t`.
- **Stopping sshd or NetworkManager** is a stop: you can lock yourself out.
- **`ufw deny 5432`** finds the listener and its connected clients.
- **`ip link set eth0 down`** is a stop when that interface carries the default route.
- **`umount /srv` or `mkfs` on its device** names every service using paths on that mount.
- **`docker compose down`** finds the project's containers from their Compose labels.
- **`pacman -Syu` / `apt upgrade`** is a caution: twin explains it can't see the package
  manager's plan, and suggests preflighting specific packages.

Exit codes: `0` go, `3` caution, `4` stop, `1` error. Use it in scripts too:
`twin preflight "$cmd" && eval "$cmd"`.

---

## Principles

- **Read-only forever.** No restarts, kills, writes to the host, firewall changes, installs, or
  container/Kubernetes mutations. `twin-safety` scans every production source file for
  execution primitives (`Command::new`, `kill`, Kubernetes `patch`/`delete`). Only the preflight
  parser may name commands such as `systemctl restart`, and only as text to recognize.
- **Evidence first.** Every relationship cites its observations and an evidence strength;
  observed facts are kept separate from inferences.
- **Risk ≠ confidence.** "How bad" and "how sure" are reported separately.
- **Unknown is not absent.** Permission gaps, unmapped sockets and unavailable collectors are
  reported as coverage gaps and lower confidence. They never turn into a green result.
- **Secrets never stored.** Process argument values, environment variables, Kubernetes secret
  data and log bodies are never persisted.

---

## All commands

| Command | Purpose |
|---|---|
| `twin brief [target]` | one-call host overview or target briefing |
| `twin preflight "<cmd>"` | predict a command's blast radius |
| `twin mcp` | MCP server for agents |
| `twin setup <agent>` | first run: store, scan, visibility check, agent snippets |
| `twin agent-config <agent>` | print agent setup snippets |
| `twin scan` / `twin watch` | collect host state once, or continuously (optionally with eBPF) |
| `twin graph [target]` | explore the dependency graph (also `--format dot|mermaid`) |
| `twin impact <target>` | dependents of a service, port or package |
| `twin emulate restart|delete|fill-disk|upgrade|rollout` | detailed what-if overlays |
| `twin what-changed --since 1h [--target X]` | incident timeline |
| `twin why <target>` | evidence chains |
| `twin snapshot` / `twin diff` | baselines and structural diffs |
| `twin test run twin.yaml` | health contracts; see [operational workflows](docs/operational-workflows.md) |
| `twin k8s scan|graph|impact|watch|logs` | read-only Kubernetes |
| `twin doctor` | permissions, collectors, eBPF/Docker/Kubernetes availability |

Add `--json` to any command for machine-readable output. Lists are capped at 100 items with a
`<field>_omitted` count beside them; add `--full` to get everything.

---

## How it works

```text
read-only collectors (/proc, systemd + D-Bus, sockets, Docker/containerd, K8s, packages, eBPF)
        ↓ observations (facts, with source + timestamp)
temporal dependency graph (SQLite, retained history)
        ↓
overlay emulation (hypothetical; never persisted, never executed)
        ↓
brief · preflight · impact · why · what-changed · contracts
```

State lives under XDG paths (`~/.config/twin/config.toml`, `~/.local/share/twin/twin.db`).

## Development

```bash
cargo build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check
```

Tests use fixtures (fake `/proc`, systemd, Docker, Kubernetes, eBPF streams); no root, Docker
or cluster required. See [AGENTS.md](AGENTS.md) for conventions and [docs/](docs/) for design.

## License

MIT
