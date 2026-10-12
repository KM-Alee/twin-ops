use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use twin_core::{EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_k8s::{kubeconfig_check, open_source, ClusterView, OpenSource, ServiceView};
use twin_store::{CollectorRunRow, Store, StoreError};

use crate::commands::scan_history::{FinalizeScope, ScanHistorySession};
use crate::error::{AppError, K8sCmdError};
use crate::model::K8sScanResult;
use crate::paths::{resolve_command_paths, TwinLayout};

#[derive(Debug, Clone, Default)]
pub struct K8sScanRequest {
    pub fixture: Option<std::path::PathBuf>,
    pub config_override: Option<std::path::PathBuf>,
}

pub fn run_home(request: K8sScanRequest) -> Result<K8sScanResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request)
}

pub fn run(layout: &TwinLayout, request: &K8sScanRequest) -> Result<K8sScanResult, AppError> {
    crate::config_io::load(layout, request.config_override.as_deref())?;
    let kubeconfig = kubeconfig_check();
    match open_source(request.fixture.as_deref()) {
        OpenSource::Gap { detail } => {
            let detail = gap_detail(&kubeconfig, &detail);
            persist_gap(layout, &detail)?;
            Ok(gap_result(&kubeconfig, &detail))
        }
        source => {
            let source_name = if matches!(source, OpenSource::Live(_)) {
                "live"
            } else {
                "fixture"
            };
            let Some(view) = twin_k8s::cluster_view(&source) else {
                return Ok(gap_result(&kubeconfig, "source unavailable"));
            };
            scan_view(layout, &kubeconfig, view, source_name)
        }
    }
}

fn scan_view(
    layout: &TwinLayout,
    kubeconfig: &twin_k8s::KubeconfigCheck,
    view: &ClusterView,
    source: &str,
) -> Result<K8sScanResult, AppError> {
    let warnings = view.warnings.clone();
    persist(layout, view)?;
    Ok(K8sScanResult {
        source: source.to_string(),
        kubeconfig_present: kubeconfig.present,
        kubeconfig_path: kubeconfig.path.clone(),
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

pub(super) fn persist_gap(layout: &TwinLayout, detail: &str) -> Result<(), AppError> {
    let db_path = layout.db_file();
    if !db_path.exists() {
        return Ok(());
    }
    let mut store = Store::open(&db_path).map_err(K8sCmdError::StoreOpen)?;
    if !store.is_initialized().map_err(K8sCmdError::Store)? {
        return Ok(());
    }
    let now = TimestampNs::now().as_i64();
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "k8s".into(),
            started_at_ns: now,
            ended_at_ns: now,
            status: "unavailable".into(),
            observation_count: 0,
            warning_count: 1,
            error_message: Some(detail.into()),
            metadata_json: r#"{"source":"k8s"}"#.into(),
        })
        .map_err(K8sCmdError::Store)?;
    Ok(())
}

fn gap_detail(kubeconfig: &twin_k8s::KubeconfigCheck, detail: &str) -> String {
    if kubeconfig.present {
        format!("{detail}; kubeconfig found at {}", kubeconfig.path)
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

pub(super) fn persist(layout: &TwinLayout, view: &ClusterView) -> Result<(), AppError> {
    let db_path = layout.db_file();
    if !db_path.exists() {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    let mut store = Store::open(&db_path).map_err(K8sCmdError::StoreOpen)?;
    if !store.is_initialized().map_err(K8sCmdError::Store)? {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    let now = TimestampNs::now();
    let persisted = store.with_transaction(|store| {
    let previous_source = store
        .list_collector_runs(Some("k8s"))
        ?
        .iter()
        .rev()
        .find_map(|run| {
            serde_json::from_str::<serde_json::Value>(&run.metadata_json)
                .ok()
                .and_then(|metadata| {
                    metadata
                        .get("source_identity")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
        });
    if previous_source
        .as_deref()
        .zip(view.source_identity.as_deref())
        .is_some_and(|(old, new)| old != new)
    {
        return Ok(false);
    }
    let run_id = store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "k8s".to_string(),
            started_at_ns: now.as_i64(),
            ended_at_ns: now.as_i64(),
            status: if view.warnings.is_empty() {
                "success"
            } else {
                "degraded"
            }
            .to_string(),
            observation_count: 0,
            warning_count: view.warnings.len() as i64,
            error_message: None,
            metadata_json: serde_json::json!({"source":"k8s", "source_identity":view.source_identity, "endpoints_complete":view.endpoints_complete}).to_string(),
        })
        ?;
    record(store, view, run_id)?;
    Ok(true)
    }).map_err(K8sCmdError::Store)?;
    if !persisted {
        return Err(K8sCmdError::SourceMismatch.into());
    }
    Ok(())
}

fn record(store: &mut Store, view: &ClusterView, collector_run_id: i64) -> Result<(), StoreError> {
    // A fixture read that emitted warnings may hide objects, so an absent row
    // goes stale instead of gone.
    let mut history = ScanHistorySession::new(
        TimestampNs::now().as_i64(),
        Some(collector_run_id),
        !view.warnings.is_empty(),
        true,
        FinalizeScope::K8s,
    );
    for name in &view.namespaces {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_namespace(name),
            name,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for deployment in &view.deployments {
        let metadata =
            serde_json::json!({"source":"k8s", "rollout": deployment.rollout,
                "desired_replicas": deployment.desired_replicas, "ready_replicas": deployment.ready_replicas,
                "generation": deployment.generation, "observed_generation": deployment.observed_generation,
                "readiness_known": deployment.readiness_known,
            }).to_string();
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_deployment(&deployment.namespace, &deployment.name),
            &deployment.name,
            &metadata,
        )?;
    }
    for replicaset in &view.replicasets {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_replicaset(&replicaset.namespace, &replicaset.name),
            &replicaset.name,
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
            view,
            &mut history,
            NodeId::k8s_pod(&pod.namespace, &pod.name),
            &pod.name,
            &metadata,
        )?;
        for image in &pod.images {
            upsert_image(store, &mut history, image)?;
        }
        for name in &pod.config_map_names {
            upsert_node(
                store,
                view,
                &mut history,
                NodeId::k8s_configmap(&pod.namespace, name),
                name,
                r#"{"source":"k8s"}"#,
            )?;
        }
        for name in &pod.secret_names {
            upsert_node(
                store,
                view,
                &mut history,
                NodeId::k8s_secret_ref(&pod.namespace, name),
                name,
                r#"{"source":"k8s","ref":"name"}"#,
            )?;
        }
        for name in &pod.pvc_names {
            upsert_node(
                store,
                view,
                &mut history,
                NodeId::k8s_pvc(&pod.namespace, name),
                name,
                r#"{"source":"k8s"}"#,
            )?;
        }
    }
    for service in &view.services {
        let ready = ready_pod_names(view, service);
        let metadata = serde_json::json!({
            "source": "k8s",
            "ready_pods": ready,
            "ready_pods_known": view.endpoints_complete,
            "ready_pods_source": "endpoint_membership",
        })
        .to_string();
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_service(&service.namespace, &service.name),
            &service.name,
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
            view,
            &mut history,
            NodeId::k8s_endpoints(&endpoint.namespace, &endpoint.name),
            &endpoint.name,
            &metadata,
        )?;
    }
    for ingress in &view.ingresses {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_ingress(&ingress.namespace, &ingress.name),
            &ingress.name,
            r#"{"source":"k8s"}"#,
        )?;
        for service_name in &ingress.service_names {
            let service_id = NodeId::k8s_service(&ingress.namespace, service_name);
            if store.get_node_typed(&service_id)?.is_none() {
                upsert_node(
                    store,
                    view,
                    &mut history,
                    service_id,
                    service_name,
                    r#"{"source":"k8s"}"#,
                )?;
            }
        }
    }
    for configmap in &view.configmaps {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_configmap(&configmap.namespace, &configmap.name),
            &configmap.name,
            r#"{"source":"k8s"}"#,
        )?;
    }
    for secret in &view.secret_refs {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_secret_ref(&secret.namespace, &secret.name),
            &secret.name,
            r#"{"source":"k8s","ref":"name"}"#,
        )?;
    }
    for pvc in &view.pvcs {
        upsert_node(
            store,
            view,
            &mut history,
            NodeId::k8s_pvc(&pvc.namespace, &pvc.name),
            &pvc.name,
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
            view,
            &mut history,
            NodeId::k8s_event(&event.namespace, &event.name),
            &event.name,
            &metadata,
        )?;
    }
    link_edges(store, view, &mut history)?;
    history.finalize_missing_rows(store)
}

fn link_edges(
    store: &mut Store,
    view: &ClusterView,
    history: &mut ScanHistorySession,
) -> Result<(), StoreError> {
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
                    view,
                    history,
                    parent.clone(),
                    &owner.name,
                    r#"{"source":"k8s"}"#,
                )?;
            }
            upsert_edge(store, history, &parent, EdgeKind::Owns, &child)?;
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
                    view,
                    history,
                    parent.clone(),
                    &owner.name,
                    r#"{"source":"k8s"}"#,
                )?;
            }
            upsert_edge(store, history, &parent, EdgeKind::Owns, &pod_id)?;
        }
        for image in &pod.images {
            let image_id = NodeId::image(image);
            if NodeId::from_str(image_id.as_str()).is_err() {
                continue;
            }
            upsert_edge(store, history, &pod_id, EdgeKind::RunsImage, &image_id)?;
        }
        for name in &pod.config_map_names {
            let target = NodeId::k8s_configmap(&pod.namespace, name);
            upsert_edge(store, history, &pod_id, EdgeKind::UsesConfigMap, &target)?;
        }
        for name in &pod.secret_names {
            let target = NodeId::k8s_secret_ref(&pod.namespace, name);
            upsert_edge(store, history, &pod_id, EdgeKind::UsesSecretRef, &target)?;
        }
        for name in &pod.pvc_names {
            let target = NodeId::k8s_pvc(&pod.namespace, name);
            upsert_edge(store, history, &pod_id, EdgeKind::UsesPvc, &target)?;
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
            upsert_edge(store, history, &service_id, EdgeKind::Selects, &pod_id)?;
        }
    }
    for ingress in &view.ingresses {
        let ingress_id = NodeId::k8s_ingress(&ingress.namespace, &ingress.name);
        for service_name in &ingress.service_names {
            let service_id = NodeId::k8s_service(&ingress.namespace, service_name);
            upsert_edge(store, history, &ingress_id, EdgeKind::RoutesTo, &service_id)?;
        }
    }
    Ok(())
}

fn ready_pod_names(view: &ClusterView, service: &ServiceView) -> Vec<String> {
    if !view.endpoints_complete {
        return Vec::new();
    }
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
    Vec::new()
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

fn upsert_image(
    store: &mut Store,
    history: &mut ScanHistorySession,
    reference: &str,
) -> Result<(), StoreError> {
    let id = NodeId::image(reference);
    if NodeId::from_str(id.as_str()).is_err() {
        return Ok(());
    }
    let existing = store.get_node_typed(&id)?;
    let node = GraphNode::image_sourced(reference, history.recorded_at(), existing.as_ref(), "k8s");
    crate::commands::scan_history::upsert_node(store, history, &node)
}

fn upsert_node(
    store: &mut Store,
    view: &ClusterView,
    history: &mut ScanHistorySession,
    id: NodeId,
    label: &str,
    metadata: &str,
) -> Result<(), StoreError> {
    if NodeId::from_str(id.as_str()).is_err() {
        return Ok(());
    }
    let Some(kind) = id.kind() else {
        return Ok(());
    };
    let existing = store.get_node_typed(&id)?;
    let mut metadata: serde_json::Value =
        serde_json::from_str(metadata).map_err(|_| StoreError::Decode {
            detail: "invalid Kubernetes metadata projection".into(),
        })?;
    let uid = view.resource_uids.get(id.as_str());
    if let Some(uid) = uid {
        metadata["uid"] = uid.clone().into();
    }
    metadata["source_identity"] = view.source_identity.clone().into();
    let recreated = existing
        .as_ref()
        .and_then(|node| serde_json::from_str::<serde_json::Value>(node.metadata().as_str()).ok())
        .and_then(|metadata| {
            metadata
                .get("uid")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .zip(uid)
        .is_some_and(|(old, new)| old != *new);
    if recreated {
        let mut edges = store.list_active_edges_from(id.as_str())?;
        edges.extend(store.list_active_edges_to(id.as_str())?);
        store.mark_edges_missing(
            &edges.into_iter().map(|row| row.id).collect::<Vec<_>>(),
            history.recorded_at().as_i64(),
            false,
            history.collector_run_id(),
        )?;
        store.mark_nodes_missing(
            &[id.to_string()],
            history.recorded_at().as_i64(),
            false,
            history.collector_run_id(),
        )?;
        store.reset_node_incarnation(&id, history.recorded_at())?;
    }
    let node = GraphNode::k8s(
        id,
        kind,
        label,
        history.recorded_at(),
        existing.as_ref().filter(|_| !recreated),
        &metadata.to_string(),
    );
    crate::commands::scan_history::upsert_node(store, history, &node)
}

fn upsert_edge(
    store: &mut Store,
    history: &mut ScanHistorySession,
    from: &NodeId,
    kind: EdgeKind,
    to: &NodeId,
) -> Result<(), StoreError> {
    if NodeId::from_str(from.as_str()).is_err() || NodeId::from_str(to.as_str()).is_err() {
        return Ok(());
    }
    let id = EdgeId::new(from, kind, to);
    let existing = store
        .get_edge(id.as_str())?
        .as_ref()
        .and_then(|row| GraphEdge::try_from(row).ok());
    let edge = GraphEdge::observed_k8s(from, kind, to, history.recorded_at(), existing.as_ref());
    crate::commands::scan_history::upsert_edge(store, history, &edge)
}
