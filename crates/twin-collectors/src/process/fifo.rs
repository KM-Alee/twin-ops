use std::path::{Component, Path};

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use super::{ProcReader, ProcessRecord, ProcessWarning, ProcessWarningKind};

pub(crate) fn collect_fifo_descriptors(
    reader: &dyn ProcReader,
    proc_root: &Path,
    record: &ProcessRecord,
    timestamp: TimestampNs,
    warnings: &mut Vec<ProcessWarning>,
) -> Vec<RawObservation> {
    let root = proc_root.join(record.pid().to_string());
    let Ok(entries) = reader.read_dir(&root.join("fd")) else {
        return Vec::new();
    };
    let mut observations = Vec::new();
    for entry in entries.iter().take(32768) {
        let Some(fd) = entry
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(target) = reader.read_link(entry) else {
            continue;
        };
        if !target.is_absolute() {
            continue;
        }
        let identity = match reader.file_identity(entry) {
            Ok(identity) => identity,
            Err(_) => {
                warnings.push(ProcessWarning::new(
                    ProcessWarningKind::FdPermissionDenied,
                    entry.clone(),
                    "FIFO descriptor type unavailable",
                ));
                continue;
            }
        };
        if !identity.is_fifo() {
            continue;
        }
        let Some(path) = target
            .to_str()
            .filter(|value| value.len() <= 4096 && !value.chars().any(char::is_control))
        else {
            continue;
        };
        if target
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
            || reader.file_identity(&target).ok() != Some(identity)
            || reader.file_identity(entry).ok() != Some(identity)
            || reader.read_link(entry).ok().as_ref() != Some(&target)
        {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::FdVanished,
                entry.clone(),
                "FIFO host path and descriptor identity do not agree",
            ));
            continue;
        }
        let mut metadata = ObservationMetadata::new();
        metadata.insert_u32("pid", record.pid());
        metadata.insert_u32("fd", fd);
        metadata.insert_str("path", path);
        metadata.insert_u64("device", identity.device());
        metadata.insert_u64("inode", identity.inode());
        if let Some(start) = record.start_ticks() {
            metadata.insert_u64("start_ticks", start);
        }
        if let Some(boot) = record.boot_id() {
            metadata.insert_str("boot_id", boot);
        }
        observations.push(RawObservation {
            source: ObservationSource::Proc,
            kind: ObservationKind::ProcessFifoFdSeen,
            collector: CollectorName::new(super::COLLECTOR_NAME),
            subject: Some(RawIdentity::Process { pid: record.pid() }),
            object: Some(RawIdentity::File { path: path.into() }),
            timestamp,
            raw_ref: Some(RawEvidenceRef::new(entry.to_string_lossy().as_ref())),
            confidence_hint: ConfidenceHint::High,
            metadata,
        });
    }
    if entries.len() > 32768 {
        warnings.push(ProcessWarning::new(
            ProcessWarningKind::FdMalformed,
            root.join("fd"),
            "FIFO descriptor enumeration truncated",
        ));
    }
    if !observations.is_empty()
        && (record.boot_id().is_none()
            || record.start_ticks().is_none_or(|start| {
                !super::procfs::has_start_ticks(reader, &root.join("stat"), start)
            }))
    {
        warnings.push(ProcessWarning::new(
            ProcessWarningKind::FdVanished,
            root.join("stat"),
            "FIFO process incarnation unavailable or changed",
        ));
        observations.clear();
    }
    observations
}
