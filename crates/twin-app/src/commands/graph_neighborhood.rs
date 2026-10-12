use std::collections::HashMap;

use twin_core::{GraphEdge, GraphNode};
use twin_observation::Observation;
use twin_store::{Store, StoreError};

pub(crate) struct NeighborhoodProof {
    nodes: HashMap<String, GraphNode>,
    links: HashMap<String, Vec<(String, String)>>,
    observations: HashMap<String, Observation>,
}

impl NeighborhoodProof {
    pub(crate) fn load<'a>(
        store: &Store,
        edges: impl IntoIterator<Item = &'a GraphEdge>,
    ) -> Result<Self, StoreError> {
        let edges: Vec<_> = edges.into_iter().collect();
        let mut node_ids: Vec<_> = edges
            .iter()
            .flat_map(|edge| [edge.from().as_str(), edge.to().as_str()])
            .collect();
        node_ids.sort_unstable();
        node_ids.dedup();
        let nodes = store.get_nodes_typed_by_ids(&node_ids)?;
        let edge_ids: Vec<_> = edges.iter().map(|edge| edge.id().as_str()).collect();
        let links = store.list_observations_for_edges(&edge_ids)?;
        let mut observation_ids: Vec<_> = links
            .values()
            .flatten()
            .map(|(id, _)| id.as_str())
            .collect();
        observation_ids.sort_unstable();
        observation_ids.dedup();
        // Existing specialized views omit malformed observation rows instead of inventing evidence.
        let observations = store
            .get_observations_by_ids(&observation_ids)?
            .into_iter()
            .filter_map(|(id, row)| {
                Observation::try_from(&row)
                    .ok()
                    .map(|observation| (id, observation))
            })
            .collect();
        Ok(Self {
            nodes,
            links,
            observations,
        })
    }

    pub(crate) fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub(crate) fn observation_ids(&self, edge: &GraphEdge) -> Vec<String> {
        self.links
            .get(edge.id().as_str())
            .into_iter()
            .flatten()
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn observation(&self, id: &str) -> Option<&Observation> {
        self.observations.get(id)
    }
}
