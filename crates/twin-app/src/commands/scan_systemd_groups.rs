use std::collections::HashMap;

use twin_core::{EdgeKind, GraphEdge, NodeId, ObservationId};
use twin_observation::Observation;
use twin_store::{Store, StoreError};

pub(crate) fn group_unit_dep_observations<F>(
    observations: &[Observation],
    include: F,
) -> HashMap<(String, String), Vec<ObservationId>>
where
    F: Fn(&Observation) -> bool,
{
    let mut map: HashMap<(String, String), Vec<ObservationId>> = HashMap::new();
    for obs in observations {
        if !include(obs) {
            continue;
        }
        let Some(from_unit) = obs.metadata().get("from_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(to_unit) = obs.metadata().get("to_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        map.entry((from_unit.to_string(), to_unit.to_string()))
            .or_default()
            .push(obs.id());
    }
    map
}

pub(crate) enum EdgeObservationLink<'a> {
    AllIds(&'a [ObservationId]),
    Single(&'a ObservationId, &'static str),
}

pub(crate) fn persist_depends_on_group(
    store: &mut Store,
    cache: &mut super::scan::ScanEdgeCache,
    from: &NodeId,
    to: &NodeId,
    link: EdgeObservationLink<'_>,
    build_edge: impl FnOnce(Option<&GraphEdge>) -> GraphEdge,
) -> Result<GraphEdge, StoreError> {
    let existing = super::scan::load_existing_edge(store, cache, from, EdgeKind::DependsOn, to)?;
    let edge = build_edge(existing.as_ref());
    match link {
        EdgeObservationLink::Single(obs_id, role) => {
            super::scan::upsert_edge_with_link(store, &edge, Some((obs_id, role)))?;
        }
        EdgeObservationLink::AllIds(obs_ids) => {
            super::scan::upsert_edge_with_link(store, &edge, None)?;
            for obs_id in obs_ids {
                store.link_edge_observation(edge.id().as_str(), &obs_id.to_string(), "direct")?;
            }
        }
    }
    Ok(edge)
}
