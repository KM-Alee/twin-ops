# Slice 8.6 — Pre-implementation research

Research agent: [Slice 8.6 web research](3fe9a488-3235-47d6-9e6e-d5df109686d2) — **complete**.

Implementation plan body: user-provided slice 8.6 spec (sections 1–11). This file captures **authoritative external context** and **codebase deltas** post–8.5.

---

## Executive summary

1. **`/proc/net/unix` is not TCP.** The `St` column is *not* `0A` for listeners; use **`Flags == 0x00010000`** (`__SO_ACCEPTCON` when `sk_state == TCP_LISTEN`) to detect listen sockets. `St` is `SS_*` (00–03), not TCP states.
2. **Path is always the last field** and may contain spaces; parse by fixed columns through `Inode`, then take the remainder as path. Abstract sockets start with `@` in the Path column.
3. **systemd D-Bus:** bus `org.freedesktop.systemd1`, manager `/org/freedesktop/systemd1`, `ListUnits()` for loaded units; per-unit `Requires`/`Wants`/`ActiveState`/`ControlGroup` are **`org.freedesktop.systemd1.Unit` properties** (`as` / `s`), read via `GetUnit` → unit object path → `DBus.Properties.Get`.
4. **zbus:** use `zbus` with default features (includes `blocking-api`); `Connection::system()` + `#[proxy]` for sync `twin scan`. Gate real bus behind trait; mock in tests. Avoid `libsystemd`/`sd-bus` (linking, read-only policy surface).
5. **Enable graph:** scan `**/*.wants/*.service` and `**/*.requires/*.service` under the same search roots as 8.5; symlink name encodes target unit; weak edges = `Wants` class, `.requires` = stronger.
6. **`ListUnits()` is not enough for deps.** Tuple `a(ssssssouso)` has no `Requires`/`Wants`; fetch per-unit `Unit` properties after `GetUnit` (or use path from tuple field 7).
7. **Unix runtime inference is weaker than TCP.** Client rows often have an empty `Path`; match via listener-path map + declared `ListenStream=`, and record coverage gaps instead of guessing.
8. **Parser reference:** [prometheus/procfs `net_unix.go`](https://github.com/prometheus/procfs/blob/master/net_unix.go) (`netUnixFlagListen = 1 << 16`).

---

## `/proc/net/unix` reference

**Sources:** [proc_net(5)](https://man.archlinux.org/man/proc_net.5.en), [proc_pid_net(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_net.5.html), kernel `unix_seq_show()` in [net/unix/af_unix.c](https://github.com/torvalds/linux/blob/master/net/unix/af_unix.c).

### Header and columns

```text
Num RefCount Protocol Flags Type St Inode Path
```

| Field | Meaning |
|-------|---------|
| Num | Kernel slot (`NN:` prefix on row) |
| RefCount | Socket refcount |
| Protocol | Always 0 today |
| Flags | `__SO_ACCEPTCON` (0x10000) when listening |
| Type | 0001 stream, 0002 dgram, 0005 seqpacket |
| St | Internal `SS_*` state, **not** TCP_LISTEN |
| Inode | Join key → `/proc/<pid>/fd/*` → `socket:[inode]` |
| Path | Bound path; abstract shown as `@…`; may be empty |

### Example rows (from man page)

```text
 0: 00000002 00000000 00000000 0001 03    42
 1: 00000001 00000000 00010000 0001 01  1948 /dev/printer
```

Row 1: `Flags=00010000` → listener on `/dev/printer`.

### Listen vs connected (twin parser rules)

| Role | Detection |
|------|-----------|
| **Listener** | `Flags & 0x10000 != 0` (and prefer `Type == 0001` stream for depends_on MVP) |
| **Connected client** | Accept bit clear, `St == 03` (SS_CONNECTED), inode ≠ 0 — emit `UnixConnectionSeen`; infer `depends_on` only when client path matches a known listener path (many clients have **empty** path) |

**Do not** map `St == 01` to listen (common mistake; listening unix sockets often show `St=01`).

### Path → `NodeId`

| Proc Path column | Canonical node ID |
|------------------|-------------------|
| `/run/dbus/system_bus_socket` | `unix:/run/dbus/system_bus_socket` |
| `@00007f8c2c1a3b00` (abstract) | `unix:@00007f8c2c1a3b00` (opaque; label in CLI) |

Normalize filesystem paths with existing `lexical_canonical()` (no trailing slash).

### Inode join recipe (same as TCP)

```text
/proc/net/unix  →  inode
  → walk /proc/*/fd  →  socket:[inode]
  → process:pid:N
  → cgroup  →  service:unit (existing 8.5 path)
```

### Edge cases

- **Empty path:** no `unix:` node; count as unmapped / warning only.
- **Path with spaces/tabs:** treat everything after inode field as path (split from the right or skip 7 fixed tokens after `Num:`).
- **Dgram (0002):** collect observations; skip stream-only depends_on inference unless tested.
- **Permission:** same as TCP — unreadable fds → `UnixSocketUnmapped` warning.

---

## systemd D-Bus cheat sheet (read-only)

**Sources:** [org.freedesktop.systemd1](https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.systemd1.html), [systemd D-Bus wiki](https://www.freedesktop.org/wiki/Software/systemd/dbus/).

| Step | D-Bus |
|------|-------|
| Bus | system bus |
| Service | `org.freedesktop.systemd1` |
| Manager path | `/org/freedesktop/systemd1` |
| List loaded units | `Manager.ListUnits()` → `a(ssssssouso)` — **no dependency arrays in tuple** |
| Resolve unit | `Manager.GetUnit(s name)` → object path e.g. `/org/freedesktop/systemd1/unit/docker_2eservice` |
| Cgroup lookup | `Manager.GetUnitByControlGroup(s path)` (validation only in 8.6) |
| Requires / Wants | Unit property `Requires` / `Wants` → **`as`** (unit names) |
| Runtime state | `ActiveState`, `SubState`, `LoadState` → **`s`** |
| Cgroup | `ControlGroup` → **`s`** |
| Fragment | `FragmentPath` → **`s`** |

Properties live on **`org.freedesktop.systemd1.Unit`**; access via `org.freedesktop.DBus.Properties.Get(interface, name)`.

### Read-only vs forbidden

| Allowed | Forbidden (never call) |
|---------|-------------------------|
| `ListUnits`, `ListUnitsByNames`, `GetUnit`, `GetUnitByPID`, `GetUnitByControlGroup` | `StartUnit`, `StopUnit`, `ReloadUnit`, `RestartUnit`, `KillUnit`, `ResetFailed` |
| Property reads | `EnableUnitFiles`, `DisableUnitFiles`, `LinkUnitFiles`, `SetProperty` |

**File-first, bus-second:** 8.5 unit files always run; D-Bus enriches **loaded** units and effective deps. Dedup edges by `EdgeId` (`from|depends_on|to`); second source links observation only.

### Permissions / degradation

| Environment | Behavior |
|-------------|----------|
| Normal desktop (root or in systemd cgroup) | System bus usually works for reads |
| User session | Use **system** bus for system units, not session bus |
| Container / no systemd | Skip D-Bus + enable scan under container paths; unix still works |
| Permission denied | Warning `systemd_dbus_unavailable`; continue scan |

Polkit rarely blocks property reads; failures are usually missing bus/socket.

---

## zbus implementation notes

**Sources:** [zbus blocking book](https://github.com/dbus2/zbus/blob/main/book/src/blocking.md), [systemd-zbus](https://crates.io/crates/systemd-zbus) (optional).

### Cargo.toml (`twin-collectors`)

```toml
zbus = { version = "5", default-features = true }  # keeps blocking-api; MSRV 1.87 on zbus 5.16
```

Do **not** disable default features on `zbus` (drops `blocking-api` since 5.0). Set `method_timeout` (e.g. 5s) on the scan connection. `LoadUnit` is read-only for twin purposes but loads unit state into PID 1 — prefer file parse for unloaded units.

### Minimal pattern (sync scan)

```rust
use zbus::{blocking::Connection, proxy, Result};

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn list_units(&self) -> Result<Vec<(String, String, String, String, String, String,
        zbus::zvariant::OwnedObjectPath, u32, String, zbus::zvariant::OwnedObjectPath)>>;
    fn get_unit(&self, name: &str) -> Result<zbus::zvariant::OwnedObjectPath>;
}

// Unit properties: second proxy on dynamic path with #[zbus(property)] active_state, requires, wants, control_group
```

### Test strategy

```rust
trait SystemdDBusReader {
    fn list_units(&self) -> Result<Vec<UnitSnapshot>, SystemdDBusError>;
}
```

Fixture JSON in `twin-fixtures`; production `StdSystemdDBusReader` wraps `Connection::system()`.

### Why not `libsystemd` / `sd-bus`

- Extra native link + distro variance
- Harder hermetic CI
- `zbus` is pure Rust, matches project dep policy; read-only surface is small (properties only)

---

## Enable symlink scan

**Reuse** `default_search_paths()` from `unit_paths.rs` (8.5).

### Algorithm

```text
for root in search_roots:
  for entry in root.read_dir():
    if entry.name ends with ".wants" or ".requires":
      target_unit = stem before suffix  # multi-user.target
      for link in entry.read_dir().filter(*.service):
        resolve symlink → enabled unit name
        emit SystemdUnitWantedBy / weak depends_on
        metadata: enable_symlink, symlink_path, kind=wants|requires
```

| Directory suffix | Edge semantics |
|------------------|----------------|
| `.wants` | Weak (`Wants` class), moderate evidence |
| `.requires` | Stronger than wants, still `Observed` |

Example: `multi-user.target.wants/docker.service` → `multi-user.target` **Wants** `docker.service` (enable-time; complements unit-file `WantedBy=`).

---

## Socket activation (minimal)

From unit file only (extend 8.5 parser):

- `[Socket]` `ListenStream=` / `ListenDatagram=` → unix path observations
- `Service=` in socket unit → `depends_on` observed, metadata `socket_activation: true`
- D-Bus `TriggeredBy` on loaded units can corroborate later

---

## Codebase alignment (post–8.5)

| Area | Current | 8.6 action |
|------|---------|------------|
| `NodeId` | `port:tcp:` only | Add `unix:` + `NodeKind::UnixSocket` |
| `twin-observation` | TCP + unit file | Add `ProcNetUnix`, `SystemdDBus`, `SystemdEnableSymlink`, unix kinds |
| `process/socket.rs` | TCP parser | Add `unix.rs` or extend module; **Flags-based listen** |
| `scan.rs` | TCP block ~250–500 | Parallel unix block + `listeners_by_unix_path` map for inference |
| `systemd/` | `SystemdUnitCollector` | `dbus.rs`, `enable_symlinks.rs`, orchestration |
| PRD example | `socket:unix:…` | Implement **`unix:`** per tech doc; note PRD drift in slice state |
| Graph/impact CLI | `port:tcp:` | Add `unix:` prefix parsing |

---

## Pitfalls and degradation

| Condition | Behavior |
|-----------|----------|
| No systemd | Skip D-Bus + enable; unix OK |
| D-Bus denied | Warning; file + unix only |
| No `/proc/net/unix` | Warning; TCP-only |
| Abstract socket | Opaque `unix:@…`; unmapped if no fd join |
| Template units `foo@.service` | ListUnits returns instance names; handle in 8.6 MVP as literal names |
| Duplicate Requires file + D-Bus | One edge, `evidence_count` + dual observation links |

---

## Implementation order (validated)

| Step | Work | Research note |
|------|------|----------------|
| 1 | `unix:` NodeId + parser | **Flags 0x10000** for listen; path = remainder after inode |
| 2 | Inode join + listens_on/connects_to | Reuse `owners_by_inode` from process batch |
| 3 | Unix depends_on inference | `listeners_by_unix_path` map (mirror TCP); honest gaps when client path empty |
| 4 | Enable symlink scanner | Same roots as unit files; `.wants`/`.requires` |
| 5 | `SystemdDBusReader` mock | JSON fixtures before real bus |
| 6 | zbus reader + reconcile | Property `Requires`/`Wants` as `Vec<String>` |
| 7 | Graph/impact/doctor/CLI | `dbus_available`, `unix_socket_coverage` |

---

## Real-world validation targets (manual, non-CI)

| Target | What to check |
|--------|----------------|
| `dbus-broker.service` | Listens `unix:/run/dbus/system_bus_socket`; many connectors |
| `containerd.service` / `docker.service` | Unix + declared Requires; docker in `multi-user.target.wants` |
| `pipewire.service` | Desktop client unix depends_on dbus |

---

## Source links

- https://man.archlinux.org/man/proc_net.5.en
- https://www.man7.org/linux/man-pages/man5/proc_pid_net.5.html
- https://github.com/torvalds/linux/blob/master/net/unix/af_unix.c
- https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.systemd1.html
- https://www.freedesktop.org/wiki/Software/systemd/dbus/
- https://github.com/dbus2/zbus/blob/main/book/src/blocking.md
- https://crates.io/crates/zbus
- https://unix.stackexchange.com/questions/183140/what-is-the-meaning-of-the-contents-of-proc-net-unix
- https://stackoverflow.com/questions/76937422/linux-af-unix-socket-states
- https://github.com/prometheus/procfs/blob/master/net_unix.go
- https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html
