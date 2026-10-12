use std::convert::TryInto;

use crate::clock::EventClock;
use aya::maps::{Array, RingBuf};
use aya::programs::trace_point::TracePointLink;
use aya::programs::{ProgramError, TracePoint};

use crate::decode::{decode_exec, EbpfExec};
use crate::error::EbpfError;
use crate::tcp::{decode_tcp_event, TcpEvent};

const BUNDLED_PROBE: &[u8] = include_bytes!("../probes/twin.bpf.o");

const EXEC_PROGRAM_NAME: &str = "exec";
const EXEC_MAP_NAME: &str = "EVENTS";
const EXEC_TRACE_CATEGORY: &str = "sched";
const EXEC_TRACE_NAME: &str = "sched_process_exec";

pub struct ExecSession {
    _bpf: aya::Ebpf,
    ring: Option<RingBuf<aya::maps::MapData>>,
    links: Vec<TracePointLink>,
    clock: EventClock,
    drops: Array<aya::maps::MapData, u64>,
}

impl ExecSession {
    pub fn poll_exec(&mut self) -> Result<Option<EbpfExec>, EbpfError> {
        let Some(ring) = self.ring.as_mut() else {
            return Ok(None);
        };
        match ring.next() {
            Some(item) => {
                let event = decode_exec(item.as_ref())?;
                let timestamp = self.clock.timestamp(event.timestamp_ns())?;
                Ok(Some(event.with_timestamp(timestamp)))
            }
            None => Ok(None),
        }
    }

    pub fn dropped_count(&self) -> Result<u64, EbpfError> {
        read_drops(&self.drops)
    }

    pub fn stop_tracing(&mut self) {
        self.links.clear();
    }

    pub fn detach(&mut self) {
        // Dropping the tracepoint link detaches it. Do that before the loader
        // and the ring buffer so every exit path releases the kernel hook.
        self.stop_tracing();
        self.ring.take();
    }
}

impl Drop for ExecSession {
    fn drop(&mut self) {
        self.detach();
    }
}

pub fn attach() -> Result<ExecSession, EbpfError> {
    attach_from_bytes(BUNDLED_PROBE)
}

pub fn attach_from_bytes(bytes: &[u8]) -> Result<ExecSession, EbpfError> {
    if bytes.is_empty() {
        return Err(EbpfError::Unavailable {
            reason: "exec program object is empty".to_string(),
        });
    }
    let clock = EventClock::new()?;
    let owned = align8(bytes);
    let view = aligned_prefix(&owned, bytes.len());
    let mut bpf = aya::Ebpf::load(view).map_err(map_load_error)?;
    configure_pid_namespace(&mut bpf)?;
    let program: &mut TracePoint = bpf
        .program_mut(EXEC_PROGRAM_NAME)
        .ok_or_else(|| EbpfError::Unavailable {
            reason: format!("program `{EXEC_PROGRAM_NAME}` not found"),
        })?
        .try_into()
        .map_err(map_program_error)?;
    program.load().map_err(map_program_error)?;
    let link_id = program
        .attach(EXEC_TRACE_CATEGORY, EXEC_TRACE_NAME)
        .map_err(map_program_error)?;
    let link = program.take_link(link_id).map_err(map_program_error)?;
    let map = bpf
        .take_map(EXEC_MAP_NAME)
        .ok_or_else(|| EbpfError::Unavailable {
            reason: format!("map `{EXEC_MAP_NAME}` not found"),
        })?;
    let ring = RingBuf::try_from(map).map_err(|err| EbpfError::Unavailable {
        reason: err.to_string(),
    })?;
    let drops = take_drops(&mut bpf, "EXEC_DROPS")?;
    Ok(ExecSession {
        _bpf: bpf,
        ring: Some(ring),
        links: vec![link],
        clock,
        drops,
    })
}

const TCP_PROGRAM_NAME: &str = "tcp";
const TCP_MAP_NAME: &str = "TCP_EVENTS";
const TCP_TRACE_CATEGORY: &str = "sock";
const TCP_TRACE_NAME: &str = "inet_sock_set_state";

pub struct TcpSession {
    _bpf: aya::Ebpf,
    ring: Option<RingBuf<aya::maps::MapData>>,
    links: Vec<TracePointLink>,
    clock: EventClock,
    drops: Array<aya::maps::MapData, u64>,
}

impl TcpSession {
    pub fn poll_event(&mut self) -> Result<Option<TcpEvent>, EbpfError> {
        let Some(ring) = self.ring.as_mut() else {
            return Ok(None);
        };
        match ring.next() {
            Some(item) => {
                let mut bytes = item.to_vec();
                if bytes.len() < 20 {
                    return decode_tcp_event(&bytes).map(Some);
                }
                let mut timestamp = [0; 8];
                timestamp.copy_from_slice(&bytes[12..20]);
                bytes[12..20].copy_from_slice(
                    &self
                        .clock
                        .timestamp(u64::from_le_bytes(timestamp))?
                        .to_le_bytes(),
                );
                decode_tcp_event(&bytes).map(Some)
            }
            None => Ok(None),
        }
    }

    pub fn dropped_count(&self) -> Result<u64, EbpfError> {
        read_drops(&self.drops)
    }

    pub fn stop_tracing(&mut self) {
        self.links.clear();
    }

    pub fn detach(&mut self) {
        self.stop_tracing();
        self.ring.take();
    }
}

impl Drop for TcpSession {
    fn drop(&mut self) {
        self.detach();
    }
}

pub fn attach_tcp() -> Result<TcpSession, EbpfError> {
    crate::trace_format::validate_host()?;
    attach_tcp_from_bytes(BUNDLED_PROBE)
}

pub fn attach_tcp_from_bytes(bytes: &[u8]) -> Result<TcpSession, EbpfError> {
    if bytes.is_empty() {
        return Err(EbpfError::Unavailable {
            reason: "tcp program object is empty".to_string(),
        });
    }
    let clock = EventClock::new()?;
    let owned = align8(bytes);
    let view = aligned_prefix(&owned, bytes.len());
    let mut bpf = aya::Ebpf::load(view).map_err(map_load_error)?;
    configure_pid_namespace(&mut bpf)?;
    let program: &mut TracePoint = bpf
        .program_mut(TCP_PROGRAM_NAME)
        .ok_or_else(|| EbpfError::Unavailable {
            reason: format!("program `{TCP_PROGRAM_NAME}` not found"),
        })?
        .try_into()
        .map_err(map_program_error)?;
    program.load().map_err(map_program_error)?;
    let link_id = program
        .attach(TCP_TRACE_CATEGORY, TCP_TRACE_NAME)
        .map_err(map_program_error)?;
    let link = program.take_link(link_id).map_err(map_program_error)?;
    let map = bpf
        .take_map(TCP_MAP_NAME)
        .ok_or_else(|| EbpfError::Unavailable {
            reason: format!("map `{TCP_MAP_NAME}` not found"),
        })?;
    let ring = RingBuf::try_from(map).map_err(|err| EbpfError::Unavailable {
        reason: err.to_string(),
    })?;
    let mut links = vec![link];
    for (name, category, trace) in [
        ("connect_enter", "syscalls", "sys_enter_connect"),
        ("connect_exit", "syscalls", "sys_exit_connect"),
        ("connect_cleanup", "sched", "sched_process_exit"),
        ("accept_enter", "syscalls", "sys_enter_accept"),
        ("accept_exit", "syscalls", "sys_exit_accept"),
        ("accept4_enter", "syscalls", "sys_enter_accept4"),
        ("accept4_exit", "syscalls", "sys_exit_accept4"),
        ("bind_enter", "syscalls", "sys_enter_bind"),
        ("bind_exit", "syscalls", "sys_exit_bind"),
    ] {
        let program: &mut TracePoint = bpf
            .program_mut(name)
            .ok_or_else(|| EbpfError::Unavailable {
                reason: format!("program `{name}` not found"),
            })?
            .try_into()
            .map_err(map_program_error)?;
        program.load().map_err(map_program_error)?;
        let id = program.attach(category, trace).map_err(map_program_error)?;
        links.push(program.take_link(id).map_err(map_program_error)?);
    }
    let drops = take_drops(&mut bpf, "TCP_DROPS")?;
    Ok(TcpSession {
        _bpf: bpf,
        ring: Some(ring),
        links,
        clock,
        drops,
    })
}

fn map_load_error(err: aya::EbpfError) -> EbpfError {
    classify(err.to_string())
}

fn map_program_error(err: ProgramError) -> EbpfError {
    match err {
        ProgramError::LoadError {
            io_error,
            verifier_log,
        } => EbpfError::Verifier {
            reason: format!("{io_error}; {verifier_log}"),
        },
        other => classify(other.to_string()),
    }
}

fn classify(reason: String) -> EbpfError {
    if reason.to_ascii_lowercase().contains("verifier") {
        EbpfError::Verifier { reason }
    } else {
        EbpfError::Unavailable { reason }
    }
}

fn align8(bytes: &[u8]) -> Vec<u64> {
    let mut buf = vec![0u64; bytes.len().div_ceil(8)];
    // SAFETY: `buf` is a contiguous allocation of u64, so its pointer is 8-byte
    // aligned. The copy length is the original object size, which fits in `buf`.
    let dst = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), bytes.len()) };
    dst.copy_from_slice(bytes);
    buf
}

fn aligned_prefix(buf: &[u64], len: usize) -> &[u8] {
    // SAFETY: `buf` was filled by `align8` with at least `len` bytes.
    unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len) }
}

fn take_drops(
    bpf: &mut aya::Ebpf,
    name: &str,
) -> Result<Array<aya::maps::MapData, u64>, EbpfError> {
    let map = bpf.take_map(name).ok_or_else(|| EbpfError::Unavailable {
        reason: format!("map `{name}` missing"),
    })?;
    Array::try_from(map).map_err(|_| EbpfError::Unavailable {
        reason: "drop counter map has invalid ABI".into(),
    })
}
fn read_drops(drops: &Array<aya::maps::MapData, u64>) -> Result<u64, EbpfError> {
    drops.get(&0, 0).map_err(|_| EbpfError::Unavailable {
        reason: "kernel drop counters unavailable".into(),
    })
}

fn configure_pid_namespace(bpf: &mut aya::Ebpf) -> Result<(), EbpfError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata("/proc/self/ns/pid").map_err(|_| EbpfError::Unavailable {
        reason: "scanner PID namespace identity unreadable".into(),
    })?;
    let map = bpf
        .map_mut("PID_NAMESPACE")
        .ok_or_else(|| EbpfError::Unavailable {
            reason: "PID namespace configuration map missing".into(),
        })?;
    let mut configuration =
        Array::<_, [u64; 2]>::try_from(map).map_err(|_| EbpfError::Unavailable {
            reason: "PID namespace configuration ABI invalid".into(),
        })?;
    configuration
        .set(0, [metadata.dev(), metadata.ino()], 0)
        .map_err(|_| EbpfError::Unavailable {
            reason: "PID namespace configuration unavailable".into(),
        })
}
