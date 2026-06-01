use std::io;
use std::path::{Path, PathBuf};

use twin_collectors::process::ProcReader;

pub struct VanishPidReader {
    inner: twin_collectors::process::StdProcReader,
    vanished_pid: u32,
}

impl VanishPidReader {
    pub fn new(vanished_pid: u32) -> Self {
        Self {
            inner: twin_collectors::process::StdProcReader,
            vanished_pid,
        }
    }
}

impl ProcReader for VanishPidReader {
    fn list_pids(&self, proc_root: &Path) -> Result<Vec<u32>, twin_collectors::CollectorError> {
        self.inner.list_pids(proc_root)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        if path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u32>().ok())
            == Some(self.vanished_pid)
        {
            return Err(io::Error::new(io::ErrorKind::NotFound, "vanished"));
        }
        self.inner.read_file(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.read_link(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.read_dir(path)
    }
}

pub fn write_tcp_table(base: &Path, name: &str, content: &str) {
    let net = base.join("net");
    std::fs::create_dir_all(&net).expect("net dir");
    std::fs::write(net.join(name), content).expect("tcp table");
}

pub fn write_socket_fd(base: &Path, pid: u32, fd: u32, inode: u64) {
    let fd_dir = base.join(pid.to_string()).join("fd");
    std::fs::create_dir_all(&fd_dir).expect("fd dir");
    #[cfg(unix)]
    {
        let target = format!("socket:[{inode}]");
        std::os::unix::fs::symlink(target, fd_dir.join(fd.to_string())).expect("fd symlink");
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, fd, inode);
    }
}

pub fn write_proc_fixture(
    base: &Path,
    pid: u32,
    stat: &str,
    status: &str,
    cmdline: &[u8],
    exe: Option<&Path>,
) {
    write_proc_fixture_with_cgroup(base, pid, stat, status, cmdline, exe, None);
}

pub fn write_proc_fixture_with_cgroup(
    base: &Path,
    pid: u32,
    stat: &str,
    status: &str,
    cmdline: &[u8],
    exe: Option<&Path>,
    cgroup: Option<&str>,
) {
    let dir = base.join(pid.to_string());
    std::fs::create_dir_all(&dir).expect("pid dir");
    std::fs::write(dir.join("stat"), stat).expect("stat");
    std::fs::write(dir.join("status"), status).expect("status");
    std::fs::write(dir.join("cmdline"), cmdline).expect("cmdline");
    if let Some(content) = cgroup {
        std::fs::write(dir.join("cgroup"), content).expect("cgroup");
    }
    if let Some(target) = exe {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, dir.join("exe")).expect("exe symlink");
        }
        #[cfg(not(unix))]
        {
            let _ = target;
        }
    }
}
