use twin_ebpf::{attach_from_bytes, attach_tcp_from_bytes, EbpfError};

#[test]
fn bundled_probe_has_bpf_elf_header_without_attaching_to_host() {
    let bytes = include_bytes!("../probes/twin.bpf.o");
    assert_eq!(&bytes[..4], b"\x7fELF");
    assert_eq!(bytes[5], 1);
    assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 247);
    let text = String::from_utf8_lossy(bytes);
    for name in [
        "sched_process_exec",
        "inet_sock_set_state",
        "sys_enter_connect",
        "sys_exit_connect",
        "EXEC_DROPS",
        "TCP_DROPS",
    ] {
        assert!(text.contains(name));
    }
}

#[test]
fn aya_rejects_a_non_elf_object_without_panicking() {
    let bytes = [0x7f, b'E', b'L', b'F', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    match attach_from_bytes(&bytes) {
        Err(EbpfError::Unavailable { .. } | EbpfError::Verifier { .. }) => {}
        Err(err) => panic!("expected a typed load error, got {err}"),
        Ok(_) => panic!("expected aya to reject the object"),
    }
    match attach_tcp_from_bytes(&bytes) {
        Err(EbpfError::Unavailable { .. } | EbpfError::Verifier { .. }) => {}
        Err(err) => panic!("expected a typed tcp load error, got {err}"),
        Ok(_) => panic!("expected aya to reject the tcp object"),
    }
}

#[test]
fn bundled_elf_parses_with_aya_and_has_valid_bounded_maps() {
    #[repr(align(8))]
    struct Aligned([u8; include_bytes!("../probes/twin.bpf.o").len()]);
    let bytes = Aligned(*include_bytes!("../probes/twin.bpf.o"));
    let object = aya_obj::Object::parse(&bytes.0).unwrap();
    assert!(object.btf.is_some());
    assert!(object.btf_ext.is_some());
    for name in [
        "exec",
        "tcp",
        "connect_enter",
        "connect_exit",
        "connect_cleanup",
    ] {
        assert!(object.programs.contains_key(name));
    }
    for name in ["EVENTS", "TCP_EVENTS"] {
        let map = &object.maps[name];
        assert_eq!(map.map_type(), 27);
        assert_eq!(map.key_size(), 0);
        assert_eq!(map.value_size(), 0);
        assert_eq!(map.max_entries(), 1_048_576);
    }
    for name in ["EXEC_DROPS", "TCP_DROPS"] {
        assert_eq!(object.maps[name].value_size(), 8);
        assert_eq!(object.maps[name].max_entries(), 1);
    }
    assert_eq!(object.maps["PID_NAMESPACE"].value_size(), 16);
    assert_eq!(object.maps["ORIGINS"].value_size(), 72);
    assert_eq!(object.maps["ORIGINS"].max_entries(), 16_384);
}
