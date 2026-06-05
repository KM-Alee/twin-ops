use std::collections::HashMap;
use std::str::FromStr;

use twin_core::ObservationId;
use twin_observation::Observation;
use twin_store::{Store, StoreError};

pub struct EvidenceLoadContext {
    observations: HashMap<String, Observation>,
}

impl EvidenceLoadContext {
    pub fn preload_observations(
        store: &Store,
        observation_ids: &[String],
    ) -> Result<Self, StoreError> {
        let mut valid = Vec::new();
        for obs_id in observation_ids {
            if ObservationId::from_str(obs_id).is_ok() {
                valid.push(obs_id.as_str());
            }
        }
        let observations = store.get_observations_typed_by_ids(&valid)?;
        Ok(Self { observations })
    }

    pub fn observation(&self, obs_id: &str) -> Option<&Observation> {
        self.observations.get(obs_id)
    }
}

pub struct NodeLoadContext {
    nodes: HashMap<String, twin_core::GraphNode>,
}

impl NodeLoadContext {
    pub fn preload_nodes(store: &Store, node_ids: &[&str]) -> Result<Self, StoreError> {
        let nodes = store.get_nodes_typed_by_ids(node_ids)?;
        Ok(Self { nodes })
    }

    pub fn node(&self, id: &str) -> Option<&twin_core::GraphNode> {
        self.nodes.get(id)
    }
}
