# Slice 20: Filesystem, Mounts, Disk Usage, and Fill-Disk Emulation

## Status: complete

## Implemented

- Scan reads `{proc}/mounts` and keeps real filesystems. Pseudo filesystems such as `proc`, `sysfs`, and `cgroup2` are skipped. A bad line is skipped. A missing mounts file leaves the rest of the scan alone.
- Each kept mount is a `mount:` node. `used_percent` comes from `statvfs` on that mount, or on the matching directory under `TWIN_HOST_ROOT` when a fixture host root is set. The read does not write to the disk.
- Notable directories that exist (`/var/log`, `/var/lib/postgresql`, `/var/log/journal`, `/var/lib/docker`) become `directory:` nodes with `MOUNTED_ON` edges to the longest matching mount. File nodes under a mount get the same edge.
- When the service is already in the graph, `postgresql.service` `USES` `/var/lib/postgresql`, `docker.service` `USES` `/var/lib/docker`, and `systemd-journald.service` `LOGS_TO` `/var/log/journal`.
- `twin graph mount:/var` shows usage, mounted paths, and the services that use them.
- `twin emulate fill-disk /var --to 95%` builds a `FillMountOverlayBuilder` overlay. The mount is marked hypothetically filled. Affected services are listed. Nothing is written and the stored graph is unchanged. A target of 80% or more is critical risk.
- Disk YAML checks from slice 18 still use the same `statvfs` helper.

## Decisions

- Service path edges are a fixed table of conventional data and log directories. They are inferred, and only when that directory exists and the service node already exists.
- Fill risk is critical at 80% and above, high from 50% through 79%, and medium below that. The slice example treats an 80% fill as critical.
- A relative fill path is rejected. A mount that is not in the graph still produces a report, with weakened evidence, and does not invent affected services.

## Deviations from Plan

- The disk YAML check was already implemented in slice 18. This slice reuses it.
- Only the four notable directories above are created as directory nodes. Other directories appear when a file node already sits on the mount.

## Acceptance Checklist

- [x] Mount nodes exist
- [x] Disk usage is visible on the mount node and in `twin graph`
- [x] Fill-disk emulation lists affected services and writes nothing
- [x] Disk checks still evaluate `used_percent`
- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
