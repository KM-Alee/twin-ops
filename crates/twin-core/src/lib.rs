pub mod config;
pub mod edge;
pub mod error;
pub mod id;
pub mod node;

pub use edge::{EdgeClass, EdgeId, EdgeKind, EdgeState};
pub use error::{ConfigError, ParseError};
pub use id::{CollectorName, ObservationId, TimestampNs};
pub use node::{NodeId, NodeKind, NodeState};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
