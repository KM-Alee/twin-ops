use std::str::FromStr;
use twin_core::{NetworkNamespaceId, NodeId, TransportProtocol};

#[test]
fn namespace_endpoint_ids_roundtrip_and_preserve_transport_and_address() {
    let namespace = NetworkNamespaceId::new(42).unwrap();
    for (protocol, address) in [
        (TransportProtocol::Tcp, "127.0.0.1"),
        (TransportProtocol::Udp, "::1"),
    ] {
        let id = NodeId::inet_port_in_namespace(namespace, protocol, address, 53).unwrap();
        assert_eq!(NodeId::from_str(id.as_str()).unwrap(), id);
        assert_eq!(id.network_namespace(), Some(namespace));
        assert_eq!(
            id.unscoped_endpoint().unwrap(),
            NodeId::inet_port(protocol, address, 53).unwrap()
        );
        assert_ne!(
            id,
            NodeId::inet_port_in_namespace(
                NetworkNamespaceId::new(43).unwrap(),
                protocol,
                address,
                53
            )
            .unwrap()
        );
    }
    let id = NodeId::unix_socket_in_namespace(namespace, "@api").unwrap();
    assert_eq!(NodeId::from_str(id.as_str()).unwrap(), id);
    assert!(NodeId::from_str("port:tcp:127.0.0.1:53:netns:42:netns:43").is_err());
    assert!(NodeId::from_str("port:tcp:127.0.0.1:53:netns:0").is_err());
    assert!(NetworkNamespaceId::from_str("0042").is_err());
}
