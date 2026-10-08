use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::K8sError;
use crate::model::{
    ClusterView, EndpointView, EventView, IngressView, NamedResource, OwnerRef, PodView,
    ServiceView, WorkloadView,
};

pub struct FixtureK8s {
    view: ClusterView,
}

impl FixtureK8s {
    pub fn open(root: &Path) -> Result<Self, K8sError> {
        if !root.is_dir() {
            return Err(K8sError::MissingFixture {
                path: root.to_path_buf(),
            });
        }
        let mut view = ClusterView::default();
        let entries = fs::read_dir(root).map_err(|source| K8sError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| K8sError::Read {
                path: root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if path.is_file() && is_document(&path) {
                paths.push(path);
            }
        }
        paths.sort();
        for path in paths {
            read_document(&path, &mut view);
        }
        Ok(Self { view })
    }

    pub fn warnings(&self) -> &[String] {
        &self.view.warnings
    }

    pub fn view(&self) -> &ClusterView {
        &self.view
    }
}

fn is_document(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "yaml" | "yml")
    )
}

fn read_document(path: &Path, view: &mut ClusterView) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            view.warnings
                .push(format!("{}: unreadable document", path.display()));
            return;
        }
    };
    if text.trim().is_empty() {
        view.warnings
            .push(format!("{}: malformed document", path.display()));
        return;
    }
    let extension = path.extension().and_then(|ext| ext.to_str());
    if extension == Some("json") {
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => ingest(path, &value, None, view),
            Err(_) => view
                .warnings
                .push(format!("{}: malformed document", path.display())),
        }
        return;
    }
    match serde_yaml::from_str::<serde_yaml::Value>(&text) {
        Ok(yaml) => match serde_json::to_value(yaml) {
            Ok(value) => ingest(path, &value, None, view),
            Err(_) => view
                .warnings
                .push(format!("{}: malformed document", path.display())),
        },
        Err(_) => view
            .warnings
            .push(format!("{}: malformed document", path.display())),
    }
}

fn ingest(path: &Path, value: &Value, hint: Option<&str>, view: &mut ClusterView) {
    if let Some(items) = value.as_array() {
        for item in items {
            ingest(path, item, hint, view);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        view.warnings
            .push(format!("{}: malformed document", path.display()));
        return;
    };
    let declared = object
        .get("kind")
        .and_then(|kind| kind.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty());
    if let Some(kind) = declared {
        if let Some(item_kind) = kind.strip_suffix("List") {
            let Some(items) = object.get("items").and_then(|items| items.as_array()) else {
                view.warnings
                    .push(format!("{}: malformed document", path.display()));
                return;
            };
            let hint = if item_kind.is_empty() {
                None
            } else {
                Some(item_kind)
            };
            for item in items {
                ingest(path, item, hint, view);
            }
            return;
        }
    }
    let kind = declared.or(hint);
    let Some(kind) = kind else {
        view.warnings
            .push(format!("{}: skipped an item with no kind", path.display()));
        return;
    };
    match kind {
        "Namespace" => push_namespace(path, value, view),
        "Pod" => push_pod(path, value, view),
        "Deployment" => push_workload(path, value, view, true),
        "ReplicaSet" => push_workload(path, value, view, false),
        "Service" => push_service(path, value, view),
        "Endpoints" => push_endpoints(path, value, view),
        "EndpointSlice" => push_endpoint_slice(path, value, view),
        "Ingress" => push_ingress(path, value, view),
        "ConfigMap" => push_named(path, value, view, NamedSlot::ConfigMap),
        "Secret" => push_named(path, value, view, NamedSlot::SecretRef),
        "PersistentVolumeClaim" => push_named(path, value, view, NamedSlot::Pvc),
        "Event" => push_event(path, value, view),
        _ => view.warnings.push(format!(
            "{}: skipped unsupported kind {}",
            path.display(),
            bounded_kind(kind)
        )),
    }
}

fn bounded_kind(kind: &str) -> String {
    if kind.len() <= 64 && kind.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        kind.to_string()
    } else {
        "unrecognized".to_string()
    }
}

fn push_namespace(path: &Path, value: &Value, view: &mut ClusterView) {
    match meta_name(value) {
        Some((_, name)) => view.namespaces.push(name),
        None => skip_unnamed(path, view),
    }
}

fn push_pod(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some(pod) = pod_from(value) else {
        skip_unnamed(path, view);
        return;
    };
    view.pods.push(pod);
}

fn push_workload(path: &Path, value: &Value, view: &mut ClusterView, deployment: bool) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let workload = WorkloadView {
        namespace,
        name,
        owners: owners(value),
    };
    if deployment {
        view.deployments.push(workload);
    } else {
        view.replicasets.push(workload);
    }
}

fn push_service(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let selector = value
        .get("spec")
        .and_then(|spec| spec.get("selector"))
        .map(string_map)
        .unwrap_or_default();
    view.services.push(ServiceView {
        namespace,
        name,
        selector,
    });
}

fn push_endpoints(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let mut ready_pods = BTreeSet::new();
    if let Some(subsets) = value.get("subsets").and_then(|item| item.as_array()) {
        for subset in subsets {
            collect_pod_refs(subset.get("addresses"), &mut ready_pods);
        }
    }
    view.endpoints.push(EndpointView {
        namespace,
        service_name: name.clone(),
        name,
        ready_pods: ready_pods.into_iter().collect(),
    });
}

fn push_endpoint_slice(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let Some(service_name) = value
        .get("metadata")
        .and_then(|meta| meta.get("labels"))
        .and_then(|labels| labels.get("kubernetes.io/service-name"))
        .and_then(|item| item.as_str())
        .map(str::to_string)
        .filter(|service| is_dns_label(service))
    else {
        view.warnings.push(format!(
            "{}: skipped endpoint slice without a service name",
            path.display()
        ));
        return;
    };
    let mut ready_pods = BTreeSet::new();
    if let Some(endpoints) = value.get("endpoints").and_then(|item| item.as_array()) {
        for endpoint in endpoints {
            let ready = endpoint
                .get("conditions")
                .and_then(|conditions| conditions.get("ready"))
                .and_then(|flag| flag.as_bool())
                .unwrap_or(true);
            if !ready {
                continue;
            }
            if let Some(pod) = target_pod(endpoint.get("targetRef")) {
                ready_pods.insert(pod);
            }
        }
    }
    view.endpoints.push(EndpointView {
        namespace,
        name,
        service_name,
        ready_pods: ready_pods.into_iter().collect(),
    });
}

fn push_ingress(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let mut service_names = BTreeSet::new();
    if let Some(spec) = value.get("spec") {
        collect_service_name(spec.get("backend"), &mut service_names);
        collect_service_name(spec.get("defaultBackend"), &mut service_names);
        if let Some(rules) = spec.get("rules").and_then(|item| item.as_array()) {
            for rule in rules {
                let Some(paths) = rule
                    .get("http")
                    .and_then(|http| http.get("paths"))
                    .and_then(|item| item.as_array())
                else {
                    continue;
                };
                for path_item in paths {
                    collect_service_name(path_item.get("backend"), &mut service_names);
                }
            }
        }
    }
    view.ingresses.push(IngressView {
        namespace,
        name,
        service_names: service_names.into_iter().collect(),
    });
}

enum NamedSlot {
    ConfigMap,
    SecretRef,
    Pvc,
}

fn push_named(path: &Path, value: &Value, view: &mut ClusterView, slot: NamedSlot) {
    let Some(resource) = named_from(value) else {
        skip_unnamed(path, view);
        return;
    };
    match slot {
        NamedSlot::ConfigMap => view.configmaps.push(resource),
        NamedSlot::SecretRef => view.secret_refs.push(resource),
        NamedSlot::Pvc => view.pvcs.push(resource),
    }
}

fn push_event(path: &Path, value: &Value, view: &mut ClusterView) {
    let Some((namespace, name)) = meta_name(value) else {
        skip_unnamed(path, view);
        return;
    };
    let reason = value
        .get("reason")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    let involved = value.get("involvedObject");
    let involved_kind = involved
        .and_then(|item| item.get("kind"))
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    let involved_name = involved
        .and_then(|item| item.get("name"))
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    view.events.push(EventView {
        namespace,
        name,
        reason,
        involved_kind,
        involved_name,
    });
}

fn skip_unnamed(path: &Path, view: &mut ClusterView) {
    view.warnings
        .push(format!("{}: skipped an item with no name", path.display()));
}

fn named_from(value: &Value) -> Option<NamedResource> {
    let (namespace, name) = meta_name(value)?;
    Some(NamedResource { namespace, name })
}

fn pod_from(value: &Value) -> Option<PodView> {
    let (namespace, name) = meta_name(value)?;
    let labels = value
        .get("metadata")
        .and_then(|meta| meta.get("labels"))
        .map(string_map)
        .unwrap_or_default();
    let spec = value.get("spec");
    let mut images = BTreeSet::new();
    let mut config_map_names = BTreeSet::new();
    let mut secret_names = BTreeSet::new();
    let mut pvc_names = BTreeSet::new();
    if let Some(containers) = spec
        .and_then(|item| item.get("containers"))
        .and_then(|item| item.as_array())
    {
        for container in containers {
            if let Some(image) = container
                .get("image")
                .and_then(|item| item.as_str())
                .filter(|image| !image.is_empty() && image.len() <= 256)
            {
                images.insert(image.to_string());
            }
            collect_env_refs(
                container.get("env"),
                &mut config_map_names,
                &mut secret_names,
            );
            collect_env_from(
                container.get("envFrom"),
                &mut config_map_names,
                &mut secret_names,
            );
        }
    }
    if let Some(volumes) = spec
        .and_then(|item| item.get("volumes"))
        .and_then(|item| item.as_array())
    {
        for volume in volumes {
            if let Some(config_name) = volume
                .get("configMap")
                .and_then(|item| item.get("name"))
                .and_then(|item| item.as_str())
            {
                insert_dns(&mut config_map_names, config_name);
            }
            if let Some(secret_name) = volume
                .get("secret")
                .and_then(|item| item.get("secretName"))
                .and_then(|item| item.as_str())
            {
                insert_dns(&mut secret_names, secret_name);
            }
            if let Some(claim) = volume
                .get("persistentVolumeClaim")
                .and_then(|item| item.get("claimName"))
                .and_then(|item| item.as_str())
            {
                insert_dns(&mut pvc_names, claim);
            }
        }
    }
    Some(PodView {
        namespace,
        name,
        labels,
        owners: owners(value),
        images: images.into_iter().collect(),
        config_map_names: config_map_names.into_iter().collect(),
        secret_names: secret_names.into_iter().collect(),
        pvc_names: pvc_names.into_iter().collect(),
        ready: pod_ready(value.get("status")),
    })
}

fn collect_env_refs(
    env: Option<&Value>,
    config_map_names: &mut BTreeSet<String>,
    secret_names: &mut BTreeSet<String>,
) {
    let Some(items) = env.and_then(|item| item.as_array()) else {
        return;
    };
    for item in items {
        let Some(from) = item.get("valueFrom") else {
            continue;
        };
        if let Some(name) = from
            .get("configMapKeyRef")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
        {
            insert_dns(config_map_names, name);
        }
        if let Some(name) = from
            .get("secretKeyRef")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
        {
            insert_dns(secret_names, name);
        }
    }
}

fn collect_env_from(
    env_from: Option<&Value>,
    config_map_names: &mut BTreeSet<String>,
    secret_names: &mut BTreeSet<String>,
) {
    let Some(items) = env_from.and_then(|item| item.as_array()) else {
        return;
    };
    for item in items {
        if let Some(name) = item
            .get("configMapRef")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
        {
            insert_dns(config_map_names, name);
        }
        if let Some(name) = item
            .get("secretRef")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
        {
            insert_dns(secret_names, name);
        }
    }
}

fn collect_pod_refs(addresses: Option<&Value>, ready_pods: &mut BTreeSet<String>) {
    let Some(items) = addresses.and_then(|item| item.as_array()) else {
        return;
    };
    for address in items {
        if let Some(pod) = target_pod(address.get("targetRef")) {
            ready_pods.insert(pod);
        }
    }
}

fn target_pod(target: Option<&Value>) -> Option<String> {
    let target = target?;
    let kind = target.get("kind").and_then(|item| item.as_str());
    if kind.is_some_and(|kind| kind != "Pod") {
        return None;
    }
    let name = target.get("name").and_then(|item| item.as_str())?;
    if is_dns_label(name) {
        Some(name.to_string())
    } else {
        None
    }
}

fn collect_service_name(backend: Option<&Value>, names: &mut BTreeSet<String>) {
    let Some(backend) = backend else {
        return;
    };
    if let Some(name) = backend
        .get("service")
        .and_then(|service| service.get("name"))
        .and_then(|item| item.as_str())
    {
        insert_dns(names, name);
    }
    if let Some(name) = backend.get("serviceName").and_then(|item| item.as_str()) {
        insert_dns(names, name);
    }
}

fn owners(value: &Value) -> Vec<OwnerRef> {
    let Some(items) = value
        .get("metadata")
        .and_then(|meta| meta.get("ownerReferences"))
        .and_then(|item| item.as_array())
    else {
        return Vec::new();
    };
    let mut owners = Vec::new();
    for item in items {
        let Some(kind) = item.get("kind").and_then(|kind| kind.as_str()) else {
            continue;
        };
        let Some(name) = item.get("name").and_then(|name| name.as_str()) else {
            continue;
        };
        if !is_dns_label(name) || kind.is_empty() {
            continue;
        }
        owners.push(OwnerRef {
            kind: kind.to_string(),
            name: name.to_string(),
        });
    }
    owners
}

fn pod_ready(status: Option<&Value>) -> bool {
    let Some(status) = status else {
        return false;
    };
    if let Some(conditions) = status.get("conditions").and_then(|item| item.as_array()) {
        for condition in conditions {
            if condition.get("type").and_then(|item| item.as_str()) == Some("Ready") {
                return condition.get("status").and_then(|item| item.as_str()) == Some("True");
            }
        }
    }
    let Some(statuses) = status
        .get("containerStatuses")
        .and_then(|item| item.as_array())
    else {
        return false;
    };
    !statuses.is_empty()
        && statuses.iter().all(|item| {
            item.get("ready")
                .and_then(|flag| flag.as_bool())
                .unwrap_or(false)
        })
}

fn meta_name(value: &Value) -> Option<(String, String)> {
    let meta = value.get("metadata")?;
    let name = meta.get("name").and_then(|item| item.as_str())?;
    if !is_dns_label(name) {
        return None;
    }
    let namespace = meta
        .get("namespace")
        .and_then(|item| item.as_str())
        .filter(|namespace| is_dns_label(namespace))
        .unwrap_or("default");
    Some((namespace.to_string(), name.to_string()))
}

fn string_map(value: &Value) -> BTreeMap<String, String> {
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, item)| {
            let text = item.as_str()?;
            if is_label_token(key) && is_label_token(text) {
                Some((key.clone(), text.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn insert_dns(names: &mut BTreeSet<String>, name: &str) {
    if is_dns_label(name) {
        names.insert(name.to_string());
    }
}

fn is_dns_label(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if (!first.is_ascii_lowercase() && !first.is_ascii_digit()) || name.len() > 253 {
        return false;
    }
    let mut previous_mark = false;
    let mut last_alnum = true;
    for ch in chars {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            previous_mark = false;
            last_alnum = true;
            continue;
        }
        if matches!(ch, '-' | '.') {
            if previous_mark {
                return false;
            }
            previous_mark = true;
            last_alnum = false;
            continue;
        }
        return false;
    }
    last_alnum
}

fn is_label_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
}
