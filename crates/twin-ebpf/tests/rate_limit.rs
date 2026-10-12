use twin_ebpf::{dropped_observation, TcpEventKind, TcpRateLimiter, TcpRateLimits};
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

#[test]
fn excess_events_collapse_to_one_dropped_observation() {
    let mut limiter = TcpRateLimiter::new(TcpRateLimits {
        per_pid: 1,
        per_endpoint: 2,
        global_limit: 3,
        ..TcpRateLimits::session()
    });
    assert!(limiter.admit(4421, "10.0.0.8:5432", 1, TcpEventKind::Connect));
    assert!(!limiter.admit(4421, "10.0.0.9:5432", 1, TcpEventKind::Connect));
    assert!(limiter.admit(4422, "10.0.0.8:5432", 1, TcpEventKind::Connect));
    assert!(!limiter.admit(4422, "10.0.0.8:5432", 1, TcpEventKind::Connect));
    assert!(limiter.admit(4423, "10.0.0.9:80", 1, TcpEventKind::Connect));
    assert!(!limiter.admit(4424, "10.0.0.9:80", 1, TcpEventKind::Connect));
    assert_eq!(limiter.dropped(), 3);

    let raw = dropped_observation(limiter.dropped(), 10);
    assert_eq!(raw.source, ObservationSource::Ebpf);
    assert_eq!(raw.kind, ObservationKind::EbpfDroppedEvents);
    assert_eq!(raw.collector.as_str(), "ebpf_tcp");
    let observation = Pipeline::default().process(raw).expect("pipeline");
    assert_eq!(
        observation
            .metadata()
            .get("source")
            .and_then(|value| value.as_str()),
        Some("ebpf_tcp")
    );
    assert_eq!(
        observation
            .metadata()
            .get("count")
            .and_then(|value| value.as_u64()),
        Some(3)
    );
}

#[test]
fn cgroup_and_event_type_limits_are_independent_and_rejections_do_not_spend_other_budgets() {
    let mut limiter = TcpRateLimiter::new(TcpRateLimits {
        per_cgroup: 1,
        per_event_type: 2,
        ..TcpRateLimits::session()
    });
    assert!(limiter.admit(1, "one", 10, TcpEventKind::Connect));
    assert!(!limiter.admit(2, "two", 10, TcpEventKind::Accept));
    assert!(limiter.admit(2, "two", 20, TcpEventKind::Accept));
    assert!(limiter.admit(3, "three", 30, TcpEventKind::Connect));
    assert!(!limiter.admit(4, "four", 40, TcpEventKind::Connect));
    assert!(limiter.admit(4, "four", 40, TcpEventKind::Bind));
    assert_eq!(limiter.dropped(), 2);
}
