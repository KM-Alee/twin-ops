mod support;

use tempfile::TempDir;
use twin_collectors::parse_unix_table;
use twin_collectors::{ProcessCollector, UNIX_FLAG_LISTEN};
use twin_core::TimestampNs;
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

use support::{write_proc_fixture, write_socket_fd};

#[test]
fn collector_maps_unix_listener_inode_to_process() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture(
        &proc,
        99,
        "99 (dbus-broker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/dbus-broker\0",
        None,
    );
    let table = "\
Num       RefCount Protocol Flags    Type St Inode Path
 1: 00000001 00000000 00010000 0001 01  9001 /run/dbus/system_bus_socket
";
    std::fs::create_dir_all(proc.join("net")).expect("net dir");
    std::fs::write(proc.join("net/unix"), table).expect("unix table");
    write_socket_fd(&proc, 99, 3, 9001);

    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.unix_listeners().len(), 1);
    let owners = batch.owners_by_inode().get(&9001).expect("owners");
    assert_eq!(owners[0].pid, 99);

    let pipeline = Pipeline::default();
    let obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::UnixSocketSeen)
        .expect("unix obs");
    assert_eq!(obs.source, ObservationSource::ProcNetUnix);
    let normalized = pipeline.process(obs.clone()).expect("pipeline");
    assert_eq!(
        normalized.subject().map(|s| s.as_str()),
        Some("unix:/run/dbus/system_bus_socket")
    );
}

#[test]
fn parse_listener_with_flags() {
    let table = "\
Num       RefCount Protocol Flags    Type St Inode Path
 1: 00000001 00000000 00010000 0001 01  1948 /run/dbus/system_bus_socket
";
    let parsed = parse_unix_table(table);
    assert_eq!(parsed.listeners.len(), 1);
    assert_eq!(parsed.listeners[0].path, "/run/dbus/system_bus_socket");
    assert!(parsed.listeners[0].is_listener());
    assert_eq!(parsed.listeners[0].flags, UNIX_FLAG_LISTEN);
    assert_eq!(parsed.listeners[0].inode, 1948);
}

#[test]
fn parse_connected_stream() {
    let table = "\
Num       RefCount Protocol Flags    Type St Inode Path
 2: 00000001 00000000 00000000 0001 03  2000 /run/dbus/system_bus_socket
";
    let parsed = parse_unix_table(table);
    assert_eq!(parsed.listeners.len(), 0);
    assert_eq!(parsed.connections.len(), 1);
    assert_eq!(parsed.connections[0].path, "/run/dbus/system_bus_socket");
}

#[test]
fn parse_abstract_listener() {
    let table = "\
Num       RefCount Protocol Flags    Type St Inode Path
 3: 00000001 00000000 00010000 0001 01  42 @mypipe
";
    let parsed = parse_unix_table(table);
    assert_eq!(parsed.listeners.len(), 1);
    assert_eq!(parsed.listeners[0].path, "@mypipe");
}
