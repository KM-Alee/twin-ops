use std::path::PathBuf;

use crate::process::cgroup::CgroupMembership;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    pid: u32,
    pub(crate) start_ticks: Option<u64>,
    pub(crate) boot_id: Option<String>,
    pub(crate) network_namespace: Option<twin_core::NetworkNamespaceId>,
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
            start_ticks: None,
            boot_id: None,
            network_namespace: None,
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

    pub fn start_ticks(&self) -> Option<u64> {
        self.start_ticks
    }

    pub fn boot_id(&self) -> Option<&str> {
        self.boot_id.as_deref()
    }

    pub fn network_namespace(&self) -> Option<twin_core::NetworkNamespaceId> {
        self.network_namespace
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
        if let Some(name) = self
            .exe
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
        {
            return name.to_string();
        }
        self.comm
            .clone()
            .unwrap_or_else(|| format!("pid:{}", self.pid))
    }
}
