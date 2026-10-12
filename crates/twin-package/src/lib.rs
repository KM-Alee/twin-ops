mod bdb;
mod error;
mod ndb;
mod rpm;
mod wal_snapshot;
pub use error::PackageError;
pub use rpm::RpmPackage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpmDatabaseFormat {
    Sqlite,
    Ndb,
    Bdb,
}
impl RpmDatabaseFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Ndb => "ndb",
            Self::Bdb => "bdb",
        }
    }
}
#[derive(Debug, Clone)]
pub struct RpmDatabase {
    pub format: RpmDatabaseFormat,
    pub packages: Vec<RpmPackage>,
}
pub fn read_rpm(path: &std::path::Path) -> Result<Vec<RpmPackage>, PackageError> {
    Ok(read_rpm_database(path)?.packages)
}
pub fn read_rpm_database(path: &std::path::Path) -> Result<RpmDatabase, PackageError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|source| PackageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut header = Vec::new();
    file.take(16)
        .read_to_end(&mut header)
        .map_err(|source| PackageError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let (format, packages) = if header.starts_with(b"RpmP") {
        (RpmDatabaseFormat::Ndb, ndb::read(path)?)
    } else if header.starts_with(b"SQLite format 3\0") {
        (RpmDatabaseFormat::Sqlite, rpm::read_sqlite(path)?)
    } else if header.get(12..16).is_some_and(|magic| {
        magic == 0x00061561_u32.to_le_bytes() || magic == 0x00061561_u32.to_be_bytes()
    }) {
        (RpmDatabaseFormat::Bdb, bdb::read(path)?)
    } else {
        return Err(PackageError::Unavailable {
            path: path.to_path_buf(),
            detail: "unrecognized or unsupported RPM database format",
        });
    };
    Ok(RpmDatabase { format, packages })
}
