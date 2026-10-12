#[test]
fn reads_valid_ndb_slot_blob_checksum_and_package_without_source_mutations() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("Packages.db");
    let bytes = twin_fixtures::rpm_ndb_database(&[twin_fixtures::rpm_openssl_header()]);
    std::fs::write(&path, &bytes).expect("fixture");
    let database = twin_package::read_rpm_database(&path).expect("ndb");
    assert_eq!(database.format, twin_package::RpmDatabaseFormat::Ndb);
    assert_eq!(database.packages[0].name, "openssl");
    assert_eq!(database.packages[0].version, "3.0.1-2.fc40");
    assert_eq!(
        database.packages[0].files,
        ["/usr/lib64/libssl.so.3", "/usr/lib64/libcrypto.so.3"]
    );
    assert_eq!(std::fs::read(&path).expect("source"), bytes);
    assert_eq!(
        std::fs::read_dir(directory.path())
            .expect("directory")
            .count(),
        1
    );
}

#[test]
fn ndb_rejects_checksum_mismatch_truncation_overlaps_duplicate_ids_and_version() {
    let valid = twin_fixtures::rpm_ndb_database(&[
        twin_fixtures::rpm_openssl_header(),
        twin_fixtures::rpm_openssl_header(),
    ]);
    for case in 0..6 {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("Packages.db");
        let mut bytes = valid.clone();
        match case {
            0 => bytes[4096 + 16] ^= 1,
            1 => {
                bytes.pop();
            }
            2 => {
                let offset = bytes[40..44].to_vec();
                bytes[56..60].copy_from_slice(&offset);
            }
            3 => bytes[52..56].copy_from_slice(&1_u32.to_le_bytes()),
            4 => bytes[4..8].copy_from_slice(&1_u32.to_le_bytes()),
            _ => bytes[32..36].fill(0),
        }
        std::fs::write(&path, bytes).expect("fixture");
        assert!(twin_package::read_rpm(&path).is_err(), "case {case}");
    }
}

#[test]
fn empty_ndb_is_a_complete_empty_inventory() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("Packages.db");
    std::fs::write(&path, twin_fixtures::rpm_ndb_database(&[])).expect("fixture");
    assert!(twin_package::read_rpm(&path).expect("empty").is_empty());
}

#[test]
fn ndb_reads_multiple_slot_pages_without_truncating_inventory() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("Packages.db");
    let headers = vec![twin_fixtures::rpm_openssl_header(); 300];
    std::fs::write(&path, twin_fixtures::rpm_ndb_database(&headers)).expect("fixture");
    assert_eq!(twin_package::read_rpm(&path).expect("packages").len(), 300);
}
