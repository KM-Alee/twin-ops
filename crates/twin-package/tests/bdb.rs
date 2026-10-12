#[test]
fn reads_endian_hash_buckets_inline_and_overflow_without_mutating_sources() {
    for big in [false, true] {
        for overflow in [false, true] {
            let home = tempfile::tempdir().expect("home");
            let path = home.path().join("Packages");
            let bytes = twin_fixtures::rpm_bdb_database(
                &twin_fixtures::rpm_openssl_header(),
                big,
                overflow,
            );
            std::fs::write(&path, &bytes).expect("fixture");
            let database = twin_package::read_rpm_database(&path).expect("bdb");
            assert_eq!(database.format, twin_package::RpmDatabaseFormat::Bdb);
            assert_eq!(database.packages[0].name, "openssl");
            assert_eq!(database.packages[0].version, "3.0.1-2.fc40");
            assert_eq!(database.packages[0].files.len(), 2);
            assert_eq!(std::fs::read(&path).expect("source"), bytes);
            assert_eq!(std::fs::read_dir(home.path()).expect("entries").count(), 1);
        }
    }
}
#[test]
fn rejects_bdb_partial_inventory_features_offsets_cycles_and_active_environment() {
    let valid = twin_fixtures::rpm_bdb_database(&twin_fixtures::rpm_openssl_header(), false, true);
    for case in 0..9 {
        let home = tempfile::tempdir().expect("home");
        let path = home.path().join("Packages");
        let mut bytes = valid.clone();
        match case {
            0 => bytes[26] = 1,
            1 => bytes[24] = 1,
            2 => bytes[48] = 1,
            3 => bytes[512 + 26..512 + 28].copy_from_slice(&1_u16.to_le_bytes()),
            4 => bytes[512 + 16..512 + 20].copy_from_slice(&1_u32.to_le_bytes()),
            5 => bytes[1536 + 16..1536 + 20].copy_from_slice(&3_u32.to_le_bytes()),
            6 => {
                bytes.pop();
            }
            7 => {
                std::fs::write(home.path().join("__db.001"), b"cache").expect("cache");
            }
            _ => {
                std::fs::write(home.path().join("log.0000000001"), b"recovery").expect("log");
            }
        }
        std::fs::write(&path, bytes).expect("fixture");
        assert!(twin_package::read_rpm(&path).is_err(), "case {case}");
    }
}
#[test]
fn bdb_overflow_spans_multiple_pages_and_ignores_unlinked_free_bytes() {
    let home = tempfile::tempdir().expect("home");
    let path = home.path().join("Packages");
    let mut header = twin_fixtures::rpm_openssl_header();
    header.resize(1500, 0);
    let mut bytes = twin_fixtures::rpm_bdb_database(&header, false, true);
    bytes.extend_from_slice(&vec![0x7f; 512]);
    let last = (bytes.len() / 512 - 1) as u32;
    bytes[32..36].copy_from_slice(&last.to_le_bytes());
    std::fs::write(&path, bytes).expect("fixture");
    assert_eq!(
        twin_package::read_rpm(&path).expect("overflow")[0].name,
        "openssl"
    );
}

#[test]
fn reads_legacy_unsorted_hash_and_skips_only_the_four_byte_zero_key_counter() {
    let home = tempfile::tempdir().expect("home");
    let path = home.path().join("Packages");
    let mut bytes =
        twin_fixtures::rpm_bdb_database(&twin_fixtures::rpm_openssl_header(), false, false);
    bytes[16..20].copy_from_slice(&8_u32.to_le_bytes());
    bytes[512 + 25] = 2;
    let root = 1024;
    bytes[root + 25] = 2;
    bytes[root + 20..root + 22].copy_from_slice(&2_u16.to_le_bytes());
    bytes[root + 22..root + 24].copy_from_slice(&502_u16.to_le_bytes());
    bytes[root + 26..root + 28].copy_from_slice(&507_u16.to_le_bytes());
    bytes[root + 28..root + 30].copy_from_slice(&502_u16.to_le_bytes());
    bytes[root + 507] = 1;
    bytes[root + 502] = 1;
    bytes[root + 503..root + 507].copy_from_slice(&1_u32.to_le_bytes());
    std::fs::write(&path, &bytes).expect("fixture");
    assert_eq!(twin_package::read_rpm(&path).expect("counter").len(), 1);
    bytes[root + 508] = 2;
    std::fs::write(&path, bytes).expect("bad header record");
    assert!(twin_package::read_rpm(&path).is_err());
}
