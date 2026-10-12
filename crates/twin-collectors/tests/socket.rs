mod support;

use tempfile::TempDir;
use twin_collectors::{parse_inet_table, parse_socket_fd_target, InetTableKind};
use twin_collectors::{ProcessCollector, ProcessWarningKind};
use twin_core::TimestampNs;
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

use support::{write_proc_fixture, write_socket_fd, write_tcp_table};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

#[test]
fn parses_ipv4_listen_from_fixture_table() {
    let content = format!(
        "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
    );
    let parsed = parse_inet_table(InetTableKind::Tcp, &content);
    assert!(parsed.warnings.is_empty());
    assert_eq!(parsed.listeners.len(), 1);
    assert_eq!(parsed.listeners[0].local_ip, "127.0.0.1");
    assert_eq!(parsed.listeners[0].local_port, 5432);
}

#[test]
fn collector_maps_listener_inode_to_process_fd() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);

    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.inet_listeners().len(), 1);
    let owners = batch.owners_by_inode().get(&12345).expect("owners");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].pid, 721);
    assert_eq!(owners[0].fd, 8);

    let pipeline = Pipeline::default();
    let tcp_obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::TcpSocketSeen)
        .expect("tcp obs");
    assert_eq!(tcp_obs.source, ObservationSource::ProcNetTcp);
    let normalized = pipeline.process(tcp_obs.clone()).expect("pipeline");
    assert_eq!(
        normalized.subject().map(|s| s.as_str()),
        Some("port:tcp:127.0.0.1:5432")
    );
}

#[test]
fn unmapped_listener_emits_socket_unmapped_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 99999 1 0000000000000000 100 0 0 10 0"
        ),
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch
        .warnings()
        .iter()
        .any(|w| w.kind() == ProcessWarningKind::SocketUnmapped));
}

#[test]
fn parse_socket_fd_target_rejects_non_socket() {
    assert_eq!(parse_socket_fd_target("socket:[42]"), Some(42));
    assert_eq!(parse_socket_fd_target("pipe:[1]"), None);
}

#[test]
fn collector_maps_established_inode_to_process_fd() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 8841, 12, 456);

    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.inet_connections().len(), 1);
    let owners = batch.owners_by_inode().get(&456).expect("owners");
    assert_eq!(owners[0].pid, 8841);

    let tcp_obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::TcpConnectionSeen)
        .expect("tcp connection obs");
    assert_eq!(tcp_obs.source, ObservationSource::ProcNetTcp);
    let normalized = Pipeline::default()
        .process(tcp_obs.clone())
        .expect("pipeline");
    assert_eq!(
        normalized.subject().map(|s| s.as_str()),
        Some("port:tcp:127.0.0.1:5432")
    );
}

#[test]
fn collector_maps_established_connection_inode_to_process_fd() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 8841, 12, 456);

    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.inet_connections().len(), 1);
    let conn = &batch.inet_connections()[0];
    assert_eq!(conn.remote_ip, "127.0.0.1");
    assert_eq!(conn.remote_port, 5432);
    let owners = batch.owners_by_inode().get(&456).expect("owners");
    assert_eq!(owners[0].pid, 8841);

    let pipeline = Pipeline::default();
    let tcp_obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::TcpConnectionSeen)
        .expect("tcp connection obs");
    assert_eq!(tcp_obs.source, ObservationSource::ProcNetTcp);
    let normalized = pipeline.process(tcp_obs.clone()).expect("pipeline");
    assert_eq!(
        normalized.subject().map(|s| s.as_str()),
        Some("port:tcp:127.0.0.1:5432")
    );
}

#[test]
fn malformed_non_ascii_addresses_are_warnings_without_panics() {
    for (table, address) in [
        (InetTableKind::Tcp, "000é000"),
        (InetTableKind::Tcp6, "000é0000000000000000000000000000"),
    ] {
        let text = format!("{TCP_HEADER}\n0: {address}:1538 00000000:0000 0A 0 0 0 0 0 12345");
        let parsed = parse_inet_table(table, &text);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.listeners.is_empty());
    }
}

#[test]
fn fdinfo_verifies_inode_and_detects_descriptor_reuse() {
    let temp = TempDir::new().expect("temp");
    let proc = temp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(&proc, 721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n", b"postgres\0", None);
    write_tcp_table(
        &proc,
        "tcp",
        &format!("{TCP_HEADER}\n0: 0100007F:1538 00000000:0000 0A 0 0 0 0 0 12345"),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    std::fs::create_dir_all(proc.join("721/fdinfo")).expect("fdinfo");
    std::fs::write(
        proc.join("721/fdinfo/8"),
        "pos:\t0\nflags:\t02\nino:\t12345\n",
    )
    .expect("info");
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.owners_by_inode()[&12345][0].is_fdinfo_verified);
    let observation = batch
        .observations()
        .iter()
        .find(|obs| obs.kind == ObservationKind::ProcessSocketFdSeen)
        .expect("fd observation");
    assert_eq!(
        observation
            .metadata
            .get("inode")
            .and_then(|value| value.as_str()),
        Some("12345")
    );
    std::fs::write(proc.join("721/fdinfo/8"), "ino:\t99999\n").expect("reuse");
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(2))
        .expect("collect");
    assert!(!batch.owners_by_inode().contains_key(&12345));
    assert!(batch
        .warnings()
        .iter()
        .any(|warning| warning.kind() == ProcessWarningKind::FdMalformed));
}

#[test]
fn malformed_and_vanished_fdinfo_do_not_claim_socket_ownership() {
    let temp = TempDir::new().expect("temp");
    let proc = temp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(&proc, 721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid: 999 999 999 999\n", b"postgres\0", None);
    write_tcp_table(
        &proc,
        "tcp",
        &format!("{TCP_HEADER}\n0: 0100007F:1538 00000000:0000 0A 0 0 0 0 0 12345"),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    std::fs::create_dir_all(proc.join("721/fdinfo")).expect("fdinfo");
    std::fs::write(proc.join("721/fdinfo/8"), "ino: malformed\n").expect("info");
    for kind in [
        ProcessWarningKind::FdMalformed,
        ProcessWarningKind::FdVanished,
    ] {
        if kind == ProcessWarningKind::FdVanished {
            std::fs::remove_file(proc.join("721/fdinfo/8")).expect("vanish");
        }
        let batch = ProcessCollector::new(&proc)
            .collect(TimestampNs::new(1))
            .expect("collect");
        assert!(!batch.owners_by_inode().contains_key(&12345));
        assert!(batch
            .warnings()
            .iter()
            .any(|warning| warning.kind() == kind));
        assert!(!batch
            .observations()
            .iter()
            .any(|obs| obs.kind == ObservationKind::ProcessSocketFdSeen));
    }
}

#[test]
fn udp_bound_and_connected_sockets_keep_protocol_and_ipv6_identity() {
    for (table, local, remote, ip) in [
        (InetTableKind::Udp, "0100007F", "0100007F", "127.0.0.1"),
        (
            InetTableKind::Udp6,
            "00000000000000000000000001000000",
            "00000000000000000000000001000000",
            "::1",
        ),
    ] {
        let text = format!("{TCP_HEADER}\n0: {local}:0035 {remote}:0000 07 0 0 0 0 0 10\n1: {local}:C3CA {remote}:0035 01 0 0 0 0 0 11\n2: bad:0035 {remote}:0035 07 0 0 0 0 0 12");
        let parsed = parse_inet_table(table, &text);
        assert_eq!(parsed.listeners.len(), 1);
        assert_eq!(parsed.connections.len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.listeners[0].local_ip, ip);
        assert_eq!(parsed.connections[0].remote_port, 53);
    }
    let temp = TempDir::new().expect("temp");
    let proc = temp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(&proc, 721,
        "721 (resolver) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid: 999 999 999 999\n", b"resolver\0", None);
    write_tcp_table(
        &proc,
        "udp",
        &format!("{TCP_HEADER}\n0: 0100007F:0035 00000000:0000 07 0 0 0 0 0 12345"),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let obs = batch
        .observations()
        .iter()
        .find(|obs| obs.kind == ObservationKind::UdpSocketSeen)
        .expect("UDP observation");
    assert_eq!(obs.source, ObservationSource::ProcNetUdp);
    assert_eq!(
        Pipeline::default()
            .process(obs.clone())
            .expect("normalize")
            .subject()
            .expect("subject")
            .as_str(),
        "port:udp:127.0.0.1:53"
    );
    assert!(batch.owners_by_inode().contains_key(&12345));
}
