use std::path::{Path, PathBuf};

/// The host's filesystem as twin sees it. Unset, twin reads `/`. In a container the
/// host is mounted read-only (for example at `/host`) and `TWIN_HOST_ROOT` names it, so
/// `/proc`, unit directories, package databases, configuration and the Docker,
/// containerd and D-Bus sockets are all read under that one root.
pub fn host_root() -> Option<PathBuf> {
    std::env::var_os("TWIN_HOST_ROOT")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute() && root != Path::new("/"))
}

/// A host path (`/etc/nginx/nginx.conf`) as readable from here.
pub fn host_path(path: &str) -> PathBuf {
    match (host_root(), path.strip_prefix('/')) {
        (Some(root), Some(rest)) => root.join(rest),
        _ => PathBuf::from(path),
    }
}

/// The host's system bus socket when twin reads a mounted host root; `None` uses the
/// default system bus.
pub fn host_dbus_socket() -> Option<PathBuf> {
    host_root().map(|_| host_path("/run/dbus/system_bus_socket"))
}

/// The host path a file under the mounted host root stands for: `/host/etc/x` becomes
/// `/etc/x`. Paths elsewhere are returned as they are. Node IDs and evidence always use
/// this form, so a containerised twin and a host twin name files the same way.
pub fn logical_path(path: &Path) -> String {
    match host_root().and_then(|root| path.strip_prefix(&root).ok().map(Path::to_path_buf)) {
        Some(rest) => Path::new("/").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

/// The host's `/proc`.
pub fn default_proc_root() -> PathBuf {
    host_path("/proc")
}

/// Whether `proc_root` is the live host rather than a fixture, which decides whether
/// host-wide sources (unit files, crontabs) belong to this scan.
pub fn is_host_proc_root(proc_root: &Path) -> bool {
    proc_root == default_proc_root()
}
