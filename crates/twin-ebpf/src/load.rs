use std::convert::TryInto;

use aya::maps::RingBuf;
use aya::programs::trace_point::TracePointLink;
use aya::programs::{ProgramError, TracePoint};

use crate::decode::{decode_exec, EbpfExec};
use crate::error::EbpfError;
use crate::tcp::{decode_tcp_event, TcpEvent};

const EXEC_PROGRAM_NAME: &str = "exec";
const EXEC_MAP_NAME: &str = "EVENTS";
const EXEC_TRACE_CATEGORY: &str = "sched";
const EXEC_TRACE_NAME: &str = "sched_process_exec";

pub struct ExecSession {
    _bpf: aya::Ebpf,
    ring: Option<RingBuf<aya::maps::MapData>>,
    link: Option<TracePointLink>,
}

impl ExecSession {
    pub fn poll_exec(&mut self) -> Result<Option<EbpfExec>, EbpfError> {
        let Some(ring) = self.ring.as_mut() else {
            return Ok(None);
        };
        match ring.next() {
            Some(item) => decode_exec(item.as_ref()).map(Some),
            None => Ok(None),
        }
    }

    pub fn detach(&mut self) {
        // Dropping the tracepoint link detaches it. Do that before the loader
        // and the ring buffer so every exit path releases the kernel hook.
        self.link.take();
        self.ring.take();
    }
}

impl Drop for ExecSession {
    fn drop(&mut self) {
        self.detach();
    }
}

pub fn attach() -> Result<ExecSession, EbpfError> {
    Err(EbpfError::Unavailable {
        reason: "exec tracing program is not embedded in this build".to_string(),
    })
}

pub fn attach_from_bytes(bytes: &[u8]) -> Result<ExecSession, EbpfError> {
    if bytes.is_empty() {
        return Err(EbpfError::Unavailable {
            reason: "exec program object is empty".to_string(),
        });
    }
    let owned = align4(bytes);
    let view = aligned_prefix(&owned, bytes.len());
    let mut bpf = aya::Ebpf::load(view).map_err(map_load_error)?;
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
    Ok(ExecSession {
        _bpf: bpf,
        ring: Some(ring),
        link: Some(link),
    })
}

const TCP_PROGRAM_NAME: &str = "tcp";
const TCP_MAP_NAME: &str = "TCP_EVENTS";
const TCP_TRACE_CATEGORY: &str = "sock";
const TCP_TRACE_NAME: &str = "inet_sock_set_state";

pub struct TcpSession {
    _bpf: aya::Ebpf,
    ring: Option<RingBuf<aya::maps::MapData>>,
    link: Option<TracePointLink>,
}

impl TcpSession {
    pub fn poll_event(&mut self) -> Result<Option<TcpEvent>, EbpfError> {
        let Some(ring) = self.ring.as_mut() else {
            return Ok(None);
        };
        match ring.next() {
            Some(item) => decode_tcp_event(item.as_ref()).map(Some),
            None => Ok(None),
        }
    }

    pub fn detach(&mut self) {
        self.link.take();
        self.ring.take();
    }
}

impl Drop for TcpSession {
    fn drop(&mut self) {
        self.detach();
    }
}

pub fn attach_tcp() -> Result<TcpSession, EbpfError> {
    Err(EbpfError::Unavailable {
        reason: "tcp tracing program is not embedded in this build".to_string(),
    })
}

pub fn attach_tcp_from_bytes(bytes: &[u8]) -> Result<TcpSession, EbpfError> {
    if bytes.is_empty() {
        return Err(EbpfError::Unavailable {
            reason: "tcp program object is empty".to_string(),
        });
    }
    let owned = align4(bytes);
    let view = aligned_prefix(&owned, bytes.len());
    let mut bpf = aya::Ebpf::load(view).map_err(map_load_error)?;
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
    Ok(TcpSession {
        _bpf: bpf,
        ring: Some(ring),
        link: Some(link),
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

fn align4(bytes: &[u8]) -> Vec<u32> {
    let mut buf = vec![0u32; bytes.len().div_ceil(4)];
    // SAFETY: `buf` is a contiguous allocation of u32, so its pointer is 4-byte
    // aligned. The copy length is the original object size, which fits in `buf`.
    let dst = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), bytes.len()) };
    dst.copy_from_slice(bytes);
    buf
}

fn aligned_prefix(buf: &[u32], len: usize) -> &[u8] {
    // SAFETY: `buf` was filled by `align4` with at least `len` bytes.
    unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len) }
}
