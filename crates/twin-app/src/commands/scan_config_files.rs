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
    crate::paths::host_path(path)
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
        let path_str = crate::paths::logical_path(&discovery.file_path);
        let file_id = NodeId::file(&path_str);
        let existing_file = store.get_node_typed(&file_id)?;
        let file_node = GraphNode::file(&path_str, scan_time, existing_file.as_ref());
        super::scan_history::upsert_node(store, history, &file_node)?;

        let service_id = NodeId::service(&discovery.service_unit);
        let existing_service = store.get_node_typed(&service_id)?;
        if existing_service.is_none() {
            continue;
        }

        let obs = config_file_observation(discovery, scan_time)?;
        insert_for_session(store, history, &obs)?;
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

pub(crate) fn insert_for_session(
    store: &mut Store,
    history: &super::scan_history::ScanHistorySession,
    observation: &Observation,
) -> Result<(), StoreError> {
    match history.collector_run_id() {
        Some(run) => store.insert_observation_typed_for_run(observation, run),
        None => store.insert_observation_typed(observation),
    }
}

pub(crate) struct ConfigParseWarning {
    pub is_nginx: bool,
    pub path: String,
    pub detail: String,
}

pub(crate) fn apply_nginx_proxies(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    discoveries: &[ServiceConfigFileDiscovery],
    scan_time: TimestampNs,
    dns: &impl twin_collectors::dns::DnsReader,
) -> Result<Vec<ConfigParseWarning>, StoreError> {
    let mut warnings = Vec::new();
    let mut parsed_files = Vec::new();
    let mut origins = std::collections::BTreeMap::new();
    let has_root = discoveries.iter().any(|item| {
        item.service_unit == "nginx.service"
            && item
                .file_path
                .file_name()
                .is_some_and(|name| name == "nginx.conf")
    });
    for discovery in discoveries {
        let path_str = crate::paths::logical_path(&discovery.file_path);
        let is_nginx = discovery.service_unit == "nginx.service"
            && discovery.source == ConfigFileSource::KnownServiceConfigPath;
        let host_path = if discovery.source == ConfigFileSource::KnownServiceConfigPath {
            host_path_for_discovery(&path_str)
        } else {
            discovery.file_path.clone()
        };
        let bytes = match super::nginx_includes::read_config(&host_path) {
            Ok(bytes) => bytes,
            Err(detail) => {
                warnings.push(ConfigParseWarning {
                    is_nginx,
                    path: path_str.clone(),
                    detail: detail.into(),
                });
                continue;
            }
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

        if !is_nginx {
            continue;
        }
        let standalone = twin_config::parse_nginx(&String::from_utf8_lossy(&bytes));
        for name in standalone.upstreams.keys() {
            origins
                .entry((
                    NodeId::service(&discovery.service_unit),
                    name.to_ascii_lowercase(),
                ))
                .or_insert_with(|| file_id.clone());
        }
        if has_root
            && discovery
                .file_path
                .file_name()
                .is_none_or(|name| name != "nginx.conf")
        {
            continue;
        }
        let (text, include_warnings) = super::nginx_includes::contextual_text(
            &discovery.file_path,
            &String::from_utf8_lossy(&bytes),
        );
        warnings.extend(include_warnings);
        let parsed = twin_config::parse_nginx(&text);
        for warning in &parsed.warnings {
            warnings.push(ConfigParseWarning {
                is_nginx,
                path: path_str.clone(),
                detail: warning.clone(),
            });
        }
        let service_id = NodeId::service(&discovery.service_unit);
        if store.get_node_typed(&service_id)?.is_some() {
            parsed_files.push((path_str, file_id, service_id, parsed));
        }
    }
    let mut upstreams = std::collections::BTreeMap::new();
    for (_, file_id, service, parsed) in &parsed_files {
        for (name, targets) in &parsed.upstreams {
            let key = (service.clone(), name.to_ascii_lowercase());
            upstreams
                .entry(key.clone())
                .and_modify(|value| *value = None)
                .or_insert_with(|| {
                    Some((
                        origins
                            .get(&key)
                            .cloned()
                            .unwrap_or_else(|| file_id.clone()),
                        targets.clone(),
                    ))
                });
        }
    }
    let hosts = parsed_files
        .iter()
        .flat_map(|(_, _, _, parsed)| {
            parsed
                .proxies
                .iter()
                .filter(|proxy| {
                    let name = match &proxy.target {
                        twin_config::ProxyTarget::Tcp { host, .. } => Some(host),
                        twin_config::ProxyTarget::Named { name } => Some(name),
                        _ => None,
                    };
                    name.is_none_or(|name| {
                        !parsed
                            .upstreams
                            .contains_key(&twin_config::upstream_key(name, proxy.is_stream))
                    })
                })
                .map(|proxy| &proxy.target)
                .chain(parsed.upstreams.values().flatten())
        })
        .filter_map(|target| match target {
            twin_config::ProxyTarget::Tcp { host, .. }
                if host.parse::<std::net::IpAddr>().is_err() && valid_dns_host(host) =>
            {
                Some(host.to_ascii_lowercase())
            }
            _ => None,
        })
        .collect();
    let answers = dns.resolve(&hosts);
    for (path_str, file_id, service_id, parsed) in parsed_files {
        let mut proxies = Vec::new();
        for proxy in parsed.proxies {
            let name = match &proxy.target {
                twin_config::ProxyTarget::Tcp { host, .. } => Some(host),
                twin_config::ProxyTarget::Named { name } => Some(name),
                _ => None,
            };
            let upstream = name.and_then(|name| {
                upstreams.get(&(
                    service_id.clone(),
                    twin_config::upstream_key(name, proxy.is_stream).to_ascii_lowercase(),
                ))
            });
            match upstream {
                Some(_) if proxy.has_explicit_port => warnings.push(ConfigParseWarning {
                    is_nginx: true,
                    path: path_str.clone(),
                    detail: "named upstream proxy_pass must not specify a port".into(),
                }),
                Some(Some((definition, targets))) if !targets.is_empty() => {
                    for target in targets {
                        proxies.push((
                            twin_config::ProxyPass {
                                has_explicit_port: false,
                                is_stream: proxy.is_stream,
                                protocol: proxy.protocol,
                                raw: format!(
                                    "{} via upstream declared in {}",
                                    proxy.raw, definition
                                ),
                                target: target.clone(),
                            },
                            Some(definition.clone()),
                        ));
                    }
                }
                Some(_) => warnings.push(ConfigParseWarning {
                    is_nginx: true,
                    path: path_str.clone(),
                    detail: "upstream is empty or has ambiguous definitions".into(),
                }),
                None => proxies.push((proxy, None)),
            }
        }
        for (proxy, definition) in proxies {
            match proxy.target {
                twin_config::ProxyTarget::Tcp { host, port } => {
                    let addresses = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                        vec![ip]
                    } else {
                        match answers.get(&host.to_ascii_lowercase()) {
                            Some(Ok(addresses)) => addresses.clone(),
                            _ => {
                                warnings.push(ConfigParseWarning { is_nginx: true, path: path_str.clone(), detail: "proxy hostname resolution unavailable, invalid or over budget".into() });
                                continue;
                            }
                        }
                    };
                    let resolved = host.parse::<std::net::IpAddr>().is_err();
                    for ip in addresses {
                        let address = ip.to_string();
                        let port_id =
                            NodeId::inet_port(proxy.protocol, &address, port).map_err(|err| {
                                StoreError::Decode {
                                    detail: err.to_string(),
                                }
                            })?;
                        let statement = if resolved {
                            format!("{}; current DNS observation {host} -> {address}; configured backend ownership inferred", proxy.raw)
                        } else {
                            proxy.raw.clone()
                        };
                        let proof = if resolved {
                            ProxyProof::CurrentDns(&statement)
                        } else {
                            ProxyProof::Config(&statement)
                        };
                        let existing_port = store.get_node_typed(&port_id)?;
                        let port_node = GraphNode::inet_port(
                            proxy.protocol,
                            &address,
                            port,
                            scan_time,
                            existing_port.as_ref(),
                        )
                        .map_err(|err| StoreError::Decode {
                            detail: err.to_string(),
                        })?;
                        super::scan_history::upsert_node(store, history, &port_node)?;
                        link_reference(
                            store, history, edge_cache, &file_id, &port_id, scan_time, proof,
                        )?;
                        if let Some(definition) = &definition {
                            if definition != &file_id {
                                link_reference(
                                    store, history, edge_cache, definition, &port_id, scan_time,
                                    proof,
                                )?;
                            }
                        }
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
                                    proof,
                                    file: &file_id,
                                },
                            )?;
                        }
                    }
                }
                twin_config::ProxyTarget::Unix { path } => {
                    let Ok(unix_id) = NodeId::unix_socket(&path) else {
                        warnings.push(ConfigParseWarning {
                            is_nginx: true,
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
                        store,
                        history,
                        edge_cache,
                        &file_id,
                        &unix_id,
                        scan_time,
                        ProxyProof::Config(&proxy.raw),
                    )?;
                    for listener in listener_services(store, &unix_id)? {
                        if listener != service_id {
                            link_proxy(
                                store,
                                history,
                                edge_cache,
                                &service_id,
                                &listener,
                                scan_time,
                                ProxyUpstream {
                                    port: &unix_id,
                                    proof: ProxyProof::Config(&proxy.raw),
                                    file: &file_id,
                                },
                            )?;
                        }
                    }
                }
                twin_config::ProxyTarget::Named { name } => {
                    warnings.push(ConfigParseWarning {
                        is_nginx: true,
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
    proof: ProxyProof<'_>,
) -> Result<(), StoreError> {
    let proxy_pass = proof.statement();
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
    let edge = if proof.is_dns() {
        GraphEdge::from_stored(twin_core::GraphEdgeParts { from: edge.from().clone(), to: edge.to().clone(), kind: edge.kind(), class: twin_core::EdgeClass::Inferred, state: edge.state(), evidence_count: edge.evidence_count(), first_seen: edge.first_seen(), last_seen: edge.last_seen(), metadata: twin_core::GraphMetadata::from_json(serde_json::json!({"inference":"configured_hostname_current_dns", "proxy_pass":proxy_pass}).to_string()) })
    } else {
        edge
    };
    let obs = proxy_observation(file_id, target_id, proof, scan_time, file_id.as_str())?;
    let obs_id = obs.id();
    insert_for_session(store, history, &obs)?;
    super::scan::upsert_edge_with_link(store, history, &edge, Some((&obs_id, "direct")))?;
    Ok(())
}

struct ProxyUpstream<'a> {
    port: &'a NodeId,
    proof: ProxyProof<'a>,
    file: &'a NodeId,
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
    let proxy_pass = upstream.proof.statement();
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
    let obs = proxy_observation(
        source,
        port_id,
        upstream.proof,
        scan_time,
        upstream.file.as_str(),
    )?;
    let obs_id = obs.id();
    insert_for_session(store, history, &obs)?;
    super::scan::upsert_edge_with_link(store, history, &edge, Some((&obs_id, "support")))?;
    Ok(())
}

fn proxy_observation(
    subject: &NodeId,
    object: &NodeId,
    proof: ProxyProof<'_>,
    timestamp: TimestampNs,
    source_file: &str,
) -> Result<Observation, StoreError> {
    let proxy_pass = proof.statement();
    let mut meta = ObservationMetadata::new();
    meta.insert_str("proxy_pass", proxy_pass);
    meta.insert_str("target", object.as_str());
    if let Some(scheme) = proxy_scheme(proxy_pass) {
        meta.insert_str("proxy_scheme", scheme);
    }
    let raw = RawObservation {
        source: if proof.is_dns() {
            ObservationSource::SystemDns
        } else {
            ObservationSource::ConfigFileDiscovery
        },
        kind: if proof.is_dns() {
            ObservationKind::DnsAddressResolved
        } else {
            ObservationKind::ConfigProxyPass
        },
        collector: CollectorName::new(CONFIG_DISCOVERY_COLLECTOR),
        subject: identity_for(subject),
        object: identity_for(object),
        timestamp,
        raw_ref: Some(RawEvidenceRef::new(source_file)),
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
            let (protocol, rest) = if let Some(rest) = id.as_str().strip_prefix("port:tcp:") {
                (twin_core::TransportProtocol::Tcp, rest)
            } else {
                (
                    twin_core::TransportProtocol::Udp,
                    id.as_str().strip_prefix("port:udp:")?,
                )
            };
            let (host, port) = rest.rsplit_once(':')?;
            let port = port.parse().ok()?;
            let ip = host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            Some(match protocol {
                twin_core::TransportProtocol::Tcp => RawIdentity::TcpEndpoint { ip, port },
                twin_core::TransportProtocol::Udp => RawIdentity::UdpEndpoint { ip, port },
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
) -> Result<Observation, StoreError> {
    let path_str = crate::paths::logical_path(&discovery.file_path);
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
        .map_err(|err| StoreError::Decode {
            detail: err.to_string(),
        })
}

pub(crate) fn discover_config_files_for_scan(
    units: &[twin_collectors::EffectiveUnit],
    unit_roots: &[PathBuf],
    existing_services: &HashSet<String>,
) -> Vec<ServiceConfigFileDiscovery> {
    let mut discoveries =
        discover_service_config_files(units, unit_roots, existing_services, |path| {
            let mapped = host_path_for_discovery(&path.to_string_lossy());
            path_exists_for_discovery(&mapped)
        });
    let root = host_path_for_discovery("/etc/postgresql");
    if let Ok(versions) = std::fs::read_dir(&root) {
        for version in versions
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let Ok(clusters) = std::fs::read_dir(version.path()) else {
                continue;
            };
            for cluster in clusters
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            {
                let unit = format!(
                    "postgresql@{}-{}.service",
                    version.file_name().to_string_lossy(),
                    cluster.file_name().to_string_lossy()
                );
                let service = if existing_services.contains(&unit) {
                    unit
                } else if existing_services.contains("postgresql.service") {
                    "postgresql.service".to_string()
                } else {
                    continue;
                };
                let Ok(files) = std::fs::read_dir(cluster.path()) else {
                    continue;
                };
                for file in files.flatten() {
                    if !file.file_type().is_ok_and(|kind| kind.is_file())
                        || file
                            .path()
                            .extension()
                            .is_none_or(|extension| extension != "conf")
                    {
                        continue;
                    }
                    let logical = PathBuf::from("/etc/postgresql")
                        .join(version.file_name())
                        .join(cluster.file_name())
                        .join(file.file_name());
                    discoveries.push(ServiceConfigFileDiscovery {
                        service_unit: service.clone(),
                        file_path: logical,
                        source: ConfigFileSource::KnownServiceConfigPath,
                    });
                }
            }
        }
    }
    discoveries.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then_with(|| a.service_unit.cmp(&b.service_unit))
    });
    discoveries
}

pub(crate) fn existing_service_units(store: &Store) -> Result<HashSet<String>, StoreError> {
    let nodes = store.list_nodes_by_kind_typed(twin_core::NodeKind::Service)?;
    Ok(nodes
        .iter()
        .filter_map(|n| n.id().as_str().strip_prefix("service:").map(str::to_string))
        .collect())
}

pub(crate) fn valid_dns_host(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[derive(Clone, Copy)]
enum ProxyProof<'a> {
    Config(&'a str),
    CurrentDns(&'a str),
}
impl<'a> ProxyProof<'a> {
    fn statement(self) -> &'a str {
        match self {
            Self::Config(value) | Self::CurrentDns(value) => value,
        }
    }
    fn is_dns(self) -> bool {
        matches!(self, Self::CurrentDns(_))
    }
}
