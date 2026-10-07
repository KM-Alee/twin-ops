# twin

**A read-only operational twin for Linux.**

`twin` builds an evidence-backed dependency graph of what is running on your machine, then lets you explore it and simulate risky operations — without changing anything on the host.

Most tools answer *what is happening right now*. `twin` answers:

> What does this machine depend on, what changed, and what would likely break if I did this?

---

## What it is

`twin` is a Rust CLI for sysadmins, DevOps engineers, and Linux power users who need to reason about blast radius before touching production.

| `twin` is | `twin` is not |
|-----------|---------------|
| A local, read-only dependency graph | A monitoring dashboard |
| Evidence-backed impact analysis | A process manager or remediation tool |
| Overlay-based “what if” emulation | A VM sandbox or K8s platform |
| A `top` / `htop` / `lsof` replacement | — |

Every conclusion separates **observed facts** from **inferred conclusions**, and reports **risk** separately from **evidence strength**.

---

## Quick start

### Requirements

- Linux (reads `/proc`, systemd, socket tables, and more)
- [Rust](https://rustup.rs/) toolchain (edition 2021)

### Build and run

```bash
git clone <repo-url>
cd twin-ops
cargo build --release

# First-time setup
./target/release/twin init
./target/release/twin doctor

# Collect system state into the local graph
./target/release/twin scan

# Explore
./target/release/twin graph process
./target/release/twin graph nginx
./target/release/twin impact postgresql
```

Add `--json` to any command for machine-readable output.

### Local state

`twin` stores config and observations under XDG paths:

```text
~/.config/twin/config.toml    # configuration
~/.local/share/twin/twin.db   # SQLite graph store
```

---

## Commands

### `twin init`

Create local config and database layout.

```bash
twin init
twin init --force          # overwrite existing config
```

### `twin doctor`

Check CLI, config, database, permissions, and scan quality.

```bash
twin doctor
twin doctor --json
```

### `twin scan`

Read the host and persist nodes, edges, and evidence into the graph.

```bash
twin scan
twin scan --samples 5 --interval 2   # merge short-lived connections across samples
```

Collectors cover processes, file descriptors, listening sockets, systemd units, and config file discovery. Missing permissions produce coverage gaps — the scan continues and reports what was hidden.

### `twin watch`

Rescan on an interval and print operational changes. The host is not modified.

```bash
twin watch
twin watch --interval 5s
twin watch --duration 10m
twin watch --ticks 3
```

Stops on Ctrl+C, when `--duration` elapses, or after `--ticks` scans.

### `twin graph`

Inspect the dependency graph.

```bash
twin graph                          # list processes (default)
twin graph --kind service           # list systemd services
twin graph 1234                     # neighborhood around a PID
twin graph process:pid:1234         # same, by canonical node id
twin graph service:nginx.service    # service and its dependencies
twin graph file:/etc/nginx/nginx.conf
```

### `twin impact`

Show what depends on a service or port — runtime, restart, and configured impact buckets.

```bash
twin impact postgresql
twin impact port:tcp:127.0.0.1:5432
```

### `twin emulate`

Hypothetically perform an action using a graph overlay. **Nothing is changed on the host.**

```bash
twin emulate restart postgresql
twin emulate restart service:postgresql.service

twin emulate delete /etc/nginx/nginx.conf
twin emulate delete file:/etc/nginx/nginx.conf
```

Emulation applies a temporary overlay on top of the scanned graph. Overlays are never persisted.

---

## How it works

```text
read-only collectors
        ↓
evidence observations
        ↓
temporal operational graph  (SQLite)
        ↓
overlay-based emulation
        ↓
impact, risk, and evidence reports
```

**Collectors** read `/proc`, systemd, socket tables, and config paths. They emit observations — not conclusions.

**The graph** connects processes, ports, services, files, and sockets with typed edges and linked evidence.

**Emulation** patches a hypothetical overlay onto the base graph to predict blast radius for restarts, file deletion, and (planned) network blocks — without cloning or mutating the system.

---

## Safety

`twin` is **read-only forever**. It will never:

- kill, restart, or stop processes or services
- edit, delete, or create files
- change firewall rules or block traffic
- install packages or mutate containers
- apply, patch, or delete Kubernetes resources

Safety constraints are enforced in code and checked by automated scans in `twin-safety` tests. If a collector cannot read something (permissions, vanished PID), it records a coverage gap and moves on — no panics, no crashes.

---

## Project status

Development follows [23 vertical slices](docs/plan-slices.md). Each slice ships a working CLI command with tests.

**Implemented today**

| Area | Commands / behavior |
|------|---------------------|
| Foundation | `init`, `doctor`, config, SQLite store |
| Scan | processes, FDs, sockets, systemd, config files |
| Graph | process/service/file neighborhoods |
| Impact | service and port blast radius |
| Emulate | `restart`, `delete` with overlay impact |
| Temporal | `what-changed`, `snapshot`, `diff` |
| Watch | polling `twin watch` (interval, duration, Ctrl+C) |

**Planned** (see [PRD](docs/prd.md))

```bash
twin watch --ebpf
twin why nginx
twin emulate block endpoint api.stripe.com:443
twin test run twin.yaml
twin k8s graph deployment/api
```

---

## Development

### Workspace crates

```text
twin-cli          # binary + human/JSON output
twin-app          # command orchestration
twin-core         # domain types, config
twin-store        # SQLite persistence
twin-observation  # observation vocabulary
twin-collectors   # read-only host collectors
twin-emulate      # overlay-based impact emulation
```

Dependency direction flows inward: `cli → app → {core, store, collectors, emulate}`. Domain types live in `twin-core`; collectors never write conclusions directly.

### Build and test

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Integration tests use `twin-fixtures` with fake `/proc`, systemd, and socket data — no root, Docker, or live K8s required.

### Documentation

| Document | Purpose |
|----------|---------|
| [docs/prd.md](docs/prd.md) | Product requirements and vision |
| [docs/tech-document.md](docs/tech-document.md) | Technical design |
| [docs/plan-slices.md](docs/plan-slices.md) | Incremental build plan |
| [docs/code-layout.md](docs/code-layout.md) | Module layout and test conventions |
| [AGENTS.md](AGENTS.md) | Contributor and agent conventions |

---

## License

MIT
