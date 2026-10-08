use crate::graph_metadata::GraphMetadata;
use crate::node::{NodeId, NodeKind, NodeState};
use crate::TimestampNs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    id: NodeId,
    kind: NodeKind,
    label: String,
    state: NodeState,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
    valid_from: TimestampNs,
    valid_to: Option<TimestampNs>,
    metadata: GraphMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodeParts {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    pub state: NodeState,
    pub first_seen: TimestampNs,
    pub last_seen: TimestampNs,
    pub valid_from: TimestampNs,
    pub valid_to: Option<TimestampNs>,
    pub metadata: GraphMetadata,
}

impl GraphNode {
    pub fn from_parts(parts: GraphNodeParts) -> Self {
        Self {
            id: parts.id,
            kind: parts.kind,
            label: parts.label,
            state: parts.state,
            first_seen: parts.first_seen,
            last_seen: parts.last_seen,
            valid_from: parts.valid_from,
            valid_to: parts.valid_to,
            metadata: parts.metadata,
        }
    }

    pub fn process(
        pid: u32,
        label: impl Into<String>,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let id = NodeId::process(pid);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        Self {
            id,
            kind: NodeKind::Process,
            label: label.into(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::empty(),
        }
    }

    pub fn file(path: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::file(path);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        Self {
            id,
            kind: NodeKind::File,
            label: path.to_string(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::empty(),
        }
    }

    pub fn file_fingerprinted(
        path: &str,
        seen_at: TimestampNs,
        existing: Option<&Self>,
        fingerprint: &str,
    ) -> Self {
        let mut node = Self::file(path, seen_at, existing);
        let metadata = serde_json::json!({ "content_hash": fingerprint }).to_string();
        node.metadata = GraphMetadata::from_json(&metadata);
        node
    }

    pub fn mount(
        path: &str,
        seen_at: TimestampNs,
        existing: Option<&Self>,
        device: &str,
        fstype: &str,
        used_percent: Option<u8>,
    ) -> Self {
        let id = NodeId::mount(path);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        let mut meta = serde_json::Map::new();
        meta.insert(
            "device".to_string(),
            serde_json::Value::String(device.to_string()),
        );
        meta.insert(
            "fstype".to_string(),
            serde_json::Value::String(fstype.to_string()),
        );
        if let Some(percent) = used_percent {
            meta.insert("used_percent".to_string(), serde_json::json!(percent));
        }
        Self {
            id,
            kind: NodeKind::Mount,
            label: path.to_string(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(serde_json::Value::Object(meta).to_string()),
        }
    }

    pub fn directory(path: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::directory(path);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        Self {
            id,
            kind: NodeKind::Directory,
            label: path.to_string(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::empty(),
        }
    }

    pub fn library(path: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::library(path);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        let label = id
            .as_str()
            .strip_prefix("library:")
            .unwrap_or(path)
            .to_string();
        Self {
            id,
            kind: NodeKind::Library,
            label,
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"source":"proc_maps"}"#),
        }
    }

    pub fn package(name: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::package(name);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        Self {
            id,
            kind: NodeKind::Package,
            label: name.to_string(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"source":"dpkg"}"#),
        }
    }

    pub fn cgroup(path: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::cgroup(path);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        Self {
            id,
            kind: NodeKind::Cgroup,
            label: path.to_string(),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"source":"proc_cgroup"}"#),
        }
    }

    pub fn unix_socket(
        path: &str,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Result<Self, crate::error::ParseError> {
        let id = NodeId::unix_socket(path)?;
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        let label = id
            .as_str()
            .strip_prefix("unix:")
            .unwrap_or(path)
            .to_string();
        Ok(Self {
            id,
            kind: NodeKind::UnixSocket,
            label,
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"protocol":"unix"}"#),
        })
    }

    pub fn tcp_port(
        ip: &str,
        port: u16,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Result<Self, crate::error::ParseError> {
        let id = NodeId::port_tcp(ip, port)?;
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        let display_ip = if ip.contains(':') {
            format!("[{ip}]")
        } else {
            ip.to_string()
        };
        Ok(Self {
            id,
            kind: NodeKind::Port,
            label: format!("tcp:{display_ip}:{port}"),
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"protocol":"tcp"}"#),
        })
    }

    pub fn service(unit: &str, seen_at: TimestampNs, existing: Option<&Self>) -> Self {
        let id = NodeId::service(unit);
        let (first_seen, valid_from) = match existing {
            Some(node) => (node.first_seen, node.valid_from),
            None => (seen_at, seen_at),
        };
        let label = id
            .as_str()
            .strip_prefix("service:")
            .unwrap_or(unit)
            .to_string();
        Self {
            id,
            kind: NodeKind::Service,
            label,
            state: NodeState::Active,
            first_seen,
            last_seen: seen_at,
            valid_from,
            valid_to: None,
            metadata: GraphMetadata::from_json(r#"{"source":"systemd_cgroup_inference"}"#),
        }
    }

    pub fn id(&self) -> &NodeId {
        &self.id
    }

    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn state(&self) -> NodeState {
        self.state
    }

    pub fn first_seen(&self) -> TimestampNs {
        self.first_seen
    }

    pub fn last_seen(&self) -> TimestampNs {
        self.last_seen
    }

    pub fn valid_from(&self) -> TimestampNs {
        self.valid_from
    }

    pub fn valid_to(&self) -> Option<TimestampNs> {
        self.valid_to
    }

    pub fn metadata(&self) -> &GraphMetadata {
        &self.metadata
    }
}
