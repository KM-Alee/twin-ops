use twin_ebpf::{attach, attach_from_bytes, EbpfError};

#[test]
fn attach_without_an_embedded_program_is_unavailable() {
    match attach() {
        Err(EbpfError::Unavailable { .. }) => {}
        Err(err) => panic!("expected unavailable, got {err}"),
        Ok(_) => panic!("expected attach to fail"),
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
}
