pub mod change_kind;
pub mod config;
pub mod edge;
pub mod error;
pub mod evidence;
pub mod graph_edge;
pub mod graph_metadata;
pub mod graph_node;
pub mod id;
pub mod impact_kind;
pub mod node;
pub mod risk;
pub mod snapshot_id;
pub mod unknown_kind;

pub use change_kind::ChangeKind;
pub use edge::{EdgeClass, EdgeId, EdgeKind, EdgeState};
pub use error::{ConfigError, ParseError};
pub use evidence::{
    factors_from_lines, score_capped_evidence, score_evidence, EvidenceAdjustments,
    EvidenceExplanation, EvidenceFactors, EvidenceLineRef, EvidenceRecency, EvidenceSourceKind,
};
pub use graph_edge::{GraphEdge, GraphEdgeParts, PortPublish};
pub use graph_metadata::GraphMetadata;
pub use graph_node::{GraphNode, GraphNodeParts};
pub use id::{CollectorName, ObservationId, TimestampNs};
pub use impact_kind::DependentImpactKind;
pub use node::{lexical_canonical, NodeId, NodeKind, NodeState};
pub use risk::{
    cap_dependent_evidence_score, EvidenceLabel, EvidenceStrength, RiskLevel,
    EVIDENCE_CAP_INFERRED_ONLY, EVIDENCE_CAP_NO_OBSERVATIONS, EVIDENCE_CAP_WEAKENING_UNKNOWN,
};
pub use snapshot_id::{parse_graph_ref, validate_snapshot_name, GraphRef, SnapshotId};
pub use unknown_kind::UnknownKind;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
