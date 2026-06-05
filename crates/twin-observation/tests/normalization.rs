use twin_observation::{Normalizer, ObservationError, RawIdentity};

#[test]
fn each_identity() {
    let n = Normalizer;
    assert_eq!(
        n.normalize(&RawIdentity::Host {
            hostname: "myhost".to_string()
        })
        .expect("host")
        .as_str(),
        "host:myhost"
    );
    assert_eq!(
        n.normalize(&RawIdentity::Process { pid: 42 })
            .expect("process")
            .as_str(),
        "process:pid:42"
    );
    assert_eq!(
        n.normalize(&RawIdentity::Service {
            unit: "system.slice/nginx.service".to_string()
        })
        .expect("service")
        .as_str(),
        "service:nginx.service"
    );
    assert_eq!(
        n.normalize(&RawIdentity::TcpEndpoint {
            ip: "127.0.0.1".to_string(),
            port: 80
        })
        .expect("port")
        .as_str(),
        "port:tcp:127.0.0.1:80"
    );
    assert_eq!(
        n.normalize(&RawIdentity::File {
            path: "/etc/nginx/../nginx/nginx.conf".to_string()
        })
        .expect("file")
        .as_str(),
        "file:/etc/nginx/nginx.conf"
    );
    assert_eq!(
        n.normalize(&RawIdentity::Cgroup {
            path: "/sys/fs/cgroup/system.slice".to_string()
        })
        .expect("cgroup")
        .as_str(),
        "cgroup:/sys/fs/cgroup/system.slice"
    );
    assert_eq!(
        n.normalize(&RawIdentity::UnixSocket {
            path: "/run/dbus/system_bus_socket".to_string()
        })
        .expect("unix")
        .as_str(),
        "unix:/run/dbus/system_bus_socket"
    );
}

#[test]
fn bad_ip_errors() {
    let err = Normalizer
        .normalize(&RawIdentity::TcpEndpoint {
            ip: "not-ip".to_string(),
            port: 1,
        })
        .expect_err("bad ip");
    assert!(matches!(err, ObservationError::Normalize(_)));
}
