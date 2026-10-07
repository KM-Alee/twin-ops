#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountRecord {
    pub device: String,
    pub mount_point: String,
    pub fstype: String,
}

const SKIP_FSTYPE: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "bpf",
    "tracefs",
    "debugfs",
    "securityfs",
    "configfs",
    "fusectl",
    "mqueue",
    "nsfs",
    "autofs",
    "binfmt_misc",
    "pstore",
    "ramfs",
];

pub fn parse_mounts(text: &str) -> Vec<MountRecord> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(record) = parse_mount_line(line) else {
            continue;
        };
        if skip_mount(&record) {
            continue;
        }
        out.push(record);
    }
    out
}

fn parse_mount_line(line: &str) -> Option<MountRecord> {
    let mut fields = line.split_whitespace();
    let device = fields.next()?;
    let mount_point = fields.next()?;
    let fstype = fields.next()?;
    fields.next()?;
    let mount_point = unescape_mount(mount_point);
    if !mount_point.starts_with('/') {
        return None;
    }
    Some(MountRecord {
        device: unescape_mount(device),
        mount_point,
        fstype: fstype.to_string(),
    })
}

fn skip_mount(record: &MountRecord) -> bool {
    if SKIP_FSTYPE.contains(&record.fstype.as_str()) {
        return true;
    }
    let point = record.mount_point.as_str();
    point.starts_with("/proc") || point.starts_with("/sys") || point.starts_with("/dev")
}

fn unescape_mount(field: &str) -> String {
    field
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_var_and_skips_pseudo_filesystems() {
        let text = "\
/dev/sda1 /var ext4 rw,relatime 0 0
proc /proc proc rw 0 0
sysfs /sys sysfs rw 0 0
tmpfs /dev/shm tmpfs rw 0 0
not-a-mount
";
        let mounts = parse_mounts(text);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mount_point, "/var");
        assert_eq!(mounts[0].fstype, "ext4");
        assert_eq!(mounts[0].device, "/dev/sda1");
    }

    #[test]
    fn unescapes_spaces_in_mount_points() {
        let mounts = parse_mounts("tmpfs /var/lib/my\\040disk tmpfs rw 0 0\n");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mount_point, "/var/lib/my disk");
    }
}
