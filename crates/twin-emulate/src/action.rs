use twin_core::NodeId;

pub const RESTART_ACTION: &str = "restart";
pub const DELETE_ACTION: &str = "delete";
pub const FILL_DISK_ACTION: &str = "fill-disk";
pub const UPGRADE_ACTION: &str = "upgrade";
pub const SAFETY_STATEMENT: &str = "No action was performed.";
pub const DELETE_SAFETY_STATEMENT: &str = "No file was deleted.";
pub const FILL_DISK_SAFETY_STATEMENT: &str = "No disk was written.";
pub const UPGRADE_SAFETY_STATEMENT: &str = "No package manager ran. Nothing was upgraded.";
pub const CONTAINER_RESTART_SAFETY_STATEMENT: &str = "No container was restarted.";
pub const ROLLOUT_ACTION: &str = "rollout";
pub const K8S_POD_DELETE_SAFETY_STATEMENT: &str = "No pod was deleted.";
pub const K8S_ROLLOUT_SAFETY_STATEMENT: &str = "No deployment was rolled out.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulationAction {
    RestartService { target: NodeId },
    RestartContainer { target: NodeId },
    DeleteFile { target: NodeId },
    FillMount { target: NodeId, to_percent: u8 },
    UpgradePackage { target: NodeId },
    DeleteK8sPod { target: NodeId },
    RolloutK8sDeployment { target: NodeId },
}

impl EmulationAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RestartService { .. } | Self::RestartContainer { .. } => RESTART_ACTION,
            Self::DeleteFile { .. } | Self::DeleteK8sPod { .. } => DELETE_ACTION,
            Self::FillMount { .. } => FILL_DISK_ACTION,
            Self::UpgradePackage { .. } => UPGRADE_ACTION,
            Self::RolloutK8sDeployment { .. } => ROLLOUT_ACTION,
        }
    }
}
