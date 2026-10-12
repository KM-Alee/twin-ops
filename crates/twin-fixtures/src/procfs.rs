use std::io;
use std::path::Path;

pub fn write_proc_fixture_with_cgroup(
    base: &Path,
    pid: u32,
    stat: &str,
    status: &str,
    cmdline: &[u8],
    exe: Option<&Path>,
    cgroup: Option<&str>,
) -> io::Result<()> {
    let boot_directory = base.join("sys/kernel/random");
    std::fs::create_dir_all(&boot_directory)?;
    let boot_path = boot_directory.join("boot_id");
    if !boot_path.exists() {
        std::fs::write(boot_path, "00000000-0000-0000-0000-000000000001\n")?;
    }
    let directory = base.join(pid.to_string());
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    for namespace_root in [base.join("self"), directory.clone()] {
        let ns = namespace_root.join("ns");
        std::fs::create_dir_all(&ns)?;
        let path = ns.join("net");
        if std::fs::symlink_metadata(&path).is_err() {
            std::os::unix::fs::symlink("net:[1]", path)?;
        }
    }
    std::fs::write(directory.join("stat"), stat)?;
    std::fs::write(directory.join("status"), status)?;
    std::fs::write(directory.join("cmdline"), cmdline)?;
    if let Some(content) = cgroup {
        std::fs::write(directory.join("cgroup"), content)?;
    }
    if let Some(target) = exe {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, directory.join("exe"))?;
        #[cfg(not(unix))]
        let _ = target;
    }
    Ok(())
}
