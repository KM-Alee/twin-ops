use std::io;
use std::path::Path;

use super::PeerCredentials;

/// Who may query the daemon: root, the daemon's own user, and members of the daemon's
/// group (the group that owns its socket).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessPolicy {
    uid: u32,
    gid: u32,
}

impl AccessPolicy {
    pub fn new(uid: u32, gid: u32) -> Self {
        Self { uid, gid }
    }

    /// The running process's effective user and group.
    pub fn current() -> io::Result<Self> {
        Self::from_status(Path::new("/proc/self/status"))
    }

    fn from_status(path: &Path) -> io::Result<Self> {
        let status = std::fs::read_to_string(path)?;
        let effective = |key: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .and_then(|ids| ids.split_whitespace().nth(1))
                .and_then(|id| id.parse::<u32>().ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} has no effective {key}", path.display()),
                    )
                })
        };
        Ok(Self::new(effective("Uid:")?, effective("Gid:")?))
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn allows(&self, peer: &PeerCredentials) -> bool {
        peer.uid() == 0
            || peer.uid() == self.uid
            || peer.gid() == self.gid
            || peer.groups().contains(&self.gid)
    }
}
