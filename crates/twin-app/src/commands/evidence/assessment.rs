use std::collections::BTreeSet;
use std::str::FromStr;

use twin_core::{EvidenceFactors, EvidenceSourceKind, ObservationId, TimestampNs};
use twin_observation::{ObservationKind, ObservationSource};
use twin_store::{Store, StoreError};

pub(crate) struct ObservationAssessment {
    pub factors: EvidenceFactors,
    pub supporting_ids: Vec<String>,
    pub missing: usize,
}

pub(crate) fn assess_observations(
    store: &Store,
    ids: &[String],
    at_ns: i64,
) -> Result<ObservationAssessment, StoreError> {
    assess_observations_at(store, ids, at_ns, true)
}

pub(crate) fn assess_snapshot_observations(
    store: &Store,
    ids: &[String],
    at_ns: i64,
) -> Result<ObservationAssessment, StoreError> {
    assess_observations_at(store, ids, at_ns, false)
}

fn assess_observations_at(
    store: &Store,
    ids: &[String],
    at_ns: i64,
    current: bool,
) -> Result<ObservationAssessment, StoreError> {
    let mut factors = EvidenceFactors::none();
    let mut supporting_ids = Vec::new();
    let mut sources = BTreeSet::new();
    let mut newest_ns: Option<i64> = None;
    let mut socket = false;
    let mut config = false;
    let mut ownership = false;
    let mut ebpf = 0u32;
    let mut missing = 0;
    for raw in ids.iter().collect::<BTreeSet<_>>() {
        let observation = match ObservationId::from_str(raw) {
            Ok(id) => store.get_observation_typed(id)?,
            Err(_) => None,
        };
        let Some(obs) = observation.filter(|obs| obs.timestamp().as_i64() <= at_ns) else {
            missing += 1;
            continue;
        };
        supporting_ids.push(raw.clone());
        newest_ns = Some(newest_ns.unwrap_or(0).max(obs.timestamp().as_i64()));
        sources.insert(obs.source().to_string());
        match obs.kind() {
            ObservationKind::EbpfConnect
            | ObservationKind::EbpfAccept
            | ObservationKind::EbpfBind => ebpf = ebpf.saturating_add(1),
            ObservationKind::TcpSocketSeen
            | ObservationKind::TcpConnectionSeen
            | ObservationKind::UdpSocketSeen
            | ObservationKind::UdpConnectionSeen
            | ObservationKind::UnixSocketSeen
            | ObservationKind::UnixConnectionSeen
            | ObservationKind::ProcessSocketFdSeen => socket = true,
            ObservationKind::ProcessBelongsToCgroup | ObservationKind::SystemdCgroupCorrection => {
                ownership = true
            }
            _ => {}
        }
        config |= matches!(
            obs.source(),
            ObservationSource::SystemdUnitFile
                | ObservationSource::SystemdDBus
                | ObservationSource::SystemdEnableSymlink
                | ObservationSource::ConfigFileDiscovery
        );
    }
    factors.source = if ebpf > 0 {
        EvidenceSourceKind::Ebpf
    } else if socket {
        EvidenceSourceKind::SocketTable
    } else if config || ownership || !supporting_ids.is_empty() {
        EvidenceSourceKind::Config
    } else {
        EvidenceSourceKind::None
    };
    factors.runtime_confirmed = socket && ebpf > 0;
    factors.static_confirmed = (config || ownership) && (socket || ebpf > 0);
    factors.repeat_count = if ebpf > 0 {
        ebpf
    } else {
        u32::from(!supporting_ids.is_empty())
    };
    factors.independent_sources = u32::try_from(sources.len()).unwrap_or(u32::MAX);
    factors.recency = super::super::coverage::recency_between(newest_ns, Some(at_ns));
    let coverage = super::super::coverage::load_at(store, at_ns).map_err(|err| match err {
        crate::error::ScanError::Store(source) => source,
        other => StoreError::Decode {
            detail: other.to_string(),
        },
    })?;
    factors.permission_gaps = u32::try_from(
        coverage
            .restricted_processes
            .saturating_add(coverage.unavailable_collectors.len()),
    )
    .unwrap_or(u32::MAX);
    if current {
        factors.dropped_ebpf = super::super::ebpf_tcp::ebpf_drops_recorded(store)?;
        factors.conflicting = super::observations_conflict(store, &supporting_ids)?;
    }
    Ok(ObservationAssessment {
        factors,
        supporting_ids,
        missing,
    })
}

pub(crate) fn assess_edge(
    store: &Store,
    edge: &twin_store::EdgeRow,
    at_ns: i64,
) -> Result<ObservationAssessment, StoreError> {
    let ids = store
        .list_observations_for_edge(&edge.id)?
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    let mut assessment = assess_observations(store, &ids, at_ns)?;
    assessment.factors.conflicting |= super::ownership_conflict(
        store,
        &twin_core::NodeId::from_str(&edge.from_node_id).map_err(|err| StoreError::Decode {
            detail: err.to_string(),
        })?,
    )? || super::ownership_conflict(
        store,
        &twin_core::NodeId::from_str(&edge.to_node_id).map_err(|err| StoreError::Decode {
            detail: err.to_string(),
        })?,
    )?;
    Ok(assessment)
}

pub(crate) fn now_ns() -> i64 {
    TimestampNs::now().as_i64()
}
