use std::path::{Path, PathBuf};
use std::str::FromStr;

use twin_container::{
    ContainerInspect, ContainerSummary, DockerBackend, DockerReadOnly, PortMapping, VolumeMount,
    DOCKER_SOCKET,
};
use twin_core::{
    EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind, PortPublish, TimestampNs,
};
use twin_store::{Store, StoreError};

use crate::commands::scan::ScanEdgeCache;
use crate::model::{ScanWarning, ScanWarningDetail};

pub(crate) struct ContainerScanNotes {
    pub warning: Option<ScanWarning>,
    pub detail: Option<ScanWarningDetail>,
    pub unavailable: bool,
}

pub(crate) struct ContainerServiceLink {
    pub service: NodeId,
    pub label: String,
    pub edge_id: EdgeId,
    pub kind: EdgeKind,
    pub port: NodeId,
}

pub(crate) struct ContainerImpactFacts {
    pub in_graph: bool,
    pub affected: Vec<ContainerServiceLink>,
}

struct SeenContainer {
    name: String,
    image: String,
    pid: Option<u32>,
    ports: Vec<PortMapping>,
    mounts: Vec<VolumeMount>,
}

enum RecordError {
    Store(StoreError),
    Docker(String),
}

impl From<StoreError> for RecordError {
    fn from(source: StoreError) -> Self {
        Self::Store(source)
    }
}

pub(crate) fn persist_containers_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    scan_time: TimestampNs,
) -> Result<ContainerScanNotes, StoreError> {
    let fixture = std::env::var("TWIN_DOCKER_FIXTURE")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let backend = match DockerBackend::open(fixture.as_deref(), Path::new(DOCKER_SOCKET)) {
        Ok(backend) => backend,
        Err(err) => return Ok(unavailable(&err.to_string())),
    };
    match record(store, history, edge_cache, &backend, scan_time) {
        Ok(()) => Ok(ContainerScanNotes {
            warning: None,
            detail: None,
            unavailable: false,
        }),
        Err(RecordError::Store(err)) => Err(err),
        Err(RecordError::Docker(detail)) => Ok(unavailable(&detail)),
    }
}

pub(crate) fn container_impact_facts(
    store: &Store,
    container: &NodeId,
) -> Result<ContainerImpactFacts, StoreError> {
    if store.get_node_typed(container)?.is_none() {
        return Ok(ContainerImpactFacts {
            in_graph: false,
            affected: Vec::new(),
        });
    }
    let mut affected = Vec::new();
    for row in store.list_active_edges_from(container.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != EdgeKind::MapsPort {
            continue;
        }
        affected.extend(services_on_port(store, edge.to())?);
    }
    affected.sort_by(|left, right| left.service.as_str().cmp(right.service.as_str()));
    affected.dedup_by(|left, right| left.service == right.service);
    Ok(ContainerImpactFacts {
        in_graph: true,
        affected,
    })
}

fn services_on_port(store: &Store, port: &NodeId) -> Result<Vec<ContainerServiceLink>, StoreError> {
    let mut links = Vec::new();
    for row in store.list_active_edges_to(port.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != EdgeKind::ConnectsTo && edge.kind() != EdgeKind::DependsOn {
            continue;
        }
        if edge.from().kind() != Some(NodeKind::Service) {
            continue;
        }
        let Some(service) = store.get_node_typed(edge.from())? else {
            continue;
        };
        links.push(ContainerServiceLink {
            service: service.id().clone(),
            label: service.label().to_string(),
            edge_id: edge.id().clone(),
            kind: edge.kind(),
            port: port.clone(),
        });
    }
    Ok(links)
}

fn record(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    docker: &DockerBackend,
    scan_time: TimestampNs,
) -> Result<(), RecordError> {
    let summaries = docker
        .list_containers()
        .map_err(|err| RecordError::Docker(err.to_string()))?;
    for summary in summaries {
        let seen = seen_container(docker, summary);
        persist_one(store, history, edge_cache, scan_time, &seen)?;
    }
    Ok(())
}

fn seen_container(docker: &DockerBackend, summary: ContainerSummary) -> SeenContainer {
    match docker.inspect_container(&summary.id) {
        Ok(inspect) => seen_from_inspect(docker, inspect),
        Err(_) => SeenContainer {
            name: summary.name,
            image: confirmed_image(docker, &summary.image),
            pid: None,
            ports: summary.ports,
            mounts: summary.mounts,
        },
    }
}

fn seen_from_inspect(docker: &DockerBackend, inspect: ContainerInspect) -> SeenContainer {
    let image = confirmed_image(docker, &inspect.image);
    let ports = inspect.port_mappings().to_vec();
    let mounts = inspect.mounts().to_vec();
    SeenContainer {
        name: inspect.name,
        image,
        pid: inspect.pid,
        ports,
        mounts,
    }
}

fn confirmed_image(docker: &DockerBackend, reference: &str) -> String {
    match docker.inspect_image(reference) {
        Ok(image) => image.reference,
        Err(_) => reference.to_string(),
    }
}

fn persist_one(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    scan_time: TimestampNs,
    seen: &SeenContainer,
) -> Result<(), StoreError> {
    let container = NodeId::container(&seen.name);
    if NodeId::from_str(container.as_str()).is_err() {
        return Ok(());
    }
    let image = NodeId::image(&seen.image);
    if NodeId::from_str(image.as_str()).is_err() {
        return Ok(());
    }
    let existing = store.get_node_typed(&container)?;
    let node = GraphNode::container(&seen.name, scan_time, existing.as_ref());
    super::scan_history::upsert_node(store, history, &node)?;
    let image_existing = store.get_node_typed(&image)?;
    let image_node = GraphNode::image(&seen.image, scan_time, image_existing.as_ref());
    super::scan_history::upsert_node(store, history, &image_node)?;
    let image_edge = GraphEdge::observed_runs_image(
        &container,
        &image,
        scan_time,
        super::scan::load_existing_edge(
            store,
            edge_cache,
            &container,
            EdgeKind::RunsImage,
            &image,
        )?
        .as_ref(),
    );
    super::scan::upsert_edge_with_link(store, history, &image_edge, None)?;
    for port in &seen.ports {
        persist_port(store, history, edge_cache, scan_time, &container, port)?;
    }
    for mount in &seen.mounts {
        persist_mount(store, history, edge_cache, scan_time, &container, mount)?;
    }
    if let Some(pid) = seen.pid {
        persist_process(store, history, edge_cache, scan_time, &container, pid)?;
    }
    Ok(())
}

fn persist_port(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    scan_time: TimestampNs,
    container: &NodeId,
    port: &PortMapping,
) -> Result<(), StoreError> {
    let Ok(port_id) = NodeId::port_tcp(&port.host_ip, port.host_port) else {
        return Ok(());
    };
    let existing = store.get_node_typed(&port_id)?;
    let node = GraphNode::tcp_port(&port.host_ip, port.host_port, scan_time, existing.as_ref())
        .map_err(|err| StoreError::Decode {
            detail: err.to_string(),
        })?;
    super::scan_history::upsert_node(store, history, &node)?;
    let edge = GraphEdge::observed_maps_port(
        container,
        &port_id,
        scan_time,
        super::scan::load_existing_edge(
            store,
            edge_cache,
            container,
            EdgeKind::MapsPort,
            &port_id,
        )?
        .as_ref(),
        &PortPublish {
            container_port: port.container_port,
            host_ip: &port.host_ip,
            host_port: port.host_port,
            protocol: &port.protocol,
        },
    );
    super::scan::upsert_edge_with_link(store, history, &edge, None)
}

fn persist_mount(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    scan_time: TimestampNs,
    container: &NodeId,
    mount: &VolumeMount,
) -> Result<(), StoreError> {
    let directory = NodeId::directory(&mount.destination);
    if NodeId::from_str(directory.as_str()).is_err() {
        return Ok(());
    }
    let destination = directory
        .as_str()
        .strip_prefix("directory:")
        .unwrap_or(&mount.destination);
    let existing = store.get_node_typed(&directory)?;
    let node = GraphNode::directory(destination, scan_time, existing.as_ref());
    super::scan_history::upsert_node(store, history, &node)?;
    let edge = GraphEdge::observed_mounts_volume(
        container,
        &directory,
        scan_time,
        super::scan::load_existing_edge(
            store,
            edge_cache,
            container,
            EdgeKind::MountsVolume,
            &directory,
        )?
        .as_ref(),
        destination,
    );
    super::scan::upsert_edge_with_link(store, history, &edge, None)
}

fn persist_process(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    scan_time: TimestampNs,
    container: &NodeId,
    pid: u32,
) -> Result<(), StoreError> {
    let process = NodeId::process(pid);
    if store.get_node_typed(&process)?.is_none() {
        return Ok(());
    }
    let edge = GraphEdge::observed_container_owns_process(
        container,
        &process,
        scan_time,
        super::scan::load_existing_edge(store, edge_cache, container, EdgeKind::Owns, &process)?
            .as_ref(),
    );
    super::scan::upsert_edge_with_link(store, history, &edge, None)
}

fn unavailable(detail: &str) -> ContainerScanNotes {
    ContainerScanNotes {
        warning: Some(ScanWarning {
            kind: "docker_unavailable".to_string(),
            count: 1,
        }),
        detail: Some(ScanWarningDetail {
            kind: "docker".to_string(),
            path: DOCKER_SOCKET.to_string(),
            detail: detail.to_string(),
        }),
        unavailable: true,
    }
}
