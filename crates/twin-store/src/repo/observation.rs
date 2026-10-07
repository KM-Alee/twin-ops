use std::str::FromStr;

use rusqlite::{params, Row};
use twin_core::{NodeId, ObservationId, TimestampNs};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RedactionState,
};

use crate::error::StoreError;
use crate::repo::decode::parse_field;
use crate::store::Store;

const RAW_REF_KEY: &str = "__raw_ref";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRow {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub subject_node_id: Option<String>,
    pub object_node_id: Option<String>,
    pub timestamp_ns: i64,
    pub confidence_hint: String,
    pub redaction_state: String,
    pub metadata_json: String,
    pub collector_run_id: Option<i64>,
}

impl Store {
    pub fn insert_observation(&mut self, obs: &ObservationRow) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO observations (
                    id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                    confidence_hint, redaction_state, metadata_json, collector_run_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    obs.id,
                    obs.source,
                    obs.kind,
                    obs.subject_node_id,
                    obs.object_node_id,
                    obs.timestamp_ns,
                    obs.confidence_hint,
                    obs.redaction_state,
                    obs.metadata_json,
                    obs.collector_run_id,
                ],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(())
    }

    pub fn insert_observations(&mut self, obs: &[ObservationRow]) -> Result<(), StoreError> {
        self.with_transaction(|store| {
            for row in obs {
                store.insert_observation(row)?;
            }
            Ok(())
        })
    }

    pub fn get_observation(&self, id: &str) -> Result<Option<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE id = ?1",
            )
            .map_err(|source| StoreError::Query { source })?;

        let mut rows = stmt
            .query(params![id])
            .map_err(|source| StoreError::Query { source })?;

        match rows.next().map_err(|source| StoreError::Query { source })? {
            Some(row) => Ok(Some(
                row_from_observation(row).map_err(|source| StoreError::Query { source })?,
            )),
            None => Ok(None),
        }
    }

    pub fn delete_observation(&mut self, id: &str) -> Result<(), StoreError> {
        self.conn
            .execute(
                "DELETE FROM edge_observations WHERE observation_id = ?1",
                params![id],
            )
            .map_err(|source| StoreError::Query { source })?;
        self.conn
            .execute("DELETE FROM observations WHERE id = ?1", params![id])
            .map_err(|source| StoreError::Query { source })?;
        Ok(())
    }

    pub fn list_observations_by_kind(&self, kind: &str) -> Result<Vec<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE kind = ?1 ORDER BY timestamp_ns",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map(params![kind], row_from_observation)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_observations_by_source(
        &self,
        source: &str,
    ) -> Result<Vec<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE source = ?1 ORDER BY timestamp_ns",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![source], row_from_observation)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn list_observations_by_subject(
        &self,
        node_id: &str,
    ) -> Result<Vec<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE subject_node_id = ?1 ORDER BY timestamp_ns",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![node_id], row_from_observation)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn count_observations(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })
    }

    pub fn insert_observation_typed(&mut self, obs: &Observation) -> Result<(), StoreError> {
        self.insert_observation(&ObservationRow::from(obs))
    }

    pub fn insert_observation_typed_for_run(
        &mut self,
        obs: &Observation,
        run_id: i64,
    ) -> Result<(), StoreError> {
        let mut row = ObservationRow::from(obs);
        row.collector_run_id = Some(run_id);
        self.insert_observation(&row)
    }

    pub fn get_observation_typed(
        &self,
        id: ObservationId,
    ) -> Result<Option<Observation>, StoreError> {
        match self.get_observation(&id.to_string())? {
            Some(row) => Ok(Some(Observation::try_from(&row)?)),
            None => Ok(None),
        }
    }
}

impl From<&Observation> for ObservationRow {
    fn from(obs: &Observation) -> Self {
        let mut metadata = obs.metadata().clone();
        metadata.remove(RAW_REF_KEY);
        if let Some(raw_ref) = obs.raw_ref() {
            metadata.insert_str(RAW_REF_KEY, raw_ref.as_str());
        }
        ObservationRow {
            id: obs.id().to_string(),
            source: obs.source().to_string(),
            kind: obs.kind().to_string(),
            subject_node_id: obs.subject().map(|n| n.as_str().to_string()),
            object_node_id: obs.object().map(|n| n.as_str().to_string()),
            timestamp_ns: obs.timestamp().as_i64(),
            confidence_hint: obs.confidence_hint().to_string(),
            redaction_state: obs.redaction_state().to_string(),
            metadata_json: metadata.to_json(),
            collector_run_id: None,
        }
    }
}

impl TryFrom<&ObservationRow> for Observation {
    type Error = StoreError;

    fn try_from(row: &ObservationRow) -> Result<Self, Self::Error> {
        decode_observation_row(row)
    }
}

fn decode_observation_row(row: &ObservationRow) -> Result<Observation, StoreError> {
    let id = parse_field("observation id", &row.id, ObservationId::from_str)?;
    let source = parse_field("source", &row.source, ObservationSource::from_str)?;
    let kind = parse_field("kind", &row.kind, ObservationKind::from_str)?;
    let confidence_hint = parse_field(
        "confidence_hint",
        &row.confidence_hint,
        ConfidenceHint::from_str,
    )?;
    let redaction_state = parse_field(
        "redaction_state",
        &row.redaction_state,
        RedactionState::from_str,
    )?;
    let subject = row
        .subject_node_id
        .as_deref()
        .map(|s| parse_field("subject_node_id", s, NodeId::from_str))
        .transpose()?;
    let object = row
        .object_node_id
        .as_deref()
        .map(|s| parse_field("object_node_id", s, NodeId::from_str))
        .transpose()?;
    let mut metadata =
        ObservationMetadata::from_json(&row.metadata_json).map_err(|e| StoreError::Decode {
            detail: e.to_string(),
        })?;
    let raw_ref = metadata
        .remove(RAW_REF_KEY)
        .map(|v| {
            v.as_str()
                .ok_or_else(|| StoreError::Decode {
                    detail: format!("{RAW_REF_KEY} must be a string"),
                })
                .map(|s| RawEvidenceRef::new(s.to_string()))
        })
        .transpose()?;
    Ok(Observation::from_parts(
        id,
        source,
        kind,
        subject,
        object,
        TimestampNs::new(row.timestamp_ns),
        raw_ref,
        confidence_hint,
        redaction_state,
        metadata,
    ))
}

pub(crate) fn row_from_observation(row: &Row<'_>) -> Result<ObservationRow, rusqlite::Error> {
    Ok(ObservationRow {
        id: row.get(0)?,
        source: row.get(1)?,
        kind: row.get(2)?,
        subject_node_id: row.get(3)?,
        object_node_id: row.get(4)?,
        timestamp_ns: row.get(5)?,
        confidence_hint: row.get(6)?,
        redaction_state: row.get(7)?,
        metadata_json: row.get(8)?,
        collector_run_id: row.get(9)?,
    })
}

pub(crate) fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>, StoreError>
where
    F: FnMut(&Row<'_>) -> Result<T, rusqlite::Error>,
{
    rows.map(|row| row.map_err(|source| StoreError::Query { source }))
        .collect()
}
