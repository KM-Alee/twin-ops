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
