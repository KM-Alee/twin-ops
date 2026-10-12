use twin_store::{EdgeRow, NodeRow};
use twin_test::{ContainerFact, K8sReadyFact, RoutingFact};

type HealthFacts = (Vec<ContainerFact>, Vec<K8sReadyFact>, Vec<RoutingFact>);

pub(crate) fn facts(
    store: &twin_store::Store,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    at: i64,
    age: Option<u64>,
) -> Result<HealthFacts, crate::AppError> {
    let docker_missing = source_missing(store, "docker", at, age)?;
    let containerd_missing = source_missing(store, "containerd", at, age)?;
    let k8s_missing = source_missing(store, "k8s", at, age)?;
    let mut containers = Vec::new();
    let mut workloads = Vec::new();
    let mut routing = Vec::new();
    for node in nodes.iter().filter(|node| node.state != "gone") {
        let metadata =
            serde_json::from_str::<serde_json::Value>(&node.metadata_json).unwrap_or_default();
        let stale =
            node.state == "stale" || super::test_context::is_old(node.last_seen_ns, at, age);
        match node.kind.as_str() {
            "container" => containers.push(ContainerFact {
                id: node.id.clone(),
                is_running: metadata.get("is_running").and_then(|value| value.as_bool()),
                is_healthy: match metadata
                    .get("health_status")
                    .and_then(|value| value.as_str())
                {
                    Some("healthy") => Some(true),
                    Some("unhealthy" | "starting") => Some(false),
                    _ => None,
                },
                stale: stale
                    || if metadata.get("runtime").and_then(|value| value.as_str())
                        == Some("containerd")
                    {
                        containerd_missing
                    } else {
                        docker_missing
                    },
            }),
            "k8s_deployment" => {
                let known = metadata
                    .get("readiness_known")
                    .and_then(|value| value.as_bool())
                    == Some(true);
                let desired = metadata
                    .get("desired_replicas")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok());
                let ready = known
                    .then(|| {
                        metadata
                            .get("ready_replicas")
                            .and_then(|value| value.as_u64())
                            .and_then(|value| u32::try_from(value).ok())
                    })
                    .flatten();
                let is_current_generation = metadata
                    .get("generation")
                    .and_then(|value| value.as_u64())
                    .zip(
                        metadata
                            .get("observed_generation")
                            .and_then(|value| value.as_u64()),
                    )
                    .map(|(desired, observed)| observed >= desired);
                workloads.push(K8sReadyFact {
                    id: node.id.clone(),
                    desired,
                    ready,
                    is_current_generation,
                    stale: stale || k8s_missing,
                });
            }
            "k8s_service" => {
                let namespace = node
                    .id
                    .strip_prefix("k8s:service:")
                    .and_then(|value| value.split_once('/'))
                    .map(|(namespace, _)| namespace);
                let pods = metadata
                    .get("ready_pods")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str())
                    .filter_map(|pod| {
                        namespace.map(|namespace| format!("k8s:pod:{namespace}/{pod}"))
                    })
                    .collect::<Vec<_>>();
                let mut ready_workloads = Vec::new();
                let mut has_unknown_ownership = false;
                for pod in pods.into_iter().collect::<std::collections::BTreeSet<_>>() {
                    let mut owners = std::collections::BTreeSet::new();
                    for replicaset in edges.iter().filter(|edge| {
                        edge.to_node_id == pod && edge.kind == "owns" && edge.state == "active"
                    }) {
                        for deployment in edges.iter().filter(|edge| {
                            edge.to_node_id == replicaset.from_node_id
                                && edge.kind == "owns"
                                && edge.state == "active"
                        }) {
                            if [
                                pod.as_str(),
                                replicaset.from_node_id.as_str(),
                                deployment.from_node_id.as_str(),
                            ]
                            .iter()
                            .all(|id| {
                                nodes.iter().any(|node| {
                                    node.id == *id
                                        && node.state == "active"
                                        && !super::test_context::is_old(node.last_seen_ns, at, age)
                                })
                            }) && !super::test_context::is_old(replicaset.last_seen_ns, at, age)
                                && !super::test_context::is_old(deployment.last_seen_ns, at, age)
                            {
                                owners.insert(deployment.from_node_id.clone());
                            }
                        }
                    }
                    if owners.len() == 1 {
                        ready_workloads.extend(owners);
                    } else {
                        has_unknown_ownership = true;
                    }
                }
                routing.push(RoutingFact {
                    id: node.id.clone(),
                    ready_workloads,
                    is_known: metadata
                        .get("ready_pods_known")
                        .and_then(|value| value.as_bool())
                        == Some(true)
                        && !has_unknown_ownership,
                    stale: stale || k8s_missing,
                });
            }
            _ => {}
        }
    }
    Ok((containers, workloads, routing))
}

pub(crate) fn source_missing(
    store: &twin_store::Store,
    source: &str,
    at: i64,
    age: Option<u64>,
) -> Result<bool, crate::AppError> {
    let run = super::coverage::latest_run_at(store, source, at)
        .map_err(crate::error::ScanError::Store)?;
    Ok(run.is_none_or(|run| {
        !matches!(run.status.as_str(), "success" | "degraded")
            || super::test_context::is_old(run.ended_at_ns, at, age)
    }))
}
