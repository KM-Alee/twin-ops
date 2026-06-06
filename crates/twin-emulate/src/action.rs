use twin_core::NodeId;

pub const RESTART_ACTION: &str = "restart";
pub const SAFETY_STATEMENT: &str = "No action was performed.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulationAction {
    RestartService { target: NodeId },
}

impl EmulationAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RestartService { .. } => RESTART_ACTION,
        }
    }
}
