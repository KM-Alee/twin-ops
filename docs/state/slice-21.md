# Slice 21: Libraries, Packages, and Upgrade Emulation

## Status: complete

## Implemented

- Scan reads `{proc}/{pid}/maps` for each collected process. Absolute pathnames become `library:` nodes with observed `loads_library` edges from the process. `[heap]`, `[stack]`, `[vdso]`, `[vvar]`, `[vsyscall]`, other `[...]` entries, empty pathnames, and non-absolute pathnames are skipped. A bad line is skipped. A missing maps file leaves that process without library edges.
- Package adapter reads dpkg first: `{TWIN_HOST_ROOT}/var/lib/dpkg/status` and `{TWIN_HOST_ROOT}/var/lib/dpkg/info/*.list`. It does not execute dpkg, apt, rpm, or pacman. Installed packages that own a mapped library become `package:` nodes with observed `installed_by` edges. If the dpkg status file is absent, scan falls back to the pacman local database at `{TWIN_HOST_ROOT}/var/lib/pacman/local/<name-version>/desc` and applies the same rule to its `%FILES%` paths (made absolute and canonicalised). Pacman uses the same `package:` nodes, `installed_by` edges and `depends_on` inference as dpkg. If the dpkg status is absent or unreadable and no pacman database is present, scan records a warning and continues. Malformed pacman entries are counted in a `package_manager_malformed` warning and the scan continues. The unsupported-distro warning is included in `warning_count` so the human scan report shows it.
- When a service owns a process that loads a library installed by a package, scan infers `service DEPENDS_ON package`.
- `twin graph libssl.so.3` resolves a library by basename, or by a `.so` / path suffix. `twin graph library:/usr/lib/libssl.so.3` addresses the node directly.
- `twin impact package:openssl` lists the services that depend on the package (restart blast radius). Existing service and port impact is unchanged.
- `twin emulate upgrade package:openssl` builds an `UpgradePackageOverlayBuilder` overlay. Runtime impact is LOW when a packaged library is already mapped, with the reason that running processes already have that library mapped. Restart impact is HIGH when one or more services depend on the package, and those services are listed. A package that is not in the graph returns a report with weakened evidence and no invented services. `action_performed` is false. The overlay is not stored. The safety line says no package manager ran and nothing was upgraded.

## Decisions

- Every absolute mapped pathname is a library node, including the process executable. The slice's skip list is pseudo entries and non-absolute paths, so the scan does not guess which files are shared objects.
- A package node is created only when an installed package's file list (dpkg `*.list` or pacman `%FILES%`) contains a library that a process has mapped. Files that are listed but not mapped do not become library nodes.
- Package databases are read at `{host}/var/lib/dpkg/status` and `{host}/var/lib/pacman/local`. Host paths use `TWIN_HOST_ROOT` the same way config discovery does. Dpkg wins when both databases exist: pacman is only scanned when the dpkg status file is absent, because both describe the same set of installed libraries and the graph must not hold two owners for one library. An rpm parser is not included.
- Pacman `%FILES%` entries are relative to `/`, so they are prefixed with `/` and canonicalised like dpkg list paths. The parser reads only `%NAME%` and `%FILES%`; an entry without a non-empty `%NAME%` or with an unreadable `desc` counts as malformed and is skipped. The malformed count is reported with the `package_manager_malformed` warning kind and a `package_manager` detail whose path is the pacman local directory.
- An unreadable pacman local directory is treated as no pacman database, so the unsupported warning still fires. The warning text now says "no readable dpkg or pacman database is present".
- Library lookup prefers an exact basename match. If several libraries share that basename, the lexicographically smallest node id wins.
- Runtime versus restart does not compare ELF versions. Runtime is LOW when the graph shows the packaged library mapped. Restart is HIGH when `depends_on` edges from services exist. The library name in the runtime reason is the path stem before `.so` (`libssl.so.3` → `libssl`).
- Package impact stops at the direct dependent services. It does not reuse the service transitive path walk.
- Library and package rows are written like mounts: graph nodes and edges, without a new observation kind.

## Deviations from Plan

- Human output uses the shared title, status, and tree helpers. The upgrade report's section headers are `Runtime impact: LOW`, `Restart impact: HIGH`, and `Affected on restart:`.
- The plan says "dpkg first, then pacman/rpm later". This slice now parses pacman when dpkg is absent, and still warns and continues when neither database is present. Rpm is not parsed.
- Upgrade emulation of an unknown package returns a report instead of a hard error, so a missing node does not look like a crash and does not invent services.

## Acceptance Checklist

- [x] Loaded libraries are visible
- [x] Package ownership works on dpkg file data
- [x] Package ownership works on pacman file data when dpkg is absent, with dpkg preferred when both exist
- [x] Malformed pacman entries warn and the scan still succeeds
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

## 2026-10-08 follow-up: Mapped library filtering

Only absolute mappings whose filename ends in .so or contains .so. become library nodes. Executables and arbitrary mapped files are excluded. Existing dpkg/pacman package attribution remains; Modern RPM SQLite support is implemented as described below; legacy formats remain unsupported. Maps and library tests cover the filter.

## Additional 2026-10-08 follow-up

twin-package reads modern RPM SQLite Packages blobs at /usr/lib/sysimage/rpm/rpmdb.sqlite or /var/lib/rpm/rpmdb.sqlite. Name/version/release/epoch and file ownership use bounded big-endian RPM header decoding. Reads use SQLite immutable read-only mode, reject nonempty WAL files, and discard databases that change during the read. No journal/shared-memory file is created. Malformed/unsupported formats produce coverage gaps and retain prior package nodes AND edges touching unavailable-source nodes. dpkg/pacman versions and accurate source metadata also persist. twin-fixtures now provides the RPM header fixture shared by package/app integration tests; proc/systemd support still lives in per-crate tests/support. Tests cover immutable bytes/directory contents, invalid headers/offsets, missing/corrupt databases, WAL rejection, app attribution/version persistence, and outage preservation. Legacy RPM NDB/BDB and active-WAL reads remain unsupported.

Immutable RPM reads check for a nonempty WAL both before and after reading, rejecting concurrent WAL activity. This bounds stale-read acceptance without creating SQLite journal/shared-memory files.


## 2026-10-08 follow-up: active WAL snapshots

RPM SQLite databases with a nonempty WAL are read through a private temporary snapshot. Only source database/WAL bytes are opened for reading; no SQLite connection, lock, journal, or shared-memory file is created beside the source. Inputs and reconstructed database are each capped at 256 MiB. File device/inode/size/mtime/ctime identities must remain stable through copying; changes reject the snapshot with a retry gap. WAL header/version/page size/salts and cumulative checksums are validated before committed frames are replayed. Uncommitted frames never become graph facts. The private recovered database is queried immutably and removed automatically.

Malformed/truncated WAL and stale frames from a reused WAL are conservatively rejected, preserving prior package coverage rather than choosing a partial view. Nonempty rollback journals also reject direct reads. Tests use an active fixture writer to prove committed packages are included, uncommitted deletions excluded, and source database/WAL/shared-memory bytes and directory entries unchanged. Corrupt checksums/salts/truncation are error paths. No real RPM manager runs. Legacy NDB/BDB remains unsupported. tempfile is a production dependency solely to own and clean up private RPM snapshot files.

## 2026-10-09 WAL validation depth

Pure replay regressions construct checksum-valid WALs in both supported checksum byte orders. They verify that valid trailing uncommitted replacements are excluded, later commits include preceding transaction frames, and a WAL without any commit leaves base bytes unchanged. Metadata stability tests reject same-content/same-size inode replacement and changed-size writes. Existing integration checks retain source database/WAL/shm bytes and reject corrupted salts/checksums/truncation and nonempty rollback journals. These checks cover the stability predicate and replay boundaries; they do not establish an atomic snapshot under every concurrent writer schedule.


## 2026-10-09 legacy RPM adapters

Scan discovers `Packages.db` (NDB) and `Packages` (BDB) in `/usr/lib/sysimage/rpm` and `/var/lib/rpm`, after existing SQLite candidates. Magic selects the actual parser; package metadata records `database_format` and the logical `database_path`, alongside RPM source/version. Both adapters read bounded private byte copies and reject file identity/size/mtime/ctime changes; they never open source bytes with SQLite or invoke a package manager. Immutable direct SQLite reads now use the same strengthened identity predicate.

NDB v0 reads every declared slot page, validates slot/package/blob identity, unique package IDs, nonoverlapping block extents, zero padding, payload length and Adler checksum before decoding bounded RPM headers. Tests cover empty and multiple-page inventories, ownership/version, corruption, truncation, duplicate IDs/overlap/version and unchanged bytes/directory entries. Format reference: [RPM NDB package storage](https://github.com/rpm-software-management/rpm/blob/master/lib/backend/ndb/rpmpkg.c).

The standalone BDB decoder supports hash versions 8/9, both byte orders, sorted/legacy hash page types and linked overflow payloads. It follows the declared bucket/spares roots instead of scanning arbitrary/free pages. Page/index/header bounds, page identities, unique keys, chain cycles and exact overflow lengths are validated; a malformed reachable record discards the inventory. Encrypted/checksummed/partitioned/duplicate/subdatabase variants and sibling `__db.*` cache or `log.*` recovery environments return coverage gaps because standalone bytes cannot establish their current contents. No locks, repairs or environment recovery are attempted. This is intentionally a standalone-format subset, not a full libdb replacement. Primary format references: [Berkeley DB page layouts](https://github.com/berkeleydb/libdb/blob/master/src/dbinc/db_page.h), [hash bucket mapping](https://github.com/berkeleydb/libdb/blob/master/src/dbinc/hash.h).

Package fixtures cover endian variants, inline/multiple-page overflow values, unlinked free bytes, unsupported flags, invalid indices/cycles/truncation and active-environment refusal. App integration proves both formats create library ownership/service dependency links with actual format/path provenance; later unreadability warns and preserves prior source relationships. Selected package/app tests and package/fixture strict Clippy pass. WAL reuse with stale trailing frames remains conservatively rejected as documented above; metadata checks do not establish atomicity across every writer schedule.
