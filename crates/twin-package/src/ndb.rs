use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{PackageError, RpmPackage};

pub(crate) fn read(path: &Path) -> Result<Vec<RpmPackage>, PackageError> {
    let (bytes, before) = crate::wal_snapshot::read_bounded(path)?;
    let packages = parse(&bytes).ok_or_else(|| PackageError::Unavailable {
        path: path.to_path_buf(),
        detail:
            "malformed, oversized or unsupported RPM NDB database; incomplete inventory discarded",
    })?;
    if !crate::wal_snapshot::unchanged(path, &before)? {
        return Err(PackageError::Unavailable {
            path: path.to_path_buf(),
            detail: "RPM NDB database changed during read; retry the scan",
        });
    }
    Ok(packages)
}

fn parse(bytes: &[u8]) -> Option<Vec<RpmPackage>> {
    if bytes.get(..4)? != b"RpmP" || word(bytes, 4)? != 0 || !bytes.len().is_multiple_of(16) {
        return None;
    }
    let slot_end = (word(bytes, 12)? as usize).checked_mul(4096)?;
    if slot_end < 4096 || slot_end > bytes.len() {
        return None;
    }
    let mut entries = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for slot in bytes.get(32..slot_end)?.as_chunks::<16>().0 {
        if slot.get(..4)? != b"Slot" {
            return None;
        }
        let block = word(slot, 8)? as usize;
        if block == 0 {
            continue;
        }
        let id = word(slot, 4)?;
        let size = (word(slot, 12)? as usize).checked_mul(16)?;
        let offset = block.checked_mul(16)?;
        if id == 0
            || !ids.insert(id)
            || offset < slot_end
            || !(32..=16_777_248).contains(&size)
            || entries.len() >= 100_000
        {
            return None;
        }
        if entries.insert(offset, (size, id)).is_some() {
            return None;
        }
    }
    let mut packages = Vec::new();
    let mut end = slot_end;
    for (offset, (size, id)) in entries {
        if offset < end {
            return None;
        }
        end = offset.checked_add(size)?;
        let blob = bytes.get(offset..end)?;
        if blob.get(..4)? != b"BlbS" || word(blob, 4)? != id {
            return None;
        }
        let length = word(blob, 12)? as usize;
        if length > 16_777_216
            || (16usize
                .checked_add(length)?
                .checked_add(12)?
                .checked_add(15)?
                / 16)
                * 16
                != size
        {
            return None;
        }
        let tail = size.checked_sub(12)?;
        if blob.get(tail + 8..)? != b"BlbE"
            || word(blob, tail + 4)? as usize != length
            || word(blob, tail)? != adler(blob.get(..tail)?)
        {
            return None;
        }
        if blob.get(16 + length..tail)?.iter().any(|byte| *byte != 0) {
            return None;
        }
        packages.push(crate::rpm::parse_header(blob.get(16..16 + length)?)?);
    }
    Some(packages)
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn adler(bytes: &[u8]) -> u32 {
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
