mod support;

use twin_app::{
    EmulateActionRequest, EmulateRequest, InitRequest, K8sScanRequest, K8sTargetRequest,
};
use twin_core::{NodeId, NodeState, RiskLevel};
use twin_store::Store;

use support::{clear_scan_env, lock_scan_env, IsolatedHome};

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
    assert_eq!(removed.risk.level, RiskLevel::Low);
    assert_eq!(
        removed.risk.reasons,
        vec!["service still has 1 ready endpoint.".to_string()]
    );
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
