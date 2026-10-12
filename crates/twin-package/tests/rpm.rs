use rusqlite::{params, Connection};

fn database(path: &std::path::Path, bytes: &[u8]) {
    let connection = Connection::open(path).expect("db");
    connection
        .execute(
            "CREATE TABLE Packages (hnum INTEGER PRIMARY KEY, blob BLOB NOT NULL)",
            [],
        )
        .expect("schema");
    connection
        .execute("INSERT INTO Packages VALUES (1, ?1)", params![bytes])
        .expect("row");
}

#[test]
fn sqlite_header_ownership_and_version_are_read_without_creating_or_changing_files() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("special ? # name.sqlite");
    database(&path, &twin_fixtures::rpm_openssl_header());
    let before = std::fs::read(&path).expect("before");
    let package = twin_package::read_rpm(&path).expect("read").remove(0);
    assert_eq!(package.name, "openssl");
    assert_eq!(package.version, "3.0.1-2.fc40");
    assert_eq!(
        package.files,
        vec!["/usr/lib64/libssl.so.3", "/usr/lib64/libcrypto.so.3"]
    );
    assert_eq!(std::fs::read(&path).expect("after"), before);
    assert_eq!(std::fs::read_dir(dir.path()).expect("entries").count(), 1);
}

#[test]
fn missing_corrupt_truncated_and_bad_offset_databases_fail_cleanly() {
    let dir = tempfile::tempdir().expect("dir");
    assert!(twin_package::read_rpm(&dir.path().join("missing")).is_err());
    for (index, mut bytes) in [
        vec![0; 7],
        twin_fixtures::rpm_openssl_header(),
        twin_fixtures::rpm_openssl_header(),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 {
            bytes.truncate(bytes.len() - 1);
        }
        if index == 2 {
            bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        }
        let path = dir.path().join(format!("bad-{index}.sqlite"));
        database(&path, &bytes);
        assert!(twin_package::read_rpm(&path).is_err(), "{index}");
    }
    let path = dir.path().join("corrupt.sqlite");
    std::fs::write(&path, b"not a sqlite database").expect("file");
    assert!(twin_package::read_rpm(&path).is_err());
}

#[test]
fn malformed_wal_is_rejected_without_modifying_it() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("rpmdb.sqlite");
    database(&path, &twin_fixtures::rpm_openssl_header());
    let wal = dir.path().join("rpmdb.sqlite-wal");
    std::fs::write(&wal, b"pending WAL").expect("wal");
    let before = std::fs::read(&path).expect("before");
    let error = twin_package::read_rpm(&path).expect_err("wal");
    assert!(error.to_string().contains("WAL snapshot"));
    assert_eq!(std::fs::read(&path).expect("after"), before);
    assert_eq!(std::fs::read(&wal).expect("wal"), b"pending WAL");
}

#[test]
fn committed_wal_snapshot_includes_new_packages_without_touching_source_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rpmdb.sqlite");
    database(&path, &twin_fixtures::rpm_openssl_header());
    let writer = Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "INSERT INTO Packages VALUES (2, ?1)",
            params![twin_fixtures::rpm_openssl_header()],
        )
        .unwrap();
    let wal = dir.path().join("rpmdb.sqlite-wal");
    let shm = dir.path().join("rpmdb.sqlite-shm");
    let before: Vec<_> = [&path, &wal, &shm]
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let count = std::fs::read_dir(dir.path()).unwrap().count();
    let packages = twin_package::read_rpm(&path).unwrap();
    assert_eq!(packages.len(), 2);
    for (file, bytes) in [&path, &wal, &shm].iter().zip(before) {
        assert_eq!(std::fs::read(file).unwrap(), bytes);
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
    writer
        .execute_batch("BEGIN; DELETE FROM Packages;")
        .unwrap();
    assert_eq!(
        twin_package::read_rpm(&path).unwrap().len(),
        2,
        "uncommitted transaction must not be applied"
    );
    writer.execute_batch("ROLLBACK;").unwrap();
}

#[test]
fn corrupted_checksum_salt_and_truncated_wal_are_rejected() {
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("rpmdb.sqlite");
    database(&path, &twin_fixtures::rpm_openssl_header());
    let writer = Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute(
            "INSERT INTO Packages VALUES (2, ?1)",
            params![twin_fixtures::rpm_openssl_header()],
        )
        .unwrap();
    let database = std::fs::read(&path).unwrap();
    let wal = std::fs::read(source.path().join("rpmdb.sqlite-wal")).unwrap();
    for offset in [0, 24, 40, 48, 56, wal.len() - 1, wal.len()] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rpmdb.sqlite");
        std::fs::write(&path, &database).unwrap();
        let mut damaged = wal.clone();
        if offset == damaged.len() {
            damaged.pop();
        } else {
            damaged[offset] ^= 0xff;
        }
        std::fs::write(dir.path().join("rpmdb.sqlite-wal"), damaged).unwrap();
        assert!(twin_package::read_rpm(&path).is_err(), "{offset}");
    }
}

#[test]
fn rollback_journal_is_rejected_without_recovering_or_modifying_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rpmdb.sqlite");
    database(&path, &twin_fixtures::rpm_openssl_header());
    let before = std::fs::read(&path).unwrap();
    let journal = dir.path().join("rpmdb.sqlite-journal");
    std::fs::write(&journal, b"unfinished transaction").unwrap();
    let error = twin_package::read_rpm(&path).unwrap_err();
    assert!(error.to_string().contains("rollback journal"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(&journal).unwrap(), b"unfinished transaction");
}
