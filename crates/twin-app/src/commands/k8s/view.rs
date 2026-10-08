use twin_core::{EdgeKind, GraphEdge, GraphMetadata, GraphNode, NodeId, NodeKind};
use twin_emulate::{DeleteK8sPodInput, EmulationNode, ReadyService, RolloutK8sDeploymentInput};
use twin_store::{Store, StoreError};

use crate::error::{AppError, K8sCmdError};
use crate::model::{K8sGraphResult, K8sImpactResult};
use crate::paths::{resolve_command_paths, TwinLayout};

#[derive(Debug, Clone)]
pub struct K8sTargetRequest {
    pub config_override: Option<std::path::PathBuf>,
    pub target: NodeId,
}

pub fn graph_home(request: K8sTargetRequest) -> Result<K8sGraphResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    graph(&paths.layout, &request)
}

pub fn graph(layout: &TwinLayout, request: &K8sTargetRequest) -> Result<K8sGraphResult, AppError> {
    let store = open_store(layout, request.config_override.as_deref())?;
    let node = require_node(&store, &request.target)?;
    let mut owns_workloads = Vec::new();
    let mut owns_pods = Vec::new();
    let mut selects = Vec::new();
    let mut uses = Vec::new();
    let mut routes_to = Vec::new();
    match node.kind() {
        NodeKind::K8sDeployment | NodeKind::K8sReplicaSet => {
            let pods = k8s_store(owned_pod_ids(&store, node.id()))?;
            if node.kind() == NodeKind::K8sDeployment {
                owns_workloads =
                    k8s_store(child_shorts(&store, node.id(), NodeKind::K8sReplicaSet))?;
            }
            owns_pods = pods.iter().map(short_id).collect();
        }
        NodeKind::K8sService => {
            selects = k8s_store(child_shorts(&store, node.id(), NodeKind::K8sPod))?;
        }
        NodeKind::K8sPod => {
            uses = k8s_store(use_shorts(&store, node.id()))?;
        }
        NodeKind::K8sIngress => {
            routes_to = k8s_store(child_shorts(&store, node.id(), NodeKind::K8sService))?;
        }
        _ => {}
    }
    let pod_ids = if matches!(
        node.kind(),
        NodeKind::K8sDeployment | NodeKind::K8sReplicaSet
    ) {
        k8s_store(owned_pod_ids(&store, node.id()))?
    } else if node.kind() == NodeKind::K8sPod {
        vec![node.id().clone()]
    } else if node.kind() == NodeKind::K8sService {
        k8s_store(child_ids(
            &store,
            node.id(),
            EdgeKind::Selects,
            NodeKind::K8sPod,
        ))?
    } else {
        Vec::new()
    };
    let services = if node.kind() == NodeKind::K8sService {
        vec![node.id().clone()]
    } else {
        k8s_store(services_for_pods(&store, &pod_ids))?
    };
    let ingresses = if node.kind() == NodeKind::K8sIngress {
        Vec::new()
    } else {
        k8s_store(ingresses_for_services(&store, &services))?
    };
    let mut routed_by = services.iter().map(short_id).collect::<Vec<_>>();
    routed_by.extend(ingresses.iter().map(short_id));
    sort_unique(&mut owns_workloads);
    sort_unique(&mut owns_pods);
    sort_unique(&mut selects);
    sort_unique(&mut uses);
    sort_unique(&mut routes_to);
    let mut owns = owns_workloads;
    owns.extend(owns_pods);
    Ok(K8sGraphResult {
        id: node.id().to_string(),
        owns,
        routed_by,
        selects,
        uses,
        routes_to,
    })
}

pub fn impact_home(request: K8sTargetRequest) -> Result<K8sImpactResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    impact(&paths.layout, &request)
}

pub fn impact(
    layout: &TwinLayout,
    request: &K8sTargetRequest,
) -> Result<K8sImpactResult, AppError> {
    let store = open_store(layout, request.config_override.as_deref())?;
    let node = require_node(&store, &request.target)?;
    let (selects, owned_by, routed_by, affected) = match node.kind() {
        NodeKind::K8sService => {
            let impact = k8s_store(service_impact(&store, node.id()))?;
            (
                impact.selects,
                impact.owned_by,
                impact.routed_by,
                impact.affected,
            )
        }
        NodeKind::K8sDeployment | NodeKind::K8sReplicaSet => {
            let pods = k8s_store(owned_pod_ids(&store, node.id()))?;
            let services = k8s_store(services_for_pods(&store, &pods))?;
            let ingresses = k8s_store(ingresses_for_services(&store, &services))?;
            let mut routed_by = services.iter().map(short_id).collect::<Vec<_>>();
            routed_by.extend(ingresses.iter().map(short_id));
            sort_unique(&mut routed_by);
            (
                Vec::new(),
                Vec::new(),
                routed_by,
                pods.iter().map(short_id).collect(),
            )
        }
        NodeKind::K8sPod => {
            let services = k8s_store(services_for_pods(&store, &[node.id().clone()]))?;
            let ingresses = k8s_store(ingresses_for_services(&store, &services))?;
            let mut routed_by = services.iter().map(short_id).collect::<Vec<_>>();
            routed_by.extend(ingresses.iter().map(short_id));
            sort_unique(&mut routed_by);
            let owned_by = k8s_store(owners_of(&store, node.id()))?;
            (Vec::new(), owned_by, routed_by, Vec::new())
        }
        _ => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    Ok(K8sImpactResult {
        id: node.id().to_string(),
        selects,
        owned_by,
        routed_by,
        affected,
    })
}

struct ServiceImpact {
    selects: Vec<String>,
    owned_by: Vec<String>,
    routed_by: Vec<String>,
    affected: Vec<String>,
}

fn service_impact(store: &Store, service: &NodeId) -> Result<ServiceImpact, StoreError> {
    let pods = child_ids(store, service, EdgeKind::Selects, NodeKind::K8sPod)?;
    let mut deployments = Vec::new();
    let mut replicasets = Vec::new();
    for pod in &pods {
        for owner in incoming(store, pod, EdgeKind::Owns, NodeKind::K8sReplicaSet)? {
            replicasets.push(owner.clone());
            deployments.extend(incoming(
                store,
                &owner,
                EdgeKind::Owns,
                NodeKind::K8sDeployment,
            )?);
        }
    }
    let services = [service.clone()];
    let ingresses = ingresses_for_services(store, &services)?;
    let mut owned_by = deployments.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut owned_by);
    let mut rs = replicasets.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut rs);
    owned_by.extend(rs);
    let mut selects = pods.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut selects);
    let mut routed_by = ingresses.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut routed_by);
    let affected = selects.clone();
    Ok(ServiceImpact {
        selects,
        owned_by,
        routed_by,
        affected,
    })
}

pub(crate) fn delete_pod_input(store: &Store, pod: &NodeId) -> Result<DeleteK8sPodInput, AppError> {
    let node = require_node(store, pod)?;
    if node.kind() != NodeKind::K8sPod {
        return Err(K8sCmdError::UnsupportedTarget {
            kind: node.kind().to_string(),
        }
        .into());
    }
    let pods = [pod.clone()];
    let services = k8s_store(services_for_pods(store, &pods))?;
    let mut ready_services = Vec::new();
    for service_id in services {
        let Some(service) = store
            .get_node_typed(&service_id)
            .map_err(K8sCmdError::Store)?
        else {
            continue;
        };
        ready_services.push(ReadyService {
            id: service.id().clone(),
            label: service.label().to_string(),
            ready_pods: ready_pods(service.metadata()),
        });
    }
    let pod_name = pod
        .k8s_namespace_and_name()
        .map(|(_, name)| name.to_string())
        .unwrap_or_default();
    Ok(DeleteK8sPodInput {
        pod: EmulationNode {
            id: pod.clone(),
            label: node.label().to_string(),
        },
        pod_name,
        pod_in_graph: true,
        services: ready_services,
    })
}

pub(crate) fn rollout_input(
    store: &Store,
    deployment: &NodeId,
) -> Result<RolloutK8sDeploymentInput, AppError> {
    let node = require_node(store, deployment)?;
    if node.kind() != NodeKind::K8sDeployment {
        return Err(K8sCmdError::UnsupportedTarget {
            kind: node.kind().to_string(),
        }
        .into());
    }
    let pods = owned_pod_ids(store, deployment).map_err(K8sCmdError::Store)?;
    let mut emulation_pods = Vec::new();
    for pod_id in pods {
        let label = store
            .get_node_typed(&pod_id)
            .map_err(K8sCmdError::Store)?
            .map(|pod| pod.label().to_string())
            .unwrap_or_else(|| short_id(&pod_id));
        emulation_pods.push(EmulationNode { id: pod_id, label });
    }
    Ok(RolloutK8sDeploymentInput {
        deployment: EmulationNode {
            id: deployment.clone(),
            label: node.label().to_string(),
        },
        deployment_in_graph: true,
        pods: emulation_pods,
    })
}

fn owners_of(store: &Store, pod: &NodeId) -> Result<Vec<String>, StoreError> {
    let replicasets = incoming(store, pod, EdgeKind::Owns, NodeKind::K8sReplicaSet)?;
    let mut deployments = Vec::new();
    for replicaset in &replicasets {
        deployments.extend(incoming(
            store,
            replicaset,
            EdgeKind::Owns,
            NodeKind::K8sDeployment,
        )?);
    }
    let mut owned_by = deployments.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut owned_by);
    let mut rs = replicasets.iter().map(short_id).collect::<Vec<_>>();
    sort_unique(&mut rs);
    owned_by.extend(rs);
    Ok(owned_by)
}

fn owned_pod_ids(store: &Store, workload: &NodeId) -> Result<Vec<NodeId>, StoreError> {
    let node = store.get_node_typed(workload)?;
    let Some(node) = node else {
        return Ok(Vec::new());
    };
    if node.kind() == NodeKind::K8sReplicaSet {
        return child_ids(store, workload, EdgeKind::Owns, NodeKind::K8sPod);
    }
    let replicasets = child_ids(store, workload, EdgeKind::Owns, NodeKind::K8sReplicaSet)?;
    let mut pods = child_ids(store, workload, EdgeKind::Owns, NodeKind::K8sPod)?;
    for replicaset in replicasets {
        pods.extend(child_ids(
            store,
            &replicaset,
            EdgeKind::Owns,
            NodeKind::K8sPod,
        )?);
    }
    pods.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    pods.dedup();
    Ok(pods)
}

fn services_for_pods(store: &Store, pods: &[NodeId]) -> Result<Vec<NodeId>, StoreError> {
    let mut services = Vec::new();
    for pod in pods {
        services.extend(incoming(
            store,
            pod,
            EdgeKind::Selects,
            NodeKind::K8sService,
        )?);
    }
    services.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    services.dedup();
    Ok(services)
}

fn ingresses_for_services(store: &Store, services: &[NodeId]) -> Result<Vec<NodeId>, StoreError> {
    let mut ingresses = Vec::new();
    for service in services {
        ingresses.extend(incoming(
            store,
            service,
            EdgeKind::RoutesTo,
            NodeKind::K8sIngress,
        )?);
    }
    ingresses.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    ingresses.dedup();
    Ok(ingresses)
}

fn child_shorts(store: &Store, from: &NodeId, kind: NodeKind) -> Result<Vec<String>, StoreError> {
    let edge_kind = if kind == NodeKind::K8sPod || kind == NodeKind::K8sReplicaSet {
        EdgeKind::Owns
    } else if kind == NodeKind::K8sService {
        EdgeKind::RoutesTo
    } else {
        EdgeKind::Owns
    };
    let adjusted = match kind {
        NodeKind::K8sPod if from.kind() == Some(NodeKind::K8sService) => EdgeKind::Selects,
        NodeKind::K8sService => EdgeKind::RoutesTo,
        _ => edge_kind,
    };
    Ok(child_ids(store, from, adjusted, kind)?
        .iter()
        .map(short_id)
        .collect())
}

fn use_shorts(store: &Store, pod: &NodeId) -> Result<Vec<String>, StoreError> {
    let mut uses = Vec::new();
    for edge in edges_from(store, pod)? {
        if matches!(
            edge.kind(),
            EdgeKind::UsesConfigMap
                | EdgeKind::UsesSecretRef
                | EdgeKind::UsesPvc
                | EdgeKind::RunsImage
        ) {
            uses.push(short_id(edge.to()));
        }
    }
    sort_unique(&mut uses);
    Ok(uses)
}

fn child_ids(
    store: &Store,
    from: &NodeId,
    kind: EdgeKind,
    peer_kind: NodeKind,
) -> Result<Vec<NodeId>, StoreError> {
    let mut ids = Vec::new();
    for edge in edges_from(store, from)? {
        if edge.kind() != kind {
            continue;
        }
        if edge.to().kind() == Some(peer_kind) {
            ids.push(edge.to().clone());
        }
    }
    ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    ids.dedup();
    Ok(ids)
}

fn incoming(
    store: &Store,
    to: &NodeId,
    kind: EdgeKind,
    peer_kind: NodeKind,
) -> Result<Vec<NodeId>, StoreError> {
    let mut ids = Vec::new();
    for edge in edges_to(store, to)? {
        if edge.kind() != kind {
            continue;
        }
        if edge.from().kind() == Some(peer_kind) {
            ids.push(edge.from().clone());
        }
    }
    ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    ids.dedup();
    Ok(ids)
}

fn edges_from(store: &Store, id: &NodeId) -> Result<Vec<GraphEdge>, StoreError> {
    let mut edges = Vec::new();
    for row in store.list_active_edges_from(id.as_str())? {
        if let Ok(edge) = GraphEdge::try_from(&row) {
            edges.push(edge);
        }
    }
    Ok(edges)
}

fn edges_to(store: &Store, id: &NodeId) -> Result<Vec<GraphEdge>, StoreError> {
    let mut edges = Vec::new();
    for row in store.list_active_edges_to(id.as_str())? {
        if let Ok(edge) = GraphEdge::try_from(&row) {
            edges.push(edge);
        }
    }
    Ok(edges)
}

fn ready_pods(metadata: &GraphMetadata) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata.as_str()) else {
        return Vec::new();
    };
    value
        .get("ready_pods")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn k8s_store<T>(result: Result<T, StoreError>) -> Result<T, AppError> {
    result.map_err(|err| K8sCmdError::Store(err).into())
}

fn short_id(id: &NodeId) -> String {
    id.k8s_short().unwrap_or(id.as_str()).to_string()
}

fn sort_unique(items: &mut Vec<String>) {
    items.sort();
    items.dedup();
}

fn require_node(store: &Store, id: &NodeId) -> Result<GraphNode, AppError> {
    store
        .get_node_typed(id)
        .map_err(K8sCmdError::Store)?
        .ok_or_else(|| K8sCmdError::NodeNotFound { id: id.clone() }.into())
}

fn open_store(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
) -> Result<Store, AppError> {
    let _ = layout.config_file(config_override);
    let db_path = layout.db_file();
    if !db_path.exists() {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    let store = Store::open(&db_path).map_err(K8sCmdError::StoreOpen)?;
    if !store.is_initialized().map_err(K8sCmdError::Store)? {
        return Err(K8sCmdError::DatabaseNotInitialized.into());
    }
    Ok(store)
}
