use std::ffi::CString;

pub(crate) fn mount_used_percent(path: &str) -> Option<u8> {
    let path = CString::new(path).ok()?;
    let mut stat = unsafe { std::mem::zeroed::<libc::statvfs>() };
    // SAFETY: `path` is NUL-terminated. `stat` is a writable statvfs buffer.
    // statvfs only reads filesystem metadata for the path.
    let rc = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let blocks = stat.f_blocks as u128;
    if blocks == 0 {
        return None;
    }
    let available = stat.f_bavail as u128;
    let used = blocks.saturating_sub(available);
    let percent = used.saturating_mul(100) / blocks;
    u8::try_from(percent.min(100)).ok()
}
