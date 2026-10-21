//! Dependencies that configuration declares but a single scan may never see in use:
//! endpoints in service environments, HAProxy and Caddy backends, PHP-FPM pool listeners,
//! replication sources and Docker Compose files.
//!
//! Every value is parsed transiently by `twin-config`; only the sanitized endpoint (scheme,
//! host, port or socket path), the setting's name and the declaring file are stored.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use twin_collectors::{ConfigFileSource, EffectiveUnit, ServiceConfigFileDiscovery};
use twin_config::{DeclaredEndpoint, ProxyTarget};
use twin_core::{
    EdgeClass, EdgeKind, EdgeState, GraphEdge, GraphEdgeParts, GraphMetadata, GraphNode, NodeId,
    NodeKind, NodeState, TimestampNs, TransportProtocol,
};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RedactionState,
};
use twin_store::{Store, StoreError};

use super::scan::ScanEdgeCache;
use super::scan_config_files::{
    host_path_for_discovery, insert_for_session, valid_dns_host, ConfigParseWarning,
};
use super::scan_history::ScanHistorySession;

pub(crate) const SOURCE: &str = "config_declared";
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_DECLARATIONS: usize = 4096;

#[derive(Default)]
pub(crate) struct DeclaredNotes {
    pub endpoints: usize,
    pub dependencies: usize,
    pub warnings: Vec<ConfigParseWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    /// The owner connects to the endpoint (an environment URL, a replication source).
    Uses,
    /// The owner forwards requests to the endpoint (HAProxy, Caddy).
    Proxies,
    /// The owner is configured to listen on the endpoint (a PHP-FPM pool).
    Listens,
}

struct Declaration {
    owner: NodeId,
    file: String,
    format: &'static str,
    relation: Relation,
    endpoint: DeclaredEndpoint,
    /// A Compose project, whose single-label hosts name sibling services.
    compose: Option<ComposeScope>,
}

#[derive(Clone)]
struct ComposeScope {
    project: String,
    containers: BTreeMap<String, Vec<NodeId>>,
}

#[derive(Default)]
struct Collected {
    declarations: Vec<Declaration>,
    compose_edges: Vec<ComposeEdge>,
    warnings: Vec<ConfigParseWarning>,
    is_incomplete: bool,
}

struct ComposeEdge {
    from: NodeId,
    to: NodeId,
    file: String,
    key: String,
}

impl Collected {
    fn warn(&mut self, path: impl Into<String>, detail: impl Into<String>) {
        self.warnings.push(ConfigParseWarning {
            is_nginx: false,
            path: path.into(),
            detail: detail.into(),
        });
    }

    fn unreadable(&mut self, path: &str, detail: &str) {
        self.is_incomplete = true;
        self.warn(path, detail);
    }

    fn push(&mut self, declaration: Declaration) {
        if self.declarations.len() >= MAX_DECLARATIONS {
            self.is_incomplete = true;
            return;
        }
        self.declarations.push(declaration);
    }
}

pub(crate) fn persist_declared_in_scan(
    store: &mut Store,
    history: &mut ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    units: &[EffectiveUnit],
    discoveries: &[ServiceConfigFileDiscovery],
    scan_time: TimestampNs,
    dns: &impl twin_collectors::dns::DnsReader,
) -> Result<DeclaredNotes, StoreError> {
    let services = super::scan_config_files::existing_service_units(store)?;
    let mut collected = Collected::default();
    collect_unit_environments(&mut collected, units, &services);
    collect_config_files(&mut collected, discoveries);
    collect_service_directories(&mut collected, &services);
    collect_compose(&mut collected, store)?;
    if collected.is_incomplete {
        history.uncertain_source(SOURCE);
    }

    let hosts: BTreeSet<String> = collected
        .declarations
        .iter()
        .filter(|declaration| declaration.compose.is_none() || is_dns_name(&declaration.endpoint))
        .filter_map(|declaration| match &declaration.endpoint.target {
            ProxyTarget::Tcp { host, .. } if is_dns_name(&declaration.endpoint) => {
                Some(host.clone())
            }
            _ => None,
        })
        .collect();
    let answers = dns.resolve(&hosts);

    let mut notes = DeclaredNotes::default();
    let Collected {
        declarations,
        compose_edges,
        mut warnings,
        ..
    } = collected;
    let listeners = declared_listeners(&declarations);
    for declaration in &declarations {
        let targets = match targets(declaration, &answers) {
            Ok(targets) => targets,
            Err(detail) => {
                warnings.push(ConfigParseWarning {
                    is_nginx: false,
                    path: declaration.file.clone(),
                    detail,
                });
                continue;
            }
        };
        for target in targets {
            notes.endpoints += 1;
            notes.dependencies += persist_declaration(
                store,
                history,
                edge_cache,
                declaration,
                &target,
                &listeners,
                scan_time,
            )?;
        }
    }
    for edge in &compose_edges {
        notes.dependencies += persist_compose_edge(store, history, edge_cache, edge, scan_time)?;
    }
    notes.warnings = warnings;
    Ok(notes)
}

fn collect_unit_environments(
    collected: &mut Collected,
    units: &[EffectiveUnit],
    services: &HashSet<String>,
) {
    for unit in units {
        if !unit.unit_name.ends_with(".service") || !services.contains(&unit.unit_name) {
            continue;
        }
        let owner = NodeId::service(&unit.unit_name);
        let unit_file = crate::paths::logical_path(&unit.main_path);
        let variables = unit
            .environment
            .variables
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()));
        for endpoint in twin_config::endpoints_in_environment(variables) {
            collected.push(Declaration {
                owner: owner.clone(),
                file: unit_file.clone(),
                format: "systemd_environment",
                relation: Relation::Uses,
                endpoint,
                compose: None,
            });
        }
        for file in &unit.environment.files {
            let path = file.path.display().to_string();
            let text = match read_bounded(&host_path_for_discovery(&path)) {
                Ok(text) => text,
                Err(ReadFailure::Missing) if file.is_optional => continue,
                Err(failure) => {
                    collected.unreadable(&path, failure.detail());
                    continue;
                }
            };
            for endpoint in environment_file_endpoints(&text) {
                collected.push(Declaration {
                    owner: owner.clone(),
                    file: path.clone(),
                    format: "environment_file",
                    relation: Relation::Uses,
                    endpoint,
                    compose: None,
                });
            }
        }
    }
}

fn environment_file_endpoints(text: &str) -> Vec<DeclaredEndpoint> {
    let assignments = twin_config::parse_env_file(text);
    let lines: BTreeMap<&str, usize> = assignments
        .iter()
        .map(|assignment| (assignment.name.as_str(), assignment.line))
        .collect();
    twin_config::endpoints_in_environment(
        assignments
            .iter()
            .map(|assignment| (assignment.name.as_str(), assignment.value.as_str())),
    )
    .into_iter()
    .map(|endpoint| {
        let first_key = endpoint.key.split('+').next().unwrap_or_default();
        let line = lines.get(first_key).copied().unwrap_or(0);
        endpoint.at_line(line)
    })
    .collect()
}

fn collect_config_files(collected: &mut Collected, discoveries: &[ServiceConfigFileDiscovery]) {
    for discovery in discoveries {
        if discovery.source != ConfigFileSource::KnownServiceConfigPath {
            continue;
        }
        let Some(parser) = config_parser(&discovery.file_path) else {
            continue;
        };
        let path = discovery.file_path.display().to_string();
        let text = match read_bounded(&host_path_for_discovery(&path)) {
            Ok(text) => text,
            Err(failure) => {
                collected.unreadable(&path, failure.detail());
                continue;
            }
        };
        let owner = NodeId::service(&discovery.service_unit);
        let (format, relation, endpoints, warnings) = parser(&text);
        for warning in warnings {
            collected.warn(path.clone(), warning);
        }
        for endpoint in endpoints {
            collected.push(Declaration {
                owner: owner.clone(),
                file: path.clone(),
                format,
                relation,
                endpoint,
                compose: None,
            });
        }
    }
}

type ParsedFile = (&'static str, Relation, Vec<DeclaredEndpoint>, Vec<String>);

fn config_parser(path: &Path) -> Option<fn(&str) -> ParsedFile> {
    let name = path.file_name()?.to_str()?;
    Some(match name {
        "haproxy.cfg" => |text| {
            let parsed = twin_config::parse_haproxy(text);
            (
                "haproxy",
                Relation::Proxies,
                parsed.servers,
                parsed.warnings,
            )
        },
        "Caddyfile" => |text| {
            let parsed = twin_config::parse_caddy(text);
            (
                "caddy",
                Relation::Proxies,
                parsed.upstreams,
                parsed.warnings,
            )
        },
        "redis.conf" | "valkey.conf" => |text| {
            let endpoints = twin_config::redis_primary(text).into_iter().collect();
            ("redis_replicaof", Relation::Uses, endpoints, Vec::new())
        },
        "postgresql.conf" | "postgresql.auto.conf" | "recovery.conf" => |text| {
            let endpoints = twin_config::postgres_primary(text).into_iter().collect();
            (
                "postgres_primary_conninfo",
                Relation::Uses,
                endpoints,
                Vec::new(),
            )
        },
        "master.info" => |text| {
            let endpoints = twin_config::mysql_master_info(text).into_iter().collect();
            ("mysql_master_info", Relation::Uses, endpoints, Vec::new())
        },
        _ if path
            .extension()
            .is_some_and(|extension| extension == "conf")
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|parent| parent == "pool.d" || parent == "php-fpm.d") =>
        {
            |text| {
                let pools = twin_config::parse_fpm_pools(text);
                ("php_fpm_pool", Relation::Listens, pools, Vec::new())
            }
        }
        _ => return None,
    })
}

/// Service files that live in directories rather than at one known path: PHP-FPM pools,
/// PostgreSQL data directories and MariaDB's replication state. Each is read only when the
/// service that owns it is in the graph.
fn collect_service_directories(collected: &mut Collected, services: &HashSet<String>) {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let fpm_services: Vec<&String> = services
        .iter()
        .filter(|unit| unit.contains("php") && unit.contains("fpm"))
        .collect();
    if !fpm_services.is_empty() {
        for (directory, version_service) in [
            ("/etc/php", true),
            ("/etc/php-fpm.d", false),
            ("/etc/php/php-fpm.d", false),
        ] {
            if version_service {
                for version in list_dir(directory) {
                    let Some(version_name) = version.file_name().and_then(|name| name.to_str())
                    else {
                        continue;
                    };
                    let unit = format!("php{version_name}-fpm.service");
                    let Some(owner) = fpm_services
                        .iter()
                        .find(|known| **known == &unit)
                        .or_else(|| fpm_services.first())
                    else {
                        continue;
                    };
                    for pool in list_dir(&format!("{}/fpm/pool.d", version.display())) {
                        found.push(((*owner).clone(), pool));
                    }
                }
            } else if let Some(owner) = fpm_services.first() {
                for pool in list_dir(directory) {
                    found.push(((*owner).clone(), pool));
                }
            }
        }
    }
    for (service, data_dirs) in [
        (
            "postgresql.service",
            &[
                "/var/lib/pgsql/data",
                "/var/lib/postgres/data",
                "/var/lib/postgresql/data",
            ][..],
        ),
        ("mariadb.service", &["/var/lib/mysql"][..]),
        ("mysql.service", &["/var/lib/mysql"][..]),
        ("mysqld.service", &["/var/lib/mysql"][..]),
    ] {
        if !services.contains(service) {
            continue;
        }
        for directory in data_dirs {
            for name in ["postgresql.auto.conf", "master.info"] {
                let path = PathBuf::from(directory).join(name);
                if host_path_for_discovery(&path.display().to_string()).is_file() {
                    found.push((service.to_string(), path));
                }
            }
        }
    }
    for version in list_dir("/var/lib/postgresql") {
        for cluster in list_dir(&version.display().to_string()) {
            let path = cluster.join("postgresql.auto.conf");
            if !host_path_for_discovery(&path.display().to_string()).is_file() {
                continue;
            }
            let instance = format!(
                "postgresql@{}-{}.service",
                version.file_name().unwrap_or_default().to_string_lossy(),
                cluster.file_name().unwrap_or_default().to_string_lossy()
            );
            if services.contains(&instance) {
                found.push((instance, path));
            } else if services.contains("postgresql.service") {
                found.push(("postgresql.service".to_string(), path));
            }
        }
    }
    found.sort();
    found.dedup();
    let discoveries: Vec<ServiceConfigFileDiscovery> = found
        .into_iter()
        .map(|(service_unit, file_path)| ServiceConfigFileDiscovery {
            service_unit,
            file_path,
            source: ConfigFileSource::KnownServiceConfigPath,
        })
        .collect();
    collect_config_files(collected, &discoveries);
}

fn list_dir(logical: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(host_path_for_discovery(logical)) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| Path::new(logical).join(entry.file_name()))
        .collect();
    paths.sort();
    paths
}

/// One Compose project's files and its containers by service name.
type ComposeProject = (BTreeSet<String>, BTreeMap<String, Vec<NodeId>>);

/// Compose files named by running containers' `com.docker.compose.project.config_files`.
fn collect_compose(collected: &mut Collected, store: &Store) -> Result<(), StoreError> {
    let mut projects: BTreeMap<String, ComposeProject> = BTreeMap::new();
    for node in store.list_nodes_by_kind_typed(NodeKind::Container)? {
        if node.state() != NodeState::Active {
            continue;
        }
        let Ok(metadata) = serde_json::from_str::<serde_json::Value>(node.metadata().as_str())
        else {
            continue;
        };
        let (Some(project), Some(service)) = (
            metadata
                .get("compose_project")
                .and_then(|value| value.as_str()),
            metadata
                .get("compose_service")
                .and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        let entry = projects.entry(project.to_string()).or_default();
        for file in metadata
            .get("compose_config_files")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
        {
            entry.0.insert(file.to_string());
        }
        entry
            .1
            .entry(service.to_string())
            .or_default()
            .push(node.id().clone());
    }
    for (project, (files, containers)) in projects {
        let scope = ComposeScope {
            project: project.clone(),
            containers,
        };
        for file in files {
            collect_compose_file(collected, &scope, &file);
        }
    }
    Ok(())
}

fn collect_compose_file(collected: &mut Collected, scope: &ComposeScope, file: &str) {
    let text = match read_bounded(&host_path_for_discovery(file)) {
        Ok(text) => text,
        Err(failure) => {
            collected.unreadable(file, failure.detail());
            return;
        }
    };
    let parsed = match twin_config::parse_compose(&text) {
        Ok(parsed) => parsed,
        Err(detail) => {
            collected.unreadable(file, &detail);
            return;
        }
    };
    for warning in parsed.warnings {
        collected.warn(file, warning);
    }
    let directory = Path::new(file).parent().unwrap_or(Path::new("/"));
    for service in parsed.services {
        let Some(owners) = scope.containers.get(&service.name) else {
            continue;
        };
        for dependency in &service.depends_on {
            for from in owners {
                for to in scope.containers.get(dependency).into_iter().flatten() {
                    collected.compose_edges.push(ComposeEdge {
                        from: from.clone(),
                        to: to.clone(),
                        file: file.to_string(),
                        key: format!("services.{}.depends_on", service.name),
                    });
                }
            }
        }
        let mut endpoints = service.endpoints.clone();
        for env_file in &service.env_files {
            let path = directory.join(env_file).display().to_string();
            match read_bounded(&host_path_for_discovery(&path)) {
                Ok(text) => endpoints.extend(environment_file_endpoints(&text)),
                Err(ReadFailure::Missing) => {}
                Err(failure) => collected.unreadable(&path, failure.detail()),
            }
        }
        for port in &service.ports {
            let host = port.host_ip.as_deref().unwrap_or("0.0.0.0");
            let mut endpoint = DeclaredEndpoint::tcp(
                format!("services.{}.ports", service.name),
                None,
                host,
                port.host_port,
            );
            endpoint.protocol = port.protocol;
            for owner in owners {
                collected.push(Declaration {
                    owner: owner.clone(),
                    file: file.to_string(),
                    format: "compose_ports",
                    relation: Relation::Listens,
                    endpoint: endpoint.clone(),
                    compose: None,
                });
            }
        }
        for endpoint in endpoints {
            for owner in owners {
                collected.push(Declaration {
                    owner: owner.clone(),
                    file: file.to_string(),
                    format: "compose_environment",
                    relation: Relation::Uses,
                    endpoint: endpoint.clone(),
                    compose: Some(scope.clone()),
                });
            }
        }
    }
}

enum ReadFailure {
    Missing,
    Unreadable,
    TooLarge,
}

impl ReadFailure {
    fn detail(&self) -> &'static str {
        match self {
            Self::Missing => "declared file is missing; its endpoints are unknown",
            Self::TooLarge => "declared file is larger than 256 KiB and was not read",
            Self::Unreadable => "declared file is unreadable; its endpoints are unknown",
        }
    }
}

fn read_bounded(path: &Path) -> Result<String, ReadFailure> {
    let file = std::fs::File::open(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadFailure::Missing,
        _ => ReadFailure::Unreadable,
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadFailure::Unreadable)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ReadFailure::TooLarge);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_dns_name(endpoint: &DeclaredEndpoint) -> bool {
    match &endpoint.target {
        ProxyTarget::Tcp { host, .. } => {
            host.parse::<std::net::IpAddr>().is_err()
                && host != "localhost"
                && host.contains('.')
                && valid_dns_host(host)
        }
        _ => false,
    }
}

enum Target {
    Node {
        id: NodeId,
        protocol: TransportProtocol,
        resolved: Option<String>,
    },
    Container(NodeId),
}

fn targets(
    declaration: &Declaration,
    answers: &twin_collectors::dns::DnsAnswers,
) -> Result<Vec<Target>, String> {
    let endpoint = &declaration.endpoint;
    let host = match &endpoint.target {
        ProxyTarget::Unix { path } => {
            let id = NodeId::unix_socket(path).map_err(|error| error.to_string())?;
            return Ok(vec![Target::Node {
                id,
                protocol: endpoint.protocol,
                resolved: None,
            }]);
        }
        ProxyTarget::Named { .. } => return Ok(Vec::new()),
        ProxyTarget::Tcp { host, .. } => host.as_str(),
    };
    let ProxyTarget::Tcp { port, .. } = &endpoint.target else {
        return Ok(Vec::new());
    };
    if let Some(scope) = &declaration.compose {
        if let Some(containers) = scope.containers.get(host) {
            return Ok(containers.iter().cloned().map(Target::Container).collect());
        }
    }
    let address = |ip: &str, resolved: Option<String>| {
        NodeId::inet_port(endpoint.protocol, ip, *port)
            .map(|id| Target::Node {
                id,
                protocol: endpoint.protocol,
                resolved,
            })
            .map_err(|error| error.to_string())
    };
    if host == "localhost" {
        return Ok(vec![address("127.0.0.1", None)?]);
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(vec![address(host, None)?]);
    }
    match answers.get(host) {
        Some(Ok(addresses)) => addresses
            .iter()
            .map(|ip| address(&ip.to_string(), Some(format!("{host} -> {ip}"))))
            .collect(),
        _ if declaration.compose.is_some() => Err(format!(
            "{}: host `{host}` is neither a service of Compose project {} nor resolvable",
            endpoint.key,
            declaration
                .compose
                .as_ref()
                .map_or("", |scope| scope.project.as_str())
        )),
        _ => Err(format!(
            "{}: declared host `{host}` could not be resolved",
            endpoint.key
        )),
    }
}

/// Endpoints configuration says a service listens on, for matching clients to providers
/// when the listener itself was not observed (a stopped service, an unreadable socket).
fn declared_listeners(declarations: &[Declaration]) -> BTreeMap<String, Vec<NodeId>> {
    let mut listeners: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
    for declaration in declarations {
        if declaration.relation != Relation::Listens {
            continue;
        }
        if let Some(key) = listener_key(&declaration.endpoint) {
            let owners = listeners.entry(key).or_default();
            if !owners.contains(&declaration.owner) {
                owners.push(declaration.owner.clone());
            }
        }
    }
    listeners
}

/// Loopback, wildcard and `localhost` addresses of one port share a key: a client of
/// `127.0.0.1:9000` reaches a pool listening on `0.0.0.0:9000`.
fn listener_key(endpoint: &DeclaredEndpoint) -> Option<String> {
    match &endpoint.target {
        ProxyTarget::Unix { path } => Some(format!("unix:{path}")),
        ProxyTarget::Tcp { host, port } => {
            let local = host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
            Some(if local {
                format!("{}:local:{port}", endpoint.protocol)
            } else {
                format!("{}:{host}:{port}", endpoint.protocol)
            })
        }
        ProxyTarget::Named { .. } => None,
    }
}

fn persist_declaration(
    store: &mut Store,
    history: &mut ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    declaration: &Declaration,
    target: &Target,
    listeners: &BTreeMap<String, Vec<NodeId>>,
    scan_time: TimestampNs,
) -> Result<usize, StoreError> {
    let endpoint = &declaration.endpoint;
    let origin = endpoint.origin();
    let file_id = NodeId::file(&declaration.file);
    let existing_file = store.get_node_typed(&file_id)?;
    super::scan_history::upsert_node(
        store,
        history,
        &GraphNode::file(&declaration.file, scan_time, existing_file.as_ref()),
    )?;
    let target_id = match target {
        Target::Container(id) => id.clone(),
        Target::Node { id, protocol, .. } => {
            let existing = store.get_node_typed(id)?;
            let node = match &endpoint.target {
                ProxyTarget::Unix { path } => {
                    GraphNode::unix_socket(path, scan_time, existing.as_ref())
                }
                _ => {
                    let (ip, port) = inet_parts(id).ok_or_else(|| StoreError::Decode {
                        detail: format!("declared endpoint {id} is not an address"),
                    })?;
                    GraphNode::inet_port(*protocol, &ip, port, scan_time, existing.as_ref())
                }
            }
            .map_err(|error| StoreError::Decode {
                detail: error.to_string(),
            })?;
            super::scan_history::upsert_node(store, history, &node)?;
            id.clone()
        }
    };
    let resolved = match target {
        Target::Node { resolved, .. } => resolved.as_deref(),
        Target::Container(_) => None,
    };
    let observation = declared_observation(declaration, &target_id, &origin, resolved, scan_time)?;
    let observation_id = observation.id();
    insert_for_session(store, history, &observation)?;

    let facts = serde_json::json!({
        "source": SOURCE,
        "format": declaration.format,
        "key": endpoint.key,
        "endpoint": origin,
        "declared_in": declaration.file,
    });
    upsert_declared_edge(
        store,
        history,
        edge_cache,
        DeclaredEdge {
            from: &file_id,
            kind: EdgeKind::References,
            to: &target_id,
            class: EdgeClass::Observed,
            metadata: facts.clone(),
        },
        &observation_id,
        "direct",
        scan_time,
    )?;
    let mut owner_facts = facts.clone();
    owner_facts["inference"] = serde_json::json!(match declaration.relation {
        Relation::Listens => "config_declared_listener",
        _ => "config_declared",
    });
    if resolved.is_some() {
        owner_facts["resolved_by"] = serde_json::json!("current_dns");
    }
    upsert_declared_edge(
        store,
        history,
        edge_cache,
        DeclaredEdge {
            from: &declaration.owner,
            kind: EdgeKind::References,
            to: &target_id,
            class: EdgeClass::Inferred,
            metadata: owner_facts.clone(),
        },
        &observation_id,
        "support",
        scan_time,
    )?;
    if declaration.relation == Relation::Listens {
        return Ok(0);
    }
    let providers = match target {
        Target::Container(id) => vec![id.clone()],
        Target::Node { id, .. } => providers(store, id, endpoint, listeners)?,
    };
    let kind = match declaration.relation {
        Relation::Proxies => EdgeKind::ProxiesTo,
        _ => EdgeKind::DependsOn,
    };
    let mut linked = 0;
    for provider in providers {
        if provider == declaration.owner {
            continue;
        }
        let mut metadata = owner_facts.clone();
        metadata["inference"] = serde_json::json!("config_declared");
        metadata["via"] = serde_json::json!(target_id.as_str());
        upsert_declared_edge(
            store,
            history,
            edge_cache,
            DeclaredEdge {
                from: &declaration.owner,
                kind,
                to: &provider,
                class: EdgeClass::Inferred,
                metadata,
            },
            &observation_id,
            "support",
            scan_time,
        )?;
        linked += 1;
    }
    Ok(linked)
}

fn inet_parts(id: &NodeId) -> Option<(String, u16)> {
    let rest = id.as_str().strip_prefix("port:")?;
    let (_, address) = rest.split_once(':')?;
    let (ip, port) = address.rsplit_once(':')?;
    let ip = ip.trim_start_matches('[').trim_end_matches(']');
    Some((ip.to_string(), port.parse().ok()?))
}

/// Services (and containers) that serve an endpoint: observed listeners on the exact
/// address, on the wildcard address of a loopback client's port, and listeners the
/// configuration of another service declares.
fn providers(
    store: &Store,
    target: &NodeId,
    endpoint: &DeclaredEndpoint,
    declared: &BTreeMap<String, Vec<NodeId>>,
) -> Result<Vec<NodeId>, StoreError> {
    let mut ports = vec![target.clone()];
    if let ProxyTarget::Tcp { host, port } = &endpoint.target {
        let is_local = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if is_local {
            for wildcard in ["0.0.0.0", "::"] {
                if let Ok(id) = NodeId::inet_port(endpoint.protocol, wildcard, *port) {
                    ports.push(id);
                }
            }
        }
    }
    let mut found: Vec<NodeId> = Vec::new();
    for port in &ports {
        for row in store.list_active_edges_to(port.as_str())? {
            let edge = GraphEdge::try_from(&row)?;
            let is_listener = match edge.kind() {
                EdgeKind::ListensOn => edge.from().kind() == Some(NodeKind::Service),
                EdgeKind::MapsPort => edge.from().kind() == Some(NodeKind::Container),
                _ => false,
            };
            if is_listener && !found.contains(edge.from()) {
                found.push(edge.from().clone());
            }
        }
    }
    if let Some(owners) = listener_key(endpoint).and_then(|key| declared.get(&key)) {
        for owner in owners {
            if !found.contains(owner) {
                found.push(owner.clone());
            }
        }
    }
    Ok(found)
}

struct DeclaredEdge<'a> {
    from: &'a NodeId,
    kind: EdgeKind,
    to: &'a NodeId,
    class: EdgeClass,
    metadata: serde_json::Value,
}

/// Writes a declared edge. When another writer already holds this edge in the current
/// pass (a live connection, a unit `Requires=`), the declaration only adds its evidence and
/// leaves that writer's edge as it is; an edge nobody else saw this pass becomes declared.
fn upsert_declared_edge(
    store: &mut Store,
    history: &mut ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    edge: DeclaredEdge<'_>,
    observation: &twin_core::ObservationId,
    role: &str,
    scan_time: TimestampNs,
) -> Result<(), StoreError> {
    let existing =
        super::scan::load_existing_edge(store, edge_cache, edge.from, edge.kind, edge.to)?;
    let id = twin_core::EdgeId::new(edge.from, edge.kind, edge.to);
    let held_by_other = existing
        .as_ref()
        .is_some_and(|existing| history.has_seen_edge(id.as_str()) && !is_declared(existing));
    let written = match &existing {
        Some(existing) if held_by_other => existing.clone(),
        _ => GraphEdge::from_stored(GraphEdgeParts {
            from: edge.from.clone(),
            to: edge.to.clone(),
            kind: edge.kind,
            class: edge.class,
            state: EdgeState::Active,
            evidence_count: existing
                .as_ref()
                .map_or(1, |existing| existing.evidence_count().saturating_add(1)),
            first_seen: existing
                .as_ref()
                .map_or(scan_time, |existing| existing.first_seen()),
            last_seen: scan_time,
            metadata: GraphMetadata::from_json(edge.metadata.to_string()),
        }),
    };
    super::scan::upsert_edge_with_link(store, history, &written, Some((observation, role)))?;
    edge_cache.remember(&written);
    Ok(())
}

pub(crate) fn is_declared(edge: &GraphEdge) -> bool {
    serde_json::from_str::<serde_json::Value>(edge.metadata().as_str())
        .ok()
        .and_then(|metadata| {
            metadata
                .get("source")
                .and_then(|source| source.as_str())
                .map(|source| source == SOURCE)
        })
        .unwrap_or(false)
}

fn persist_compose_edge(
    store: &mut Store,
    history: &mut ScanHistorySession,
    edge_cache: &mut ScanEdgeCache,
    edge: &ComposeEdge,
    scan_time: TimestampNs,
) -> Result<usize, StoreError> {
    if edge.from == edge.to {
        return Ok(0);
    }
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("declared_in", &edge.file);
    metadata.insert_str("key", &edge.key);
    metadata.insert_str("format", "compose_depends_on");
    metadata.insert_str("target", edge.to.as_str());
    let observation = declared_fact(
        &edge.from,
        &edge.to,
        &edge.file,
        ConfidenceHint::High,
        metadata,
        scan_time,
    );
    let observation_id = observation.id();
    insert_for_session(store, history, &observation)?;
    upsert_declared_edge(
        store,
        history,
        edge_cache,
        DeclaredEdge {
            from: &edge.from,
            kind: EdgeKind::DependsOn,
            to: &edge.to,
            class: EdgeClass::Observed,
            metadata: serde_json::json!({
                "source": SOURCE,
                "format": "compose_depends_on",
                "key": edge.key,
                "declared_in": edge.file,
            }),
        },
        &observation_id,
        "direct",
        scan_time,
    )?;
    Ok(1)
}

fn declared_observation(
    declaration: &Declaration,
    target: &NodeId,
    origin: &str,
    resolved: Option<&str>,
    scan_time: TimestampNs,
) -> Result<Observation, StoreError> {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("declared_in", &declaration.file);
    metadata.insert_str("key", &declaration.endpoint.key);
    metadata.insert_str("endpoint", origin);
    metadata.insert_str("format", declaration.format);
    metadata.insert_str("target", target.as_str());
    if let Some(resolved) = resolved {
        metadata.insert_str("current_dns", resolved);
    }
    let raw_ref = match declaration.endpoint.line {
        0 => declaration.file.clone(),
        line => format!("{}:{line}", declaration.file),
    };
    let confidence = if resolved.is_some() {
        ConfidenceHint::Low
    } else {
        ConfidenceHint::Moderate
    };
    Ok(declared_fact(
        &declaration.owner,
        target,
        &raw_ref,
        confidence,
        metadata,
        scan_time,
    ))
}

fn declared_fact(
    subject: &NodeId,
    object: &NodeId,
    raw_ref: &str,
    confidence: ConfidenceHint,
    metadata: ObservationMetadata,
    scan_time: TimestampNs,
) -> Observation {
    Observation::from_parts(
        twin_core::ObservationId::new(),
        ObservationSource::ConfigFileDiscovery,
        ObservationKind::ConfigDeclaredEndpoint,
        Some(subject.clone()),
        Some(object.clone()),
        scan_time,
        Some(RawEvidenceRef::new(raw_ref)),
        confidence,
        RedactionState::None,
        metadata,
    )
}
