use std::path::Path;

use twin_collectors::parse_mounts;
use twin_core::{lexical_canonical, EdgeKind, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_store::{Store, StoreError};

use crate::commands::scan::ScanEdgeCache;
use crate::commands::scan_config_files::host_path_for_discovery;

use super::disk_usage::mount_used_percent;

struct KnownServicePath {
    unit: &'static str,
    path: &'static str,
    logs: bool,
}

const NOTABLE_DIRECTORIES: &[&str] = &[
    "/var/log",
    "/var/lib/postgresql",
    "/var/log/journal",
    "/var/lib/docker",
];

const KNOWN_SERVICE_PATHS: &[KnownServicePath] = &[
    KnownServicePath {
        unit: "postgresql.service",
        path: "/var/lib/postgresql",
        logs: false,
    },
    KnownServicePath {
        unit: "docker.service",
        path: "/var/lib/docker",
        logs: false,
    },
    KnownServicePath {
        unit: "systemd-journald.service",
        path: "/var/log/journal",
        logs: true,
    },
];

pub(crate) struct MountServiceUse {
    pub service: NodeId,
    pub service_label: String,
    pub path: String,
    pub logs: bool,
}

pub(crate) fn used_percent_of(metadata: &str) -> Option<u8> {
    let value: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let number = value.get("used_percent")?.as_u64()?;
    u8::try_from(number).ok()
}

pub(crate) fn persist_mounts_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    proc_root: &Path,
    scan_time: TimestampNs,
) -> Result<(), StoreError> {
    let text = match std::fs::read_to_string(proc_root.join("mounts")) {
        Ok(text) => text,
        Err(_) => return Ok(()),
    };
    let records = parse_mounts(&text);
    let mut mount_points = Vec::new();
    for record in &records {
        let mount_id = NodeId::mount(&record.mount_point);
        let existing = store.get_node_typed(&mount_id)?;
        let stat_path = stat_path_for(&record.mount_point);
        let used = mount_used_percent(&stat_path);
        let node = GraphNode::mount(
            &record.mount_point,
            scan_time,
            existing.as_ref(),
            &record.device,
            &record.fstype,
            used,
        );
        super::scan_history::upsert_node(store, history, &node)?;
        mount_points.push(lexical_canonical(&record.mount_point));
    }

    for path in NOTABLE_DIRECTORIES {
        if !host_dir_exists(path) {
            continue;
        }
        upsert_directory(store, history, path, scan_time)?;
    }

    for known in KNOWN_SERVICE_PATHS {
        if !host_dir_exists(known.path) {
            continue;
        }
        upsert_directory(store, history, known.path, scan_time)?;
        let service_id = NodeId::service(known.unit);
        if store.get_node_typed(&service_id)?.is_none() {
            continue;
        }
        let directory_id = NodeId::directory(known.path);
        let kind = if known.logs {
            EdgeKind::LogsTo
        } else {
            EdgeKind::Uses
        };
        let existing =
            super::scan::load_existing_edge(store, edge_cache, &service_id, kind, &directory_id)?;
        let edge = if known.logs {
            GraphEdge::inferred_service_logs_to(
                &service_id,
                &directory_id,
                scan_time,
                existing.as_ref(),
                known.path,
            )
        } else {
            GraphEdge::inferred_service_uses(
                &service_id,
                &directory_id,
                scan_time,
                existing.as_ref(),
                known.path,
            )
        };
        super::scan::upsert_edge_with_link(store, history, &edge, None)?;
    }

    let path_nodes = path_nodes(store)?;
    for (path_id, path) in path_nodes {
        let Some(mount_point) = mount_covering(&path, &mount_points) else {
            continue;
        };
        let mount_id = NodeId::mount(mount_point);
        let existing = super::scan::load_existing_edge(
            store,
            edge_cache,
            &path_id,
            EdgeKind::MountedOn,
            &mount_id,
        )?;
        let edge =
            GraphEdge::observed_mounted_on(&path_id, &mount_id, scan_time, existing.as_ref());
        super::scan::upsert_edge_with_link(store, history, &edge, None)?;
    }
    Ok(())
}

pub(crate) fn services_on_mount(
    store: &Store,
    mount: &NodeId,
) -> Result<Vec<MountServiceUse>, StoreError> {
    let mut uses = Vec::new();
    for row in store.list_active_edges_to(mount.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != EdgeKind::MountedOn {
            continue;
        }
        let path_node = edge.from();
        let Some(path) = path_of(path_node) else {
            continue;
        };
        for incoming in store.list_active_edges_to(path_node.as_str())? {
            let use_edge = GraphEdge::try_from(&incoming)?;
            let logs = match use_edge.kind() {
                EdgeKind::LogsTo => true,
                EdgeKind::Uses => false,
                _ => continue,
            };
            if use_edge.from().kind() != Some(twin_core::NodeKind::Service) {
                continue;
            }
            let Some(service_node) = store.get_node_typed(use_edge.from())? else {
                continue;
            };
            let item = MountServiceUse {
                service: use_edge.from().clone(),
                service_label: service_node.label().to_string(),
                path: path.to_string(),
                logs,
            };
            if !uses.iter().any(|existing: &MountServiceUse| {
                existing.service == item.service
                    && existing.path == item.path
                    && existing.logs == item.logs
            }) {
                uses.push(item);
            }
        }
    }
    uses.sort_by(|a, b| a.service.as_str().cmp(b.service.as_str()));
    Ok(uses)
}

fn upsert_directory(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    path: &str,
    scan_time: TimestampNs,
) -> Result<(), StoreError> {
    let id = NodeId::directory(path);
    let existing = store.get_node_typed(&id)?;
    let node = GraphNode::directory(path, scan_time, existing.as_ref());
    super::scan_history::upsert_node(store, history, &node)
}

fn path_nodes(store: &Store) -> Result<Vec<(NodeId, String)>, StoreError> {
    let mut out = Vec::new();
    for kind in [twin_core::NodeKind::File, twin_core::NodeKind::Directory] {
        for node in store.list_nodes_by_kind_typed(kind)? {
            if let Some(path) = path_of(node.id()) {
                out.push((node.id().clone(), path.to_string()));
            }
        }
    }
    Ok(out)
}

fn path_of(id: &NodeId) -> Option<&str> {
    id.as_str()
        .strip_prefix("file:")
        .or_else(|| id.as_str().strip_prefix("directory:"))
}

fn host_dir_exists(path: &str) -> bool {
    host_path_for_discovery(path).is_dir()
}

fn stat_path_for(mount_point: &str) -> String {
    let mapped = host_path_for_discovery(mount_point);
    if mapped.is_dir() {
        mapped.display().to_string()
    } else {
        mount_point.to_string()
    }
}

fn mount_covering<'a>(path: &str, mounts: &'a [String]) -> Option<&'a str> {
    let path = lexical_canonical(path);
    mounts
        .iter()
        .map(|mount| mount.as_str())
        .filter(|mount| on_mount(&path, mount))
        .max_by_key(|mount| mount.len())
}

fn on_mount(path: &str, mount: &str) -> bool {
    if mount == "/" {
        return path.starts_with('/');
    }
    path == mount || path.starts_with(&format!("{mount}/"))
}
