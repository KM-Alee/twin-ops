//! The privileged daemon's host-facing pieces: who may connect, and the unit an
//! administrator installs. The socket transport itself lives with the MCP handler.

mod access;
mod peer;
mod unit;

pub use access::AccessPolicy;
pub use peer::{peer_credentials, PeerCredentials};
pub use unit::systemd_unit;

pub const DEFAULT_SOCKET: &str = "/run/twin/twin.sock";
pub const DEFAULT_STATE_DIR: &str = "/var/lib/twin";
pub const DEFAULT_INTERVAL: &str = "15s";
