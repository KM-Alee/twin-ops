use std::path::PathBuf;

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::systemd::unit_parse::socket_activation_target;
use crate::systemd::unit_paths::{
    default_search_paths, discover_units, EffectiveUnit, StdUnitFileReader, UnitFileReader,
};
use crate::systemd::warning::SystemdWarning;
use crate::CollectorError;

pub const COLLECTOR_NAME: &str = "systemd_unit";

pub struct SystemdUnitBatch {
    observations: Vec<RawObservation>,
    warnings: Vec<SystemdWarning>,
    units_scanned: usize,
    dependency_count: usize,
    started_at: TimestampNs,
    ended_at: TimestampNs,
}

impl SystemdUnitBatch {
    pub fn observations(&self) -> &[RawObservation] {
        &self.observations
    }

    pub fn warnings(&self) -> &[SystemdWarning] {
        &self.warnings
    }

    pub fn units_scanned(&self) -> usize {
        self.units_scanned
    }

    pub fn dependency_count(&self) -> usize {
        self.dependency_count
    }

    pub fn started_at(&self) -> TimestampNs {
        self.started_at
    }

    pub fn ended_at(&self) -> TimestampNs {
        self.ended_at
    }

    pub fn drain_observations(&mut self) -> Vec<RawObservation> {
        std::mem::take(&mut self.observations)
    }
}

pub struct SystemdUnitCollector<R: UnitFileReader = StdUnitFileReader> {
    search_roots: Vec<PathBuf>,
    reader: R,
}

impl SystemdUnitCollector<StdUnitFileReader> {
    pub fn from_default_paths() -> Self {
        Self::new(default_search_paths())
    }

    pub fn new(search_roots: Vec<PathBuf>) -> Self {
        Self {
            search_roots,
            reader: StdUnitFileReader,
        }
    }
}

impl<R: UnitFileReader> SystemdUnitCollector<R> {
    pub fn with_reader(search_roots: Vec<PathBuf>, reader: R) -> Self {
        Self {
            search_roots,
            reader,
        }
    }

    pub fn collect(&self, started_at: TimestampNs) -> Result<SystemdUnitBatch, CollectorError> {
        let (units, warnings) = discover_units(&self.search_roots, &self.reader);
        let mut observations = Vec::new();
        let mut dependency_count = 0usize;

        for unit in &units {
            observations.push(raw_unit_seen(unit, started_at));
            if let Some(socket) = &unit.socket_config {
                observations.push(raw_socket_activates(unit, socket, started_at));
            }
            for (targets, key, kind) in [
                (
                    unit.dependencies.requires.as_slice(),
                    "Requires",
                    ObservationKind::SystemdUnitRequires,
                ),
                (
                    unit.dependencies.binds_to.as_slice(),
                    "BindsTo",
                    ObservationKind::SystemdUnitRequires,
                ),
                (
                    unit.dependencies.wants.as_slice(),
                    "Wants",
                    ObservationKind::SystemdUnitWants,
                ),
            ] {
                emit_dependencies(
                    &mut observations,
                    &mut dependency_count,
                    unit,
                    targets,
                    key,
                    kind,
                    started_at,
                );
            }
        }

        Ok(SystemdUnitBatch {
            observations,
            warnings,
            units_scanned: units.len(),
            dependency_count,
            started_at,
            ended_at: TimestampNs::now(),
        })
    }
}

fn emit_dependencies(
    observations: &mut Vec<RawObservation>,
    dependency_count: &mut usize,
    unit: &EffectiveUnit,
    targets: &[String],
    key: &str,
    kind: ObservationKind,
    timestamp: TimestampNs,
) {
    for target in targets {
        *dependency_count += 1;
        observations.push(unit_dependency_obs(unit, target, key, kind, timestamp));
    }
}

fn raw_socket_activates(
    unit: &EffectiveUnit,
    socket: &crate::systemd::unit_parse::SocketUnitConfig,
    timestamp: TimestampNs,
) -> RawObservation {
    let target = socket_activation_target(&unit.unit_name, socket);
    let raw_ref = RawEvidenceRef::new(unit.main_path.display().to_string());
    let mut meta = ObservationMetadata::new();
    meta.insert_str("socket_unit", &unit.unit_name);
    meta.insert_str("service_unit", &target);
    meta.insert_str("unit_path", &unit.main_path.display().to_string());
    RawObservation {
        source: ObservationSource::SystemdUnitFile,
        kind: ObservationKind::SystemdSocketActivates,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(RawIdentity::Service {
            unit: unit.unit_name.clone(),
        }),
        object: Some(RawIdentity::Service { unit: target }),
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
    }
}

fn raw_unit_seen(unit: &EffectiveUnit, timestamp: TimestampNs) -> RawObservation {
    let raw_ref = RawEvidenceRef::new(unit.main_path.display().to_string());
    let mut meta = ObservationMetadata::new();
    meta.insert_str("unit", &unit.unit_name);
    meta.insert_str("unit_path", &unit.main_path.display().to_string());
    let kind = if unit.unit_name.ends_with(".socket") {
        ObservationKind::SystemdSocketSeen
    } else {
        ObservationKind::SystemdUnitSeen
    };
    RawObservation {
        source: ObservationSource::SystemdUnitFile,
        kind,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(RawIdentity::Service {
            unit: unit.unit_name.clone(),
        }),
        object: None,
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
    }
}

fn unit_dependency_obs(
    unit: &EffectiveUnit,
    target_unit: &str,
    key: &str,
    kind: ObservationKind,
    timestamp: TimestampNs,
) -> RawObservation {
    super::observation::unit_dependency_raw(super::observation::UnitDependencyRawInput {
        source: ObservationSource::SystemdUnitFile,
        collector: COLLECTOR_NAME,
        kind,
        from_unit: &unit.unit_name,
        to_unit: target_unit,
        key,
        timestamp,
        raw_ref: RawEvidenceRef::new(unit.main_path.display().to_string()),
        hint: if key == "Wants" {
            ConfidenceHint::Moderate
        } else {
            ConfidenceHint::High
        },
        extra_metadata: &[("unit_path", &unit.main_path.display().to_string())],
    })
}
