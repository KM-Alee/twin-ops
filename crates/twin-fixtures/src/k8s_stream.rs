use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct K8sStreamFixture {
    url: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl K8sStreamFixture {
    pub fn new(log_mode: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stops = stop.clone();
        let captured = requests.clone();
        let counters = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
        let mode = log_mode.to_string();
        let thread = std::thread::spawn(move || {
            while !stops.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let captured = captured.clone();
                let counters = counters.clone();
                let mode = mode.clone();
                std::thread::spawn(move || {
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 2048];
                    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        let count = stream.read(&mut buffer)?;
                        if count == 0 {
                            return Ok(());
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let request = String::from_utf8(bytes)
                        .map_err(|_| std::io::Error::other("invalid request encoding"))?;
                    captured
                        .lock()
                        .map_err(|_| std::io::Error::other("request lock poisoned"))?
                        .push(request.clone());
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .ok_or_else(|| std::io::Error::other("missing request path"))?;
                    let mut status = "200 OK";
                    let body = if path.contains("/log?") {
                        if mode == "denied" {
                            status = "403 Forbidden";
                            json!({"apiVersion":"v1","kind":"Status","code":403,"status":"Failure","reason":"Forbidden","message":"PRIVATE_API_LOG_SENTINEL"}).to_string()
                        } else if mode == "timeout" {
                            std::thread::sleep(Duration::from_secs(2));
                            "PRIVATE_LOG_BODY_SENTINEL".into()
                        } else {
                            "Authorization: Bearer PRIVATE_LOG_BODY_SENTINEL\n".repeat(20)
                        }
                    } else if path.contains("watch=true") {
                        let counter = {
                            let mut counters = counters
                                .lock()
                                .map_err(|_| std::io::Error::other("counter lock poisoned"))?;
                            let count = counters
                                .entry(
                                    path.split('?')
                                        .next()
                                        .ok_or_else(|| {
                                            std::io::Error::other("missing request path")
                                        })?
                                        .to_string(),
                                )
                                .or_default();
                            *count += 1;
                            *count
                        };
                        if path.starts_with("/api/v1/pods?") {
                            match counter {
                                1 => format!(
                                    "{}\n{}\n",
                                    json!({"type":"ADDED","object":{"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"api","namespace":"default","resourceVersion":"11","annotations":{"private":"PRIVATE_WATCH_SENTINEL"}}}}),
                                    json!({"type":"BOOKMARK","object":{"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"resourceVersion":"12"}}})
                                ),
                                2 => format!(
                                    "{}\n",
                                    json!({"type":"ERROR","object":{"apiVersion":"v1","kind":"Status","code":410,"reason":"Expired","message":"PRIVATE_EXPIRED_SENTINEL"}})
                                ),
                                _ => format!(
                                    "{}\n",
                                    json!({"type":"MODIFIED","object":{"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"api","namespace":"default","resourceVersion":"21"}}})
                                ),
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        let metadata = request.contains("PartialObjectMetadataList");
                        let version = if path.starts_with("/api/v1/pods?") && metadata {
                            let mut counters = counters
                                .lock()
                                .map_err(|_| std::io::Error::other("counter lock poisoned"))?;
                            let count = counters.entry("pod-lists".into()).or_default();
                            *count += 1;
                            if *count == 1 {
                                "10"
                            } else {
                                "20"
                            }
                        } else {
                            "10"
                        };
                        json!({"apiVersion": if metadata { "meta.k8s.io/v1" } else { "v1" },"kind":if metadata { "PartialObjectMetadataList" } else { "List" },"metadata":{"resourceVersion":version},"items":[]}).to_string()
                    };
                    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes());
                    Ok::<_, std::io::Error>(())
                });
            }
            Ok(())
        });
        Ok(Self {
            url,
            stop,
            requests,
            thread: Some(thread),
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn requests(&self) -> std::io::Result<Vec<String>> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|_| std::io::Error::other("request lock poisoned"))
    }
}
impl Drop for K8sStreamFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
