use std::collections::HashSet;
use std::path::{Path, PathBuf};

use twin_collectors::{
    discover_service_config_files, ConfigFileSource, ServiceConfigFileDiscovery,
};
use twin_core::{CollectorName, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RawIdentity, RawObservation,
};
use twin_store::{Store, StoreError};

use crate::commands::scan::ScanEdgeCache;

const CONFIG_DISCOVERY_COLLECTOR: &str = "config_file_discovery";

pub(crate) fn host_path_for_discovery(path: &str) -> PathBuf {
    if let Ok(root) = std::env::var("TWIN_HOST_ROOT") {
        if let Some(rest) = path.strip_prefix('/') {
            return PathBuf::from(root).join(rest);
        }
    }
    PathBuf::from(path)
}

fn path_exists_for_discovery(path: &Path) -> bool {
    path.is_file()
}

pub(crate) fn persist_config_files_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    discoveries: &[ServiceConfigFileDiscovery],
    scan_time: TimestampNs,
) -> Result<usize, StoreError> {
    let mut edge_count = 0usize;
    for discovery in discoveries {
        let path_str = discovery.file_path.display().to_string();
        let file_id = NodeId::file(&path_str);
        let existing_file = store.get_node_typed(&file_id)?;
        let file_node = GraphNode::file(&path_str, scan_time, existing_file.as_ref());
        super::scan_history::upsert_node(store, history, &file_node)?;

        let service_id = NodeId::service(&discovery.service_unit);
        let existing_service = store.get_node_typed(&service_id)?;
        if existing_service.is_none() {
            continue;
        }

        let obs = config_file_observation(discovery, scan_time);
        store.insert_observation_typed(&obs)?;
        let obs_id = obs.id();

        let existing_edge = super::scan::load_existing_edge(
            store,
            edge_cache,
            &service_id,
            twin_core::EdgeKind::ConfiguredBy,
            &file_id,
        )?;
        let edge = GraphEdge::observed_service_configured_by_file(
            &service_id,
            &file_id,
            scan_time,
            existing_edge.as_ref(),
            discovery.source.metadata_key(),
            &path_str,
        );
        super::scan::upsert_edge_with_link(store, history, &edge, Some((&obs_id, "direct")))?;
        edge_count += 1;
    }
    Ok(edge_count)
}

pub(crate) struct ConfigParseWarning {
    pub path: String,
    pub detail: String,
}

pub(crate) fn apply_nginx_proxies(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    discoveries: &[ServiceConfigFileDiscovery],
    scan_time: TimestampNs,
) -> Result<Vec<ConfigParseWarning>, StoreError> {
    let mut warnings = Vec::new();
    for discovery in discoveries {
        let path_str = discovery.file_path.display().to_string();
        let host_path = host_path_for_discovery(&path_str);
        let Ok(bytes) = std::fs::read(&host_path) else {
            continue;
        };
        let fingerprint = twin_config::content_fingerprint(&bytes);
        let file_id = NodeId::file(&path_str);
        let existing_file = store.get_node_typed(&file_id)?;
        let file_node = GraphNode::file_fingerprinted(
            &path_str,
            scan_time,
            existing_file.as_ref(),
            &fingerprint,
        );
        super::scan_history::upsert_node(store, history, &file_node)?;

        let parsed = twin_config::parse_nginx(&String::from_utf8_lossy(&bytes));
        for warning in parsed.warnings {
            warnings.push(ConfigParseWarning {
                path: path_str.clone(),
                detail: warning,
            });
        }
        let service_id = NodeId::service(&discovery.service_unit);
        if store.get_node_typed(&service_id)?.is_none() {
            continue;
        }
        for proxy in parsed.proxies {
            match proxy.target {
                twin_config::ProxyTarget::Tcp { host, port } => {
                    let Ok(port_id) = NodeId::port_tcp(&host, port) else {
                        warnings.push(ConfigParseWarning {
                            path: path_str.clone(),
                            detail: format!("proxy_pass host `{host}` is not an IP address"),
                        });
                        continue;
                    };
                    let existing_port = store.get_node_typed(&port_id)?;
                    let port_node =
                        GraphNode::tcp_port(&host, port, scan_time, existing_port.as_ref())
                            .map_err(|err| StoreError::Decode {
                                detail: err.to_string(),
                            })?;
                    super::scan_history::upsert_node(store, history, &port_node)?;
                    link_reference(
                        store, history, edge_cache, &file_id, &port_id, scan_time, &proxy.raw,
                    )?;
                    for listener in listener_services(store, &port_id)? {
                        if listener == service_id {
                            continue;
                        }
                        link_proxy(
                            store,
                            history,
                            edge_cache,
                            &service_id,
                            &listener,
                            scan_time,
                            ProxyUpstream {
                                port: &port_id,
                                statement: &proxy.raw,
                            },
                        )?;
                    }
                }
                twin_config::ProxyTarget::Unix { path } => {
                    let Ok(unix_id) = NodeId::unix_socket(&path) else {
                        warnings.push(ConfigParseWarning {
                            path: path_str.clone(),
                            detail: format!("proxy_pass unix path `{path}` is invalid"),
                        });
                        continue;
                    };
                    let existing = store.get_node_typed(&unix_id)?;
                    let node = GraphNode::unix_socket(&path, scan_time, existing.as_ref())
                        .map_err(|err| StoreError::Decode {
                            detail: err.to_string(),
                        })?;
                    super::scan_history::upsert_node(store, history, &node)?;
                    link_reference(
                        store, history, edge_cache, &file_id, &unix_id, scan_time, &proxy.raw,
                    )?;
                }
                twin_config::ProxyTarget::Named { name } => {
                    warnings.push(ConfigParseWarning {
                        path: path_str.clone(),
                        detail: format!("proxy_pass host `{name}` is not a concrete address"),
                    });
                }
            }
        }
    }
    Ok(warnings)
}

fn listener_services(store: &Store, port_id: &NodeId) -> Result<Vec<NodeId>, StoreError> {
    let mut listeners = Vec::new();
    for row in store.list_active_edges_to(port_id.as_str())? {
        let edge = GraphEdge::try_from(&row)?;
        if edge.kind() != twin_core::EdgeKind::ListensOn {
            continue;
        }
        if edge.from().kind() != Some(twin_core::NodeKind::Service) {
            continue;
        }
        if !listeners.iter().any(|id| id == edge.from()) {
            listeners.push(edge.from().clone());
        }
    }
    Ok(listeners)
}

fn link_reference(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    file_id: &NodeId,
    target_id: &NodeId,
    scan_time: TimestampNs,
    proxy_pass: &str,
) -> Result<(), StoreError> {
    let existing = super::scan::load_existing_edge(
        store,
        edge_cache,
        file_id,
        twin_core::EdgeKind::References,
        target_id,
    )?;
    let edge = GraphEdge::observed_file_references(
        file_id,
        target_id,
        scan_time,
        existing.as_ref(),
        proxy_pass,
    );
    let obs = proxy_observation(file_id, target_id, proxy_pass, scan_time)?;
    let obs_id = obs.id();
    store.insert_observation_typed(&obs)?;
    super::scan::upsert_edge_with_link(store, history, &edge, Some((&obs_id, "direct")))?;
    Ok(())
}

struct ProxyUpstream<'a> {
    port: &'a NodeId,
    statement: &'a str,
}

fn link_proxy(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    source: &NodeId,
    target: &NodeId,
    scan_time: TimestampNs,
    upstream: ProxyUpstream<'_>,
) -> Result<(), StoreError> {
    let proxy_pass = upstream.statement;
    let port_id = upstream.port;
    let existing = super::scan::load_existing_edge(
        store,
        edge_cache,
        source,
        twin_core::EdgeKind::ProxiesTo,
        target,
    )?;
    let edge = GraphEdge::inferred_service_proxies_to(
        source,
        target,
        scan_time,
        existing.as_ref(),
        proxy_pass,
    );
    let obs = proxy_observation(source, port_id, proxy_pass, scan_time)?;
    let obs_id = obs.id();
    store.insert_observation_typed(&obs)?;
    super::scan::upsert_edge_with_link(store, history, &edge, Some((&obs_id, "support")))?;
    Ok(())
}

fn proxy_observation(
    subject: &NodeId,
    object: &NodeId,
    proxy_pass: &str,
    timestamp: TimestampNs,
) -> Result<Observation, StoreError> {
    let mut meta = ObservationMetadata::new();
    meta.insert_str("proxy_pass", proxy_pass);
    meta.insert_str("target", object.as_str());
    if let Some(scheme) = proxy_scheme(proxy_pass) {
        meta.insert_str("proxy_scheme", scheme);
    }
    let raw = RawObservation {
        source: ObservationSource::ConfigFileDiscovery,
        kind: ObservationKind::ConfigProxyPass,
        collector: CollectorName::new(CONFIG_DISCOVERY_COLLECTOR),
        subject: identity_for(subject),
        object: identity_for(object),
        timestamp,
        raw_ref: Some(RawEvidenceRef::new("nginx proxy_pass")),
        confidence_hint: ConfidenceHint::Moderate,
        metadata: meta,
    };
    twin_observation::Pipeline::default()
        .process(raw)
        .map_err(|err| StoreError::Decode {
            detail: err.to_string(),
        })
}

fn proxy_scheme(raw: &str) -> Option<&'static str> {
    if raw.starts_with("https://") {
        Some("https")
    } else if raw.starts_with("http://") {
        Some("http")
    } else {
        None
    }
}

fn identity_for(id: &NodeId) -> Option<RawIdentity> {
    match id.kind() {
        Some(twin_core::NodeKind::Service) => {
            id.as_str()
                .strip_prefix("service:")
                .map(|unit| RawIdentity::Service {
                    unit: unit.to_string(),
                })
        }
        Some(twin_core::NodeKind::File) => {
            id.as_str()
                .strip_prefix("file:")
                .map(|path| RawIdentity::File {
                    path: path.to_string(),
                })
        }
        Some(twin_core::NodeKind::Port) => {
            let rest = id.as_str().strip_prefix("port:tcp:")?;
            let (host, port) = rest.rsplit_once(':')?;
            let port = port.parse().ok()?;
            Some(RawIdentity::TcpEndpoint {
                ip: host.to_string(),
                port,
            })
        }
        Some(twin_core::NodeKind::UnixSocket) => {
            id.as_str()
                .strip_prefix("unix:")
                .map(|path| RawIdentity::UnixSocket {
                    path: path.to_string(),
                })
        }
        _ => None,
    }
}

pub(crate) fn config_file_evidence_statement(
    source: ConfigFileSource,
    path: &str,
    service: &str,
) -> String {
    match source {
        ConfigFileSource::SystemdUnitFile => {
            format!("systemd unit file {path} configures {service}")
        }
        ConfigFileSource::SystemdDropIn => {
            format!("systemd drop-in {path} configures {service}")
        }
        ConfigFileSource::KnownServiceConfigPath => {
            format!("known config path {path} was discovered for {service}")
        }
    }
}

fn config_file_observation(
    discovery: &ServiceConfigFileDiscovery,
    timestamp: TimestampNs,
) -> Observation {
    let path_str = discovery.file_path.display().to_string();
    let mut meta = ObservationMetadata::new();
    meta.insert_str("source", discovery.source.metadata_key());
    meta.insert_str("path", &path_str);
    meta.insert_str("service", &discovery.service_unit);
    if discovery.source == ConfigFileSource::KnownServiceConfigPath {
        meta.insert_str("discovery", "known_config_mapping");
    }
    let raw = RawObservation {
        source: ObservationSource::ConfigFileDiscovery,
        kind: ObservationKind::ServiceConfiguredByFile,
        collector: CollectorName::new(CONFIG_DISCOVERY_COLLECTOR),
        subject: Some(RawIdentity::Service {
            unit: discovery.service_unit.clone(),
        }),
        object: Some(RawIdentity::File {
            path: path_str.clone(),
        }),
        timestamp,
        raw_ref: Some(RawEvidenceRef::new(path_str)),
        confidence_hint: match discovery.source {
            ConfigFileSource::SystemdUnitFile | ConfigFileSource::SystemdDropIn => {
                ConfidenceHint::High
            }
            ConfigFileSource::KnownServiceConfigPath => ConfidenceHint::Moderate,
        },
        metadata: meta,
    };
    twin_observation::Pipeline::default()
        .process(raw)
        .expect("config file observation")
}

pub(crate) fn discover_config_files_for_scan(
    units: &[twin_collectors::EffectiveUnit],
    unit_roots: &[PathBuf],
    existing_services: &HashSet<String>,
) -> Vec<ServiceConfigFileDiscovery> {
    discover_service_config_files(units, unit_roots, existing_services, |path| {
        let mapped = host_path_for_discovery(&path.to_string_lossy());
        path_exists_for_discovery(&mapped)
    })
}

pub(crate) fn existing_service_units(store: &Store) -> Result<HashSet<String>, StoreError> {
    let nodes = store.list_nodes_by_kind_typed(twin_core::NodeKind::Service)?;
    Ok(nodes
        .iter()
        .filter_map(|n| n.id().as_str().strip_prefix("service:").map(str::to_string))
        .collect())
}
