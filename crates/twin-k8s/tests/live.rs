use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use kube::Config;
use serde_json::json;

struct FakeApi {
    address: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl FakeApi {
    fn new(deny: bool, stall: bool) -> Option<Self> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("runtime capability unavailable: local TCP fixture bind denied; skipping live API test");
                return None;
            }
            Err(error) => panic!("listen: {error}"),
        };
        listener.set_nonblocking(true).expect("nonblocking");
        let address = format!("http://{}", listener.local_addr().expect("addr"));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop_server = stop.clone();
        let captured = requests.clone();
        let thread = std::thread::spawn(move || {
            while !stop_server.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("timeout");
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = socket.read(&mut buffer).expect("read request");
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(request).expect("utf8");
                captured.lock().expect("lock").push(request.clone());
                if stall {
                    while !stop_server.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    break;
                }
                let path = request.split_whitespace().nth(1).expect("path");
                let metadata = request.contains("PartialObjectMetadataList");
                let mut body = json!({"apiVersion":"v1","kind":"List","metadata":{},"items":[]});
                let status = if deny {
                    body = json!({"apiVersion":"v1","kind":"Status","status":"Failure","code":403,"reason":"Forbidden","message":"PRIVATE_API_ERROR_MUST_NOT_ESCAPE"});
                    "403 Forbidden"
                } else {
                    if path.starts_with("/api/v1/pods") {
                        let next = path.contains("continue=next-page");
                        body["items"] = json!([{"apiVersion":"v1","kind":"Pod","metadata":{"name":if next { "second" } else { "first" },"namespace":"default"},"spec":{"containers":[{"name":"api","image":"api:1"}]},"status":{"conditions":[{"type":"Ready","status":"True"}]}}]);
                        if !next {
                            body["metadata"] = json!({"continue":"next-page"});
                        }
                    } else if path.starts_with("/api/v1/secrets")
                        || path.starts_with("/api/v1/configmaps")
                    {
                        body["items"] = json!([{"metadata":{"name":"reference","namespace":"default","annotations":{"private":"ANNOTATION_MUST_NOT_ESCAPE"}},"data":{"private":"BODY_MUST_NOT_ESCAPE"},"stringData":{"private":"BODY_MUST_NOT_ESCAPE"}}]);
                    }
                    if metadata {
                        body["kind"] = json!("PartialObjectMetadataList");
                        body["apiVersion"] = json!("meta.k8s.io/v1");
                    }
                    "200 OK"
                };
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes());
            }
        });
        Some(Self {
            address,
            stop,
            requests,
            thread: Some(thread),
        })
    }
    fn config(&self) -> Config {
        Config::new(self.address.parse().expect("uri"))
    }
}
impl Drop for FakeApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("server");
        }
    }
}
fn run(config: Config) -> Result<twin_k8s::ClusterView, twin_k8s::K8sError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(twin_k8s::read_cluster_async(config))
}

#[test]
fn live_reads_paginate_and_keep_only_safe_resource_fields() {
    let Some(api) = FakeApi::new(false, false) else {
        return;
    };
    let view = run(api.config()).expect("live scan");
    assert_eq!(view.pods.len(), 2);
    assert!(view.warnings.is_empty(), "{:?}", view.warnings);
    assert_eq!(view.secret_refs[0].name, "reference");
    assert_eq!(view.configmaps[0].name, "reference");
    let printed = format!("{view:?}");
    for secret in ["BODY_MUST_NOT_ESCAPE", "ANNOTATION_MUST_NOT_ESCAPE"] {
        assert!(!printed.contains(secret));
    }
    let requests = api.requests.lock().expect("requests");
    assert_eq!(requests.len(), 13);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    for request in requests
        .iter()
        .filter(|request| request.contains("/secrets?") || request.contains("/configmaps?"))
    {
        assert!(request.contains("PartialObjectMetadataList"));
    }
    assert!(requests
        .iter()
        .any(|request| request.contains("continue=next-page")));
}

#[test]
fn permission_errors_are_coverage_failures_and_do_not_include_server_messages() {
    let Some(api) = FakeApi::new(true, false) else {
        return;
    };
    let error = run(api.config()).expect_err("denied");
    assert!(error.to_string().contains("all resource reads failed"));
    assert!(!error.to_string().contains("PRIVATE_API_ERROR"));
}

#[test]
fn external_authentication_commands_are_rejected_before_any_request() {
    let Some(api) = FakeApi::new(false, false) else {
        return;
    };
    let mut config = api.config();
    config.auth_info.exec =
        Some(serde_yaml::from_str("command: must-never-run\n").expect("config"));
    let error = run(config).expect_err("disabled");
    assert!(error
        .to_string()
        .contains("external credential commands are disabled"));
    assert!(api.requests.lock().expect("requests").is_empty());
}

#[test]
fn unresponsive_api_has_a_bounded_timeout() {
    let Some(api) = FakeApi::new(false, true) else {
        return;
    };
    let started = std::time::Instant::now();
    assert!(run(api.config()).is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}
