# Slice 21: Libraries, Packages, and Upgrade Emulation

## Status: complete

## Implemented

- Scan reads `{proc}/{pid}/maps` for each collected process. Absolute pathnames become `library:` nodes with observed `loads_library` edges from the process. `[heap]`, `[stack]`, `[vdso]`, `[vvar]`, `[vsyscall]`, other `[...]` entries, empty pathnames, and non-absolute pathnames are skipped. A bad line is skipped. A missing maps file leaves that process without library edges.
- Package adapter v1 reads the dpkg database only: `{TWIN_HOST_ROOT}/var/lib/dpkg/status` and `{TWIN_HOST_ROOT}/var/lib/dpkg/info/*.list`. It does not execute dpkg, apt, rpm, or pacman. Installed packages that own a mapped library become `package:` nodes with observed `installed_by` edges. If the status file is absent or unreadable, scan records a warning and continues.
- When a service owns a process that loads a library installed by a package, scan infers `service DEPENDS_ON package`.
- `twin graph libssl.so.3` resolves a library by basename, or by a `.so` / path suffix. `twin graph library:/usr/lib/libssl.so.3` addresses the node directly.
- `twin impact package:openssl` lists the services that depend on the package (restart blast radius). Existing service and port impact is unchanged.
- `twin emulate upgrade package:openssl` builds an `UpgradePackageOverlayBuilder` overlay. Runtime impact is LOW when a packaged library is already mapped, with the reason that running processes already have that library mapped. Restart impact is HIGH when one or more services depend on the package, and those services are listed. A package that is not in the graph returns a report with weakened evidence and no invented services. `action_performed` is false. The overlay is not stored. The safety line says no package manager ran and nothing was upgraded.

## Decisions

- Every absolute mapped pathname is a library node, including the process executable. The slice's skip list is pseudo entries and non-absolute paths, so the scan does not guess which files are shared objects.
- A package node is created only when an installed dpkg package's file list contains a library that a process has mapped. Files that are listed but not mapped do not become library nodes.
- The dpkg database is `{host}/var/lib/dpkg/status`. Host paths use `TWIN_HOST_ROOT` the same way config discovery does. Pacman and rpm parsers are not included.
- Library lookup prefers an exact basename match. If several libraries share that basename, the lexicographically smallest node id wins.
- Runtime versus restart does not compare ELF versions. Runtime is LOW when the graph shows the packaged library mapped. Restart is HIGH when `depends_on` edges from services exist. The library name in the runtime reason is the path stem before `.so` (`libssl.so.3` → `libssl`).
- Package impact stops at the direct dependent services. It does not reuse the service transitive path walk.
- Library and package rows are written like mounts: graph nodes and edges, without a new observation kind.

## Deviations from Plan

- Human output uses the shared title, status, and tree helpers. The upgrade report's section headers are `Runtime impact: LOW`, `Restart impact: HIGH`, and `Affected on restart:`.
- The plan says "dpkg first, then pacman/rpm later". This slice warns and continues when the dpkg database is missing. It does not parse other package managers.
- Upgrade emulation of an unknown package returns a report instead of a hard error, so a missing node does not look like a crash and does not invent services.

## Acceptance Checklist

- [x] Loaded libraries are visible
- [x] Package ownership works on dpkg file data
- [x] Unsupported or missing package databases warn, and the scan still succeeds
- [x] Upgrade emulation does not run a package manager and does not change the stored graph
- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
