pub mod config;
pub mod edge;
pub mod error;
pub mod graph_edge;
pub mod graph_metadata;
pub mod graph_node;
pub mod id;
pub mod node;

pub use edge::{EdgeClass, EdgeId, EdgeKind, EdgeState};
pub use error::{ConfigError, ParseError};
pub use graph_edge::{GraphEdge, GraphEdgeParts};
pub use graph_metadata::GraphMetadata;
pub use graph_node::{GraphNode, GraphNodeParts};
pub use id::{CollectorName, ObservationId, TimestampNs};
pub use node::{lexical_canonical, NodeId, NodeKind, NodeState};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
