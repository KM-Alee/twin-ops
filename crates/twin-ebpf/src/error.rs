#[derive(Debug, thiserror::Error)]
pub enum EbpfError {
    #[error("eBPF unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("eBPF verifier rejected the program: {reason}")]
    Verifier { reason: String },
    #[error("malformed eBPF event: {reason}")]
    Malformed { reason: String },
}
