use std::collections::{BTreeSet, HashMap};

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::error::StoreError;
use crate::repo::observation::ObservationRow;
use crate::store::Store;

/// Keys that change on every sighting of an unchanged fact (sampling time, table line).
const VOLATILE_KEYS: [&str; 3] = ["sample_at_ns", "sample_index", "raw_line"];
const RAW_REF_KEY: &str = "__raw_ref";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObservationMergeProgress {
    pub checked: usize,
    pub merged: usize,
    pub pending: usize,
}

struct MergeRow {
    row: ObservationRow,
    fingerprint: String,
}

impl Store {
    /// Folds up to `limit` newly inserted observations into the row that holds the same
    /// fact, when that row continues the same sighting: it was seen by the same collector's
    /// current or previous run, no node incarnation reset sealed it, and it supports exactly
    /// the same relationships. Anything else stays a separate row, so a fact that
    /// disappears and reappears, or a reborn process or edge, starts a new incarnation.
    pub fn merge_repeated_observations(
        &mut self,
        limit: usize,
    ) -> Result<ObservationMergeProgress, StoreError> {
        self.with_transaction(|store| {
            let pending = pending_rows(&store.conn, limit)?;
            let snapshot_cited = pending_rows_cited_by_snapshots(&store.conn)?;
            let mut progress = ObservationMergeProgress::default();
            let mut previous_runs: HashMap<i64, Option<i64>> = HashMap::new();
            for pending_row in pending {
                progress.checked += 1;
                let candidate = merge_candidate(&store.conn, &pending_row, &mut previous_runs)?;
                match candidate {
                    Some(candidate) => {
                        fold_into(&store.conn, &pending_row.row, &candidate, snapshot_cited)?;
                        progress.merged += 1;
                    }
                    None => {
                        execute_cached(
                            &store.conn,
                            "UPDATE observations SET merge_pending = 0 WHERE id = ?1",
                            params![pending_row.row.id],
                        )?;
                    }
                }
            }
            progress.pending = store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM observations WHERE merge_pending = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|source| StoreError::Query { source })?
                .try_into()
                .unwrap_or(usize::MAX);
            Ok(progress)
        })
    }

    pub fn merge_all_repeated_observations(
        &mut self,
        batch: usize,
    ) -> Result<ObservationMergeProgress, StoreError> {
        let mut total = ObservationMergeProgress::default();
        loop {
            let step = self.merge_repeated_observations(batch.max(1))?;
            total.checked += step.checked;
            total.merged += step.merged;
            total.pending = step.pending;
            if step.pending == 0 || step.checked == 0 {
                return Ok(total);
            }
        }
    }
}

/// 128-bit FNV-1a of the canonical fact. At this width an accidental collision is not a
/// practical concern, so equal fingerprints are treated as the same fact.
pub(crate) fn fingerprint(row: &ObservationRow) -> String {
    let key = canonical_key(row);
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    for byte in key.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000000001000000000000000000013b);
    }
    format!("{hash:032x}")
}

fn canonical_key(row: &ObservationRow) -> String {
    let metadata = match serde_json::from_str::<Value>(&row.metadata_json) {
        Ok(Value::Object(mut map)) => {
            let raw_line = map.get("raw_line").and_then(Value::as_u64);
            for key in VOLATILE_KEYS {
                map.remove(key);
            }
            if let (Some(line), Some(Value::String(raw_ref))) = (raw_line, map.get_mut(RAW_REF_KEY))
            {
                if let Some(base) = raw_ref.strip_suffix(&format!(":{line}")) {
                    *raw_ref = base.to_string();
                }
            }
            serde_json::to_string(&sorted(Value::Object(map))).unwrap_or_default()
        }
        _ => row.metadata_json.clone(),
    };
    [
        row.source.as_str(),
        row.kind.as_str(),
        row.subject_node_id.as_deref().unwrap_or(""),
        row.object_node_id.as_deref().unwrap_or(""),
        row.confidence_hint.as_str(),
        row.redaction_state.as_str(),
        metadata.as_str(),
    ]
    .join("\u{1f}")
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

pub(crate) fn backfill_fingerprints(conn: &Connection) -> Result<(), rusqlite::Error> {
    let rows = {
        let mut statement = conn.prepare(&format!(
            "SELECT {} FROM observations",
            super::observation::OBSERVATION_COLUMNS
        ))?;
        let rows = statement
            .query_map([], super::observation::row_from_observation)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut update =
        conn.prepare("UPDATE observations SET fingerprint = ?2, merge_pending = 1 WHERE id = ?1")?;
    for row in rows {
        update.execute(params![row.id, fingerprint(&row)])?;
    }
    Ok(())
}

fn pending_rows(conn: &Connection, limit: usize) -> Result<Vec<MergeRow>, StoreError> {
    let mut statement = conn
        .prepare(&format!(
            "SELECT {}, fingerprint FROM observations WHERE merge_pending = 1
             ORDER BY timestamp_ns, rowid LIMIT ?1",
            super::observation::OBSERVATION_COLUMNS
        ))
        .map_err(|source| StoreError::Query { source })?;
    let rows = statement
        .query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], merge_row)
        .map_err(|source| StoreError::Query { source })?;
    super::observation::collect_rows(rows)
}

fn merge_row(row: &rusqlite::Row<'_>) -> Result<MergeRow, rusqlite::Error> {
    let observation = super::observation::row_from_observation(row)?;
    let fingerprint = row
        .get::<_, Option<String>>(super::observation::OBSERVATION_COLUMN_COUNT)?
        .unwrap_or_else(|| fingerprint(&observation));
    Ok(MergeRow {
        row: observation,
        fingerprint,
    })
}

fn merge_candidate(
    conn: &Connection,
    pending: &MergeRow,
    previous_runs: &mut HashMap<i64, Option<i64>>,
) -> Result<Option<ObservationRow>, StoreError> {
    let mut statement = conn
        .prepare_cached(&format!(
            "SELECT {}, fingerprint FROM observations
             WHERE fingerprint = ?1 AND merge_pending = 0 AND id <> ?2
             ORDER BY last_seen_ns DESC, rowid DESC LIMIT 1",
            super::observation::OBSERVATION_COLUMNS
        ))
        .map_err(|source| StoreError::Query { source })?;
    let mut rows = statement
        .query_map(params![pending.fingerprint, pending.row.id], merge_row)
        .map_err(|source| StoreError::Query { source })?;
    let Some(candidate) = rows
        .next()
        .transpose()
        .map_err(|source| StoreError::Query { source })?
        .map(|row| row.row)
    else {
        return Ok(None);
    };
    if candidate.last_seen_ns > pending.row.last_seen_ns
        || !same_run_sequence(conn, &pending.row, &candidate, previous_runs)?
        || links(conn, &candidate.id)? != links(conn, &pending.row.id)?
    {
        return Ok(None);
    }
    Ok(Some(candidate))
}

fn same_run_sequence(
    conn: &Connection,
    pending: &ObservationRow,
    candidate: &ObservationRow,
    previous_runs: &mut HashMap<i64, Option<i64>>,
) -> Result<bool, StoreError> {
    let Some(run) = pending.collector_run_id else {
        return Ok(candidate.collector_run_id.is_none());
    };
    let Some(candidate_run) = candidate.collector_run_id else {
        return Ok(false);
    };
    if candidate_run == run {
        return Ok(true);
    }
    let previous = match previous_runs.get(&run) {
        Some(previous) => *previous,
        None => {
            let previous = conn
                .query_row(
                    "SELECT MAX(id) FROM collector_runs
                     WHERE id < ?1 AND collector = (SELECT collector FROM collector_runs WHERE id = ?1)",
                    params![run],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(|source| StoreError::Query { source })?;
            previous_runs.insert(run, previous);
            previous
        }
    };
    Ok(previous == Some(candidate_run))
}

fn links(
    conn: &Connection,
    observation_id: &str,
) -> Result<BTreeSet<(String, String)>, StoreError> {
    let mut statement = conn
        .prepare_cached("SELECT edge_id, role FROM edge_observations WHERE observation_id = ?1")
        .map_err(|source| StoreError::Query { source })?;
    let rows = statement
        .query_map(params![observation_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|source| StoreError::Query { source })?;
    rows.map(|row| row.map_err(|source| StoreError::Query { source }))
        .collect()
}

fn pending_rows_cited_by_snapshots(conn: &Connection) -> Result<bool, StoreError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM snapshot_edge_observations s
                        JOIN observations o ON o.id = s.observation_id
                        WHERE o.merge_pending = 1)",
        [],
        |row| row.get(0),
    )
    .map_err(|source| StoreError::Query { source })
}

fn fold_into(
    conn: &Connection,
    pending: &ObservationRow,
    candidate: &ObservationRow,
    snapshot_cited: bool,
) -> Result<(), StoreError> {
    execute_cached(
        conn,
        "UPDATE observations SET
            last_seen_ns = ?2,
            seen_count = seen_count + ?3,
            metadata_json = ?4,
            collector_run_id = ?5
         WHERE id = ?1",
        params![
            candidate.id,
            pending.last_seen_ns,
            pending.seen_count,
            pending.metadata_json,
            pending.collector_run_id,
        ],
    )?;
    if snapshot_cited {
        repoint_snapshot_proof(conn, pending, candidate)?;
    }
    for sql in [
        "DELETE FROM edge_observations WHERE observation_id = ?1",
        "DELETE FROM observations WHERE id = ?1",
    ] {
        execute_cached(conn, sql, params![pending.id])?;
    }
    Ok(())
}

fn repoint_snapshot_proof(
    conn: &Connection,
    pending: &ObservationRow,
    candidate: &ObservationRow,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE snapshot_edge_observations SET last_seen_ns = MAX(
            COALESCE(last_seen_ns, 0),
            COALESCE((SELECT folded.last_seen_ns FROM snapshot_edge_observations folded
                      WHERE folded.observation_id = ?1
                        AND folded.snapshot_name = snapshot_edge_observations.snapshot_name
                        AND folded.edge_id = snapshot_edge_observations.edge_id
                        AND folded.role = snapshot_edge_observations.role), 0))
         WHERE observation_id = ?2",
        params![pending.id, candidate.id],
    )
    .map_err(|source| StoreError::Upsert { source })?;
    conn.execute(
        "UPDATE OR IGNORE snapshot_edge_observations SET observation_id = ?2 WHERE observation_id = ?1",
        params![pending.id, candidate.id],
    )
    .map_err(|source| StoreError::Upsert { source })?;
    conn.execute(
        "DELETE FROM snapshot_edge_observations WHERE observation_id = ?1",
        params![pending.id],
    )
    .map_err(|source| StoreError::Upsert { source })?;
    Ok(())
}

fn execute_cached(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<usize, StoreError> {
    conn.prepare_cached(sql)
        .and_then(|mut statement| statement.execute(params))
        .map_err(|source| StoreError::Upsert { source })
}
