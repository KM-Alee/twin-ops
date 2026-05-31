use twin_core::ObservationId;

use crate::error::ObservationError;
use crate::normalize::Normalizer;
use crate::observation::Observation;
use crate::raw::RawObservation;
use crate::redact::{BasicRedactor, Redactor};

pub struct Pipeline<R: Redactor> {
    redactor: R,
    normalizer: Normalizer,
}

impl<R: Redactor> Pipeline<R> {
    pub fn new(redactor: R) -> Self {
        Self {
            redactor,
            normalizer: Normalizer,
        }
    }

    pub fn process(&self, mut raw: RawObservation) -> Result<Observation, ObservationError> {
        let redaction_state = self.redactor.redact(&mut raw);
        let subject = raw
            .subject
            .as_ref()
            .map(|i| self.normalizer.normalize(i))
            .transpose()?;
        let object = raw
            .object
            .as_ref()
            .map(|i| self.normalizer.normalize(i))
            .transpose()?;
        Ok(Observation::new(
            ObservationId::new(),
            raw.source,
            raw.kind,
            subject,
            object,
            raw.timestamp,
            raw.raw_ref,
            raw.confidence_hint,
            redaction_state,
            raw.metadata,
        ))
    }
}

impl Default for Pipeline<BasicRedactor> {
    fn default() -> Self {
        Self::new(BasicRedactor)
    }
}
