mod rpm_bdb;
pub use rpm_bdb::rpm_bdb_database;
mod k8s_stream;
pub use k8s_stream::K8sStreamFixture;
mod procfs;
pub use procfs::write_proc_fixture_with_cgroup;

pub fn rpm_openssl_header() -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (tag, kind, strings) in [
        (1000u32, 6u32, vec!["openssl"]),
        (1001, 6, vec!["3.0.1"]),
        (1002, 6, vec!["2.fc40"]),
        (1117, 8, vec!["libssl.so.3", "libcrypto.so.3"]),
        (1118, 8, vec!["/usr/lib64/"]),
    ] {
        entries.push((tag, kind, data.len() as u32, strings.len() as u32));
        for text in strings {
            data.extend_from_slice(text.as_bytes());
            data.push(0);
        }
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    entries.push((1116, 4, data.len() as u32, 2));
    data.extend_from_slice(&[0; 8]);
    entries.sort_by_key(|entry| entry.0);
    let mut header = Vec::new();
    header.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    header.extend_from_slice(&(data.len() as u32).to_be_bytes());
    for (tag, kind, offset, count) in entries {
        for word in [tag, kind, offset, count] {
            header.extend_from_slice(&word.to_be_bytes());
        }
    }
    header.extend(data);
    header
}

pub fn rpm_ndb_database(headers: &[Vec<u8>]) -> Vec<u8> {
    let pages = (32 + headers.len() * 16).div_ceil(4096);
    let mut bytes = vec![0; pages * 4096];
    bytes[..4].copy_from_slice(b"RpmP");
    bytes[12..16].copy_from_slice(&(pages as u32).to_le_bytes());
    for slot in bytes[32..].as_chunks_mut::<16>().0 {
        slot[..4].copy_from_slice(b"Slot");
    }
    for (index, header) in headers.iter().enumerate() {
        let id = index as u32 + 1;
        let offset = bytes.len();
        let size = (16 + header.len() + 12).div_ceil(16) * 16;
        bytes.resize(offset + size, 0);
        let slot = 32 + index * 16;
        bytes[slot + 4..slot + 8].copy_from_slice(&id.to_le_bytes());
        bytes[slot + 8..slot + 12].copy_from_slice(&((offset / 16) as u32).to_le_bytes());
        bytes[slot + 12..slot + 16].copy_from_slice(&((size / 16) as u32).to_le_bytes());
        bytes[offset..offset + 4].copy_from_slice(b"BlbS");
        bytes[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
        bytes[offset + 12..offset + 16].copy_from_slice(&(header.len() as u32).to_le_bytes());
        bytes[offset + 16..offset + 16 + header.len()].copy_from_slice(header);
        let tail = offset + size - 12;
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in &bytes[offset..tail] {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        bytes[tail..tail + 4].copy_from_slice(&((b << 16) | a).to_le_bytes());
        bytes[tail + 4..tail + 8].copy_from_slice(&(header.len() as u32).to_le_bytes());
        bytes[tail + 8..tail + 12].copy_from_slice(b"BlbE");
    }
    bytes
}
