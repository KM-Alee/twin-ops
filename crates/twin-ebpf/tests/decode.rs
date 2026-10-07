use twin_ebpf::{decode_exec, encode_exec, exec_to_raw, EbpfExec, EXEC_EVENT_LEN};
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

#[test]
fn short_and_long_buffers_are_rejected() {
    let short = decode_exec(&[0, 1, 2, 3]).expect_err("short");
    assert!(short.to_string().contains("malformed"));
    let mut long = vec![0u8; EXEC_EVENT_LEN + 1];
    long[0] = 1;
    let malformed = decode_exec(&long).expect_err("long");
    assert!(malformed.to_string().contains("malformed"));
}

#[test]
fn valid_exec_event_round_trips() {
    let event = EbpfExec::new(4421, 4400, "curl", 0x11, 99);
    let bytes = encode_exec(&event);
    assert_eq!(bytes.len(), EXEC_EVENT_LEN);
    let decoded = decode_exec(&bytes).expect("decode");
    assert_eq!(decoded.pid(), 4421);
    assert_eq!(decoded.ppid(), 4400);
    assert_eq!(decoded.comm(), "curl");
    assert_eq!(decoded.cgroup_id(), 0x11);
    assert_eq!(decoded.timestamp_ns(), 99);
}

#[test]
fn comm_is_task_name_only() {
    let mut bytes = [0u8; EXEC_EVENT_LEN];
    bytes[0..4].copy_from_slice(&4421u32.to_le_bytes());
    let name = b"curl\0argv-secret";
    bytes[24..24 + name.len()].copy_from_slice(name);
    let decoded = decode_exec(&bytes).expect("decode");
    assert_eq!(decoded.comm(), "curl");
    assert!(!decoded.comm().contains("argv"));
}

#[test]
fn observation_is_exec_on_the_process_and_stores_no_argv() {
    let event = EbpfExec::new(4421, 4400, "curl\nsecret", 77, 5);
    let observation = Pipeline::default()
        .process(exec_to_raw(&event))
        .expect("pipeline");
    assert_eq!(observation.kind(), ObservationKind::EbpfExecObserved);
    assert_eq!(observation.source(), ObservationSource::Ebpf);
    assert_eq!(
        observation.subject().map(|id| id.to_string()).as_deref(),
        Some("process:pid:4421")
    );
    assert_eq!(
        observation.metadata().get("comm").and_then(|v| v.as_str()),
        Some("curlsecret")
    );
    assert_eq!(
        observation.metadata().get("ppid").and_then(|v| v.as_u64()),
        Some(4400)
    );
    assert_eq!(
        observation
            .metadata()
            .get("cgroup_id")
            .and_then(|v| v.as_str()),
        Some("77")
    );
    for key in ["argv", "argv_json", "env", "uid", "password", "credential"] {
        assert!(observation.metadata().get(key).is_none(), "{key}");
    }
}
