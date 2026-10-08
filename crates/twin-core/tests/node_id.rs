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
        NodeId::library("/usr/lib/../lib/libssl.so.3").as_str(),
        "library:/usr/lib/libssl.so.3"
    );
    assert_eq!(
        NodeId::library("/usr/lib/libssl.so.3").kind(),
        Some(NodeKind::Library)
    );
    assert_eq!(
        NodeId::from_str("library:/usr/lib/libssl.so.3")
            .expect("library")
            .as_str(),
        "library:/usr/lib/libssl.so.3"
    );
    assert_eq!(NodeId::package("openssl").as_str(), "package:openssl");
    assert_eq!(NodeId::package("openssl").kind(), Some(NodeKind::Package));
    assert!(NodeId::from_str("library:libssl.so.3").is_err());
    assert!(NodeId::from_str("package:").is_err());
    assert!(NodeId::from_str("package:openssl/libssl").is_err());
    assert_eq!(NodeId::container("redis").as_str(), "container:redis");
    assert_eq!(NodeId::container("redis").kind(), Some(NodeKind::Container));
    assert_eq!(
        NodeId::from_str("container:redis")
            .expect("container")
            .as_str(),
        "container:redis"
    );
    assert!(NodeId::from_str("container:").is_err());
    assert!(NodeId::from_str("container:docker:redis").is_err());
    assert_eq!(NodeId::image("redis:7").as_str(), "image:redis:7");
    assert_eq!(NodeId::image("redis:7").kind(), Some(NodeKind::Image));
    assert_eq!(
        NodeId::from_str("image:redis:7").expect("image").as_str(),
        "image:redis:7"
    );
    assert!(NodeId::from_str("image:").is_err());
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
fn k8s_ids_and_short_forms() {
    let deployment = NodeId::k8s_deployment("default", "api");
    assert_eq!(deployment.as_str(), "k8s:deployment:default/api");
    assert_eq!(deployment.kind(), Some(NodeKind::K8sDeployment));
    assert_eq!(deployment.k8s_short(), Some("deployment:default/api"));
    assert_eq!(
        deployment.k8s_namespace_and_name(),
        Some(("default", "api"))
    );
    assert_eq!(
        NodeId::parse_k8s_ref("deployment/default/api").expect("slash"),
        deployment
    );
    assert_eq!(
        NodeId::parse_k8s_ref("deployment:default/api").expect("colon"),
        deployment
    );
    assert_eq!(
        NodeId::parse_k8s_ref("k8s:deployment:default/api").expect("full"),
        deployment
    );
    assert_eq!(
        NodeId::parse_k8s_ref("service/default/api")
            .expect("service")
            .as_str(),
        "k8s:service:default/api"
    );
    assert_eq!(
        NodeId::parse_k8s_ref("pod:default/api-123")
            .expect("pod")
            .as_str(),
        "k8s:pod:default/api-123"
    );
    assert_eq!(
        NodeId::k8s_replicaset("default", "api-abc").kind(),
        Some(NodeKind::K8sReplicaSet)
    );
    assert_eq!(
        NodeId::k8s_secret_ref("default", "api-tls").as_str(),
        "k8s:secretref:default/api-tls"
    );
    assert_eq!(
        NodeId::k8s_configmap("default", "api-config").kind(),
        Some(NodeKind::K8sConfigMap)
    );
    assert_eq!(
        NodeId::k8s_pvc("default", "api-data").kind(),
        Some(NodeKind::K8sPvc)
    );
    assert_eq!(
        NodeId::k8s_ingress("default", "api").kind(),
        Some(NodeKind::K8sIngress)
    );
    assert_eq!(
        NodeId::k8s_namespace("default").kind(),
        Some(NodeKind::K8sNamespace)
    );
    assert!(NodeId::from_str("k8s:pod:default").is_err());
    assert!(NodeId::from_str("k8s:pod:Default/api").is_err());
    assert!(NodeId::parse_k8s_ref("container:redis").is_err());
    assert!(NodeId::parse_k8s_ref("/etc/nginx/nginx.conf").is_err());
}

#[test]
fn from_str_rejects_non_canonical() {
    let err = NodeId::from_str("not-a-node").expect_err("bad");
    assert!(matches!(err, ParseError::InvalidNodeId { .. }));
    let err = NodeId::from_str("port:tcp:::1:80").expect_err("legacy ambiguous");
    assert!(matches!(err, ParseError::InvalidNodeId { .. }));
}
