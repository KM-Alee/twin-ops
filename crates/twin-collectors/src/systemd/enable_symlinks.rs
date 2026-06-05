use std::fs;
use std::path::PathBuf;

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::systemd::warning::{SystemdWarning, SystemdWarningKind};
use crate::CollectorError;

pub const ENABLE_COLLECTOR_NAME: &str = "systemd_enable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnableKind {
    Wants,
    Requires,
}

impl EnableKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wants => "wants",
            Self::Requires => "requires",
        }
    }

    fn dependency_key(self) -> &'static str {
        match self {
            Self::Wants => "Wants",
            Self::Requires => "Requires",
        }
    }
}

pub fn collect_enable_symlinks(
    search_roots: &[PathBuf],
    started_at: TimestampNs,
    warnings: &mut Vec<SystemdWarning>,
) -> Result<Vec<RawObservation>, CollectorError> {
    let mut observations = Vec::new();
    for root in search_roots {
        if !root.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::PathUnreadable,
                    root.clone(),
                    err.to_string(),
                ));
                continue;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let (target_unit, kind) = if let Some(stem) = name_str.strip_suffix(".wants") {
                (stem.to_string(), EnableKind::Wants)
            } else if let Some(stem) = name_str.strip_suffix(".requires") {
                (stem.to_string(), EnableKind::Requires)
            } else {
                continue;
            };
            let dir_path = entry.path();
            let links = match fs::read_dir(&dir_path) {
                Ok(links) => links,
                Err(err) => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::PathUnreadable,
                        dir_path.clone(),
                        err.to_string(),
                    ));
                    continue;
                }
            };
            for link in links.flatten() {
                let file_name = link.file_name();
                let enabled_unit = file_name.to_string_lossy().to_string();
                if !enabled_unit.contains('.') {
                    continue;
                }
                let symlink_path = link.path();
                if fs::symlink_metadata(&symlink_path)
                    .map(|m| !m.file_type().is_symlink())
                    .unwrap_or(true)
                {
                    continue;
                }
                let mut meta = ObservationMetadata::new();
                meta.insert_str("from_unit", &target_unit);
                meta.insert_str("to_unit", &enabled_unit);
                meta.insert_str("enable_kind", kind.as_str());
                meta.insert_str("symlink_path", symlink_path.to_string_lossy().as_ref());
                meta.insert_str("key", kind.dependency_key());
                observations.push(RawObservation {
                    source: ObservationSource::SystemdEnableSymlink,
                    kind: ObservationKind::SystemdUnitWantedBy,
                    collector: CollectorName::new(ENABLE_COLLECTOR_NAME),
                    subject: Some(RawIdentity::Service {
                        unit: target_unit.clone(),
                    }),
                    object: Some(RawIdentity::Service {
                        unit: enabled_unit.clone(),
                    }),
                    timestamp: started_at,
                    raw_ref: Some(RawEvidenceRef::new(symlink_path.to_string_lossy())),
                    confidence_hint: ConfidenceHint::Moderate,
                    metadata: meta,
                });
            }
        }
    }
    Ok(observations)
}
