use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use twin_collectors::{parse_dpkg_list, parse_dpkg_status, parse_mapped_libraries};
use twin_core::{
    lexical_canonical, EdgeClass, EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind,
    TimestampNs,
};
use twin_store::{Store, StoreError};

use crate::commands::scan::ScanEdgeCache;
use crate::commands::scan_config_files::host_path_for_discovery;
use crate::model::{ScanWarning, ScanWarningDetail};

const DPKG_STATUS: &str = "/var/lib/dpkg/status";
const DPKG_INFO: &str = "/var/lib/dpkg/info";
const PACMAN_LOCAL: &str = "/var/lib/pacman/local";

pub(crate) struct LibraryScanNotes {
    pub warning: Option<ScanWarning>,
    pub detail: Option<ScanWarningDetail>,
}

impl LibraryScanNotes {
    fn none() -> Self {
        Self {
            warning: None,
            detail: None,
        }
    }
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

struct PacmanPackage {
    name: String,
    version: Option<String>,
    files: Vec<String>,
}

struct PackageIdentity<'a> {
    name: &'a str,
    version: Option<&'a str>,
    source: twin_core::PackageSource,
    database: Option<(&'a str, &'a str)>,
}

type PackageOwnership = (HashMap<NodeId, NodeId>, LibraryScanNotes);

pub(crate) fn persist_libraries_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    proc_root: &Path,
    scan_time: TimestampNs,
) -> Result<LibraryScanNotes, StoreError> {
    let hits = record_mapped_libraries(store, history, edge_cache, proc_root, scan_time)?;
    let (installed, notes) = match std::fs::read_to_string(host_path_for_discovery(DPKG_STATUS)) {
        Ok(status) => (
            record_package_ownership(store, history, edge_cache, &hits, &status, scan_time)?,
            LibraryScanNotes::none(),
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            match record_pacman_ownership(store, history, edge_cache, &hits, scan_time)? {
                Some(pacman) => pacman,
                None => match record_rpm_ownership(store, history, edge_cache, &hits, scan_time)? {
                    Some(rpm) => rpm,
                    None => return Ok(unsupported_package_manager()),
                },
            }
        }
        Err(_) => return Ok(unreadable_package_manager()),
    };
    infer_service_packages(store, history, edge_cache, &hits, &installed, scan_time)?;
    Ok(notes)
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
    let edges = store
        .list_active_edges_to(package.as_str())?
        .iter()
        .map(GraphEdge::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let peer_ids: Vec<_> = edges
        .iter()
        .filter(|edge| {
            edge.kind() == EdgeKind::InstalledBy
                || (edge.kind() == EdgeKind::DependsOn
                    && edge.from().kind() == Some(NodeKind::Service))
        })
        .map(|edge| edge.from().as_str())
        .collect();
    let peers = store.get_nodes_typed_by_ids(&peer_ids)?;
    let library_ids: Vec<_> = edges
        .iter()
        .filter(|edge| {
            edge.kind() == EdgeKind::InstalledBy && peers.contains_key(edge.from().as_str())
        })
        .map(|edge| edge.from().as_str())
        .collect();
    let mapped_edges = store.list_active_edges_to_nodes(&library_ids)?;
    let mut libraries = Vec::new();
    let mut stems = Vec::new();
    for edge in &edges {
        if edge.kind() != EdgeKind::InstalledBy {
            continue;
        }
        let Some(library) = peers.get(edge.from().as_str()) else {
            continue;
        };
        let path = library
            .id()
            .as_str()
            .strip_prefix("library:")
            .unwrap_or(library.label())
            .to_string();
        let mut is_mapped = false;
        for row in mapped_edges
            .get(library.id().as_str())
            .into_iter()
            .flatten()
        {
            if GraphEdge::try_from(row)?.kind() == EdgeKind::LoadsLibrary {
                is_mapped = true;
                break;
            }
        }
        if is_mapped {
            stems.push(library_stem(&path));
        }
        libraries.push((library.id().clone(), library.label().to_string()));
    }
    stems.sort();
    stems.dedup();
    libraries.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

    let mut services = Vec::new();
    for edge in &edges {
        if edge.kind() != EdgeKind::DependsOn || edge.from().kind() != Some(NodeKind::Service) {
            continue;
        }
        let Some(service) = peers.get(edge.from().as_str()) else {
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
    let loaded = loaded_library_paths(hits);
    let info_dir = host_path_for_discovery(DPKG_INFO);
    for package in parse_dpkg_status(status) {
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
        record_package(
            store,
            history,
            edge_cache,
            PackageIdentity {
                name: &package.name,
                version: package.version.as_deref(),
                source: twin_core::PackageSource::Dpkg,
                database: None,
            },
            matched,
            scan_time,
            &mut installed,
        )?;
    }
    Ok(installed)
}

fn record_pacman_ownership(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    hits: &[MappedLibrary],
    scan_time: TimestampNs,
) -> Result<Option<PackageOwnership>, StoreError> {
    let local_dir = host_path_for_discovery(PACMAN_LOCAL);
    let Ok(entries) = std::fs::read_dir(&local_dir) else {
        return Ok(None);
    };
    let loaded = loaded_library_paths(hits);
    let mut installed = HashMap::new();
    let mut malformed = 0;
    let mut descs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("desc"))
        .collect();
    descs.sort();
    for desc in descs {
        let Ok(text) = std::fs::read_to_string(&desc) else {
            malformed += 1;
            continue;
        };
        let Some(package) = parse_pacman_desc(&text) else {
            malformed += 1;
            continue;
        };
        let matched = package
            .files
            .into_iter()
            .filter(|path| loaded.contains(path))
            .collect();
        record_package(
            store,
            history,
            edge_cache,
            PackageIdentity {
                name: &package.name,
                version: package.version.as_deref(),
                source: twin_core::PackageSource::Pacman,
                database: None,
            },
            matched,
            scan_time,
            &mut installed,
        )?;
    }
    let notes = if malformed == 0 {
        LibraryScanNotes::none()
    } else {
        malformed_pacman_entries(&local_dir, malformed)
    };
    Ok(Some((installed, notes)))
}

fn record_rpm_ownership(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    hits: &[MappedLibrary],
    scan_time: TimestampNs,
) -> Result<Option<PackageOwnership>, StoreError> {
    for logical in [
        "/usr/lib/sysimage/rpm/rpmdb.sqlite",
        "/var/lib/rpm/rpmdb.sqlite",
        "/usr/lib/sysimage/rpm/Packages.db",
        "/var/lib/rpm/Packages.db",
        "/usr/lib/sysimage/rpm/Packages",
        "/var/lib/rpm/Packages",
    ] {
        let path = host_path_for_discovery(logical);
        if !path.exists() {
            continue;
        }
        let database = match twin_package::read_rpm_database(&path) {
            Ok(packages) => packages,
            Err(error) => {
                return Ok(Some((
                    HashMap::new(),
                    LibraryScanNotes {
                        warning: Some(ScanWarning {
                            kind: "package_manager_malformed".into(),
                            count: 1,
                        }),
                        detail: Some(ScanWarningDetail {
                            kind: "package_manager".into(),
                            path: logical.into(),
                            detail: error.to_string(),
                        }),
                    },
                )))
            }
        };
        let loaded = loaded_library_paths(hits);
        let mut installed = HashMap::new();
        for package in database.packages {
            let matched = package
                .files
                .into_iter()
                .filter(|file| loaded.contains(file))
                .collect();
            record_package(
                store,
                history,
                edge_cache,
                PackageIdentity {
                    name: &package.name,
                    version: Some(&package.version),
                    source: twin_core::PackageSource::Rpm,
                    database: Some((database.format.as_str(), logical)),
                },
                matched,
                scan_time,
                &mut installed,
            )?;
        }
        return Ok(Some((installed, LibraryScanNotes::none())));
    }
    Ok(None)
}

fn record_package(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    package: PackageIdentity<'_>,
    mut matched: Vec<String>,
    scan_time: TimestampNs,
    installed: &mut HashMap<NodeId, NodeId>,
) -> Result<(), StoreError> {
    let package_id = NodeId::package(package.name);
    if NodeId::from_str(package_id.as_str()).is_err() {
        return Ok(());
    }
    matched.sort();
    matched.dedup();
    if matched.is_empty() {
        return Ok(());
    }
    let existing = store.get_node_typed(&package_id)?;
    let node = GraphNode::package_versioned(
        package.name,
        package.version,
        package.source,
        scan_time,
        existing.as_ref(),
    );
    let node = if let Some((format, path)) = package.database {
        GraphNode::from_parts(twin_core::GraphNodeParts {
            id:node.id().clone(), kind:node.kind(), label:node.label().into(), state:node.state(),
            first_seen:node.first_seen(), last_seen:node.last_seen(), valid_from:node.valid_from(), valid_to:node.valid_to(),
            metadata:twin_core::GraphMetadata::from_json(serde_json::json!({"source":package.source.to_string(), "version":package.version, "database_format":format, "database_path":path}).to_string()),
        })
    } else {
        node
    };
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
        let edge = GraphEdge::observed_installed_by_source(
            &library,
            &package_id,
            scan_time,
            existing_edge.as_ref(),
            package.source,
        );
        super::scan::upsert_edge_with_link(store, history, &edge, None)?;
        installed.insert(library, package_id.clone());
    }
    Ok(())
}

fn loaded_library_paths(hits: &[MappedLibrary]) -> HashSet<String> {
    hits.iter()
        .filter_map(|hit| hit.library.as_str().strip_prefix("library:"))
        .map(str::to_string)
        .collect()
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
            detail:
                "no readable dpkg, pacman, or RPM SQLite database is present; package manager is unsupported on this host"
                    .to_string(),
        }),
    }
}

fn malformed_pacman_entries(local_dir: &Path, count: usize) -> LibraryScanNotes {
    LibraryScanNotes {
        warning: Some(ScanWarning {
            kind: "package_manager_malformed".to_string(),
            count,
        }),
        detail: Some(ScanWarningDetail {
            kind: "package_manager".to_string(),
            path: local_dir.display().to_string(),
            detail: format!("{count} pacman database entries could not be parsed"),
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

fn parse_pacman_desc(text: &str) -> Option<PacmanPackage> {
    let text = text.replace("\r\n", "\n");
    let mut name = None;
    let mut version = None;
    let mut files = Vec::new();
    let mut field = String::new();
    for line in text.lines() {
        if let Some(key) = line
            .strip_prefix('%')
            .and_then(|line| line.strip_suffix('%'))
        {
            field = key.to_string();
            continue;
        }
        if line.is_empty() {
            continue;
        }
        match field.as_str() {
            "NAME" if name.is_none() => name = Some(line.trim().to_string()),
            "VERSION" if version.is_none() => version = Some(line.trim().to_string()),
            "FILES" => {
                if let Some(path) = pacman_file_path(line) {
                    files.push(path);
                }
            }
            _ => {}
        }
    }
    let name = name?;
    if name.is_empty() {
        return None;
    }
    Some(PacmanPackage {
        name,
        files,
        version,
    })
}

fn pacman_file_path(line: &str) -> Option<String> {
    let path = lexical_canonical(&format!("/{}", line.trim().trim_end_matches('\r')));
    path.starts_with('/').then_some(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacman_desc_reads_name_and_absolute_files() {
        let text = "\
%NAME%
openssl

%VERSION%
3.1.0-1

%FILES%
usr/bin/openssl
usr/lib/../lib/libssl.so.3
";
        let package = parse_pacman_desc(text).expect("package");
        assert_eq!(package.name, "openssl");
        assert_eq!(
            package.files,
            vec![
                "/usr/bin/openssl".to_string(),
                "/usr/lib/libssl.so.3".to_string()
            ]
        );
    }

    #[test]
    fn pacman_desc_without_name_is_malformed() {
        assert!(
            parse_pacman_desc("%VERSION%\n1.0-1\n\n%FILES%\nusr/lib/libbroken.so.3\n").is_none()
        );
        assert!(parse_pacman_desc("").is_none());
    }

    #[test]
    fn pacman_desc_without_files_owns_no_libraries() {
        let package = parse_pacman_desc("%NAME%\nmeta\n\n%VERSION%\n1.0-1\n").expect("package");
        assert!(package.files.is_empty());
    }
}
