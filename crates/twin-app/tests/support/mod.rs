// Integration-test support module: shared by multiple test binaries, and each
// binary uses only a subset of these fixtures.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use tempfile::TempDir;

static SCAN_ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct ScanEnvGuard {
    _lock: MutexGuard<'static, ()>,
}

pub fn lock_scan_env() -> ScanEnvGuard {
    ScanEnvGuard {
        _lock: SCAN_ENV_LOCK.lock().expect("scan env lock"),
    }
}

pub fn set_systemd_unit_root(root: &Path) {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::set_var("TWIN_SYSTEMD_UNIT_ROOT", root);
    }
}

pub fn set_host_root(root: &Path) {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::set_var("TWIN_HOST_ROOT", root);
    }
}

pub fn clear_scan_env() {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::remove_var("TWIN_SYSTEMD_UNIT_ROOT");
        std::env::remove_var("TWIN_HOST_ROOT");
        std::env::remove_var("TWIN_SYSTEMD_CGROUP_MAP");
        std::env::remove_var("TWIN_DOCKER_FIXTURE");
        std::env::remove_var("TWIN_K8S_FIXTURE");
    }
}
use twin_app::paths::TwinLayout;

pub struct IsolatedHome {
    pub _temp: TempDir,
    pub layout: TwinLayout,
}

impl IsolatedHome {
    pub fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let layout = TwinLayout::isolated(temp.path());
        Self {
            _temp: temp,
            layout,
        }
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

pub fn write_tcp_table(base: &std::path::Path, name: &str, content: &str) {
    let net = base.join("net");
    std::fs::create_dir_all(&net).expect("net dir");
    std::fs::write(net.join(name), content).expect("tcp table");
}

pub fn write_unix_table(base: &std::path::Path, content: &str) {
    write_tcp_table(base, "unix", content);
}

pub fn write_socket_fd(base: &std::path::Path, pid: u32, fd: u32, inode: u64) {
    let fd_dir = base.join(pid.to_string()).join("fd");
    std::fs::create_dir_all(&fd_dir).expect("fd dir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(format!("socket:[{inode}]"), fd_dir.join(fd.to_string()))
        .expect("fd symlink");
    #[cfg(not(unix))]
    let _ = (pid, fd, inode);
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
    twin_fixtures::write_proc_fixture_with_cgroup(base, pid, stat, status, cmdline, exe, cgroup)
        .expect("proc fixture");
}
