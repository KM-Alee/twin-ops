use std::net::Ipv4Addr;

use twin_ebpf::{
    decode_accept, decode_bind, decode_connect, decode_tcp_event, encode_accept, encode_bind,
    encode_connect, tcp_to_raw, TcpAccept, TcpBind, TcpConnect, TcpEvent, TCP_ACCEPT_LEN,
    TCP_BIND_LEN, TCP_CONNECT_LEN,
};
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

fn ipv4(octets: [u8; 4]) -> std::net::IpAddr {
    std::net::IpAddr::V4(Ipv4Addr::from(octets))
}

#[test]
fn short_buffer_is_rejected() {
    let err = decode_tcp_event(&[0, 1, 2, 3]).expect_err("short");
    assert!(err.to_string().contains("malformed"), "{err}");
    assert!(decode_connect(&[0u8; 8]).is_err());
    assert!(decode_accept(&[0u8; 8]).is_err());
    assert!(decode_bind(&[0u8; 8]).is_err());
}

#[test]
fn malformed_family_is_rejected() {
    let mut bytes = encode_connect(&TcpConnect::new(
        1,
        2,
        ipv4([10, 0, 0, 1]),
        ipv4([10, 0, 0, 8]),
        5432,
        3,
    ));
    bytes[22] = 9;
    let err = decode_connect(&bytes).expect_err("family");
    assert!(err.to_string().contains("malformed"), "{err}");
}

#[test]
fn valid_connect_accept_and_bind_round_trip() {
    let connect = TcpConnect::new(4421, 77, ipv4([10, 0, 0, 1]), ipv4([10, 0, 0, 8]), 5432, 99);
    let connect_bytes = encode_connect(&connect);
    assert_eq!(connect_bytes.len(), TCP_CONNECT_LEN);
    let decoded = decode_connect(&connect_bytes).expect("connect");
    assert_eq!(decoded, connect);
    assert!(matches!(
        decode_tcp_event(&connect_bytes).expect("event"),
        TcpEvent::Connect(_)
    ));

    let accept = TcpAccept::new(
        9,
        3,
        ipv4([127, 0, 0, 1]),
        5432,
        ipv4([127, 0, 0, 1]),
        40000,
        5,
    );
    let accept_bytes = encode_accept(&accept);
    assert_eq!(accept_bytes.len(), TCP_ACCEPT_LEN);
    assert_eq!(decode_accept(&accept_bytes).expect("accept"), accept);

    let bind = TcpBind::new(9, 3, ipv4([0, 0, 0, 0]), 5432, 5);
    let bind_bytes = encode_bind(&bind);
    assert_eq!(bind_bytes.len(), TCP_BIND_LEN);
    assert_eq!(decode_bind(&bind_bytes).expect("bind"), bind);
}

#[test]
fn connect_observation_is_endpoint_only() {
    let event = TcpEvent::Connect(TcpConnect::new(
        4421,
        77,
        ipv4([10, 0, 0, 1]),
        ipv4([10, 0, 0, 8]),
        5432,
        99,
    ));
    let raw = tcp_to_raw(&event);
    assert_eq!(raw.source, ObservationSource::Ebpf);
    assert_eq!(raw.kind, ObservationKind::EbpfConnect);
    let observation = Pipeline::default().process(raw).expect("pipeline");
    assert_eq!(
        observation.subject().map(|id| id.to_string()).as_deref(),
        Some("process:pid:4421")
    );
    assert_eq!(
        observation.object().map(|id| id.to_string()).as_deref(),
        Some("port:tcp:10.0.0.8:5432")
    );
    assert_eq!(
        observation
            .metadata()
            .get("cgroup_id")
            .and_then(|value| value.as_str()),
        Some("77")
    );
    let metadata = observation.metadata().to_json();
    assert!(!metadata.contains("argv"));
    assert!(!metadata.contains("password"));
    assert!(!metadata.contains("10.0.0.1"));
}
