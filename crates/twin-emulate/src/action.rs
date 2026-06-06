use twin_core::NodeId;

pub const RESTART_ACTION: &str = "restart";
pub const DELETE_ACTION: &str = "delete";
pub const SAFETY_STATEMENT: &str = "No action was performed.";
pub const DELETE_SAFETY_STATEMENT: &str = "No file was deleted.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulationAction {
    RestartService { target: NodeId },
    DeleteFile { target: NodeId },
}

impl EmulationAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RestartService { .. } => RESTART_ACTION,
            Self::DeleteFile { .. } => DELETE_ACTION,
        }
    }
}
