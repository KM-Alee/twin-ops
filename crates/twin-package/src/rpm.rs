use crate::PackageError;
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpmPackage {
    pub name: String,
    pub version: String,
    pub files: Vec<String>,
}

pub(crate) fn read_sqlite(path: &Path) -> Result<Vec<RpmPackage>, PackageError> {
    let unavailable = |detail| PackageError::Unavailable {
        path: path.to_path_buf(),
        detail,
    };
    check_journal(path)?;
    let metadata = std::fs::metadata(path).map_err(|source| PackageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let wal = path.with_file_name(format!(
        "{}-wal",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    if std::fs::metadata(&wal).is_ok_and(|meta| meta.len() > 0) {
        return crate::wal_snapshot::read_snapshot(path, &wal);
    }
    check_wal(&wal, path)?;
    let packages = read_immutable(path)?;
    if !crate::wal_snapshot::unchanged(path, &metadata)? {
        return Err(unavailable(
            "database changed during the read; retry the scan",
        ));
    }
    check_wal(&wal, path)?;
    check_journal(path)?;
    Ok(packages)
}

pub(crate) fn read_immutable(path: &Path) -> Result<Vec<RpmPackage>, PackageError> {
    let unavailable = |detail| PackageError::Unavailable {
        path: path.to_path_buf(),
        detail,
    };
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/._-".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    // Immutable mode prevents SQLite from creating journal or shared-memory files on the host.
    let uri = format!("file:{encoded}?mode=ro&immutable=1");
    let sqlite = |source| PackageError::Sqlite {
        path: path.to_path_buf(),
        source,
    };
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sqlite)?;
    let oversized: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM Packages WHERE length(blob) > 16777216",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite)?;
    if oversized > 0 {
        return Err(unavailable("package header exceeds the 16 MiB read limit"));
    }
    let mut statement = connection
        .prepare("SELECT blob FROM Packages ORDER BY hnum LIMIT 100001")
        .map_err(sqlite)?;
    let mut rows = statement.query([]).map_err(sqlite)?;
    let mut packages = Vec::new();
    while let Some(row) = rows.next().map_err(sqlite)? {
        if packages.len() >= 100000 {
            return Err(unavailable("package count exceeds the read limit"));
        }
        let bytes: Vec<u8> = row.get(0).map_err(sqlite)?;
        packages.push(parse_header(&bytes).ok_or_else(|| {
            unavailable("malformed RPM package header; incomplete database discarded")
        })?);
    }
    Ok(packages)
}

pub(crate) fn check_journal(database: &Path) -> Result<(), PackageError> {
    let path = database.with_file_name(format!(
        "{}-journal",
        database.file_name().unwrap_or_default().to_string_lossy()
    ));
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.len() > 0 => Err(PackageError::Unavailable {
            path: database.to_path_buf(),
            detail: "rollback journal prevents a consistent immutable read",
        }),
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
            Err(PackageError::Read { path, source })
        }
        _ => Ok(()),
    }
}

fn check_wal(wal: &Path, database: &Path) -> Result<(), PackageError> {
    match std::fs::metadata(wal) {
        Ok(metadata) if metadata.len() > 0 => Err(PackageError::Unavailable {
            path: database.to_path_buf(),
            detail: "active WAL prevents an immutable read",
        }),
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(PackageError::Read {
            path: wal.to_path_buf(),
            source,
        }),
        _ => Ok(()),
    }
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(raw))
}

pub(crate) fn parse_header(bytes: &[u8]) -> Option<RpmPackage> {
    let bytes = if bytes.starts_with(&[0x8e, 0xad, 0xe8, 1, 0, 0, 0, 0]) {
        bytes.get(8..)?
    } else {
        bytes
    };
    let entries = word(bytes, 0)? as usize;
    let size = word(bytes, 4)? as usize;
    if entries > 65536 {
        return None;
    }
    let data_start = 8usize.checked_add(entries.checked_mul(16)?)?;
    let store = bytes.get(data_start..data_start.checked_add(size)?)?;
    let mut tags = BTreeMap::new();
    for index in 0..entries {
        let start = 8 + index * 16;
        tags.insert(
            word(bytes, start)?,
            (
                word(bytes, start + 4)?,
                word(bytes, start + 8)? as usize,
                word(bytes, start + 12)? as usize,
            ),
        );
    }
    let strings = |tag| -> Option<Vec<String>> {
        let &(kind, offset, count) = tags.get(&tag)?;
        if !matches!(kind, 6 | 8 | 9) || count > 100000 {
            return None;
        }
        let mut raw = store.get(offset..)?;
        let mut strings = Vec::new();
        for _ in 0..count {
            let end = raw.iter().position(|byte| *byte == 0)?;
            strings.push(std::str::from_utf8(raw.get(..end)?).ok()?.to_string());
            raw = raw.get(end + 1..)?;
        }
        Some(strings)
    };
    let name = strings(1000)?.into_iter().next()?;
    let version = strings(1001)?.into_iter().next()?;
    let release = strings(1002)?.into_iter().next()?;
    if name.is_empty() || version.is_empty() || release.is_empty() {
        return None;
    }
    let epoch = match tags.get(&1003) {
        Some(&(4, offset, 1)) => word(store, offset)?,
        Some(_) => return None,
        None => 0,
    };
    let version = if epoch == 0 {
        format!("{version}-{release}")
    } else {
        format!("{epoch}:{version}-{release}")
    };
    let mut files = Vec::new();
    if tags.contains_key(&1117) {
        let basenames = strings(1117)?;
        let directories = strings(1118)?;
        let &(kind, offset, count) = tags.get(&1116)?;
        if kind != 4 || count != basenames.len() {
            return None;
        }
        for (index, base) in basenames.into_iter().enumerate() {
            let directory_index = word(store, offset.checked_add(index.checked_mul(4)?)?)? as usize;
            let directory = directories.get(directory_index)?;
            if !directory.starts_with('/') || !directory.ends_with('/') || base.contains('/') {
                return None;
            }
            files.push(twin_core::lexical_canonical(&format!("{directory}{base}")));
        }
    } else if tags.contains_key(&1027) {
        files = strings(1027)?
            .into_iter()
            .filter(|path| path.starts_with('/'))
            .map(|path| twin_core::lexical_canonical(&path))
            .collect();
    }
    Some(RpmPackage {
        name,
        version,
        files,
    })
}
