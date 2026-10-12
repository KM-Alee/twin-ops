use clap::Parser;
use twin_cli::cli::args::{Cli, Command, K8sAction};
use twin_cli::output::k8s;

#[test]
fn watch_and_log_flags_preserve_bounds_and_container_scope() {
    let cli = Cli::try_parse_from([
        "twin",
        "k8s",
        "watch",
        "--duration",
        "15",
        "--max-events",
        "8",
    ])
    .expect("watch");
    let Command::K8s(args) = cli.command else {
        panic!("k8s");
    };
    let K8sAction::Watch(args) = args.action else {
        panic!("watch");
    };
    assert_eq!(args.duration, 15);
    assert_eq!(args.max_events, 8);
    let cli = Cli::try_parse_from([
        "twin",
        "k8s",
        "logs",
        "api",
        "--namespace",
        "production",
        "--container",
        "web",
        "--previous",
        "--max-bytes",
        "4096",
    ])
    .expect("logs");
    let Command::K8s(args) = cli.command else {
        panic!("k8s");
    };
    let K8sAction::Logs(args) = args.action else {
        panic!("logs");
    };
    assert_eq!(args.namespace, "production");
    assert_eq!(args.container.as_deref(), Some("web"));
    assert!(args.previous);
    assert_eq!(args.max_bytes, 4096);
    for argv in [
        vec!["twin", "k8s", "watch", "--duration", "0"],
        vec!["twin", "k8s", "logs", "api", "--max-bytes", "1048577"],
    ] {
        assert!(Cli::try_parse_from(argv).is_err());
    }
}

#[test]
fn watch_renderer_preserves_continuity_and_coverage_sections() {
    let result = twin_app::K8sWatchResult {
        duration_seconds: 30,
        summary: twin_app::K8sWatchSummary {
            observed_events: 1,
            refreshes: 2,
            relists: 1,
            reconnects: 1,
            notices: vec![twin_app::K8sWatchNotice {
                resource: "Pod".into(),
                event: "modified".into(),
                name: Some("api-long-label".into()),
                namespace: Some("production".into()),
            }],
            warnings: vec!["Kubernetes pods: watch interrupted".into()],
            omitted_notices: 3,
            limit_reached: true,
            ..twin_app::K8sWatchSummary::default()
        },
    };
    let text = k8s::render_watch(&result);
    for section in [
        "twin k8s watch",
        "═",
        "watch continuity",
        "resource changes",
        "production/api-long-label",
        "coverage gaps",
        "omitted events",
        "event limit",
        "└──",
    ] {
        assert!(text.contains(section), "{section}: {text}");
    }
}

#[test]
fn log_renderer_labels_phrase_counts_as_observations_and_never_suggests_a_cause() {
    let result = twin_app::PodLogSummary {
        namespace: "production".into(),
        pod: "api".into(),
        container: Some("web".into()),
        previous: false,
        received_bytes: 64,
        line_breaks: 2,
        max_bytes: 64,
        tail_lines: 100,
        limit_reached: true,
        coverage_gap: Some("log read timed out; summary is partial".into()),
        phrase_matches: vec![twin_app::LogSignalCount {
            signal: twin_app::LogSignal::ConnectionRefused,
            matches: 2,
        }],
    };
    let text = k8s::render_logs(&result);
    for section in [
        "twin k8s logs",
        "═",
        "read bounds",
        "byte limit",
        "observed phrase matches",
        "connection refused",
        "diagnostic causes remain unknown",
        "coverage gap",
        "log bodies are discarded",
        "└──",
    ] {
        assert!(text.contains(section), "{section}: {text}");
    }
    assert!(!serde_json::to_string(&result)
        .expect("json")
        .contains("log_body"));
}
