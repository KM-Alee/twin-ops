mod support;

use tempfile::TempDir;
use twin_collectors::{parse_socket_fd_target, parse_tcp_table, TcpTableKind};
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
    let parsed = parse_tcp_table(TcpTableKind::Tcp, &content);
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
    assert_eq!(batch.tcp_listeners().len(), 1);
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
    assert_eq!(batch.tcp_connections().len(), 1);
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
    assert_eq!(batch.tcp_connections().len(), 1);
    let conn = &batch.tcp_connections()[0];
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
