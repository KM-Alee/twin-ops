use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContainerMetadata {
    pub id: String,
    pub image: Option<String>,
    pub created_at_ns: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Created,
    Running,
    Stopped,
    Paused,
    Pausing,
    Unknown,
}

impl TaskState {
    pub(crate) fn from_wire(value: i32) -> Self {
        match value {
            1 => Self::Created,
            2 => Self::Running,
            3 => Self::Stopped,
            4 => Self::Paused,
            5 => Self::Pausing,
            _ => Self::Unknown,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Paused => "paused",
            Self::Pausing => "pausing",
            Self::Unknown => "unknown",
        }
    }
    pub fn is_running(self) -> Option<bool> {
        match self {
            Self::Running => Some(true),
            Self::Created | Self::Stopped | Self::Paused => Some(false),
            Self::Pausing | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskMetadata {
    pub container_id: String,
    pub pid: Option<u32>,
    pub state: TaskState,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageMetadata {
    pub name: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NamespaceSnapshot {
    pub namespace: String,
    pub containers: Option<Vec<ContainerMetadata>>,
    pub tasks: Option<Vec<TaskMetadata>>,
    pub images: Option<Vec<ImageMetadata>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ContainerdSnapshot {
    pub namespaces: Vec<NamespaceSnapshot>,
    pub namespace_names: Vec<String>,
    pub is_complete: bool,
}
