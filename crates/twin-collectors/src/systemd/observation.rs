use twin_core::CollectorName;
use twin_core::TimestampNs;
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

pub struct UnitDependencyRawInput<'a> {
    pub source: ObservationSource,
    pub collector: &'a str,
    pub kind: ObservationKind,
    pub from_unit: &'a str,
    pub to_unit: &'a str,
    pub key: &'a str,
    pub timestamp: TimestampNs,
    pub raw_ref: RawEvidenceRef,
    pub hint: ConfidenceHint,
    pub extra_metadata: &'a [(&'a str, &'a str)],
}

pub fn unit_dependency_raw(input: UnitDependencyRawInput<'_>) -> RawObservation {
    let mut meta = ObservationMetadata::new();
    meta.insert_str("from_unit", input.from_unit);
    meta.insert_str("to_unit", input.to_unit);
    meta.insert_str("key", input.key);
    for (k, v) in input.extra_metadata {
        meta.insert_str(k, v);
    }
    RawObservation {
        source: input.source,
        kind: input.kind,
        collector: CollectorName::new(input.collector),
        subject: Some(RawIdentity::Service {
            unit: input.from_unit.to_string(),
        }),
        object: Some(RawIdentity::Service {
            unit: input.to_unit.to_string(),
        }),
        timestamp: input.timestamp,
        raw_ref: Some(input.raw_ref),
        confidence_hint: input.hint,
        metadata: meta,
    }
}
