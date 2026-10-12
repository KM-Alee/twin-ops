use kube::Config;
use twin_k8s::{PodLogRequest, WatchUpdate};

type Server = twin_fixtures::K8sStreamFixture;
fn server(mode: &str) -> Option<Server> {
    match Server::new(mode) {
        Ok(server) => Some(server),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("runtime capability unavailable: local TCP fixture bind denied; skipping stream API test");
            None
        }
        Err(error) => panic!("fixture bind: {error}"),
    }
}
fn config(server: &Server) -> Config {
    Config::new(server.url().parse().expect("url"))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}
fn log_request() -> PodLogRequest {
    PodLogRequest {
        namespace: "default".into(),
        pod: "api".into(),
        container: Some("web".into()),
        previous: true,
        tail_lines: 10,
        max_bytes: 64,
        timeout_seconds: 1,
    }
}

#[test]
fn metadata_watch_resumes_bookmarks_and_relists_expired_versions() {
    let Some(server) = server("logs") else {
        return;
    };
    let mut views = 0;
    let summary = runtime()
        .block_on(twin_k8s::watch_cluster_async(
            config(&server),
            5,
            2,
            |update| {
                if let WatchUpdate::View(view) = update {
                    views += 1;
                    assert!(!format!("{view:?}").contains("PRIVATE_"));
                }
                Ok(())
            },
        ))
        .expect("watch");
    assert_eq!(summary.observed_events, 2);
    assert!(summary.limit_reached);
    assert!(summary.relists >= 13);
    assert!(views > 1);
    assert!(!serde_json::to_string(&summary)
        .expect("summary")
        .contains("PRIVATE_"));
    let requests = server.requests().expect("requests");
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    assert!(requests
        .iter()
        .any(|request| request.contains("watch=true") && request.contains("resourceVersion=12")));
    assert!(requests
        .iter()
        .any(|request| request.contains("watch=true") && request.contains("resourceVersion=20")));
    assert!(requests
        .iter()
        .filter(|request| request.contains("watch=true"))
        .all(|request| request.contains("PartialObjectMetadata")));
}

#[test]
fn pod_logs_are_bounded_volume_only_and_never_return_bodies() {
    let Some(server) = server("logs") else {
        return;
    };
    let summary = runtime()
        .block_on(twin_k8s::read_pod_log_summary_async(
            config(&server),
            &log_request(),
        ))
        .expect("logs");
    assert_eq!(summary.received_bytes, 64);
    assert!(summary.limit_reached);
    assert!(summary.line_breaks > 0);
    assert!(!serde_json::to_string(&summary)
        .expect("summary")
        .contains("PRIVATE_"));
    let requests = server.requests().expect("requests");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /api/v1/namespaces/default/pods/api/log?"));
    for parameter in [
        "container=web",
        "previous=true",
        "tailLines=10",
        "limitBytes=65",
    ] {
        assert!(requests[0].contains(parameter));
    }
    assert!(!requests[0].contains("follow=true"));
}

#[test]
fn denied_logs_and_timeout_keep_only_sanitized_partial_coverage() {
    for mode in ["denied", "timeout"] {
        let Some(server) = server(mode) else {
            return;
        };
        let summary = runtime()
            .block_on(twin_k8s::read_pod_log_summary_async(
                config(&server),
                &log_request(),
            ))
            .expect("summary");
        assert!(summary.coverage_gap.is_some());
        assert_eq!(summary.received_bytes, 0);
        assert!(!serde_json::to_string(&summary)
            .expect("summary")
            .contains("PRIVATE_"));
    }
}

#[test]
fn invalid_targets_limits_and_external_credentials_are_rejected_before_requests() {
    let mut request = log_request();
    request.pod = "api/PRIVATE_SENTINEL".into();
    assert!(runtime()
        .block_on(twin_k8s::read_pod_log_summary_async(
            Config::new("http://127.0.0.1:1".parse().expect("url")),
            &request
        ))
        .is_err());
    let config = Config::new("http://127.0.0.1:1".parse().expect("url"));
    assert!(runtime()
        .block_on(twin_k8s::watch_cluster_async(config, 0, 1, |_| Ok(())))
        .is_err());
}
