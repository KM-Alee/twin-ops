use std::path::PathBuf;

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::error::CollectorError;
use crate::process::process_record::ProcessRecord;
use crate::process::procfs::{read_process, ProcReader, StdProcReader};
use crate::process::warning::ProcessWarning;

pub const COLLECTOR_NAME: &str = "proc_process";

pub struct ProcessBatch {
    observations: Vec<RawObservation>,
    records: Vec<ProcessRecord>,
    warnings: Vec<ProcessWarning>,
    started_at: TimestampNs,
    ended_at: TimestampNs,
}

impl ProcessBatch {
    pub fn observations(&self) -> &[RawObservation] {
        &self.observations
    }

    pub fn records(&self) -> &[ProcessRecord] {
        &self.records
    }

    pub fn warnings(&self) -> &[ProcessWarning] {
        &self.warnings
    }

    pub fn started_at(&self) -> TimestampNs {
        self.started_at
    }

    pub fn ended_at(&self) -> TimestampNs {
        self.ended_at
    }
}

pub struct ProcessCollector<R: ProcReader = StdProcReader> {
    proc_root: PathBuf,
    reader: R,
}

impl ProcessCollector<StdProcReader> {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            reader: StdProcReader,
        }
    }
}

impl<R: ProcReader> ProcessCollector<R> {
    pub fn with_reader(proc_root: impl Into<PathBuf>, reader: R) -> Self {
        Self {
            proc_root: proc_root.into(),
            reader,
        }
    }

    pub fn collect(&self, started_at: TimestampNs) -> Result<ProcessBatch, CollectorError> {
        if !self.proc_root.is_dir() {
            return Err(CollectorError::InvalidProcRoot {
                path: self.proc_root.clone(),
            });
        }
        let mut warnings = Vec::new();
        let pids = self.reader.list_pids(&self.proc_root)?;
        let mut observations = Vec::new();
        let mut records = Vec::new();

        for pid in pids {
            let Some(record) = read_process(&self.reader, &self.proc_root, pid, &mut warnings)
            else {
                continue;
            };
            observations.extend(raw_observations_for_record(&record, started_at));
            records.push(record);
        }

        Ok(ProcessBatch {
            observations,
            records,
            warnings,
            started_at,
            ended_at: TimestampNs::now(),
        })
    }
}

fn raw_observations_for_record(
    record: &ProcessRecord,
    timestamp: TimestampNs,
) -> Vec<RawObservation> {
    let mut out = Vec::new();
    let process = RawIdentity::Process { pid: record.pid() };
    let stat_ref = RawEvidenceRef::new(format!("/proc/{}/stat", record.pid()));

    let mut seen_meta = ObservationMetadata::new();
    seen_meta.insert_u32("pid", record.pid());
    if let Some(comm) = record.comm() {
        seen_meta.insert_str("comm", comm);
    }
    if let Some(state) = record.state() {
        seen_meta.insert_str("state", state);
    }
    if let Some(uid) = record.uid() {
        seen_meta.insert_u32("uid", uid);
    }
    if let Some(gid) = record.gid() {
        seen_meta.insert_u32("gid", gid);
    }
    out.push(RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(process.clone()),
        object: None,
        timestamp,
        raw_ref: Some(stat_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: seen_meta,
    });

    let mut cmd_meta = ObservationMetadata::new();
    cmd_meta.insert_str_array("argv_json", record.argv());
    cmd_meta.insert_u32("argc", record.argv().len() as u32);
    cmd_meta.insert_bool("command_redacted", false);
    out.push(RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessCommandSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(process.clone()),
        object: None,
        timestamp,
        raw_ref: Some(RawEvidenceRef::new(format!(
            "/proc/{}/cmdline",
            record.pid()
        ))),
        confidence_hint: ConfidenceHint::High,
        metadata: cmd_meta,
    });

    if let Some(exe) = record.exe() {
        let exe_str = exe.to_string_lossy();
        let mut exe_meta = ObservationMetadata::new();
        exe_meta.insert_str("exe_path", &exe_str);
        exe_meta.insert_bool("exe_readable", true);
        out.push(RawObservation {
            source: ObservationSource::Proc,
            kind: ObservationKind::ProcessExeSeen,
            collector: CollectorName::new(COLLECTOR_NAME),
            subject: Some(process.clone()),
            object: Some(RawIdentity::File {
                path: exe_str.into_owned(),
            }),
            timestamp,
            raw_ref: Some(RawEvidenceRef::new(format!("/proc/{}/exe", record.pid()))),
            confidence_hint: ConfidenceHint::High,
            metadata: exe_meta,
        });
    }

    if let Some(ppid) = record.ppid() {
        if ppid > 0 {
            let mut parent_meta = ObservationMetadata::new();
            parent_meta.insert_u32("ppid", ppid);
            parent_meta.insert_u32("pid", record.pid());
            out.push(RawObservation {
                source: ObservationSource::Proc,
                kind: ObservationKind::ProcessParentSeen,
                collector: CollectorName::new(COLLECTOR_NAME),
                subject: Some(RawIdentity::Process { pid: ppid }),
                object: Some(process),
                timestamp,
                raw_ref: Some(RawEvidenceRef::new(format!("/proc/{}/stat", record.pid()))),
                confidence_hint: ConfidenceHint::High,
                metadata: parent_meta,
            });
        }
    }

    out
}
