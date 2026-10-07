use std::fs;
use std::path::{Path, PathBuf};

const MIN_KERNEL_MAJOR: u32 = 5;
const MIN_KERNEL_MINOR: u32 = 8;
const CAP_SYS_ADMIN: u32 = 21;
const CAP_PERFMON: u32 = 38;
const CAP_BPF: u32 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfFacts {
    pub kernel_release: String,
    pub btf_present: bool,
    pub effective_caps: Option<u64>,
    pub exec_tracepoint_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfProbePaths {
    pub kernel_release: PathBuf,
    pub btf: PathBuf,
    pub status: PathBuf,
    pub exec_tracepoints: Vec<PathBuf>,
}

impl EbpfProbePaths {
    pub fn host() -> Self {
        Self {
            kernel_release: PathBuf::from("/proc/sys/kernel/osrelease"),
            btf: PathBuf::from("/sys/kernel/btf/vmlinux"),
            status: PathBuf::from("/proc/self/status"),
            exec_tracepoints: vec![
                PathBuf::from("/sys/kernel/tracing/events/sched/sched_process_exec"),
                PathBuf::from("/sys/kernel/debug/tracing/events/sched/sched_process_exec"),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfCheck {
    pub ok: bool,
    pub status: &'static str,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfReport {
    pub kernel: EbpfCheck,
    pub btf: EbpfCheck,
    pub capabilities: EbpfCheck,
    pub exec_tracing: EbpfCheck,
}

impl EbpfReport {
    pub fn is_ready(&self) -> bool {
        self.kernel.ok && self.btf.ok && self.capabilities.ok && self.exec_tracing.ok
    }

    pub fn failure_summary(&self) -> String {
        let mut parts = Vec::new();
        push_failure(&mut parts, "kernel", &self.kernel);
        push_failure(&mut parts, "BTF", &self.btf);
        push_failure(&mut parts, "capabilities", &self.capabilities);
        push_failure(&mut parts, "exec tracing", &self.exec_tracing);
        if parts.is_empty() {
            "eBPF checks failed".to_string()
        } else {
            parts.join("; ")
        }
    }
}

pub fn host_facts() -> EbpfFacts {
    read_facts(&EbpfProbePaths::host())
}

pub fn read_facts(paths: &EbpfProbePaths) -> EbpfFacts {
    EbpfFacts {
        kernel_release: read_to_string_lossy(&paths.kernel_release),
        btf_present: btf_file_present(&paths.btf),
        effective_caps: parse_cap_eff(&read_to_string_lossy(&paths.status)),
        exec_tracepoint_present: paths.exec_tracepoints.iter().any(|path| path_exists(path)),
    }
}

pub fn assess(facts: &EbpfFacts) -> EbpfReport {
    EbpfReport {
        kernel: kernel_check(&facts.kernel_release),
        btf: flag_check(
            facts.btf_present,
            "available",
            "unavailable",
            "BTF file not present",
        ),
        capabilities: capability_check(facts.effective_caps),
        exec_tracing: flag_check(
            facts.exec_tracepoint_present,
            "available",
            "unavailable",
            "sched_process_exec tracepoint not present",
        ),
    }
}

fn kernel_check(release: &str) -> EbpfCheck {
    let release = release.trim();
    if release.is_empty() {
        return failed("unsupported", "kernel release unavailable");
    }
    match parse_major_minor(release) {
        Some((major, minor))
            if major > MIN_KERNEL_MAJOR
                || (major == MIN_KERNEL_MAJOR && minor >= MIN_KERNEL_MINOR) =>
        {
            EbpfCheck {
                ok: true,
                status: "supported",
                detail: None,
            }
        }
        Some(_) => failed(
            "unsupported",
            &format!("{release} is older than {MIN_KERNEL_MAJOR}.{MIN_KERNEL_MINOR}"),
        ),
        None => failed("unsupported", "could not parse kernel release"),
    }
}

fn capability_check(effective: Option<u64>) -> EbpfCheck {
    match effective {
        Some(caps) if caps_allow_tracing(caps) => EbpfCheck {
            ok: true,
            status: "available",
            detail: None,
        },
        Some(_) => failed(
            "unavailable",
            "CAP_BPF and CAP_PERFMON or CAP_SYS_ADMIN required",
        ),
        None => failed("unavailable", "effective capabilities could not be read"),
    }
}

fn flag_check(ok: bool, pass: &'static str, fail: &'static str, detail: &str) -> EbpfCheck {
    if ok {
        EbpfCheck {
            ok: true,
            status: pass,
            detail: None,
        }
    } else {
        failed(fail, detail)
    }
}

fn failed(status: &'static str, detail: &str) -> EbpfCheck {
    EbpfCheck {
        ok: false,
        status,
        detail: Some(detail.to_string()),
    }
}

fn caps_allow_tracing(caps: u64) -> bool {
    has_cap(caps, CAP_SYS_ADMIN) || (has_cap(caps, CAP_BPF) && has_cap(caps, CAP_PERFMON))
}

fn has_cap(caps: u64, bit: u32) -> bool {
    caps & (1u64 << bit) != 0
}

fn parse_major_minor(release: &str) -> Option<(u32, u32)> {
    let mut parts = release.split('.');
    let major_raw = parts.next()?;
    let major_digits: String = major_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if major_digits.is_empty() || major_digits.len() != major_raw.len() {
        return None;
    }
    let major = major_digits.parse().ok()?;
    let minor_raw = parts.next()?;
    let minor_digits: String = minor_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if minor_digits.is_empty() {
        return None;
    }
    let minor = minor_digits.parse().ok()?;
    Some((major, minor))
}

fn parse_cap_eff(status: &str) -> Option<u64> {
    for line in status.lines() {
        let Some(rest) = line.trim().strip_prefix("CapEff:") else {
            continue;
        };
        let hex = rest.trim();
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let hex = if hex.len() > 16 {
            &hex[hex.len() - 16..]
        } else {
            hex
        };
        return u64::from_str_radix(hex, 16).ok();
    }
    None
}

fn btf_file_present(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.is_file() && meta.len() > 0
}

fn path_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

fn read_to_string_lossy(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn push_failure(parts: &mut Vec<String>, name: &str, check: &EbpfCheck) {
    if check.ok {
        return;
    }
    match &check.detail {
        Some(detail) => parts.push(format!("{name}: {} ({detail})", check.status)),
        None => parts.push(format!("{name}: {}", check.status)),
    }
}
