use std::fs::{File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::{PackageError, RpmPackage};

const LIMIT: usize = 256 * 1024 * 1024;

pub(crate) fn read_snapshot(
    database: &Path,
    wal_path: &Path,
) -> Result<Vec<RpmPackage>, PackageError> {
    let (mut bytes, database_before) = read_bounded(database)?;
    let (wal, wal_before) = read_bounded(wal_path)?;
    if !unchanged(database, &database_before)? || !unchanged(wal_path, &wal_before)? {
        return Err(unavailable(
            database,
            "database or WAL changed during snapshot; retry the scan",
        ));
    }
    crate::rpm::check_journal(database)?;
    replay(&mut bytes, &wal)
        .ok_or_else(|| unavailable(database, "malformed, incomplete, or oversized WAL snapshot"))?;
    let scratch = tempfile::tempdir().map_err(|source| PackageError::Snapshot { source })?;
    let path = scratch.path().join("rpmdb.sqlite");
    std::fs::write(&path, bytes).map_err(|source| PackageError::Snapshot { source })?;
    crate::rpm::read_immutable(&path)
}

pub(crate) fn read_bounded(path: &Path) -> Result<(Vec<u8>, Metadata), PackageError> {
    let error = |source| PackageError::Read {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(error)?;
    let before = file.metadata().map_err(error)?;
    if !before.is_file() || before.len() > LIMIT as u64 {
        return Err(unavailable(
            path,
            "snapshot input is not a regular file or exceeds 256 MiB",
        ));
    }
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > LIMIT || bytes.len() as u64 != before.len() {
        return Err(unavailable(
            path,
            "snapshot input changed size or exceeds 256 MiB",
        ));
    }
    Ok((bytes, before))
}

pub(crate) fn unchanged(path: &Path, before: &Metadata) -> Result<bool, PackageError> {
    let after = std::fs::metadata(path).map_err(|source| PackageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec())
}

fn unavailable(path: &Path, detail: &'static str) -> PackageError {
    PackageError::Unavailable {
        path: path.to_path_buf(),
        detail,
    }
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn checksum(bytes: &[u8], is_little: bool, mut sums: (u32, u32)) -> Option<(u32, u32)> {
    if !bytes.len().is_multiple_of(8) {
        return None;
    }
    for pair in bytes.as_chunks::<8>().0 {
        let read = |offset| {
            let bytes = pair.get(offset..offset + 4)?.try_into().ok()?;
            Some(if is_little {
                u32::from_le_bytes(bytes)
            } else {
                u32::from_be_bytes(bytes)
            })
        };
        sums.0 = sums.0.wrapping_add(read(0)?).wrapping_add(sums.1);
        sums.1 = sums.1.wrapping_add(read(4)?).wrapping_add(sums.0);
    }
    Some(sums)
}

fn replay(database: &mut Vec<u8>, wal: &[u8]) -> Option<()> {
    if database.get(..16)? != b"SQLite format 3\0" {
        return None;
    }
    let is_little = match word(wal, 0)? {
        0x377f0682 => true,
        0x377f0683 => false,
        _ => return None,
    };
    if word(wal, 4)? != 3007000 {
        return None;
    }
    let page_size = word(wal, 8)? as usize;
    if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
        return None;
    }
    let db_page_size = u16::from_be_bytes(database.get(16..18)?.try_into().ok()?);
    if (if db_page_size == 1 {
        65536
    } else {
        db_page_size as usize
    }) != page_size
    {
        return None;
    }
    let mut sums = checksum(wal.get(..24)?, is_little, (0, 0))?;
    if sums != (word(wal, 24)?, word(wal, 28)?) {
        return None;
    }
    let frame_size = page_size.checked_add(24)?;
    let frames = wal.get(32..)?;
    if frames.len() % frame_size != 0 {
        return None;
    }
    let salt = wal.get(16..24)?;
    let mut commit = None;
    for (index, frame) in frames.chunks_exact(frame_size).enumerate() {
        if frame.get(8..16)? != salt || word(frame, 0)? == 0 {
            return None;
        }
        sums = checksum(frame.get(..8)?, is_little, sums)?;
        sums = checksum(frame.get(24..)?, is_little, sums)?;
        if sums != (word(frame, 16)?, word(frame, 20)?) {
            return None;
        }
        let pages = word(frame, 4)? as usize;
        if pages != 0 {
            commit = Some((index, pages));
        }
    }
    let Some((last, pages)) = commit else {
        return Some(());
    };
    let length = pages.checked_mul(page_size)?;
    if length > LIMIT {
        return None;
    }
    database.resize(length, 0);
    for frame in frames.chunks_exact(frame_size).take(last + 1) {
        let page = word(frame, 0)? as usize;
        if page > pages {
            continue;
        }
        let offset = (page - 1).checked_mul(page_size)?;
        database
            .get_mut(offset..offset + page_size)?
            .copy_from_slice(frame.get(24..)?);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::{checksum, replay, unchanged};

    fn database() -> Vec<u8> {
        let mut bytes = vec![0; 512];
        bytes[..16].copy_from_slice(b"SQLite format 3\0");
        bytes[16..18].copy_from_slice(&512u16.to_be_bytes());
        bytes
    }

    fn wal(is_little: bool, commits: &[u32]) -> Vec<u8> {
        let mut bytes = vec![0; 32];
        let magic: u32 = if is_little { 0x377f0682 } else { 0x377f0683 };
        bytes[..4].copy_from_slice(&magic.to_be_bytes());
        bytes[4..8].copy_from_slice(&3007000u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&512u32.to_be_bytes());
        bytes[16..24].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut sums = checksum(&bytes[..24], is_little, (0, 0)).unwrap();
        bytes[24..28].copy_from_slice(&sums.0.to_be_bytes());
        bytes[28..32].copy_from_slice(&sums.1.to_be_bytes());
        for (index, committed_pages) in commits.iter().enumerate() {
            let mut frame = vec![0; 536];
            frame[..4].copy_from_slice(&1u32.to_be_bytes());
            frame[4..8].copy_from_slice(&committed_pages.to_be_bytes());
            frame[8..16].copy_from_slice(&bytes[16..24]);
            frame[24..].fill(index as u8 + 1);
            sums = checksum(&frame[..8], is_little, sums).unwrap();
            sums = checksum(&frame[24..], is_little, sums).unwrap();
            frame[16..20].copy_from_slice(&sums.0.to_be_bytes());
            frame[20..24].copy_from_slice(&sums.1.to_be_bytes());
            bytes.extend(frame);
        }
        bytes
    }

    #[test]
    fn both_checksum_orders_replay_only_through_the_last_commit() {
        for is_little in [true, false] {
            let mut bytes = database();
            replay(&mut bytes, &wal(is_little, &[1, 0])).unwrap();
            assert_eq!(
                bytes,
                vec![1; 512],
                "valid uncommitted page replacement must be excluded"
            );
            let mut bytes = database();
            replay(&mut bytes, &wal(is_little, &[1, 0, 1, 0])).unwrap();
            assert_eq!(
                bytes,
                vec![3; 512],
                "later commits include earlier transaction frames"
            );
            let mut bytes = database();
            let original = bytes.clone();
            replay(&mut bytes, &wal(is_little, &[0, 0])).unwrap();
            assert_eq!(bytes, original, "no commit leaves base bytes untouched");
        }
    }

    #[test]
    fn replaced_input_and_changed_bytes_fail_metadata_stability() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("database");
        std::fs::write(&path, b"original").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        assert!(unchanged(&path, &before).unwrap());
        let replacement = directory.path().join("replacement");
        std::fs::write(&replacement, b"original").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert!(
            !unchanged(&path, &before).unwrap(),
            "same size and content must not hide inode replacement"
        );
        let before = std::fs::metadata(&path).unwrap();
        std::fs::write(&path, b"changed length").unwrap();
        assert!(!unchanged(&path, &before).unwrap());
    }
}
