use twin_core::NodeId;

use crate::error::ObservationError;
use crate::raw::RawIdentity;

pub struct Normalizer;

impl Normalizer {
    pub fn normalize(&self, identity: &RawIdentity) -> Result<NodeId, ObservationError> {
        Ok(match identity {
            RawIdentity::Host { hostname } => NodeId::host(hostname),
            RawIdentity::Process { pid } => NodeId::process(*pid),
            RawIdentity::Service { unit } => NodeId::service(unit),
            RawIdentity::TcpEndpoint { ip, port } => NodeId::port_tcp(ip, *port)?,
            RawIdentity::UdpEndpoint { ip, port } => {
                NodeId::inet_port(twin_core::TransportProtocol::Udp, ip, *port)?
            }
            RawIdentity::NamespacedEndpoint {
                namespace,
                protocol,
                ip,
                port,
            } => NodeId::inet_port_in_namespace(*namespace, *protocol, ip, *port)?,
            RawIdentity::NamespacedUnixSocket { namespace, path } => {
                NodeId::unix_socket_in_namespace(*namespace, path)?
            }
            RawIdentity::UnixSocket { path } => NodeId::unix_socket(path)?,
            RawIdentity::File { path } => NodeId::file(path),
            RawIdentity::Cgroup { path } => NodeId::cgroup(path),
        })
    }
}
