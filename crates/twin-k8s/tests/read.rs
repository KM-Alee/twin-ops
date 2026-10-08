use twin_k8s::{FixtureK8s, K8sReadOnly};

const SECRET_DATA: &str = "SUPERSECRETVALUE_NOT_STORED";
const SECRET_STRING: &str = "SUPERSECRETSTRING_NOT_STORED";

fn write_api_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root).expect("dir");
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
"#,
    )
    .expect("pods");
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
    std::fs::write(root.join("broken.yaml"), "kind: [\n").expect("broken");
    std::fs::write(
        root.join("services.json"),
        r#"{"kind":"Service","metadata":{"name":"api","namespace":"default"},"spec":{"selector":{"app":"api"}}}"#,
    )
    .expect("service");
}

#[test]
fn fixture_lists_refs_and_drops_secret_bytes() {
    let root = tempfile::tempdir().expect("temp");
    write_api_fixture(root.path());
    let fixture = FixtureK8s::open(root.path()).expect("open");
    let pods = fixture.list_pods().expect("pods");
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].name, "api-123");
    assert!(pods[0].ready);
    assert_eq!(pods[0].images, vec!["api:1.2".to_string()]);
    assert_eq!(pods[0].secret_names, vec!["api-tls".to_string()]);
    assert_eq!(pods[0].config_map_names, vec!["api-config".to_string()]);
    assert_eq!(pods[0].pvc_names, vec!["api-data".to_string()]);
    let secret = fixture
        .get_secret_ref("default", "api-tls")
        .expect("get")
        .expect("secret");
    assert_eq!(secret.name, "api-tls");
    assert_eq!(secret.namespace, "default");
    let rendered = format!("{:?}", fixture.view());
    assert!(!rendered.contains(SECRET_DATA));
    assert!(!rendered.contains(SECRET_STRING));
    assert!(fixture
        .warnings()
        .iter()
        .any(|warning| warning.contains("broken.yaml") && warning.contains("malformed")));
    let service = fixture
        .get_service("default", "api")
        .expect("service")
        .expect("present");
    assert_eq!(service.selector.get("app").map(String::as_str), Some("api"));
}

#[test]
fn missing_directory_is_an_error() {
    let opened = FixtureK8s::open(std::path::Path::new("/tmp/twin-k8s-missing-fixture-d828"));
    let err = match opened {
        Err(err) => err,
        Ok(_) => panic!("expected a missing directory"),
    };
    assert!(err.to_string().contains("missing"));
}
