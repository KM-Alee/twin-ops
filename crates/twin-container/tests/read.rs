use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;

use twin_container::{ContainerError, DockerBackend, DockerReadOnly};

fn write_redis_fixture(root: &Path) {
    let containers = root.join("containers");
    let images = root.join("images");
    std::fs::create_dir_all(&containers).expect("containers");
    std::fs::create_dir_all(&images).expect("images");
    std::fs::write(
        containers.join("list.json"),
        r#"[{
            "Id": "abc123redis",
            "Names": ["/redis"],
            "Image": "redis:7",
            "Ports": [{"IP": "0.0.0.0", "PrivatePort": 6379, "PublicPort": 6379, "Type": "tcp"}],
            "Mounts": [{"Type": "volume", "Name": "redis-data", "Source": "/var/lib/docker/volumes/redis-data/_data", "Destination": "/data"}]
        }]"#,
    )
    .expect("list");
    std::fs::write(
        containers.join("abc123redis.json"),
        r#"{
            "Id": "abc123redis",
            "Name": "/redis",
            "Config": {"Image": "redis:7"},
            "State": {"Status": "running", "Pid": 91},
            "NetworkSettings": {"Ports": {"6379/tcp": [{"HostIp": "0.0.0.0", "HostPort": "6379"}]}},
            "Mounts": [{"Type": "volume", "Name": "redis-data", "Source": "/var/lib/docker/volumes/redis-data/_data", "Destination": "/data"}]
        }"#,
    )
    .expect("inspect");
    std::fs::write(
        images.join("redis:7.json"),
        r#"{"Id": "sha256:deadbeef", "RepoTags": ["redis:7"]}"#,
    )
    .expect("image");
}

#[test]
fn fixture_lists_ports_mounts_and_image() {
    let dir = tempfile::tempdir().expect("temp");
    write_redis_fixture(dir.path());
    let docker = DockerBackend::open(Some(dir.path()), Path::new("/no/such/docker.sock"))
        .expect("open fixture");
    let listed = docker.list_containers().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "redis");
    assert_eq!(listed[0].image, "redis:7");
    let inspect = docker.inspect_container("abc123redis").expect("inspect");
    assert_eq!(inspect.pid, Some(91));
    let ports = docker.port_mappings("abc123redis").expect("ports");
    assert_eq!(ports[0].container_port, 6379);
    assert_eq!(ports[0].host_ip, "0.0.0.0");
    assert_eq!(ports[0].host_port, 6379);
    let mounts = docker.mounts("abc123redis").expect("mounts");
    assert_eq!(mounts[0].destination, "/data");
    let image = docker.inspect_image("redis:7").expect("image");
    assert_eq!(image.reference, "redis:7");
}

#[test]
fn missing_fixture_and_missing_socket_are_errors() {
    let missing = DockerBackend::open(Some(Path::new("/no/such/fixture")), Path::new("/no/sock"));
    assert!(matches!(missing, Err(ContainerError::Missing { .. })));
    let socket = DockerBackend::open(None, Path::new("/no/such/docker.sock"));
    assert!(matches!(socket, Err(ContainerError::Unavailable { .. })));
}

#[test]
fn live_get_reads_a_unix_socket_response() {
    let dir = tempfile::tempdir().expect("temp");
    let socket = dir.path().join("docker.sock");
    let listener = UnixListener::bind(&socket).expect("bind");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = r#"[{"Id":"abc123redis","Names":["/redis"],"Image":"redis:7"}]"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write");
    });
    let docker = DockerBackend::open(None, &socket).expect("open live");
    let listed = docker.list_containers().expect("list");
    assert_eq!(listed[0].name, "redis");
    server.join().expect("server");
}
