use std::io;
use std::path::{Path, PathBuf};

use crate::error::CollectorError;
use crate::process::cgroup::parse_cgroup_memberships;
use crate::process::process_record::ProcessRecord;
use crate::process::socket::{parse_socket_fd_target, SocketOwner};
use crate::process::warning::{ProcessWarning, ProcessWarningKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    mode: u32,
    device: u64,
    inode: u64,
}

impl FileIdentity {
    pub fn new(mode: u32, device: u64, inode: u64) -> Self {
        Self {
            mode,
            device,
            inode,
        }
    }

    pub fn is_fifo(self) -> bool {
        self.mode & 0o170000 == 0o010000
    }
    pub fn device(self) -> u64 {
        self.device
    }
    pub fn inode(self) -> u64 {
        self.inode
    }
}

pub trait ProcReader {
    fn file_identity(&self, _path: &Path) -> io::Result<FileIdentity> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor metadata unavailable",
        ))
    }

    fn list_pids(&self, proc_root: &Path) -> Result<Vec<u32>, CollectorError>;
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}

pub(crate) fn read_dir_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path)? {
        names.push(entry?.path());
    }
    Ok(names)
}

pub struct StdProcReader;

impl ProcReader for StdProcReader {
    fn file_identity(&self, path: &Path) -> io::Result<FileIdentity> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)?;
        Ok(FileIdentity::new(
            metadata.mode(),
            metadata.dev(),
            metadata.ino(),
        ))
    }

    fn list_pids(&self, proc_root: &Path) -> Result<Vec<u32>, CollectorError> {
        list_pids(proc_root)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        read_dir_paths(path)
    }
}

pub(crate) fn read_inet_table_content(
    reader: &dyn ProcReader,
    proc_root: &Path,
    table: crate::process::socket::InetTableKind,
    warnings: &mut Vec<ProcessWarning>,
) -> Option<String> {
    let malformed = if table.protocol() == twin_core::TransportProtocol::Udp {
        ProcessWarningKind::UdpTableMalformed
    } else {
        ProcessWarningKind::TcpTableMalformed
    };
    let path = proc_root.join("net").join(table.net_file_name());
    match reader.read_file(&path) {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(text) => Some(text.to_string()),
            Err(e) => {
                warnings.push(ProcessWarning::new(malformed, path, e.to_string()));
                None
            }
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if table.protocol() == twin_core::TransportProtocol::Udp {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::UdpTableMissing,
                    path.clone(),
                    err.to_string(),
                ));
            }
            if table.protocol() == twin_core::TransportProtocol::Tcp {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::TcpTableMissing,
                    path,
                    err.to_string(),
                ));
            }
            None
        }
        Err(err) => {
            warnings.push(ProcessWarning::new(malformed, path, err.to_string()));
            None
        }
    }
}

pub(crate) fn read_unix_table_content(
    reader: &dyn ProcReader,
    proc_root: &Path,
    warnings: &mut Vec<ProcessWarning>,
) -> Option<String> {
    let path = proc_root.join("net").join("unix");
    match reader.read_file(&path) {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(text) => Some(text.to_string()),
            Err(e) => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::UnixTableMalformed,
                    path,
                    e.to_string(),
                ));
                None
            }
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::UnixTableMissing,
                path,
                err.to_string(),
            ));
            None
        }
        Err(err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::UnixTableMalformed,
                path,
                err.to_string(),
            ));
            None
        }
    }
}

pub(crate) fn read_fd_socket_owners(
    reader: &dyn ProcReader,
    proc_root: &Path,
    pid: u32,
    start_ticks: Option<u64>,
    warnings: &mut Vec<ProcessWarning>,
) -> Vec<SocketOwner> {
    let owners = read_fd_socket_owners_unchecked(reader, proc_root, pid, warnings);
    if let Some(expected) = start_ticks {
        let path = proc_root.join(pid.to_string()).join("stat");
        let actual = reader
            .read_file(&path)
            .ok()
            .and_then(|bytes| parse_stat(&bytes).ok())
            .and_then(|stat| stat.start_ticks);
        if actual != Some(expected) {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::FdVanished,
                path,
                "process incarnation changed during descriptor collection",
            ));
            return Vec::new();
        }
    }
    owners
}

fn read_fd_socket_owners_unchecked(
    reader: &dyn ProcReader,
    proc_root: &Path,
    pid: u32,
    warnings: &mut Vec<ProcessWarning>,
) -> Vec<SocketOwner> {
    let fd_dir = proc_root.join(pid.to_string()).join("fd");
    let entries = match reader.read_dir(&fd_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) if is_vanished(&err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::FdVanished,
                fd_dir.clone(),
                err.to_string(),
            ));
            return Vec::new();
        }
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::FdPermissionDenied,
                fd_dir,
                err.to_string(),
            ));
            return Vec::new();
        }
        Err(err) => {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::FdMalformed,
                fd_dir,
                err.to_string(),
            ));
            return Vec::new();
        }
    };

    let fdinfo_dir = proc_root.join(pid.to_string()).join("fdinfo");
    let has_fdinfo = match reader.read_dir(&fdinfo_dir) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            warnings.push(ProcessWarning::new(
                if error.kind() == io::ErrorKind::PermissionDenied {
                    ProcessWarningKind::FdPermissionDenied
                } else {
                    ProcessWarningKind::FdMalformed
                },
                fdinfo_dir.clone(),
                "fdinfo directory is unavailable",
            ));
            false
        }
    };
    let mut owners = Vec::new();
    for path in entries {
        let Some(fd_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(fd) = fd_name.parse::<u32>() else {
            continue;
        };
        match reader.read_link(&path) {
            Ok(target) => {
                let target_str = target.to_string_lossy();
                let Some(inode) = parse_socket_fd_target(&target_str) else {
                    continue;
                };
                let mut is_fdinfo_verified = false;
                if has_fdinfo {
                    let fdinfo = fdinfo_dir.join(fd_name);
                    match reader.read_file(&fdinfo) {
                        Ok(bytes) => {
                            let content = String::from_utf8_lossy(&bytes);
                            let actual = content.lines().find_map(|line| {
                                line.strip_prefix("ino:")
                                    .and_then(|value| value.trim().parse::<u64>().ok())
                            });
                            if actual != Some(inode) {
                                warnings.push(ProcessWarning::new(ProcessWarningKind::FdMalformed, fdinfo, "socket symlink and fdinfo inode disagree or fdinfo is malformed"));
                                continue;
                            }
                            is_fdinfo_verified = true;
                        }
                        Err(error) if is_vanished(&error) => {
                            warnings.push(ProcessWarning::new(
                                ProcessWarningKind::FdVanished,
                                fdinfo,
                                "socket fd vanished during fdinfo validation",
                            ));
                            continue;
                        }
                        Err(error) => warnings.push(ProcessWarning::new(
                            if error.kind() == io::ErrorKind::PermissionDenied {
                                ProcessWarningKind::FdPermissionDenied
                            } else {
                                ProcessWarningKind::FdMalformed
                            },
                            fdinfo,
                            "socket fdinfo is unreadable",
                        )),
                    }
                }
                owners.push(SocketOwner {
                    pid,
                    fd,
                    inode,
                    fd_path: format!("/proc/{pid}/fd/{fd}"),
                    is_fdinfo_verified,
                });
            }
            Err(err) if is_vanished(&err) => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::FdVanished,
                    path.clone(),
                    err.to_string(),
                ));
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::FdPermissionDenied,
                    path,
                    err.to_string(),
                ));
            }
            Err(err) => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::FdMalformed,
                    path,
                    err.to_string(),
                ));
            }
        }
    }
    owners
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
                record.start_ticks = parsed.start_ticks;
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

    if let Some(expected) = record.start_ticks {
        let actual = reader
            .read_file(&stat_path)
            .ok()
            .and_then(|bytes| parse_stat(&bytes).ok())
            .and_then(|stat| stat.start_ticks);
        if actual != Some(expected) {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::Vanished,
                stat_path,
                "process incarnation changed during collection",
            ));
            return None;
        }
    }
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
    start_ticks: Option<u64>,
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
    let start_ticks = fields.nth(17).and_then(|value| value.parse::<u64>().ok());
    let open = text
        .find('(')
        .ok_or_else(|| "missing comm open".to_string())?;
    let comm = text[open + 1..close].to_string();
    Ok(ParsedStat {
        comm,
        state,
        ppid,
        start_ticks,
    })
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

pub(crate) fn has_start_ticks(reader: &dyn ProcReader, path: &Path, expected: u64) -> bool {
    reader
        .read_file(path)
        .ok()
        .and_then(|bytes| parse_stat(&bytes).ok())
        .and_then(|stat| stat.start_ticks)
        == Some(expected)
}
