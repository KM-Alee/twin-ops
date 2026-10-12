mod support;

use std::{
    cell::Cell,
    io,
    path::{Path, PathBuf},
};
use twin_collectors::process::{ProcReader, StdProcReader};
use twin_collectors::{ProcessCollector, ProcessWarningKind};
use twin_core::TimestampNs;
use twin_observation::ObservationKind;

struct ChangingNamespace {
    path: PathBuf,
    reads: Cell<usize>,
    replacement: Option<u64>,
}

impl ProcReader for ChangingNamespace {
    fn list_pids(&self, root: &Path) -> Result<Vec<u32>, twin_collectors::CollectorError> {
        StdProcReader.list_pids(root)
    }
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        StdProcReader.read_file(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        StdProcReader.read_dir(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        if path == self.path {
            let reads = self.reads.get();
            self.reads.set(reads + 1);
            if reads > 0 {
                return self
                    .replacement
                    .map(|namespace| PathBuf::from(format!("net:[{namespace}]")))
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound));
            }
        }
        StdProcReader.read_link(path)
    }
}

#[test]
fn namespace_changed_or_vanished_during_collection_discards_socket_evidence() {
    for (host, replacement) in [
        (false, Some(3)),
        (false, None),
        (true, Some(3)),
        (true, None),
    ] {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        let namespace = if host { 1 } else { 2 };
        support::write_proc_fixture(root, 42,
            "42 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
            "Uid:\t0\t0\t0\t0\n", b"api\0", None);
        for (name, value) in [("self", 1), ("42", namespace)] {
            let directory = root.join(name).join("ns");
            std::fs::create_dir_all(&directory).unwrap();
            let link = directory.join("net");
            if std::fs::symlink_metadata(&link).is_ok() {
                std::fs::remove_file(&link).unwrap();
            }
            std::os::unix::fs::symlink(format!("net:[{value}]"), directory.join("net")).unwrap();
        }
        let table_root = if host {
            root.to_path_buf()
        } else {
            root.join("42")
        };
        support::write_tcp_table(&table_root, "tcp", "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n");
        support::write_socket_fd(root, 42, 8, 100);
        let reader = ChangingNamespace {
            path: root.join("42/ns/net"),
            reads: Cell::new(0),
            replacement,
        };
        let batch = ProcessCollector::with_reader(root, reader)
            .collect(TimestampNs::new(1))
            .unwrap();
        assert!(batch.namespaces().is_empty());
        assert!(batch.owners_by_inode().is_empty());
        assert!(!batch.observations().iter().any(|observation| matches!(
            observation.kind,
            ObservationKind::TcpSocketSeen | ObservationKind::ProcessSocketFdSeen
        )));
        assert!(batch
            .warnings()
            .iter()
            .any(|warning| warning.kind() == ProcessWarningKind::PermissionDenied));
    }
}
