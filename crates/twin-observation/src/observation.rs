use twin_core::{NodeId, ObservationId, TimestampNs};

use crate::error::ObservationError;
use crate::raw::RawEvidenceRef;
use crate::vocab::{ConfidenceHint, ObservationKind, ObservationSource, RedactionState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    id: ObservationId,
    source: ObservationSource,
    kind: ObservationKind,
    subject: Option<NodeId>,
    object: Option<NodeId>,
    timestamp: TimestampNs,
    raw_ref: Option<RawEvidenceRef>,
    confidence_hint: ConfidenceHint,
    redaction_state: RedactionState,
    metadata: ObservationMetadata,
}

#[allow(clippy::too_many_arguments)]
impl Observation {
    pub(crate) fn new(
        id: ObservationId,
        source: ObservationSource,
        kind: ObservationKind,
        subject: Option<NodeId>,
        object: Option<NodeId>,
        timestamp: TimestampNs,
        raw_ref: Option<RawEvidenceRef>,
        confidence_hint: ConfidenceHint,
        redaction_state: RedactionState,
        metadata: ObservationMetadata,
    ) -> Self {
        Self {
            id,
            source,
            kind,
            subject,
            object,
            timestamp,
            raw_ref,
            confidence_hint,
            redaction_state,
            metadata,
        }
    }

    pub fn from_parts(
        id: ObservationId,
        source: ObservationSource,
        kind: ObservationKind,
        subject: Option<NodeId>,
        object: Option<NodeId>,
        timestamp: TimestampNs,
        raw_ref: Option<RawEvidenceRef>,
        confidence_hint: ConfidenceHint,
        redaction_state: RedactionState,
        metadata: ObservationMetadata,
    ) -> Self {
        Self::new(
            id,
            source,
            kind,
            subject,
            object,
            timestamp,
            raw_ref,
            confidence_hint,
            redaction_state,
            metadata,
        )
    }

    pub fn id(&self) -> ObservationId {
        self.id
    }

    pub fn source(&self) -> ObservationSource {
        self.source
    }

    pub fn kind(&self) -> ObservationKind {
        self.kind
    }

    pub fn subject(&self) -> Option<&NodeId> {
        self.subject.as_ref()
    }

    pub fn object(&self) -> Option<&NodeId> {
        self.object.as_ref()
    }

    pub fn timestamp(&self) -> TimestampNs {
        self.timestamp
    }

    pub fn raw_ref(&self) -> Option<&RawEvidenceRef> {
        self.raw_ref.as_ref()
    }

    pub fn confidence_hint(&self) -> ConfidenceHint {
        self.confidence_hint
    }

    pub fn redaction_state(&self) -> RedactionState {
        self.redaction_state
    }

    pub fn metadata(&self) -> &ObservationMetadata {
        &self.metadata
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationMetadata(serde_json::Map<String, serde_json::Value>);

impl ObservationMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_str(&mut self, key: &str, value: &str) {
        self.0.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    pub fn insert_bool(&mut self, key: &str, value: bool) {
        self.0
            .insert(key.to_string(), serde_json::Value::Bool(value));
    }

    pub fn insert_u32(&mut self, key: &str, value: u32) {
        self.0.insert(
            key.to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    pub fn insert_u64(&mut self, key: &str, value: u64) {
        self.0.insert(
            key.to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    pub fn insert_str_array(&mut self, key: &str, values: &[String]) {
        let array = values
            .iter()
            .map(|v| serde_json::Value::String(v.clone()))
            .collect();
        self.0
            .insert(key.to_string(), serde_json::Value::Array(array));
    }

    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.0.remove(key)
    }

    pub fn to_json(&self) -> String {
        serde_json::Value::Object(self.0.clone()).to_string()
    }

    pub fn from_json(s: &str) -> Result<Self, ObservationError> {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(s).map_err(|source| ObservationError::Metadata { source })?;
        Ok(Self(map))
    }
}
