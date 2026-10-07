use std::str::FromStr;

use twin_core::{NodeId, NodeKind, ParseError};

#[test]
fn process_pid_roundtrip() {
    let id = NodeId::process(8841);
    assert_eq!(id.as_str(), "process:pid:8841");
    assert_eq!(id.process_pid(), Some(8841));
    assert_eq!(NodeId::host("x").process_pid(), None);
}

#[test]
fn file_lexical_canonical() {
    let id = NodeId::file("/etc/nginx/../nginx/nginx.conf");
    assert_eq!(id.as_str(), "file:/etc/nginx/nginx.conf");
}

#[test]
fn port_ip_normalized() {
    let id = NodeId::port_tcp("127.000.000.001", 5432).expect("ip");
    assert_eq!(id.as_str(), "port:tcp:127.0.0.1:5432");
}

#[test]
fn port_bad_ip() {
    let err = NodeId::port_tcp("not-an-ip", 80).expect_err("bad ip");
    assert!(matches!(err, ParseError::IpAddr { .. }));
}

#[test]
fn service_strips_slice() {
    let id = NodeId::service("system.slice/nginx.service");
    assert_eq!(id.as_str(), "service:nginx.service");
}

#[test]
fn kind_roundtrip() {
    assert_eq!(NodeId::process(1).kind(), Some(NodeKind::Process));
    assert_eq!(NodeId::mount("/var").kind(), Some(NodeKind::Mount));
    assert_eq!(
        NodeId::directory("/var/log").kind(),
        Some(NodeKind::Directory)
    );
    assert_eq!(
        NodeId::from_str("mount:/var").expect("mount").as_str(),
        "mount:/var"
    );
    assert_eq!(
        NodeId::from_str("directory:/var/log")
            .expect("directory")
            .as_str(),
        "directory:/var/log"
    );
}

#[test]
fn port_ipv6_bracketed() {
    let id = NodeId::port_tcp("::1", 80).expect("ip");
    assert_eq!(id.as_str(), "port:tcp:[::1]:80");
    assert_eq!(NodeId::from_str(id.as_str()).expect("parse"), id);
}

#[test]
fn unix_socket_roundtrip() {
    let id = NodeId::unix_socket("/run/dbus/system_bus_socket").expect("unix");
    assert_eq!(id.as_str(), "unix:/run/dbus/system_bus_socket");
    assert_eq!(id.kind(), Some(NodeKind::UnixSocket));
    assert_eq!(NodeId::from_str(id.as_str()).expect("parse"), id);
}

#[test]
fn unix_socket_abstract() {
    let id = NodeId::unix_socket("@dbus").expect("unix");
    assert_eq!(id.as_str(), "unix:@dbus");
}

#[test]
fn from_str_rejects_non_canonical() {
    let err = NodeId::from_str("not-a-node").expect_err("bad");
    assert!(matches!(err, ParseError::InvalidNodeId { .. }));
    let err = NodeId::from_str("port:tcp:::1:80").expect_err("legacy ambiguous");
    assert!(matches!(err, ParseError::InvalidNodeId { .. }));
}
