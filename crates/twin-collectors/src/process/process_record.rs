use std::path::PathBuf;

use crate::process::cgroup::CgroupMembership;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    pid: u32,
    pub(crate) ppid: Option<u32>,
    pub(crate) comm: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) uid: Option<u32>,
    pub(crate) gid: Option<u32>,
    pub(crate) argv: Vec<String>,
    pub(crate) exe: Option<PathBuf>,
    pub(crate) cgroup_memberships: Vec<CgroupMembership>,
}

impl ProcessRecord {
    pub(crate) fn new(pid: u32) -> Self {
        Self {
            pid,
            ppid: None,
            comm: None,
            state: None,
            uid: None,
            gid: None,
            argv: Vec::new(),
            exe: None,
            cgroup_memberships: Vec::new(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn ppid(&self) -> Option<u32> {
        self.ppid
    }

    pub fn comm(&self) -> Option<&str> {
        self.comm.as_deref()
    }

    pub fn state(&self) -> Option<&str> {
        self.state.as_deref()
    }

    pub fn uid(&self) -> Option<u32> {
        self.uid
    }

    pub fn gid(&self) -> Option<u32> {
        self.gid
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn exe(&self) -> Option<&PathBuf> {
        self.exe.as_ref()
    }

    pub fn cgroup_memberships(&self) -> &[CgroupMembership] {
        &self.cgroup_memberships
    }

    pub fn label(&self) -> String {
        if let Some(argv0) = self.argv.first() {
            let base = argv0
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(argv0.as_str());
            return base.to_string();
        }
        self.comm
            .clone()
            .unwrap_or_else(|| format!("pid:{}", self.pid))
    }
}
