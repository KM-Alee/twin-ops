use rusqlite::params;

use crate::error::StoreError;
use crate::store::Store;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrunedRows {
    pub observations: usize,
    pub node_history: usize,
    pub edge_history: usize,
    pub collector_runs: usize,
}

impl Store {
    /// History older than `graph_before_ns` is dropped except each entity's newest row
    /// before the cutoff: that row is the entity's state at the cutoff, which historical
    /// reads inside the retention window still need.
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
                        WHERE (source = 'ebpf' AND last_seen_ns < ?2)
                           OR (source != 'ebpf' AND last_seen_ns < ?1)
                    )",
                    params![observations_before_ns, ebpf_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let observations = store
                .conn
                .execute(
                    "DELETE FROM observations
                     WHERE (source = 'ebpf' AND last_seen_ns < ?2)
                        OR (source != 'ebpf' AND last_seen_ns < ?1)",
                    params![observations_before_ns, ebpf_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let node_history = store
                .conn
                .execute(
                    "DELETE FROM node_history WHERE recorded_at_ns < ?1 AND (state = 'gone' OR EXISTS (
                        SELECT 1 FROM node_history newer
                        WHERE newer.node_id = node_history.node_id AND newer.recorded_at_ns <= ?1
                          AND (newer.recorded_at_ns > node_history.recorded_at_ns
                               OR (newer.recorded_at_ns = node_history.recorded_at_ns AND newer.id > node_history.id))
                    ))",
                    params![graph_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            let edge_history = store
                .conn
                .execute(
                    "DELETE FROM edge_history WHERE recorded_at_ns < ?1 AND (state = 'gone' OR EXISTS (
                        SELECT 1 FROM edge_history newer
                        WHERE newer.edge_id = edge_history.edge_id AND newer.recorded_at_ns <= ?1
                          AND (newer.recorded_at_ns > edge_history.recorded_at_ns
                               OR (newer.recorded_at_ns = edge_history.recorded_at_ns AND newer.id > edge_history.id))
                    ))",
                    params![graph_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            // A run stays while anything retained still cites it, and the newest run of
            // each collector stays because coverage reads it.
            let collector_runs = store
                .conn
                .execute(
                    "DELETE FROM collector_runs
                     WHERE ended_at_ns < MIN(?1, ?2)
                       AND id NOT IN (SELECT MAX(id) FROM collector_runs GROUP BY collector)
                       AND NOT EXISTS (SELECT 1 FROM observations WHERE collector_run_id = collector_runs.id)
                       AND NOT EXISTS (SELECT 1 FROM node_history WHERE collector_run_id = collector_runs.id)
                       AND NOT EXISTS (SELECT 1 FROM edge_history WHERE collector_run_id = collector_runs.id)",
                    params![observations_before_ns, graph_before_ns],
                )
                .map_err(|source| StoreError::Query { source })?;
            Ok(PrunedRows {
                observations,
                node_history,
                edge_history,
                collector_runs,
            })
        })
    }
}
