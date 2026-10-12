use std::collections::BTreeSet;
use std::path::Path;

use crate::{PackageError, RpmPackage};

pub(crate) fn read(path: &Path) -> Result<Vec<RpmPackage>, PackageError> {
    check_environment(path)?;
    let (bytes, before) = crate::wal_snapshot::read_bounded(path)?;
    let packages = parse(&bytes).ok_or_else(|| {
        unavailable(
            path,
            "malformed or unsupported RPM BDB hash database; incomplete inventory discarded",
        )
    })?;
    check_environment(path)?;
    if !crate::wal_snapshot::unchanged(path, &before)? {
        return Err(unavailable(
            path,
            "RPM BDB database changed during read; retry the scan",
        ));
    }
    Ok(packages)
}
fn unavailable(path: &Path, detail: &'static str) -> PackageError {
    PackageError::Unavailable {
        path: path.to_path_buf(),
        detail,
    }
}
fn check_environment(path: &Path) -> Result<(), PackageError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for entry in std::fs::read_dir(parent).map_err(|source| PackageError::Read {
        path: parent.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PackageError::Read {
            path: parent.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("__db.") || name.starts_with("log.") {
            return Err(unavailable(path, "RPM BDB environment cache or recovery logs present; standalone bytes cannot establish current inventory"));
        }
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Order {
    big: bool,
}
impl Order {
    fn word(self, bytes: &[u8], offset: usize) -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if self.big {
            u32::from_be_bytes(value)
        } else {
            u32::from_le_bytes(value)
        })
    }
    fn short(self, bytes: &[u8], offset: usize) -> Option<usize> {
        let value = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
        Some(usize::from(if self.big {
            u16::from_be_bytes(value)
        } else {
            u16::from_le_bytes(value)
        }))
    }
}
fn parse(bytes: &[u8]) -> Option<Vec<RpmPackage>> {
    let order = Order {
        big: bytes.get(12..16)? == 0x00061561_u32.to_be_bytes(),
    };
    let size = order.word(bytes, 20)? as usize;
    if order.word(bytes, 12)? != 0x00061561
        || !matches!(order.word(bytes, 16)?, 8 | 9)
        || !(512..=32768).contains(&size)
        || !size.is_power_of_two()
        || !bytes.len().is_multiple_of(size)
        || bytes.get(24..27)? != [0, 8, 0]
        || order.word(bytes, 36)? != 0
        || order.word(bytes, 48)? != 0
        || order.word(bytes, 8)? != 0
        || (order.word(bytes, 32)? as usize).checked_add(1)? != bytes.len() / size
    {
        return None;
    }
    let max_bucket = order.word(bytes, 72)? as usize;
    if max_bucket >= bytes.len() / size {
        return None;
    }
    let mut visited = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut packages = Vec::new();
    for bucket in 0..=max_bucket {
        let doubling = (usize::BITS - bucket.leading_zeros()) as usize;
        let mut number = bucket.checked_add(order.word(bytes, 96 + doubling * 4)? as usize)?;
        let mut previous = 0;
        while number != 0 {
            if !visited.insert(number) {
                return None;
            }
            let page = page(bytes, size, number)?;
            if !matches!(*page.get(25)?, 2 | 13)
                || order.word(page, 8)? as usize != number
                || order.word(page, 12)? as usize != previous
            {
                return None;
            }
            let entries = order.short(page, 20)?;
            let lower = 26usize.checked_add(entries.checked_mul(2)?)?;
            if entries % 2 != 0 || lower > size {
                return None;
            }
            let mut end = size;
            let mut key = None;
            for index in 0..entries {
                let start = order.short(page, 26 + index * 2)?;
                if start < lower || start >= end {
                    return None;
                }
                let item = page.get(start..end)?;
                let value = match *item.first()? {
                    1 => item.get(1..)?.to_vec(),
                    3 if item.len() == 12 => overflow(bytes, size, order, item, &mut visited)?,
                    _ => return None,
                };
                if index % 2 == 0 {
                    if value.len() != 4 || !keys.insert(value.clone()) {
                        return None;
                    }
                    key = Some(value);
                } else if key.as_ref()?.iter().any(|byte| *byte != 0) {
                    if packages.len() >= 100_000 {
                        return None;
                    }
                    packages.push(crate::rpm::parse_header(&value)?);
                } else if value.len() != 4 {
                    return None;
                }
                end = start;
            }
            if entries > 0 && order.short(page, 22)? != end {
                return None;
            }
            previous = number;
            number = order.word(page, 16)? as usize;
        }
        // Every active bucket has a real root page; a zero spares entry is not an empty inventory.
        if previous == 0 {
            return None;
        }
    }
    Some(packages)
}
fn page(bytes: &[u8], size: usize, number: usize) -> Option<&[u8]> {
    let offset = number.checked_mul(size)?;
    bytes.get(offset..offset.checked_add(size)?)
}
fn overflow(
    bytes: &[u8],
    size: usize,
    order: Order,
    item: &[u8],
    visited: &mut BTreeSet<usize>,
) -> Option<Vec<u8>> {
    let length = order.word(item, 8)? as usize;
    if length == 0 || length > 16_777_216 {
        return None;
    }
    let mut result = Vec::new();
    let mut number = order.word(item, 4)? as usize;
    let mut previous = 0;
    while number != 0 {
        if !visited.insert(number) {
            return None;
        }
        let page = page(bytes, size, number)?;
        if *page.get(25)? != 7
            || order.word(page, 8)? as usize != number
            || order.word(page, 12)? as usize != previous
        {
            return None;
        }
        let count = order.short(page, 22)?;
        if count == 0 || count > size - 26 || result.len().checked_add(count)? > length {
            return None;
        }
        result.extend_from_slice(page.get(26..26 + count)?);
        previous = number;
        number = order.word(page, 16)? as usize;
    }
    (result.len() == length).then_some(result)
}
