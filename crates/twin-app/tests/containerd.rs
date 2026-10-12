mod support;
use std::path::Path;
use twin_app::{InitRequest, ScanRequest};
use twin_store::Store;

fn write_runtime(root: &Path, namespace: &str, containers: &str) {
    let dir = root.join("namespaces").join(namespace);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("containers.json"), containers).unwrap();
    std::fs::write(dir.join("tasks.json"), "[]").unwrap();
    std::fs::write(dir.join("images.json"), "[]").unwrap();
}

#[test]
fn containerd_namespace_finalization_and_outage_preserve_identity_and_uncertainty() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).unwrap();
    let runtime = home.layout.data_dir.join("runtime");
    write_runtime(
        &runtime,
        "default",
        r#"[{"id":"api","image":"example/api:1","created_at_ns":1}]"#,
    );
    write_runtime(
        &runtime,
        "k8s.io",
        r#"[{"id":"api","image":"example/api:1","created_at_ns":1}]"#,
    );
    std::fs::write(runtime.join("namespaces.json"), "[\"default\",\"k8s.io\"]").unwrap();
    let request = ScanRequest {
        containerd_fixture: Some(runtime.clone()),
        ..ScanRequest::default()
    };
    twin_app::scan_in(&home.layout, request.clone(), &proc).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    for namespace in ["default", "k8s.io"] {
        assert_eq!(
            store
                .get_node(&format!("container:containerd:{namespace}:api"))
                .unwrap()
                .unwrap()
                .state,
            "active"
        );
    }
    std::fs::remove_file(runtime.join("namespaces/default/tasks.json")).unwrap();
    write_runtime(&runtime, "k8s.io", "[]");
    twin_app::scan_in(&home.layout, request.clone(), &proc).unwrap();
    assert_eq!(
        store
            .get_node("container:containerd:k8s.io:api")
            .unwrap()
            .unwrap()
            .state,
        "gone"
    );
    assert_ne!(
        store
            .get_node("container:containerd:default:api")
            .unwrap()
            .unwrap()
            .state,
        "gone"
    );
    std::fs::remove_file(runtime.join("namespaces.json")).unwrap();
    twin_app::scan_in(&home.layout, request, &proc).unwrap();
    assert_ne!(
        store
            .get_node("container:containerd:default:api")
            .unwrap()
            .unwrap()
            .state,
        "gone"
    );
}

#[test]
fn containerd_task_ownership_requires_process_identity_and_recreation_resets_proof() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("task-proc");
    support::write_proc_fixture(
        &proc,
        42,
        "42 (api) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 100 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"api\0",
        None,
    );
    let runtime = home.layout.data_dir.join("task-runtime");
    write_runtime(
        &runtime,
        "default",
        r#"[{"id":"api","image":"example/api:1","created_at_ns":1}]"#,
    );
    std::fs::write(runtime.join("namespaces.json"), "[\"default\"]").unwrap();
    std::fs::write(
        runtime.join("namespaces/default/tasks.json"),
        r#"[{"container_id":"api","pid":42,"state":"running"}]"#,
    )
    .unwrap();
    let request = ScanRequest {
        containerd_fixture: Some(runtime.clone()),
        ..ScanRequest::default()
    };
    twin_app::scan_in(&home.layout, request.clone(), &proc).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    let ownership = "container:containerd:default:api|owns|process:pid:42";
    assert_eq!(
        store.get_edge(ownership).unwrap().unwrap().class,
        "observed"
    );
    let image =
        "container:containerd:default:api|runs_image|image:containerd:default:example/api:1";
    let previous = store.list_observations_for_edge(image).unwrap();
    assert_eq!(previous.len(), 1);
    std::fs::write(
        runtime.join("namespaces/default/containers.json"),
        r#"[{"id":"api","image":"example/api:1","created_at_ns":2}]"#,
    )
    .unwrap();
    twin_app::scan_in(&home.layout, request.clone(), &proc).unwrap();
    let current = store.list_observations_for_edge(image).unwrap();
    assert_eq!(current.len(), 1);
    assert_ne!(current, previous);
    std::fs::remove_dir_all(proc.join("42")).unwrap();
    twin_app::scan_in(&home.layout, request, &proc).unwrap();
    assert_ne!(store.get_edge(ownership).unwrap().unwrap().state, "active");
}
