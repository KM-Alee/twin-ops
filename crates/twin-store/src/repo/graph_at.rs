use rusqlite::params;
use twin_core::TimestampNs;

use crate::repo::edge::row_from_edge;
use crate::repo::node::row_from_node;
use crate::repo::observation::collect_rows;
use crate::{EdgeRow, NodeRow, Store, StoreError};

impl Store {
    pub fn nodes_at(&self, at: TimestampNs) -> Result<Vec<NodeRow>, StoreError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT node_id, kind, label, state, first_seen_ns, last_seen_ns,
                    valid_from_ns, valid_to_ns, metadata_json
             FROM (SELECT *, ROW_NUMBER() OVER (
                 PARTITION BY node_id ORDER BY recorded_at_ns DESC, id DESC
             ) AS position FROM node_history WHERE recorded_at_ns <= ?1)
             WHERE position = 1 AND state != 'gone' ORDER BY node_id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = statement
            .query_map(params![at.as_i64()], row_from_node)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn edges_at(&self, at: TimestampNs) -> Result<Vec<EdgeRow>, StoreError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT edge_id, from_node_id, to_node_id, kind, class, state,
                    evidence_score, evidence_label, evidence_count,
                    first_seen_ns, last_seen_ns, metadata_json
             FROM (SELECT *, ROW_NUMBER() OVER (
                 PARTITION BY edge_id ORDER BY recorded_at_ns DESC, id DESC
             ) AS position FROM edge_history WHERE recorded_at_ns <= ?1)
             WHERE position = 1 AND state != 'gone' ORDER BY edge_id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = statement
            .query_map(params![at.as_i64()], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }
}
