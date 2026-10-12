use crate::EbpfError;

#[repr(C)]
struct Timespec {
    seconds: std::ffi::c_long,
    nanoseconds: std::ffi::c_long,
}

unsafe extern "C" {
    fn sysconf(name: std::ffi::c_int) -> std::ffi::c_long;
    fn clock_gettime(clock: std::ffi::c_int, time: *mut Timespec) -> std::ffi::c_int;
}

pub(crate) struct EventClock {
    offset: i128,
}

impl EventClock {
    pub(crate) fn new() -> Result<Self, EbpfError> {
        if cfg!(not(all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64"
        ))) {
            return Err(EbpfError::Unavailable {
                reason: "bundled probes require little-endian 64-bit Linux".into(),
            });
        }
        // SAFETY: sysconf reads a fixed Linux _SC_CLK_TCK scalar and takes no pointers.
        if unsafe { sysconf(2) } != 100 {
            return Err(EbpfError::Unavailable {
                reason: "unsupported process clock tick ABI".into(),
            });
        }
        let monotonic = read_clock(1)?;
        let realtime = read_clock(0)?;
        let after = read_clock(1)?;
        Ok(Self {
            offset: realtime - (monotonic + after) / 2,
        })
    }
    pub(crate) fn timestamp(&self, monotonic: u64) -> Result<u64, EbpfError> {
        u64::try_from(i128::from(monotonic) + self.offset).map_err(|_| EbpfError::Malformed {
            reason: "event clock cannot be represented as UTC nanoseconds".into(),
        })
    }
}

fn read_clock(clock: i32) -> Result<i128, EbpfError> {
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: the C ABI timespec layout matches Linux long fields and the pointer is valid.
    if unsafe { clock_gettime(clock, &mut time) } != 0 {
        return Err(EbpfError::Unavailable {
            reason: "event clock read failed".into(),
        });
    }
    Ok(i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanoseconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_rejects_invalid_epoch_and_preserves_elapsed_time() {
        let clock = EventClock { offset: 1_000 };
        assert_eq!(clock.timestamp(50).unwrap(), 1_050);
        assert_eq!(clock.timestamp(70).unwrap(), 1_070);
        assert!(EventClock { offset: -1 }.timestamp(0).is_err());
    }
}
