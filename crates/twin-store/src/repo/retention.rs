use rusqlite::params;

use crate::error::StoreError;
use crate::store::Store;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrunedRows {
    pub observations: usize,
    pub node_history: usize,
    pub edge_history: usize,
}

impl Store {
    pub fn prune_retention(
        &mut self,
        observations_before_ns: i64,
        ebpf_before_ns: i64,
        graph_before_ns: i64,
    ) -> Result<PrunedRows, StoreError> {
        self.with_transaction(|store| {
            store
                .conn
                .execute(
                    "DELETE FROM edge_observations WHERE observation_id IN (
                        SELECT id FROM observations
                        WHERE (source = 'ebpf' AND timestamp_ns < ?2)
                           OR (source != 'ebpf' AND timestamp_ns < ?1)
                    )",
                    params![observations_before_ns, ebpf_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let observations = store
                .conn
                .execute(
                    "DELETE FROM observations
                     WHERE (source = 'ebpf' AND timestamp_ns < ?2)
                        OR (source != 'ebpf' AND timestamp_ns < ?1)",
                    params![observations_before_ns, ebpf_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let node_history = store
                .conn
                .execute(
                    "DELETE FROM node_history WHERE recorded_at_ns < ?1",
                    params![graph_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let edge_history = store
                .conn
                .execute(
                    "DELETE FROM edge_history WHERE recorded_at_ns < ?1",
                    params![graph_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            Ok(PrunedRows {
                observations,
                node_history,
                edge_history,
            })
        })
    }
}
