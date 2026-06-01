use std::io;
use std::path::{Path, PathBuf};

use crate::error::CollectorError;
use crate::process::cgroup::parse_cgroup_memberships;
use crate::process::process_record::ProcessRecord;
use crate::process::warning::{ProcessWarning, ProcessWarningKind};

pub trait ProcReader {
    fn list_pids(&self, proc_root: &Path) -> Result<Vec<u32>, CollectorError>;
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
}

pub struct StdProcReader;

impl ProcReader for StdProcReader {
    fn list_pids(&self, proc_root: &Path) -> Result<Vec<u32>, CollectorError> {
        list_pids(proc_root)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }
}

pub(crate) fn list_pids(proc_root: &Path) -> Result<Vec<u32>, CollectorError> {
    let mut pids = Vec::new();
    let entries = std::fs::read_dir(proc_root).map_err(|source| CollectorError::ProcRootRead {
        path: proc_root.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CollectorError::ProcRootRead {
            path: proc_root.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Ok(pid) = name.parse::<u32>() {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    Ok(pids)
}

pub(crate) fn read_process(
    reader: &dyn ProcReader,
    proc_root: &Path,
    pid: u32,
    warnings: &mut Vec<ProcessWarning>,
) -> Option<ProcessRecord> {
    let pid_dir = proc_root.join(pid.to_string());
    let mut record = ProcessRecord::new(pid);

    let stat_path = pid_dir.join("stat");
    match reader.read_file(&stat_path) {
        Ok(bytes) => match parse_stat(&bytes) {
            Ok(parsed) => {
                record.ppid = Some(parsed.ppid);
                record.comm = Some(parsed.comm);
                record.state = Some(parsed.state);
            }
            Err(detail) => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::Malformed,
                    stat_path,
                    detail,
                ));
                return None;
            }
        },
        Err(err) if is_vanished(&err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::Vanished,
                stat_path,
                err.to_string(),
            ));
            return None;
        }
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::PermissionDenied,
                stat_path,
                err.to_string(),
            ));
            return None;
        }
        Err(err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::Malformed,
                stat_path,
                err.to_string(),
            ));
            return None;
        }
    }

    let status_path = pid_dir.join("status");
    if let Ok(bytes) = reader.read_file(&status_path) {
        if let Ok((uid, gid)) = parse_status(&bytes) {
            record.uid = uid;
            record.gid = gid;
        }
    }

    let cmdline_path = pid_dir.join("cmdline");
    match reader.read_file(&cmdline_path) {
        Ok(bytes) => record.argv = parse_cmdline(&bytes),
        Err(err) if is_vanished(&err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::Vanished,
                cmdline_path,
                err.to_string(),
            ));
            return None;
        }
        Err(_) => {}
    }

    let exe_path = pid_dir.join("exe");
    match reader.read_link(&exe_path) {
        Ok(target) => record.exe = Some(target),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::ExeUnreadable,
                exe_path,
                err.to_string(),
            ));
        }
        Err(_) => {}
    }

    read_cgroup_into_record(reader, &pid_dir, pid, &mut record, warnings);

    Some(record)
}

fn read_cgroup_into_record(
    reader: &dyn ProcReader,
    pid_dir: &Path,
    pid: u32,
    record: &mut ProcessRecord,
    warnings: &mut Vec<ProcessWarning>,
) {
    let cgroup_path = pid_dir.join("cgroup");
    let bytes = match reader.read_file(&cgroup_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::CgroupMissing,
                cgroup_path,
                err.to_string(),
            ));
            return;
        }
        Err(err) if is_vanished(&err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::Vanished,
                cgroup_path.clone(),
                err.to_string(),
            ));
            return;
        }
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::CgroupPermissionDenied,
                cgroup_path,
                err.to_string(),
            ));
            return;
        }
        Err(err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::CgroupMalformed,
                cgroup_path,
                err.to_string(),
            ));
            return;
        }
    };

    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::CgroupMalformed,
                cgroup_path.clone(),
                err.to_string(),
            ));
            return;
        }
    };

    let (memberships, issues) = parse_cgroup_memberships(text);
    for (line_no, detail) in issues {
        warnings.push(ProcessWarning::new(
            ProcessWarningKind::CgroupMalformed,
            cgroup_path.clone(),
            format!("pid={pid} line {line_no}: {detail}"),
        ));
    }
    record.cgroup_memberships = memberships;
}

struct ParsedStat {
    comm: String,
    state: String,
    ppid: u32,
}

fn parse_stat(bytes: &[u8]) -> Result<ParsedStat, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let close = text
        .rfind(')')
        .ok_or_else(|| "missing comm terminator".to_string())?;
    let after = text[close + 1..].trim_start();
    let mut fields = after.split_whitespace();
    let state = fields
        .next()
        .ok_or_else(|| "missing state".to_string())?
        .to_string();
    let ppid = fields
        .next()
        .ok_or_else(|| "missing ppid".to_string())?
        .parse::<u32>()
        .map_err(|e| e.to_string())?;
    let open = text
        .find('(')
        .ok_or_else(|| "missing comm open".to_string())?;
    let comm = text[open + 1..close].to_string();
    Ok(ParsedStat { comm, state, ppid })
}

fn parse_status(bytes: &[u8]) -> Result<(Option<u32>, Option<u32>), String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut uid = None;
    let mut gid = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Uid:\t") {
            uid = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("Gid:\t") {
            gid = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        }
    }
    Ok((uid, gid))
}

fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    let mut args = Vec::new();
    for part in bytes.split(|b| *b == 0) {
        if part.is_empty() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(part) {
            args.push(s.to_string());
        }
    }
    args
}

fn is_vanished(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::NotFound
}
