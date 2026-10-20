use std::io;
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

/// Supplementary groups past this are not read; membership in the daemon's group is
/// then decided by the primary group alone.
const MAX_GROUPS: usize = 4096;

/// The identity the kernel recorded for a socket peer when it connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCredentials {
    pid: i32,
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

impl PeerCredentials {
    pub fn new(pid: i32, uid: u32, gid: u32, groups: Vec<u32>) -> Self {
        Self {
            pid,
            uid,
            gid,
            groups,
        }
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn groups(&self) -> &[u32] {
        &self.groups
    }
}

/// Reads `SO_PEERCRED` and `SO_PEERGROUPS`. Both are captured by the kernel at connect
/// time, so a peer cannot change them afterwards and no `/proc` lookup races a PID reuse.
pub fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    let fd = stream.as_raw_fd();
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `fd` is an open socket borrowed from `stream` for the whole call.
    // `credentials` and `length` are valid for writes, and `length` holds the buffer size.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short SO_PEERCRED answer",
        ));
    }
    Ok(PeerCredentials {
        pid: credentials.pid,
        uid: credentials.uid,
        gid: credentials.gid,
        groups: peer_groups(fd)?,
    })
}

fn peer_groups(fd: i32) -> io::Result<Vec<u32>> {
    let mut groups = vec![0 as libc::gid_t; 64];
    loop {
        let mut length = (groups.len() * size_of::<libc::gid_t>()) as libc::socklen_t;
        // SAFETY: `fd` is an open socket for the duration of the call. `groups` is a
        // writable buffer of exactly `length` bytes.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERGROUPS,
                groups.as_mut_ptr().cast(),
                &mut length,
            )
        };
        let count = length as usize / size_of::<libc::gid_t>();
        if rc == 0 {
            groups.truncate(count.min(groups.len()));
            return Ok(groups);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ERANGE) if count > groups.len() && count <= MAX_GROUPS => {
                groups.resize(count, 0);
            }
            Some(libc::ERANGE) | Some(libc::ENOPROTOOPT) => return Ok(Vec::new()),
            _ => return Err(error),
        }
    }
}
