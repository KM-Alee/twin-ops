use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use twin_core::{EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_k8s::{kubeconfig_check, open_source, ClusterView, OpenSource, ServiceView};
use twin_store::{Store, StoreError};

use crate::error::{AppError, K8sCmdError};
use crate::model::K8sScanResult;
use crate::paths::{resolve_command_paths, TwinLayout};

#[derive(Debug, Clone, Default)]
pub struct K8sScanRequest {
    pub config_override: Option<std::path::PathBuf>,
}

pub fn run_home(request: K8sScanRequest) -> Result<K8sScanResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request)
}

pub fn run(layout: &TwinLayout, request: &K8sScanRequest) -> Result<K8sScanResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    let kubeconfig = kubeconfig_check();
    match open_source() {
        OpenSource::Gap { detail } => {
            Ok(gap_result(&kubeconfig, &gap_detail(&kubeconfig, &detail)))
        }
        OpenSource::Fixture(fixture) => {
            let view = fixture.view().clone();
            let warnings = view.warnings.clone();
            persist(layout, &view)?;
            Ok(K8sScanResult {
                source: "fixture".to_string(),
                kubeconfig_present: kubeconfig.present,
                kubeconfig_path: kubeconfig.path,
                coverage_gap: None,
                warnings,
                namespaces: view.namespaces.len(),
                pods: view.pods.len(),
                deployments: view.deployments.len(),
                replicasets: view.replicasets.len(),
                services: view.services.len(),
                endpoints: view.endpoints.len(),
                ingresses: view.ingresses.len(),
                configmaps: view.configmaps.len(),
                secret_refs: view.secret_refs.len(),
                pvcs: view.pvcs.len(),
                events: view.events.len(),
            })
        }
    }
}

fn gap_detail(kubeconfig: &twin_k8s::KubeconfigCheck, detail: &str) -> String {
    if kubeconfig.present {
        format!(
            "{detail}; kubeconfig found at {} and no live read client is built",
            kubeconfig.path
        )
    } else {
        format!("{detail}; kubeconfig not found at {}", kubeconfig.path)
    }
}

fn gap_result(kubeconfig: &twin_k8s::KubeconfigCheck, detail: &str) -> K8sScanResult {
    K8sScanResult {
        source: "unavailable".to_string(),
        kubeconfig_present: kubeconfig.present,
        kubeconfig_path: kubeconfig.path.clone(),
        coverage_gap: Some(detail.to_string()),
        warnings: vec![detail.to_string()],
        namespaces: 0,
        pods: 0,
        deployments: 0,
        replicasets: 0,
        services: 0,
        endpoints: 0,
        ingresses: 0,
        configmaps: 0,
        secret_refs: 0,
        pvcs: 0,
        events: 0,
    }
}

fn persist(layout: &TwinLayout, view: &ClusterView) -> Result<(), AppError> {
    let db_path = layout.db_file();
    if !db_path.exists() {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    let mut store = Store::open(&db_path).map_err(K8sCmdError::StoreOpen)?;
    if !store.is_initialized().map_err(K8sCmdError::Store)? {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    record(&mut store, view).map_err(K8sCmdError::Store)?;
    Ok(())
}

fn record(store: &mut Store, view: &ClusterView) -> Result<(), StoreError> {
    let seen = TimestampNs::now();
    for name in &view.namespaces {
        upsert_node(
            store,
            NodeId::k8s_namespace(name),
            name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for deployment in &view.deployments {
        upsert_node(
            store,
            NodeId::k8s_deployment(&deployment.namespace, &deployment.name),
            &deployment.name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for replicaset in &view.replicasets {
        upsert_node(
            store,
            NodeId::k8s_replicaset(&replicaset.namespace, &replicaset.name),
            &replicaset.name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for pod in &view.pods {
        let metadata = serde_json::json!({
            "source": "k8s",
            "ready": pod.ready,
        })
        .to_string();
        upsert_node(
            store,
            NodeId::k8s_pod(&pod.namespace, &pod.name),
            &pod.name,
            seen,
            &metadata,
        )?;
        for image in &pod.images {
            upsert_image(store, image, seen)?;
        }
        for name in &pod.config_map_names {
            upsert_node(
                store,
                NodeId::k8s_configmap(&pod.namespace, name),
                name,
                seen,
                r#"{"source":"k8s"}"#,
            )?;
        }
        for name in &pod.secret_names {
            upsert_node(
                store,
                NodeId::k8s_secret_ref(&pod.namespace, name),
                name,
                seen,
                r#"{"source":"k8s","ref":"name"}"#,
            )?;
        }
        for name in &pod.pvc_names {
            upsert_node(
                store,
                NodeId::k8s_pvc(&pod.namespace, name),
                name,
                seen,
                r#"{"source":"k8s"}"#,
            )?;
        }
    }
    for service in &view.services {
        let ready = ready_pod_names(view, service);
        let metadata = serde_json::json!({
            "source": "k8s",
            "ready_pods": ready,
        })
        .to_string();
        upsert_node(
            store,
            NodeId::k8s_service(&service.namespace, &service.name),
            &service.name,
            seen,
            &metadata,
        )?;
    }
    for endpoint in &view.endpoints {
        let metadata = serde_json::json!({
            "source": "k8s",
            "service": endpoint.service_name,
            "ready_pods": endpoint.ready_pods,
        })
        .to_string();
        upsert_node(
            store,
            NodeId::k8s_endpoints(&endpoint.namespace, &endpoint.name),
            &endpoint.name,
            seen,
            &metadata,
        )?;
    }
    for ingress in &view.ingresses {
        upsert_node(
            store,
            NodeId::k8s_ingress(&ingress.namespace, &ingress.name),
            &ingress.name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
        for service_name in &ingress.service_names {
            let service_id = NodeId::k8s_service(&ingress.namespace, service_name);
            if store.get_node_typed(&service_id)?.is_none() {
                upsert_node(store, service_id, service_name, seen, r#"{"source":"k8s"}"#)?;
            }
        }
    }
    for configmap in &view.configmaps {
        upsert_node(
            store,
            NodeId::k8s_configmap(&configmap.namespace, &configmap.name),
            &configmap.name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for secret in &view.secret_refs {
        upsert_node(
            store,
            NodeId::k8s_secret_ref(&secret.namespace, &secret.name),
            &secret.name,
            seen,
            r#"{"source":"k8s","ref":"name"}"#,
        )?;
    }
    for pvc in &view.pvcs {
        upsert_node(
            store,
            NodeId::k8s_pvc(&pvc.namespace, &pvc.name),
            &pvc.name,
            seen,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for event in &view.events {
        let metadata = serde_json::json!({
            "source": "k8s",
            "reason": event.reason,
            "involved_kind": event.involved_kind,
            "involved_name": event.involved_name,
        })
        .to_string();
        upsert_node(
            store,
            NodeId::k8s_event(&event.namespace, &event.name),
            &event.name,
            seen,
            &metadata,
        )?;
    }
    link_edges(store, view, seen)
}

fn link_edges(store: &mut Store, view: &ClusterView, seen: TimestampNs) -> Result<(), StoreError> {
    for replicaset in &view.replicasets {
        let child = NodeId::k8s_replicaset(&replicaset.namespace, &replicaset.name);
        for owner in &replicaset.owners {
            if owner.kind != "Deployment" {
                continue;
            }
            let parent = NodeId::k8s_deployment(&replicaset.namespace, &owner.name);
            if store.get_node_typed(&parent)?.is_none() {
                upsert_node(
                    store,
                    parent.clone(),
                    &owner.name,
                    seen,
                    r#"{"source":"k8s"}"#,
                )?;
            }
            upsert_edge(store, &parent, EdgeKind::Owns, &child, seen)?;
        }
    }
    for pod in &view.pods {
        let pod_id = NodeId::k8s_pod(&pod.namespace, &pod.name);
        for owner in &pod.owners {
            if owner.kind != "ReplicaSet" {
                continue;
            }
            let parent = NodeId::k8s_replicaset(&pod.namespace, &owner.name);
            if store.get_node_typed(&parent)?.is_none() {
                upsert_node(
                    store,
                    parent.clone(),
                    &owner.name,
                    seen,
                    r#"{"source":"k8s"}"#,
                )?;
            }
            upsert_edge(store, &parent, EdgeKind::Owns, &pod_id, seen)?;
        }
        for image in &pod.images {
            let image_id = NodeId::image(image);
            if NodeId::from_str(image_id.as_str()).is_err() {
                continue;
            }
            upsert_edge(store, &pod_id, EdgeKind::RunsImage, &image_id, seen)?;
        }
        for name in &pod.config_map_names {
            let target = NodeId::k8s_configmap(&pod.namespace, name);
            upsert_edge(store, &pod_id, EdgeKind::UsesConfigMap, &target, seen)?;
        }
        for name in &pod.secret_names {
            let target = NodeId::k8s_secret_ref(&pod.namespace, name);
            upsert_edge(store, &pod_id, EdgeKind::UsesSecretRef, &target, seen)?;
        }
        for name in &pod.pvc_names {
            let target = NodeId::k8s_pvc(&pod.namespace, name);
            upsert_edge(store, &pod_id, EdgeKind::UsesPvc, &target, seen)?;
        }
    }
    for service in &view.services {
        let service_id = NodeId::k8s_service(&service.namespace, &service.name);
        for pod in &view.pods {
            if pod.namespace != service.namespace
                || !selector_matches(&service.selector, &pod.labels)
            {
                continue;
            }
            let pod_id = NodeId::k8s_pod(&pod.namespace, &pod.name);
            upsert_edge(store, &service_id, EdgeKind::Selects, &pod_id, seen)?;
        }
    }
    for ingress in &view.ingresses {
        let ingress_id = NodeId::k8s_ingress(&ingress.namespace, &ingress.name);
        for service_name in &ingress.service_names {
            let service_id = NodeId::k8s_service(&ingress.namespace, service_name);
            upsert_edge(store, &ingress_id, EdgeKind::RoutesTo, &service_id, seen)?;
        }
    }
    Ok(())
}

fn ready_pod_names(view: &ClusterView, service: &ServiceView) -> Vec<String> {
    let matched: Vec<_> = view
        .endpoints
        .iter()
        .filter(|endpoint| {
            endpoint.namespace == service.namespace && endpoint.service_name == service.name
        })
        .collect();
    if !matched.is_empty() {
        let mut names = BTreeSet::new();
        for endpoint in matched {
            names.extend(endpoint.ready_pods.iter().cloned());
        }
        return names.into_iter().collect();
    }
    view.pods
        .iter()
        .filter(|pod| {
            pod.namespace == service.namespace
                && pod.ready
                && selector_matches(&service.selector, &pod.labels)
        })
        .map(|pod| pod.name.clone())
        .collect()
}

fn selector_matches(
    selector: &BTreeMap<String, String>,
    labels: &BTreeMap<String, String>,
) -> bool {
    !selector.is_empty()
        && selector
            .iter()
            .all(|(key, value)| labels.get(key).is_some_and(|label| label == value))
}

fn upsert_image(store: &mut Store, reference: &str, seen: TimestampNs) -> Result<(), StoreError> {
    let id = NodeId::image(reference);
    if NodeId::from_str(id.as_str()).is_err() {
        return Ok(());
    }
    let existing = store.get_node_typed(&id)?;
    let node = GraphNode::image_sourced(reference, seen, existing.as_ref(), "k8s");
    store.upsert_node_typed(&node)
}

fn upsert_node(
    store: &mut Store,
    id: NodeId,
    label: &str,
    seen: TimestampNs,
    metadata: &str,
) -> Result<(), StoreError> {
    if NodeId::from_str(id.as_str()).is_err() {
        return Ok(());
    }
    let Some(kind) = id.kind() else {
        return Ok(());
    };
    let existing = store.get_node_typed(&id)?;
    let node = GraphNode::k8s(id, kind, label, seen, existing.as_ref(), metadata);
    store.upsert_node_typed(&node)
}

fn upsert_edge(
    store: &mut Store,
    from: &NodeId,
    kind: EdgeKind,
    to: &NodeId,
    seen: TimestampNs,
) -> Result<(), StoreError> {
    if NodeId::from_str(from.as_str()).is_err() || NodeId::from_str(to.as_str()).is_err() {
        return Ok(());
    }
    let id = EdgeId::new(from, kind, to);
    let existing = store
        .get_edge(id.as_str())?
        .as_ref()
        .and_then(|row| GraphEdge::try_from(row).ok());
    let edge = GraphEdge::observed_k8s(from, kind, to, seen, existing.as_ref());
    store.upsert_edge_typed(&edge)
}
