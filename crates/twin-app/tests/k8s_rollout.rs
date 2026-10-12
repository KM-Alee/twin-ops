mod support;

use twin_app::{EmulateActionRequest, EmulateRequest, InitRequest, K8sScanRequest};
use twin_core::{NodeId, RiskLevel};
use twin_store::Store;

fn fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root).expect("directory");
    std::fs::write(
        root.join("resources.yaml"),
        r#"
kind: List
items:
- kind: ReplicaSet
  metadata:
    name: api-rs
    namespace: default
    ownerReferences:
    - kind: Deployment
      name: api
- kind: Pod
  metadata:
    name: api-1
    namespace: default
    labels: {app: api}
    ownerReferences:
    - kind: ReplicaSet
      name: api-rs
  status:
    conditions: [{type: Ready, status: "True"}]
- kind: Pod
  metadata:
    name: api-2
    namespace: default
    labels: {app: api}
    ownerReferences:
    - kind: ReplicaSet
      name: api-rs
  status:
    conditions: [{type: Ready, status: "True"}]
- kind: Service
  metadata: {name: api, namespace: default}
  spec:
    selector: {app: api}
"#,
    )
    .expect("resources");
    std::fs::write(
        root.join("endpoints.yaml"),
        r#"
kind: EndpointSlice
metadata:
  name: api-slice
  namespace: default
  labels: {kubernetes.io/service-name: api}
endpoints:
- conditions: {ready: true}
  targetRef: {kind: Pod, name: api-1}
- conditions: {ready: true}
  targetRef: {kind: Pod, name: api-2}
"#,
    )
    .expect("endpoints");
}

fn scan(home: &support::IsolatedHome, root: &std::path::Path, strategy: &str) {
    std::fs::write(root.join("deployment.yaml"), format!("kind: Deployment\nmetadata: {{name: api, namespace: default}}\nspec:\n  replicas: 2\n{strategy}\n")).expect("deployment");
    twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(root.into()),
            config_override: None,
        },
    )
    .expect("scan");
}

fn rollout(home: &support::IsolatedHome) -> twin_app::EmulationResult {
    twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::RolloutK8sDeployment {
                target: NodeId::k8s_deployment("default", "api"),
            },
            show_evidence: true,
            ..EmulateRequest::default()
        },
    )
    .expect("rollout")
}

#[test]
fn persisted_policy_and_ready_endpoints_determine_rollout_availability() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = home.layout.data_dir.join("fixture");
    fixture(&root);
    scan(&home, &root, "");
    let protected = rollout(&home);
    assert_eq!(protected.risk.level, RiskLevel::Low);
    assert!(protected.overlay.unavailable_nodes.is_empty());
    assert!(protected.transient_impacts[0]
        .statement
        .contains("at least 2"));
    assert!(protected.transient_impacts[0].evidence[0].contains("collector_run:k8s:"));
    scan(&home, &root, "  strategy:\n    type: Recreate");
    let recreate = rollout(&home);
    assert_eq!(recreate.risk.level, RiskLevel::High);
    assert_eq!(recreate.overlay.unavailable_nodes.len(), 2);
    scan(
        &home,
        &root,
        "  strategy:\n    rollingUpdate:\n      maxUnavailable: 1\n      maxSurge: 0",
    );
    assert_eq!(rollout(&home).risk.level, RiskLevel::Medium);
    scan(
        &home,
        &root,
        "  strategy:\n    rollingUpdate:\n      maxUnavailable: 100%\n      maxSurge: 0",
    );
    assert_eq!(rollout(&home).risk.level, RiskLevel::High);
}

#[test]
fn incomplete_endpoint_reads_or_stale_deployment_cannot_claim_rollout_safety() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = home.layout.data_dir.join("fixture");
    fixture(&root);
    scan(&home, &root, "");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let mut deployment = store
        .get_node("k8s:deployment:default/api")
        .expect("read")
        .expect("node");
    deployment.last_seen_ns = 1;
    store.upsert_node(&deployment).expect("stale observation");
    assert_eq!(rollout(&home).risk.level, RiskLevel::Unknown);
    std::fs::remove_file(root.join("endpoints.yaml")).expect("remove fixture file");
    std::fs::write(root.join("broken.yaml"), "kind: [\n").expect("incomplete fixture");
    scan(&home, &root, "");
    let unknown = rollout(&home);
    assert_eq!(unknown.risk.level, RiskLevel::Unknown);
    assert!(unknown
        .unknowns
        .iter()
        .any(|gap| gap.detail.contains("endpoint coverage")));
    let removed = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteK8sPod {
                target: NodeId::k8s_pod("default", "api-1"),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("delete");
    assert_eq!(removed.risk.level, RiskLevel::Unknown);
    assert!(!removed.action_performed);
}
