use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use twin_collectors::{parse_dpkg_list, parse_dpkg_status, parse_mapped_libraries};
use twin_core::{EdgeClass, EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind, TimestampNs};
use twin_store::{Store, StoreError};

use crate::commands::scan::ScanEdgeCache;
use crate::commands::scan_config_files::host_path_for_discovery;
use crate::model::{ScanWarning, ScanWarningDetail};

const DPKG_STATUS: &str = "/var/lib/dpkg/status";
const DPKG_INFO: &str = "/var/lib/dpkg/info";

pub(crate) struct LibraryScanNotes {
    pub warning: Option<ScanWarning>,
    pub detail: Option<ScanWarningDetail>,
}

pub(crate) struct PackageServiceLink {
    pub service: NodeId,
    pub label: String,
    pub edge_id: EdgeId,
    pub edge_class: EdgeClass,
}

pub(crate) struct PackageUpgradeFacts {
    pub in_graph: bool,
    pub label: String,
    pub mapped_stems: Vec<String>,
    pub services: Vec<PackageServiceLink>,
    pub libraries: Vec<(NodeId, String)>,
}

struct MappedLibrary {
    process: NodeId,
    library: NodeId,
}

pub(crate) fn persist_libraries_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    proc_root: &Path,
    scan_time: TimestampNs,
) -> Result<LibraryScanNotes, StoreError> {
    let hits = record_mapped_libraries(store, history, edge_cache, proc_root, scan_time)?;
    let status = match std::fs::read_to_string(host_path_for_discovery(DPKG_STATUS)) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(unsupported_package_manager());
        }
        Err(_) => return Ok(unreadable_package_manager()),
    };
    let installed =
        record_package_ownership(store, history, edge_cache, &hits, &status, scan_time)?;
    infer_service_packages(store, history, edge_cache, &hits, &installed, scan_time)?;
    Ok(LibraryScanNotes {
        warning: None,
        detail: None,
    })
}

pub(crate) fn library_query(query: &str) -> bool {
    query.contains(".so") || query.contains('/')
}

pub(crate) fn find_library_id(store: &Store, query: &str) -> Result<Option<NodeId>, StoreError> {
    let mut best: Option<(u8, NodeId)> = None;
    for node in store.list_nodes_by_kind_typed(NodeKind::Library)? {
        let Some(path) = node.id().as_str().strip_prefix("library:") else {
            continue;
        };
        let Some(rank) = library_rank(path, query) else {
            continue;
        };
        let replace = match &best {
            None => true,
            Some((best_rank, best_id)) => {
                rank < *best_rank || (rank == *best_rank && node.id().as_str() < best_id.as_str())
            }
        };
        if replace {
            best = Some((rank, node.id().clone()));
        }
    }
    Ok(best.map(|(_, id)| id))
}

pub(crate) fn package_upgrade_facts(
    store: &Store,
    package: &NodeId,
) -> Result<PackageUpgradeFacts, StoreError> {
    let Some(node) = store.get_node_typed(package)? else {
        return Ok(PackageUpgradeFacts {
            in_graph: false,
            label: package
                .as_str()
                .strip_prefix("package:")
                .unwrap_or(package.as_str())
                .to_string(),
            mapped_stems: Vec::new(),
            services: Vec::new(),
            libraries: Vec::new(),
        });
    };
    let mut libraries = Vec::new();
    let mut stems = Vec::new();
    for row in store.list_active_edges_to(package.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != EdgeKind::InstalledBy {
            continue;
        }
        let Some(library) = store.get_node_typed(edge.from())? else {
            continue;
        };
        let path = library
            .id()
            .as_str()
            .strip_prefix("library:")
            .unwrap_or(library.label())
            .to_string();
        if library_is_mapped(store, library.id())? {
            stems.push(library_stem(&path));
        }
        libraries.push((library.id().clone(), library.label().to_string()));
    }
    stems.sort();
    stems.dedup();
    libraries.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

    let mut services = Vec::new();
    for row in store.list_active_edges_to(package.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != EdgeKind::DependsOn || edge.from().kind() != Some(NodeKind::Service) {
            continue;
        }
        let Some(service) = store.get_node_typed(edge.from())? else {
            continue;
        };
        services.push(PackageServiceLink {
            service: service.id().clone(),
            label: service.label().to_string(),
            edge_id: edge.id().clone(),
            edge_class: edge.class(),
        });
    }
    services.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(PackageUpgradeFacts {
        in_graph: true,
        label: node.label().to_string(),
        mapped_stems: stems,
        services,
        libraries,
    })
}

fn record_mapped_libraries(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    proc_root: &Path,
    scan_time: TimestampNs,
) -> Result<Vec<MappedLibrary>, StoreError> {
    let mut hits = Vec::new();
    for pid in pid_dirs(proc_root) {
        let process = NodeId::process(pid);
        if store.get_node_typed(&process)?.is_none() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(proc_root.join(pid.to_string()).join("maps")) else {
            continue;
        };
        for path in parse_mapped_libraries(&text) {
            let library = NodeId::library(&path);
            if NodeId::from_str(library.as_str()).is_err() {
                continue;
            }
            let existing = store.get_node_typed(&library)?;
            let node = GraphNode::library(&path, scan_time, existing.as_ref());
            super::scan_history::upsert_node(store, history, &node)?;
            let existing_edge = super::scan::load_existing_edge(
                store,
                edge_cache,
                &process,
                EdgeKind::LoadsLibrary,
                &library,
            )?;
            let edge = GraphEdge::observed_loads_library(
                &process,
                &library,
                scan_time,
                existing_edge.as_ref(),
            );
            super::scan::upsert_edge_with_link(store, history, &edge, None)?;
            hits.push(MappedLibrary {
                process: process.clone(),
                library,
            });
        }
    }
    Ok(hits)
}

fn record_package_ownership(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    hits: &[MappedLibrary],
    status: &str,
    scan_time: TimestampNs,
) -> Result<HashMap<NodeId, NodeId>, StoreError> {
    let mut installed = HashMap::new();
    let loaded: HashSet<String> = hits
        .iter()
        .filter_map(|hit| hit.library.as_str().strip_prefix("library:"))
        .map(str::to_string)
        .collect();
    let info_dir = host_path_for_discovery(DPKG_INFO);
    for package in parse_dpkg_status(status) {
        let package_id = NodeId::package(&package.name);
        if NodeId::from_str(package_id.as_str()).is_err() {
            continue;
        }
        let mut matched = Vec::new();
        for list in list_files(&info_dir, &package.name, &package.architecture) {
            let Ok(text) = std::fs::read_to_string(&list) else {
                continue;
            };
            for path in parse_dpkg_list(&text) {
                if loaded.contains(&path) {
                    matched.push(path);
                }
            }
        }
        matched.sort();
        matched.dedup();
        if matched.is_empty() {
            continue;
        }
        let existing = store.get_node_typed(&package_id)?;
        let node = GraphNode::package(&package.name, scan_time, existing.as_ref());
        super::scan_history::upsert_node(store, history, &node)?;
        for path in matched {
            let library = NodeId::library(&path);
            let existing_edge = super::scan::load_existing_edge(
                store,
                edge_cache,
                &library,
                EdgeKind::InstalledBy,
                &package_id,
            )?;
            let edge = GraphEdge::observed_installed_by(
                &library,
                &package_id,
                scan_time,
                existing_edge.as_ref(),
            );
            super::scan::upsert_edge_with_link(store, history, &edge, None)?;
            installed.insert(library, package_id.clone());
        }
    }
    Ok(installed)
}

fn infer_service_packages(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    hits: &[MappedLibrary],
    installed: &HashMap<NodeId, NodeId>,
    scan_time: TimestampNs,
) -> Result<(), StoreError> {
    let mut seen = HashSet::new();
    for hit in hits {
        let Some(package) = installed.get(&hit.library) else {
            continue;
        };
        for service in owning_services(store, &hit.process)? {
            if !seen.insert((service.clone(), package.clone())) {
                continue;
            }
            let existing = super::scan::load_existing_edge(
                store,
                edge_cache,
                &service,
                EdgeKind::DependsOn,
                package,
            )?;
            let edge = GraphEdge::inferred_service_depends_on_package(
                &service,
                package,
                scan_time,
                existing.as_ref(),
            );
            super::scan::upsert_edge_with_link(store, history, &edge, None)?;
        }
    }
    Ok(())
}

fn owning_services(store: &Store, process: &NodeId) -> Result<Vec<NodeId>, StoreError> {
    let mut services = Vec::new();
    for row in store.list_active_edges_to(process.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() == EdgeKind::Owns && edge.from().kind() == Some(NodeKind::Service) {
            services.push(edge.from().clone());
        }
    }
    Ok(services)
}

fn library_is_mapped(store: &Store, library: &NodeId) -> Result<bool, StoreError> {
    for row in store.list_active_edges_to(library.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() == EdgeKind::LoadsLibrary {
            return Ok(true);
        }
    }
    Ok(false)
}

fn library_stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.split_once(".so") {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => base.to_string(),
    }
}

fn library_rank(path: &str, query: &str) -> Option<u8> {
    if query.is_empty() {
        return None;
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    if base == query {
        return Some(0);
    }
    let query = query.trim_start_matches('/');
    if query.is_empty() {
        return None;
    }
    if (query.contains('/') || query.contains(".so")) && path.ends_with(query) {
        return Some(1);
    }
    None
}

fn unsupported_package_manager() -> LibraryScanNotes {
    LibraryScanNotes {
        warning: Some(ScanWarning {
            kind: "package_manager_unsupported".to_string(),
            count: 1,
        }),
        detail: Some(ScanWarningDetail {
            kind: "package_manager".to_string(),
            path: DPKG_STATUS.to_string(),
            detail: "dpkg database is absent; package manager is unsupported on this host"
                .to_string(),
        }),
    }
}

fn unreadable_package_manager() -> LibraryScanNotes {
    LibraryScanNotes {
        warning: Some(ScanWarning {
            kind: "package_manager_unsupported".to_string(),
            count: 1,
        }),
        detail: Some(ScanWarningDetail {
            kind: "package_manager".to_string(),
            path: DPKG_STATUS.to_string(),
            detail: "cannot read the dpkg database; package manager data is unavailable"
                .to_string(),
        }),
    }
}

fn list_files(info_dir: &Path, name: &str, architecture: &str) -> Vec<PathBuf> {
    let mut files = vec![info_dir.join(format!("{name}.list"))];
    if !architecture.is_empty() {
        files.push(info_dir.join(format!("{name}:{architecture}.list")));
    }
    files
}

fn pid_dirs(proc_root: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Ok(pid) = name.parse::<u32>() {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}
