use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use tempfile::TempDir;

static TWIN_BIN: OnceLock<PathBuf> = OnceLock::new();

pub fn twin_bin() -> &'static Path {
    TWIN_BIN
        .get_or_init(|| {
            PathBuf::from(std::env::var("CARGO_BIN_EXE_twin").expect("CARGO_BIN_EXE_twin not set"))
        })
        .as_path()
}

pub struct TwinHome {
    pub _temp: TempDir,
    pub proc_root: PathBuf,
}

impl TwinHome {
    pub fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let proc_root = temp.path().join("fixture-proc");
        std::fs::create_dir_all(&proc_root).expect("proc dir");
        Self {
            _temp: temp,
            proc_root,
        }
    }

    pub fn home_path(&self) -> &Path {
        self._temp.path()
    }

    pub fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(twin_bin());
        cmd.env("HOME", self.home_path());
        cmd.env_remove("XDG_DATA_HOME");
        cmd.env_remove("XDG_CONFIG_HOME");
        cmd.env_remove("XDG_STATE_HOME");
        cmd.env("TWIN_PROC_ROOT", &self.proc_root);
        cmd.args(args);
        cmd.output().expect("spawn twin")
    }

    pub fn run_without_proc(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(twin_bin());
        cmd.env("HOME", self.home_path());
        cmd.env_remove("XDG_DATA_HOME");
        cmd.env_remove("XDG_CONFIG_HOME");
        cmd.env_remove("XDG_STATE_HOME");
        cmd.env_remove("TWIN_PROC_ROOT");
        cmd.args(args);
        cmd.output().expect("spawn twin")
    }
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
        std::os::unix::fs::symlink(target, dir.join("exe")).expect("exe symlink");
        #[cfg(not(unix))]
        let _ = target;
    }
}

pub fn write_slice5_fixture(proc_root: &Path) {
    write_proc_fixture_with_cgroup(
        proc_root,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(Path::new("/usr/lib/systemd/systemd")),
        None,
    );
    write_proc_fixture_with_cgroup(
        proc_root,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        Some(Path::new("/usr/bin/nginx")),
        Some("0::/system.slice/nginx.service\n"),
    );
}

pub fn write_non_systemd_cgroup_fixture(proc_root: &Path) {
    write_proc_fixture_with_cgroup(
        proc_root,
        99,
        "99 (bash) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/bin/bash\0",
        Some(Path::new("/bin/bash")),
        Some("0::/user.slice/user-1000.slice/session-2.scope\n"),
    );
}

pub fn write_malformed_cgroup_fixture(proc_root: &Path) {
    write_proc_fixture_with_cgroup(
        proc_root,
        10,
        "10 (app) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/app\0",
        Some(Path::new("/usr/bin/app")),
        Some("not-a-cgroup-line\n0::/system.slice/app.service\n"),
    );
}

pub fn write_ambiguous_services_fixture(proc_root: &Path) {
    write_slice5_fixture(proc_root);
    write_proc_fixture_with_cgroup(
        proc_root,
        43,
        "43 (ang) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/ang\0",
        Some(Path::new("/usr/bin/ang")),
        Some("0::/system.slice/ang.service\n"),
    );
}

pub fn stdout_utf8(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr_utf8(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn init_and_scan(home: &TwinHome) {
    let init = home.run(&["init"]);
    assert!(init.status.success(), "init: {}", stderr_utf8(&init));
    write_slice5_fixture(&home.proc_root);
    let scan = home.run(&["scan"]);
    assert!(scan.status.success(), "scan: {}", stderr_utf8(&scan));
}
