mod support;

use std::str::FromStr;

use twin_app::{
    EmulateActionRequest, EmulateRequest, InitRequest, K8sScanRequest, K8sTargetRequest,
    ScanRequest,
};
use twin_core::{NodeId, NodeState, RiskLevel};
use twin_store::Store;

use support::{clear_scan_env, lock_scan_env, write_proc_fixture, IsolatedHome};

const SECRET_DATA: &str = "SUPERSECRETVALUE_NOT_STORED";
const SECRET_STRING: &str = "SUPERSECRETSTRING_NOT_STORED";
const EVENT_BODY: &str = "EVENT_BODY_NOT_STORED";
const CONFIG_BODY: &str = "CONFIGMAP_BODY_NOT_STORED";

struct ClearOnDrop;

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        clear_scan_env();
    }
}

fn set_fixture(root: &std::path::Path) {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::set_var("TWIN_K8S_FIXTURE", root);
    }
}

fn set_docker_fixture(root: &std::path::Path) {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::set_var("TWIN_DOCKER_FIXTURE", root);
    }
}

fn write_api_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root).expect("dir");
    std::fs::write(
        root.join("namespaces.yaml"),
        "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: default\n",
    )
    .expect("ns");
    std::fs::write(
        root.join("deployments.yaml"),
        r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api
  namespace: default
spec:
  replicas: 2
  selector:
    matchLabels:
      app: api
"#,
    )
    .expect("deploy");
    std::fs::write(
        root.join("replicasets.yaml"),
        r#"
apiVersion: apps/v1
kind: ReplicaSet
metadata:
  name: api-abc
  namespace: default
  ownerReferences:
  - kind: Deployment
    name: api
"#,
    )
    .expect("rs");
    std::fs::write(
        root.join("pods.yaml"),
        r#"
apiVersion: v1
kind: PodList
items:
- metadata:
    name: api-123
    namespace: default
    labels:
      app: api
    ownerReferences:
    - kind: ReplicaSet
      name: api-abc
  spec:
    containers:
    - name: api
      image: api:1.2
      env:
      - name: TOKEN
        valueFrom:
          secretKeyRef:
            name: api-tls
            key: token
    volumes:
    - name: cfg
      configMap:
        name: api-config
    - name: tls
      secret:
        secretName: api-tls
    - name: data
      persistentVolumeClaim:
        claimName: api-data
  status:
    conditions:
    - type: Ready
      status: "True"
- metadata:
    name: api-124
    namespace: default
    labels:
      app: api
    ownerReferences:
    - kind: ReplicaSet
      name: api-abc
  spec:
    containers:
    - name: api
      image: api:1.2
  status:
    conditions:
    - type: Ready
      status: "True"
"#,
    )
    .expect("pods");
    std::fs::write(
        root.join("services.yaml"),
        r#"
apiVersion: v1
kind: Service
metadata:
  name: api
  namespace: default
spec:
  selector:
    app: api
  ports:
  - port: 80
"#,
    )
    .expect("svc");
    std::fs::write(
        root.join("endpoints.yaml"),
        r#"
apiVersion: v1
kind: Endpoints
metadata:
  name: api
  namespace: default
subsets:
- addresses:
  - ip: 10.0.0.1
    targetRef:
      kind: Pod
      name: api-123
  - ip: 10.0.0.2
    targetRef:
      kind: Pod
      name: api-124
"#,
    )
    .expect("ep");
    std::fs::write(
        root.join("ingress.yaml"),
        r#"
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: api
  namespace: default
spec:
  rules:
  - http:
      paths:
      - path: /
        backend:
          service:
            name: api
            port:
              number: 80
"#,
    )
    .expect("ing");
    std::fs::write(
        root.join("configmaps.yaml"),
        format!(
            r#"
apiVersion: v1
kind: ConfigMap
metadata:
  name: api-config
  namespace: default
data:
  app.conf: {CONFIG_BODY}
"#
        ),
    )
    .expect("cm");
    std::fs::write(
        root.join("secrets.yaml"),
        format!(
            r#"
apiVersion: v1
kind: Secret
metadata:
  name: api-tls
  namespace: default
data:
  tls.crt: {SECRET_DATA}
stringData:
  tls.key: {SECRET_STRING}
"#
        ),
    )
    .expect("secret");
    std::fs::write(
        root.join("pvcs.yaml"),
        r#"
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: api-data
  namespace: default
"#,
    )
    .expect("pvc");
    std::fs::write(
        root.join("events.yaml"),
        format!(
            r#"
apiVersion: v1
kind: Event
metadata:
  name: api-123.1
  namespace: default
reason: Started
message: {EVENT_BODY}
involvedObject:
  kind: Pod
  name: api-123
"#
        ),
    )
    .expect("event");
    std::fs::write(root.join("broken.yaml"), "kind: [\n").expect("broken");
}

#[test]
fn scan_graphs_ownership_and_emulation_stays_hypothetical() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let fixture = home.layout.data_dir.join("k8s-fixture");
    write_api_fixture(&fixture);
    let secret_before = std::fs::read(fixture.join("secrets.yaml")).expect("secret bytes");
    set_fixture(&fixture);

    let scan = twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(fixture.clone()),
            config_override: None,
        },
    )
    .expect("scan");
    assert!(scan.coverage_gap.is_none(), "{:?}", scan.coverage_gap);
    assert_eq!(scan.pods, 2);
    assert_eq!(scan.deployments, 1);
    assert!(scan
        .warnings
        .iter()
        .any(|warning| warning.contains("broken.yaml")));

    let store = Store::open(&home.layout.db_file()).expect("open");
    for id in [
        "k8s:namespace:default",
        "k8s:deployment:default/api",
        "k8s:replicaset:default/api-abc",
        "k8s:pod:default/api-123",
        "k8s:pod:default/api-124",
        "k8s:service:default/api",
        "k8s:ingress:default/api",
        "k8s:configmap:default/api-config",
        "k8s:secretref:default/api-tls",
        "k8s:pvc:default/api-data",
        "k8s:endpoints:default/api",
        "k8s:event:default/api-123.1",
        "image:api:1.2",
    ] {
        assert!(store.get_node(id).expect("node").is_some(), "missing {id}");
    }
    for id in [
        "k8s:deployment:default/api|owns|k8s:replicaset:default/api-abc",
        "k8s:replicaset:default/api-abc|owns|k8s:pod:default/api-123",
        "k8s:replicaset:default/api-abc|owns|k8s:pod:default/api-124",
        "k8s:service:default/api|selects|k8s:pod:default/api-123",
        "k8s:service:default/api|selects|k8s:pod:default/api-124",
        "k8s:ingress:default/api|routes_to|k8s:service:default/api",
        "k8s:pod:default/api-123|uses_configmap|k8s:configmap:default/api-config",
        "k8s:pod:default/api-123|uses_secret_ref|k8s:secretref:default/api-tls",
        "k8s:pod:default/api-123|uses_pvc|k8s:pvc:default/api-data",
        "k8s:pod:default/api-123|runs_image|image:api:1.2",
        "k8s:pod:default/api-124|runs_image|image:api:1.2",
    ] {
        assert!(store.get_edge(id).expect("edge").is_some(), "missing {id}");
    }

    let db_bytes = std::fs::read(home.layout.db_file()).expect("db");
    let db_text = String::from_utf8_lossy(&db_bytes);
    assert!(!db_text.contains(SECRET_DATA));
    assert!(!db_text.contains(SECRET_STRING));
    assert!(!db_text.contains(EVENT_BODY));
    assert!(!db_text.contains(CONFIG_BODY));

    let graph = twin_app::k8s_graph_in(
        &home.layout,
        K8sTargetRequest {
            config_override: None,
            target: NodeId::parse_k8s_ref("deployment/default/api").expect("id"),
        },
    )
    .expect("graph");
    assert_eq!(graph.id, "k8s:deployment:default/api");
    assert_eq!(
        graph.owns,
        vec![
            "replicaset:default/api-abc".to_string(),
            "pod:default/api-123".to_string(),
            "pod:default/api-124".to_string(),
        ]
    );
    assert_eq!(
        graph.routed_by,
        vec![
            "service:default/api".to_string(),
            "ingress:default/api".to_string(),
        ]
    );

    let impact = twin_app::k8s_impact_in(
        &home.layout,
        K8sTargetRequest {
            config_override: None,
            target: NodeId::parse_k8s_ref("service/default/api").expect("id"),
        },
    )
    .expect("impact");
    assert!(impact.selects.iter().any(|id| id == "pod:default/api-123"));
    assert!(impact.selects.iter().any(|id| id == "pod:default/api-124"));
    assert!(impact
        .owned_by
        .iter()
        .any(|id| id == "deployment:default/api"));
    assert!(impact
        .routed_by
        .iter()
        .any(|id| id == "ingress:default/api"));

    let edges_before = store.count_edges().expect("edges");
    let nodes_before = store.count_nodes().expect("nodes");
    let removed = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteK8sPod {
                target: NodeId::parse_k8s_ref("pod:default/api-123").expect("pod"),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("delete");
    assert_eq!(removed.action, "delete");
    assert_eq!(removed.target, "k8s:pod:default/api-123");
    assert!(!removed.action_performed);
    assert_eq!(removed.risk.level, RiskLevel::Unknown);
    assert!(removed
        .unknowns
        .iter()
        .any(|gap| gap.detail.contains("endpoint coverage")));
    assert!(removed.safety_statement.contains("No pod was deleted."));

    let rolled = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::RolloutK8sDeployment {
                target: NodeId::parse_k8s_ref("deployment:default/api").expect("deployment"),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("rollout");
    assert_eq!(rolled.action, "rollout");
    assert!(!rolled.action_performed);
    assert!(rolled
        .safety_statement
        .contains("No deployment was rolled out."));
    assert_eq!(rolled.restart_impacts.len(), 2);
    assert!(store
        .get_node("k8s:pod:default/api-123")
        .expect("pod")
        .is_some());
    let pod = store
        .get_node_typed(&NodeId::k8s_pod("default", "api-123"))
        .expect("typed")
        .expect("pod node");
    assert_eq!(pod.state(), NodeState::Active);
    assert_eq!(store.count_edges().expect("edges"), edges_before);
    assert_eq!(store.count_nodes().expect("nodes"), nodes_before);
    assert_eq!(
        std::fs::read(fixture.join("secrets.yaml")).expect("secret after"),
        secret_before
    );
    let doctor = twin_app::doctor_flags_in(&home.layout, None, false, false, true).expect("doctor");
    assert!(doctor.k8s.is_some());
}

#[test]
fn scan_without_fixture_is_a_coverage_gap() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    let scan = twin_app::k8s_scan_in(&home.layout, K8sScanRequest::default()).expect("scan");
    assert!(scan.coverage_gap.is_some(), "expected a coverage gap");
    assert_eq!(scan.pods, 0);
}

#[test]
fn second_scan_marks_vanished_objects_gone_and_records_history() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let fixture = home.layout.data_dir.join("k8s-fixture");
    write_vanishing_fixture(&fixture, true);
    set_fixture(&fixture);
    twin_app::k8s_scan_in(&home.layout, K8sScanRequest::default()).expect("first scan");

    write_vanishing_fixture(&fixture, false);
    twin_app::k8s_scan_in(&home.layout, K8sScanRequest::default()).expect("second scan");

    let store = Store::open(&home.layout.db_file()).expect("open");
    let removed = store
        .get_node_typed(&NodeId::k8s_pod("default", "api-124"))
        .expect("node")
        .expect("pod node");
    assert_eq!(removed.state(), NodeState::Gone);
    let kept = store
        .get_node_typed(&NodeId::k8s_pod("default", "api-123"))
        .expect("node")
        .expect("pod node");
    assert_eq!(kept.state(), NodeState::Active);
    let edge = store
        .get_edge("k8s:replicaset:default/api-abc|owns|k8s:pod:default/api-124")
        .expect("edge")
        .expect("owned edge");
    assert_eq!(edge.state, "gone");

    let graph = twin_app::k8s_graph_in(
        &home.layout,
        K8sTargetRequest {
            config_override: None,
            target: NodeId::parse_k8s_ref("deployment/default/api").expect("id"),
        },
    )
    .expect("graph");
    assert_eq!(
        graph.owns,
        vec![
            "replicaset:default/api-abc".to_string(),
            "pod:default/api-123".to_string(),
        ]
    );

    let history = store.list_node_history_since(0).expect("history");
    assert!(
        history
            .iter()
            .any(|row| row.node_id == "k8s:pod:default/api-124" && row.change_kind == "gone"),
        "expected a gone history entry: {history:?}"
    );
}

fn write_vanishing_fixture(root: &std::path::Path, second_pod: bool) {
    std::fs::create_dir_all(root).expect("dir");
    std::fs::write(
        root.join("namespaces.yaml"),
        "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: default\n",
    )
    .expect("ns");
    std::fs::write(
        root.join("deployments.yaml"),
        "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: api\n  namespace: default\nspec:\n  replicas: 2\n",
    )
    .expect("deploy");
    std::fs::write(
        root.join("replicasets.yaml"),
        "apiVersion: apps/v1\nkind: ReplicaSet\nmetadata:\n  name: api-abc\n  namespace: default\n  ownerReferences:\n  - kind: Deployment\n    name: api\n",
    )
    .expect("rs");
    let second = if second_pod {
        "- metadata:\n    name: api-124\n    namespace: default\n    labels:\n      app: api\n    ownerReferences:\n    - kind: ReplicaSet\n      name: api-abc\n  status:\n    conditions:\n    - type: Ready\n      status: \"True\"\n"
    } else {
        ""
    };
    std::fs::write(
        root.join("pods.yaml"),
        format!(
            "apiVersion: v1\nkind: PodList\nitems:\n- metadata:\n    name: api-123\n    namespace: default\n    labels:\n      app: api\n    ownerReferences:\n    - kind: ReplicaSet\n      name: api-abc\n  status:\n    conditions:\n    - type: Ready\n      status: \"True\"\n{second}"
        ),
    )
    .expect("pods");
    std::fs::write(
        root.join("services.yaml"),
        "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n  namespace: default\nspec:\n  selector:\n    app: api\n",
    )
    .expect("svc");
    let second_address = if second_pod {
        "  - ip: 10.0.0.2\n    targetRef:\n      kind: Pod\n      name: api-124\n"
    } else {
        ""
    };
    std::fs::write(
        root.join("endpoints.yaml"),
        format!(
            "apiVersion: v1\nkind: Endpoints\nmetadata:\n  name: api\n  namespace: default\nsubsets:\n- addresses:\n  - ip: 10.0.0.1\n    targetRef:\n      kind: Pod\n      name: api-123\n{second_address}"
        ),
    )
    .expect("ep");
}

#[test]
fn kubernetes_scan_does_not_reap_docker_rows() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let k8s = home.layout.data_dir.join("k8s-fixture");
    write_api_fixture(&k8s);
    set_fixture(&k8s);
    let docker = home.layout.data_dir.join("docker-fixture");
    write_api_docker_fixture(&docker);
    set_docker_fixture(&docker);
    let proc = home.layout.data_dir.join("fixture-proc-api");
    write_proc_fixture(
        &proc,
        42,
        "42 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/api\0",
        None,
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("host scan");

    let store = Store::open(&home.layout.db_file()).expect("open");
    let container = store
        .get_node_typed(&NodeId::container("api"))
        .expect("node")
        .expect("container");
    assert_eq!(container.state(), NodeState::Active);

    twin_app::k8s_scan_in(&home.layout, K8sScanRequest::default()).expect("k8s scan");

    let store = Store::open(&home.layout.db_file()).expect("reopen");
    let container = store
        .get_node_typed(&NodeId::container("api"))
        .expect("node")
        .expect("container");
    assert_eq!(
        container.state(),
        NodeState::Active,
        "a k8s scan must not reap the docker container"
    );
    let edge = store
        .get_edge("container:api|runs_image|image:api:1.2")
        .expect("edge")
        .expect("runs image edge");
    assert_eq!(
        edge.state, "active",
        "a k8s scan must not reap the docker runs_image edge"
    );
    let image = store
        .get_node_typed(&NodeId::image("api:1.2"))
        .expect("node")
        .expect("image");
    assert_eq!(image.state(), NodeState::Active);
}

fn write_api_docker_fixture(root: &std::path::Path) {
    let containers = root.join("containers");
    let images = root.join("images");
    std::fs::create_dir_all(&containers).expect("containers");
    std::fs::create_dir_all(&images).expect("images");
    std::fs::write(
        containers.join("list.json"),
        r#"[{
            "Id": "apicontainer",
            "Names": ["/api"],
            "Image": "api:1.2",
            "Ports": [],
            "Mounts": []
        }]"#,
    )
    .expect("list");
    std::fs::write(
        containers.join("apicontainer.json"),
        r#"{
            "Id": "apicontainer",
            "Name": "/api",
            "Config": {"Image": "api:1.2"},
            "State": {"Status": "running", "Pid": 42},
            "NetworkSettings": {"Ports": {}},
            "Mounts": []
        }"#,
    )
    .expect("inspect");
    std::fs::write(
        images.join("api:1.2.json"),
        r#"{"Id": "sha256:apicafe", "RepoTags": ["api:1.2"]}"#,
    )
    .expect("image");
}

#[test]
fn host_scan_does_not_stale_kubernetes_graph() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let fixture = home.layout.data_dir.join("k8s-fixture");
    write_api_fixture(&fixture);
    std::fs::remove_file(fixture.join("broken.yaml")).expect("healthy fixture");
    set_fixture(&fixture);
    twin_app::k8s_scan_in(&home.layout, K8sScanRequest::default()).expect("k8s scan");

    let proc = home.layout.data_dir.join("fixture-proc");
    write_proc_fixture(
        &proc,
        42,
        "42 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/api\0",
        None,
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("host scan");

    let store = Store::open(&home.layout.db_file()).expect("open");
    for id in [
        "k8s:deployment:default/api",
        "k8s:replicaset:default/api-abc",
        "k8s:pod:default/api-123",
        "k8s:service:default/api",
        "image:api:1.2",
    ] {
        let node = store
            .get_node_typed(&NodeId::from_str(id).expect("parse"))
            .expect("node")
            .unwrap_or_else(|| panic!("missing node {id}"));
        assert_eq!(node.state(), NodeState::Active, "{id}");
    }
    for id in [
        "k8s:deployment:default/api|owns|k8s:replicaset:default/api-abc",
        "k8s:service:default/api|selects|k8s:pod:default/api-123",
        "k8s:pod:default/api-123|runs_image|image:api:1.2",
    ] {
        let row = store
            .get_edge(id)
            .expect("edge")
            .unwrap_or_else(|| panic!("missing edge {id}"));
        assert_eq!(row.state, "active", "{id}");
    }

    let rolled = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::RolloutK8sDeployment {
                target: NodeId::parse_k8s_ref("deployment:default/api").expect("deployment"),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("rollout");
    assert_eq!(rolled.restart_impacts.len(), 2);

    let removed = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteK8sPod {
                target: NodeId::parse_k8s_ref("pod:default/api-123").expect("pod"),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("delete");
    assert_eq!(removed.risk.level, RiskLevel::Low);
    assert_eq!(
        removed.risk.reasons,
        vec!["service still has 1 ready endpoint.".to_string()]
    );
}

#[test]
fn failed_fixture_read_updates_coverage_without_reaping_resources() {
    let _guard = lock_scan_env();
    clear_scan_env();
    let _clear = ClearOnDrop;
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let fixture = home.layout.db_file().parent().expect("parent").join("k8s");
    write_api_fixture(&fixture);
    let successful = K8sScanRequest {
        fixture: Some(fixture),
        ..K8sScanRequest::default()
    };
    twin_app::k8s_scan_in(&home.layout, successful).expect("scan");
    let missing = K8sScanRequest {
        fixture: Some(home.layout.db_file().with_extension("missing")),
        ..K8sScanRequest::default()
    };
    let result = twin_app::k8s_scan_in(&home.layout, missing).expect("gap");
    assert!(result.coverage_gap.is_some());
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert_eq!(
        store
            .latest_collector_run("k8s")
            .expect("run")
            .expect("present")
            .status,
        "unavailable"
    );
    let id = NodeId::parse_k8s_ref("deployment:default/api").expect("id");
    assert_eq!(
        store
            .get_node_typed(&id)
            .expect("read")
            .expect("node")
            .state(),
        NodeState::Active
    );
    drop(store);
    let graph = twin_app::graph_in(
        &home.layout,
        twin_app::GraphRequest {
            target: Some(NodeId::image("api:1.2")),
            ..twin_app::GraphRequest::default()
        },
    )
    .expect("image graph");
    let twin_app::GraphResult::Node(view) = graph else {
        panic!("neighborhood")
    };
    assert!(view
        .incoming
        .iter()
        .any(|edge| edge.kind == "runs_image" && edge.peer_id == "k8s:pod:default/api-123"));
    let yaml_path = home.layout.db_file().with_extension("yaml");
    std::fs::write(&yaml_path, "version: 1\nname: cluster\nchecks:\n  - name: deployment exists\n    assert:\n      node: k8s:deployment:default/api\n  - name: service exists\n    assert:\n      node: k8s:service:default/api\n").expect("yaml");
    let report = twin_app::run_file(&home.layout, &yaml_path).expect("checks");
    assert_eq!(report.passed, 2);
}
