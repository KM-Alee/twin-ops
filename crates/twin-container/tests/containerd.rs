use std::path::Path;
use twin_container::containerd::{read_containerd, TaskState};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("namespaces.json"),
        "[\"default\",\"k8s.io\"]",
    )
    .unwrap();
    for namespace in ["default", "k8s.io"] {
        let dir = root.path().join("namespaces").join(namespace);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("containers.json"), r#"[{"id":"api","image":"example/api:1","created_at_ns":1,"spec":{"env":["TOKEN=PRIVATE_SENTINEL"]},"labels":{"secret":"PRIVATE_SENTINEL"}}]"#).unwrap();
        std::fs::write(
            dir.join("tasks.json"),
            r#"[{"container_id":"api","pid":42,"state":"running","stdout":"PRIVATE_SENTINEL"}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("images.json"),
            r#"[{"name":"example/api:1","digest":null}]"#,
        )
        .unwrap();
    }
    root
}

#[test]
fn namespace_scoped_metadata_omits_specs_labels_and_task_io() {
    let root = fixture();
    let snapshot = read_containerd(Path::new("/unused"), Some(root.path())).unwrap();
    assert!(snapshot.is_complete);
    assert_eq!(snapshot.namespaces.len(), 2);
    assert!(!format!("{snapshot:?}").contains("PRIVATE_SENTINEL"));
    assert_eq!(
        snapshot.namespaces[0].tasks.as_ref().unwrap()[0].state,
        TaskState::Running
    );
    let left = twin_core::NodeId::containerd("default", "api").unwrap();
    let right = twin_core::NodeId::containerd("k8s.io", "api").unwrap();
    assert_ne!(left, right);
    assert_ne!(left, twin_core::NodeId::container("api"));
}

#[test]
fn malformed_or_missing_resource_preserves_explicit_incomplete_snapshot() {
    let root = fixture();
    std::fs::write(
        root.path().join("namespaces/default/tasks.json"),
        "{PRIVATE_SENTINEL",
    )
    .unwrap();
    std::fs::remove_file(root.path().join("namespaces/k8s.io/images.json")).unwrap();
    let snapshot = read_containerd(Path::new("/unused"), Some(root.path())).unwrap();
    assert!(!snapshot.is_complete);
    assert!(snapshot.namespaces[0].tasks.is_none());
    assert!(snapshot.namespaces[1].images.is_none());
    assert!(!format!("{snapshot:?}").contains("PRIVATE_SENTINEL"));
}

#[test]
fn traversal_and_duplicate_container_ids_are_rejected() {
    let root = fixture();
    std::fs::write(root.path().join("namespaces.json"), "[\"../escape\"]").unwrap();
    assert!(read_containerd(Path::new("/unused"), Some(root.path())).is_err());
    std::fs::write(root.path().join("namespaces.json"), "[\"default\"]").unwrap();
    std::fs::write(root.path().join("namespaces/default/containers.json"), r#"[{"id":"api","image":null,"created_at_ns":1},{"id":"api","image":null,"created_at_ns":2}]"#).unwrap();
    let snapshot = read_containerd(Path::new("/unused"), Some(root.path())).unwrap();
    assert!(!snapshot.is_complete);
    assert!(snapshot.namespaces[0].containers.is_none());
}

#[test]
fn live_async_boundaries_reject_bad_names_and_sync_runtime_reentry() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let reader = twin_container::containerd::LiveContainerd::new(std::path::Path::new(
        "/missing/containerd.sock",
    ));
    runtime.block_on(async {
        let error = reader
            .list_containers("../PRIVATE_VALUE")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid namespace"));
        assert!(!error.contains("PRIVATE_VALUE"));
        let error = twin_container::containerd::ContainerdReadOnly::list_namespaces(&reader)
            .unwrap_err()
            .to_string();
        assert!(error.contains("asynchronous reader"));
        let error = reader.list_namespaces().await.unwrap_err().to_string();
        assert!(error.contains("socket unavailable"));
    });
}
