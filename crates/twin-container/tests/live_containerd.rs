use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use http::Response;
use prost::bytes::Bytes;
use twin_container::containerd::{ContainerdReadOnly, LiveContainerd, TaskState};

#[derive(Clone, Copy)]
enum Reply {
    Inventory,
    Denied,
    Stall,
}
#[derive(Debug)]
struct Captured {
    method: String,
    path: String,
    namespace: Option<String>,
    content_type: String,
    body: Vec<u8>,
}
struct FakeGrpc {
    directory: tempfile::TempDir,
    requests: Arc<Mutex<Vec<Captured>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl FakeGrpc {
    fn bind(reply: Reply) -> Option<Self> {
        let directory = tempfile::tempdir().expect("directory");
        let listener = match std::os::unix::net::UnixListener::bind(
            directory.path().join("containerd.sock"),
        ) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("capability-gated: containerd fake gRPC test cannot bind a Unix socket: PermissionDenied");
                return None;
            }
            Err(error) => panic!("fake gRPC socket: {error}"),
        };
        listener.set_nonblocking(true).expect("nonblocking");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_requests = requests.clone();
        let thread_stopped = stopped.clone();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener).expect("listener");
                while !thread_stopped.load(Ordering::Acquire) {
                    let Ok(Ok((stream, _))) =
                        tokio::time::timeout(Duration::from_millis(25), listener.accept()).await
                    else {
                        continue;
                    };
                    let requests = thread_requests.clone();
                    tokio::spawn(async move {
                        let mut connection = h2::server::handshake(stream)
                            .await
                            .expect("HTTP/2 handshake");
                        while let Some(request) = connection.accept().await {
                            let (request, mut response) = request.expect("HTTP/2 request");
                            let requests = requests.clone();
                            tokio::spawn(async move {
                                let captured = Captured {
                                    method: request.method().to_string(),
                                    path: request.uri().path().to_string(),
                                    namespace: request
                                        .headers()
                                        .get("containerd-namespace")
                                        .map(|value| value.to_str().expect("namespace").to_owned()),
                                    content_type: request
                                        .headers()
                                        .get("content-type")
                                        .expect("content type")
                                        .to_str()
                                        .expect("content type")
                                        .to_owned(),
                                    body: Vec::new(),
                                };
                                let path = captured.path.clone();
                                let mut body = Vec::new();
                                let mut stream = request.into_body();
                                while let Some(chunk) = stream.data().await {
                                    let chunk = chunk.expect("body");
                                    stream
                                        .flow_control()
                                        .release_capacity(chunk.len())
                                        .expect("capacity");
                                    body.extend_from_slice(&chunk);
                                }
                                requests
                                    .lock()
                                    .expect("requests")
                                    .push(Captured { body, ..captured });
                                if matches!(reply, Reply::Stall) {
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                }
                                let payload = if matches!(reply, Reply::Inventory) {
                                    inventory(&path)
                                } else {
                                    Vec::new()
                                };
                                let mut frame = vec![0];
                                frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                                frame.extend_from_slice(&payload);
                                let mut trailers = http::HeaderMap::new();
                                trailers.insert(
                                    "grpc-status",
                                    if matches!(reply, Reply::Denied) {
                                        "7"
                                    } else {
                                        "0"
                                    }
                                    .parse()
                                    .expect("status"),
                                );
                                if matches!(reply, Reply::Denied) {
                                    trailers.insert(
                                        "grpc-message",
                                        "PRIVATE_DAEMON_TOKEN".parse().expect("message"),
                                    );
                                }
                                let response_headers = Response::builder()
                                    .header("content-type", "application/grpc")
                                    .body(())
                                    .expect("response");
                                if let Ok(mut send) =
                                    response.send_response(response_headers, false)
                                {
                                    let _ = send.send_data(Bytes::from(frame), false);
                                    let _ = send.send_trailers(trailers);
                                }
                            });
                        }
                    });
                }
            });
        });
        Some(Self {
            directory,
            requests,
            stopped,
            thread: Some(thread),
        })
    }
    fn reader(&self) -> LiveContainerd {
        LiveContainerd::new(&self.directory.path().join("containerd.sock"))
    }
}
impl Drop for FakeGrpc {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("server thread");
        }
    }
}
fn field(tag: u8, value: &[u8]) -> Vec<u8> {
    assert!(value.len() < 128, "small synthetic message");
    let mut result = vec![tag << 3 | 2, value.len() as u8];
    result.extend_from_slice(value);
    result
}
fn inventory(path: &str) -> Vec<u8> {
    let record = match path {
        "/containerd.services.namespaces.v1.Namespaces/List" => field(1, b"k8s.io"),
        "/containerd.services.containers.v1.Containers/List" => {
            let mut record = field(1, b"api");
            record.extend(field(3, b"example/api:1"));
            record.extend(field(2, b"PRIVATE_OCI_LABEL"));
            record.extend(field(5, b"PRIVATE_OCI_SPEC"));
            record.extend(field(8, &[8, 2, 16, 3]));
            record
        }
        "/containerd.services.tasks.v1.Tasks/List" => {
            let mut record = field(1, b"api");
            record.extend([24, 42, 32, 2]);
            record.extend(field(5, b"PRIVATE_TASK_IO"));
            record
        }
        "/containerd.services.images.v1.Images/List" => {
            let mut record = field(1, b"example/api:1");
            record.extend(field(3, &field(2, b"sha256:abc")));
            record
        }
        _ => panic!("unexpected RPC: {path}"),
    };
    field(1, &record)
}

#[test]
fn live_containerd_interoperates_with_http2_grpc_list_rpc_and_namespace_metadata() {
    let Some(server) = FakeGrpc::bind(Reply::Inventory) else {
        return;
    };
    let reader = server.reader();
    assert_eq!(
        ContainerdReadOnly::list_namespaces(&reader).expect("namespaces"),
        ["k8s.io"]
    );
    let containers = ContainerdReadOnly::list_containers(&reader, "k8s.io").expect("containers");
    assert_eq!(containers[0].id, "api");
    assert_eq!(containers[0].image.as_deref(), Some("example/api:1"));
    assert_eq!(containers[0].created_at_ns, Some(2_000_000_003));
    let tasks = ContainerdReadOnly::list_tasks(&reader, "k8s.io").expect("tasks");
    assert_eq!(tasks[0].pid, Some(42));
    assert_eq!(tasks[0].state, TaskState::Running);
    let images = ContainerdReadOnly::list_images(&reader, "k8s.io").expect("images");
    assert_eq!(images[0].digest.as_deref(), Some("sha256:abc"));
    assert!(!format!("{containers:?}{tasks:?}{images:?}").contains("PRIVATE_"));
    let requests = server.requests.lock().expect("requests");
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        assert_eq!(request.method, "POST");
        assert!(request.path.ends_with("/List"));
        assert_eq!(
            request.namespace.as_deref(),
            (index > 0).then_some("k8s.io")
        );
        assert_eq!(request.content_type, "application/grpc");
        assert_eq!(request.body, [0, 0, 0, 0, 0]);
    }
    assert_eq!(
        requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/containerd.services.namespaces.v1.Namespaces/List",
            "/containerd.services.containers.v1.Containers/List",
            "/containerd.services.tasks.v1.Tasks/List",
            "/containerd.services.images.v1.Images/List",
        ]
    );
}
#[test]
fn live_containerd_denied_rpc_discards_daemon_message() {
    let Some(server) = FakeGrpc::bind(Reply::Denied) else {
        return;
    };
    let error =
        ContainerdReadOnly::list_containers(&server.reader(), "k8s.io").expect_err("denied");
    assert!(error.to_string().contains("permission denied"));
    assert!(!format!("{error:?}{error}").contains("PRIVATE_DAEMON_TOKEN"));
}
#[test]
fn live_containerd_rpc_has_a_bounded_read_deadline() {
    let Some(server) = FakeGrpc::bind(Reply::Stall) else {
        return;
    };
    let started = Instant::now();
    assert!(ContainerdReadOnly::list_namespaces(&server.reader()).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(server.requests.lock().expect("requests").len(), 1);
}
