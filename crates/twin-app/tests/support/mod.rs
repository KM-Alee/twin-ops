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

/// postgresql.service listens on 0.0.0.0:5432 and django.service holds a live
/// connection to it.
pub fn scan_postgres_django(home: &IsolatedHome) -> std::path::PathBuf {
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        &proc,
        "tcp",
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0",
    );
    write_socket_fd(&proc, 721, 8, 12345);
    write_socket_fd(&proc, 8841, 12, 456);
    let docker = home.layout.data_dir.join("empty-docker");
    std::fs::create_dir_all(docker.join("containers")).expect("docker fixture");
    std::fs::write(docker.join("containers/list.json"), "[]").expect("container list");
    let scan = twin_app::ScanRequest {
        docker_fixture: Some(docker),
        ..twin_app::ScanRequest::default()
    };
    twin_app::scan_in(&home.layout, scan, &proc).expect("scan");
    proc
}

/// A scan request that never reads the host's Docker daemon, so graph counts do not
/// depend on which containers happen to run on the test machine.
pub fn host_isolated_scan(home: &IsolatedHome) -> twin_app::ScanRequest {
    let docker = home.layout.data_dir.join("isolated-empty-docker");
    std::fs::create_dir_all(docker.join("containers")).expect("docker fixture");
    std::fs::write(docker.join("containers/list.json"), "[]").expect("container list");
    twin_app::ScanRequest {
        docker_fixture: Some(docker),
        ..twin_app::ScanRequest::default()
    }
}
