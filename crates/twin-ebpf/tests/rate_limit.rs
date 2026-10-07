use twin_ebpf::{dropped_observation, TcpRateLimiter, TcpRateLimits};
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

#[test]
fn excess_events_collapse_to_one_dropped_observation() {
    let mut limiter = TcpRateLimiter::new(TcpRateLimits {
        per_pid: 1,
        per_endpoint: 2,
        global_limit: 3,
    });
    assert!(limiter.admit(4421, "10.0.0.8:5432"));
    assert!(!limiter.admit(4421, "10.0.0.9:5432"));
    assert!(limiter.admit(4422, "10.0.0.8:5432"));
    assert!(!limiter.admit(4422, "10.0.0.8:5432"));
    assert!(limiter.admit(4423, "10.0.0.9:80"));
    assert!(!limiter.admit(4424, "10.0.0.9:80"));
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
