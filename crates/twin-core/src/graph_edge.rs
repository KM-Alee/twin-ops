use crate::edge::{EdgeClass, EdgeId, EdgeKind, EdgeState};
use crate::graph_metadata::GraphMetadata;
use crate::node::NodeId;
use crate::TimestampNs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdgeParts {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub class: EdgeClass,
    pub state: EdgeState,
    pub evidence_count: u32,
    pub first_seen: TimestampNs,
    pub last_seen: TimestampNs,
    pub metadata: GraphMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    id: EdgeId,
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
    class: EdgeClass,
    state: EdgeState,
    evidence_count: u32,
    first_seen: TimestampNs,
    last_seen: TimestampNs,
    metadata: GraphMetadata,
}

impl GraphEdge {
    pub fn from_stored(parts: GraphEdgeParts) -> Self {
        let id = EdgeId::new(&parts.from, parts.kind, &parts.to);
        Self {
            id,
            from: parts.from,
            to: parts.to,
            kind: parts.kind,
            class: parts.class,
            state: parts.state,
            evidence_count: parts.evidence_count,
            first_seen: parts.first_seen,
            last_seen: parts.last_seen,
            metadata: parts.metadata,
        }
    }

    const INFERENCE_METADATA: &str = r#"{"inference":"systemd_cgroup_path"}"#;
    const PROCESS_LISTENER_METADATA: &str = r#"{"source":"proc_socket_inode_join"}"#;
    const SERVICE_LISTENER_METADATA: &str = r#"{"inference":"service_owns_listening_process"}"#;
    const PROCESS_CONNECTION_METADATA: &str = r#"{"source":"proc_tcp_established_inode_join"}"#;
    const SERVICE_CONNECTION_METADATA: &str = r#"{"inference":"service_owns_connected_process"}"#;
    const SERVICE_DEPENDENCY_METADATA: &str =
        r#"{"inference":"active_connection_to_listening_service"}"#;

    pub fn observed_parent(
        parent: &NodeId,
        child: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::ParentOf;
        let id = EdgeId::new(parent, kind, child);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: parent.clone(),
            to: child.clone(),
            kind,
            class: EdgeClass::Observed,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::empty(),
        }
    }

    pub fn observed_in_cgroup(
        process: &NodeId,
        cgroup: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::InCgroup;
        let id = EdgeId::new(process, kind, cgroup);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: process.clone(),
            to: cgroup.clone(),
            kind,
            class: EdgeClass::Observed,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::empty(),
        }
    }

    pub fn inferred_service_owns_process(
        service: &NodeId,
        process: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        Self::inferred_service_owns(service, process, seen_at, existing)
    }

    pub fn inferred_service_owns_cgroup(
        service: &NodeId,
        cgroup: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        Self::inferred_service_owns(service, cgroup, seen_at, existing)
    }

    pub fn observed_process_listens_on(
        process: &NodeId,
        port: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::ListensOn;
        let id = EdgeId::new(process, kind, port);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: process.clone(),
            to: port.clone(),
            kind,
            class: EdgeClass::Observed,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::PROCESS_LISTENER_METADATA),
        }
    }

    pub fn inferred_service_listens_on(
        service: &NodeId,
        port: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::ListensOn;
        let id = EdgeId::new(service, kind, port);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: service.clone(),
            to: port.clone(),
            kind,
            class: EdgeClass::Inferred,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::SERVICE_LISTENER_METADATA),
        }
    }

    pub fn observed_process_connects_to(
        process: &NodeId,
        port: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::ConnectsTo;
        let id = EdgeId::new(process, kind, port);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: process.clone(),
            to: port.clone(),
            kind,
            class: EdgeClass::Observed,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::PROCESS_CONNECTION_METADATA),
        }
    }

    pub fn inferred_service_connects_to(
        service: &NodeId,
        port: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::ConnectsTo;
        let id = EdgeId::new(service, kind, port);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: service.clone(),
            to: port.clone(),
            kind,
            class: EdgeClass::Inferred,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::SERVICE_CONNECTION_METADATA),
        }
    }

    pub fn inferred_service_depends_on(
        source_service: &NodeId,
        target_service: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::DependsOn;
        let id = EdgeId::new(source_service, kind, target_service);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: source_service.clone(),
            to: target_service.clone(),
            kind,
            class: EdgeClass::Inferred,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::SERVICE_DEPENDENCY_METADATA),
        }
    }

    pub fn inferred_service_owns(
        service: &NodeId,
        owned: &NodeId,
        seen_at: TimestampNs,
        existing: Option<&Self>,
    ) -> Self {
        let kind = EdgeKind::Owns;
        let id = EdgeId::new(service, kind, owned);
        let (first_seen, evidence_count) = match existing {
            Some(edge) => (edge.first_seen, edge.evidence_count.saturating_add(1)),
            None => (seen_at, 1),
        };
        Self {
            id,
            from: service.clone(),
            to: owned.clone(),
            kind,
            class: EdgeClass::Inferred,
            state: EdgeState::Active,
            evidence_count,
            first_seen,
            last_seen: seen_at,
            metadata: GraphMetadata::from_json(Self::INFERENCE_METADATA),
        }
    }

    pub fn id(&self) -> &EdgeId {
        &self.id
    }

    pub fn from(&self) -> &NodeId {
        &self.from
    }

    pub fn to(&self) -> &NodeId {
        &self.to
    }

    pub fn kind(&self) -> EdgeKind {
        self.kind
    }

    pub fn class(&self) -> EdgeClass {
        self.class
    }

    pub fn state(&self) -> EdgeState {
        self.state
    }

    pub fn evidence_count(&self) -> u32 {
        self.evidence_count
    }

    pub fn first_seen(&self) -> TimestampNs {
        self.first_seen
    }

    pub fn last_seen(&self) -> TimestampNs {
        self.last_seen
    }

    pub fn metadata(&self) -> &GraphMetadata {
        &self.metadata
    }
}
