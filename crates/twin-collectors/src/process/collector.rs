use std::collections::HashMap;
use std::path::{Path, PathBuf};

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::error::CollectorError;
use crate::process::process_record::ProcessRecord;
use crate::process::procfs::{
    read_fd_socket_owners, read_process, read_tcp_table_content, read_unix_table_content,
    ProcReader, StdProcReader,
};
use crate::process::socket::{
    owners_by_inode, parse_tcp_table, SocketOwner, TcpConnectionRecord, TcpSocketRecord,
    TcpTableKind,
};
use crate::process::unix::{parse_unix_table, UnixConnectionRecord, UnixSocketRecord};
use crate::process::warning::{ProcessWarning, ProcessWarningKind};

pub const COLLECTOR_NAME: &str = "proc_process";

type UnixSocketCollectOutput = (
    Vec<UnixSocketRecord>,
    Vec<UnixConnectionRecord>,
    Vec<RawObservation>,
    HashMap<u64, Vec<SocketOwner>>,
);

pub struct ProcessBatch {
    observations: Vec<RawObservation>,
    records: Vec<ProcessRecord>,
    tcp_listeners: Vec<TcpSocketRecord>,
    tcp_connections: Vec<TcpConnectionRecord>,
    unix_listeners: Vec<UnixSocketRecord>,
    unix_connections: Vec<UnixConnectionRecord>,
    owners_by_inode: HashMap<u64, Vec<SocketOwner>>,
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

    pub fn tcp_listeners(&self) -> &[TcpSocketRecord] {
        &self.tcp_listeners
    }

    pub fn tcp_connections(&self) -> &[TcpConnectionRecord] {
        &self.tcp_connections
    }

    pub fn unix_listeners(&self) -> &[UnixSocketRecord] {
        &self.unix_listeners
    }

    pub fn unix_connections(&self) -> &[UnixConnectionRecord] {
        &self.unix_connections
    }

    pub fn owners_by_inode(&self) -> &HashMap<u64, Vec<SocketOwner>> {
        &self.owners_by_inode
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

    pub fn drain_observations(&mut self) -> Vec<RawObservation> {
        std::mem::take(&mut self.observations)
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

        let (tcp_listeners, tcp_connections, mut owners_by_inode, tcp_observations) =
            collect_tcp_sockets(
                &self.reader,
                &self.proc_root,
                &records,
                started_at,
                &mut warnings,
            );
        observations.extend(tcp_observations);

        let (unix_listeners, unix_connections, unix_observations, mut unix_owners) =
            collect_unix_sockets(
                &self.reader,
                &self.proc_root,
                &records,
                started_at,
                &owners_by_inode,
                &mut warnings,
            );
        observations.extend(unix_observations);
        for (inode, owners) in unix_owners.drain() {
            owners_by_inode.entry(inode).or_default().extend(owners);
        }

        Ok(ProcessBatch {
            observations,
            records,
            tcp_listeners,
            tcp_connections,
            unix_listeners,
            unix_connections,
            owners_by_inode,
            warnings,
            started_at,
            ended_at: TimestampNs::now(),
        })
    }
}

type TcpSocketCollectResult = (
    Vec<TcpSocketRecord>,
    Vec<TcpConnectionRecord>,
    HashMap<u64, Vec<SocketOwner>>,
    Vec<RawObservation>,
);

fn collect_tcp_sockets<R: ProcReader>(
    reader: &R,
    proc_root: &Path,
    records: &[ProcessRecord],
    started_at: TimestampNs,
    warnings: &mut Vec<ProcessWarning>,
) -> TcpSocketCollectResult {
    let mut listeners = Vec::new();
    let mut connections = Vec::new();
    for table in [TcpTableKind::Tcp, TcpTableKind::Tcp6] {
        let Some(content) = read_tcp_table_content(reader, proc_root, table, warnings) else {
            continue;
        };
        let parsed = parse_tcp_table(table, &content);
        listeners.extend(parsed.listeners);
        connections.extend(parsed.connections);
        for issue in parsed.warnings {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::TcpTableMalformed,
                proc_root.join("net").join(table.net_file_name()),
                format!("line {}: {}", issue.line, issue.detail),
            ));
        }
    }

    if listeners.is_empty() && connections.is_empty() {
        return (listeners, connections, HashMap::new(), Vec::new());
    }

    let mut socket_owners = Vec::new();
    for record in records {
        socket_owners.extend(read_fd_socket_owners(
            reader,
            proc_root,
            record.pid(),
            warnings,
        ));
    }
    let owner_map = owners_by_inode(&socket_owners);

    let mut observations = Vec::new();
    for listener in &listeners {
        let owners = owner_map.get(&listener.inode).cloned().unwrap_or_default();
        let mapped = !owners.is_empty();
        if !mapped {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::SocketUnmapped,
                proc_root.join("net").join(listener.table.net_file_name()),
                format!(
                    "inode {} listening on {}:{}",
                    listener.inode, listener.local_ip, listener.local_port
                ),
            ));
        }
        observations.push(raw_observation_for_listener(listener, &owners, started_at));
    }

    for connection in &connections {
        let owners = owner_map
            .get(&connection.inode)
            .cloned()
            .unwrap_or_default();
        if owners.is_empty() {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::ActiveSocketUnmapped,
                proc_root.join("net").join(connection.table.net_file_name()),
                format!(
                    "inode {} established to {}:{}",
                    connection.inode, connection.remote_ip, connection.remote_port
                ),
            ));
        }
        observations.push(raw_observation_for_connection(
            connection, &owners, started_at,
        ));
    }

    (listeners, connections, owner_map, observations)
}

fn collect_unix_sockets<R: ProcReader>(
    reader: &R,
    proc_root: &Path,
    records: &[ProcessRecord],
    started_at: TimestampNs,
    existing_owners: &HashMap<u64, Vec<SocketOwner>>,
    warnings: &mut Vec<ProcessWarning>,
) -> UnixSocketCollectOutput {
    let mut listeners = Vec::new();
    let mut connections = Vec::new();
    let Some(content) = read_unix_table_content(reader, proc_root, warnings) else {
        return (listeners, connections, Vec::new(), HashMap::new());
    };
    let parsed = parse_unix_table(&content);
    listeners.extend(parsed.listeners);
    connections.extend(parsed.connections);
    for issue in parsed.warnings {
        warnings.push(ProcessWarning::new(
            ProcessWarningKind::UnixTableMalformed,
            proc_root.join("net").join("unix"),
            format!("line {}: {}", issue.line, issue.detail),
        ));
    }

    if listeners.is_empty() && connections.is_empty() {
        return (listeners, connections, Vec::new(), HashMap::new());
    }

    let mut supplemental_owners = HashMap::new();
    if existing_owners.is_empty() {
        let mut socket_owners = Vec::new();
        for record in records {
            socket_owners.extend(read_fd_socket_owners(
                reader,
                proc_root,
                record.pid(),
                warnings,
            ));
        }
        supplemental_owners = owners_by_inode(&socket_owners);
    }

    let inode_lookup: &HashMap<u64, Vec<SocketOwner>> = if existing_owners.is_empty() {
        &supplemental_owners
    } else {
        existing_owners
    };

    let mut observations = Vec::new();
    for listener in &listeners {
        let owners = inode_lookup
            .get(&listener.inode)
            .cloned()
            .unwrap_or_default();
        if owners.is_empty() {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::UnixSocketUnmapped,
                proc_root.join("net").join("unix"),
                format!("inode {} listening on {}", listener.inode, listener.path),
            ));
        }
        observations.push(raw_observation_for_unix_listener(
            listener, &owners, started_at,
        ));
    }

    for connection in &connections {
        let owners = inode_lookup
            .get(&connection.inode)
            .cloned()
            .unwrap_or_default();
        if owners.is_empty() {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::UnixConnectionUnmapped,
                proc_root.join("net").join("unix"),
                format!(
                    "inode {} connected on {}",
                    connection.inode,
                    if connection.path.is_empty() {
                        "(no path)"
                    } else {
                        &connection.path
                    }
                ),
            ));
        }
        observations.push(raw_observation_for_unix_connection(
            connection, &owners, started_at,
        ));
    }

    (listeners, connections, observations, supplemental_owners)
}

fn raw_observation_for_unix_listener(
    listener: &UnixSocketRecord,
    owners: &[SocketOwner],
    timestamp: TimestampNs,
) -> RawObservation {
    let raw_ref = RawEvidenceRef::new(format!("/proc/net/unix:{}", listener.raw_line));
    let mut meta = ObservationMetadata::new();
    meta.insert_str("inode", &listener.inode.to_string());
    meta.insert_str("state", "listen");
    meta.insert_str("path", &listener.path);
    meta.insert_u32("raw_line", listener.raw_line as u32);
    meta.insert_bool("mapped", !owners.is_empty());
    let owner_pids: Vec<String> = owners.iter().map(|o| o.pid.to_string()).collect();
    let owner_fds: Vec<String> = owners.iter().map(|o| o.fd_path.clone()).collect();
    meta.insert_str_array("owner_pids", &owner_pids);
    meta.insert_str_array("owner_fds", &owner_fds);
    RawObservation {
        source: ObservationSource::ProcNetUnix,
        kind: ObservationKind::UnixSocketSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(RawIdentity::UnixSocket {
            path: listener.path.clone(),
        }),
        object: None,
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
    }
}

fn raw_observation_for_unix_connection(
    connection: &UnixConnectionRecord,
    owners: &[SocketOwner],
    timestamp: TimestampNs,
) -> RawObservation {
    let raw_ref = RawEvidenceRef::new(format!("/proc/net/unix:{}", connection.raw_line));
    let mut meta = ObservationMetadata::new();
    meta.insert_str("inode", &connection.inode.to_string());
    meta.insert_str("state", "connected");
    meta.insert_str("path", &connection.path);
    meta.insert_u32("raw_line", connection.raw_line as u32);
    meta.insert_bool("mapped", !owners.is_empty());
    let owner_pids: Vec<String> = owners.iter().map(|o| o.pid.to_string()).collect();
    let owner_fds: Vec<String> = owners.iter().map(|o| o.fd_path.clone()).collect();
    meta.insert_str_array("owner_pids", &owner_pids);
    meta.insert_str_array("owner_fds", &owner_fds);
    let subject = if connection.path.is_empty() {
        None
    } else {
        Some(RawIdentity::UnixSocket {
            path: connection.path.clone(),
        })
    };
    RawObservation {
        source: ObservationSource::ProcNetUnix,
        kind: ObservationKind::UnixConnectionSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject,
        object: None,
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
    }
}

fn raw_observation_for_connection(
    connection: &TcpConnectionRecord,
    owners: &[SocketOwner],
    timestamp: TimestampNs,
) -> RawObservation {
    let table_name = match connection.table {
        TcpTableKind::Tcp => "tcp",
        TcpTableKind::Tcp6 => "tcp6",
    };
    let raw_ref = RawEvidenceRef::new(format!("/proc/net/{table_name}:{}", connection.raw_line));
    let mut meta = ObservationMetadata::new();
    meta.insert_str("inode", &connection.inode.to_string());
    meta.insert_str("state", "established");
    meta.insert_str("table", table_name);
    meta.insert_str("local_ip", &connection.local_ip);
    meta.insert_u32("local_port", connection.local_port as u32);
    meta.insert_str("remote_ip", &connection.remote_ip);
    meta.insert_u32("remote_port", connection.remote_port as u32);
    meta.insert_u32("raw_line", connection.raw_line as u32);
    meta.insert_bool("mapped", !owners.is_empty());
    let owner_pids: Vec<String> = owners.iter().map(|o| o.pid.to_string()).collect();
    let owner_fds: Vec<String> = owners.iter().map(|o| o.fd_path.clone()).collect();
    meta.insert_str_array("owner_pids", &owner_pids);
    meta.insert_str_array("owner_fds", &owner_fds);
    RawObservation {
        source: ObservationSource::ProcNetTcp,
        kind: ObservationKind::TcpConnectionSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(RawIdentity::TcpEndpoint {
            ip: connection.remote_ip.clone(),
            port: connection.remote_port,
        }),
        object: None,
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
    }
}

fn raw_observation_for_listener(
    listener: &TcpSocketRecord,
    owners: &[SocketOwner],
    timestamp: TimestampNs,
) -> RawObservation {
    let table_name = match listener.table {
        TcpTableKind::Tcp => "tcp",
        TcpTableKind::Tcp6 => "tcp6",
    };
    let raw_ref = RawEvidenceRef::new(format!("/proc/net/{table_name}:{}", listener.raw_line));
    let mut meta = ObservationMetadata::new();
    meta.insert_str("inode", &listener.inode.to_string());
    meta.insert_str("state", "listen");
    meta.insert_str("table", table_name);
    meta.insert_str("local_ip", &listener.local_ip);
    meta.insert_u32("local_port", listener.local_port as u32);
    meta.insert_u32("raw_line", listener.raw_line as u32);
    meta.insert_bool("mapped", !owners.is_empty());
    let owner_pids: Vec<String> = owners.iter().map(|o| o.pid.to_string()).collect();
    let owner_fds: Vec<String> = owners.iter().map(|o| o.fd_path.clone()).collect();
    meta.insert_str_array("owner_pids", &owner_pids);
    meta.insert_str_array("owner_fds", &owner_fds);
    RawObservation {
        source: ObservationSource::ProcNetTcp,
        kind: ObservationKind::TcpSocketSeen,
        collector: CollectorName::new(COLLECTOR_NAME),
        subject: Some(RawIdentity::TcpEndpoint {
            ip: listener.local_ip.clone(),
            port: listener.local_port,
        }),
        object: None,
        timestamp,
        raw_ref: Some(raw_ref),
        confidence_hint: ConfidenceHint::High,
        metadata: meta,
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

    for membership in record.cgroup_memberships() {
        let mut meta = ObservationMetadata::new();
        meta.insert_u32("pid", record.pid());
        meta.insert_str("cgroup_path", &membership.path);
        meta.insert_str("hierarchy_id", &membership.hierarchy_id);
        if !membership.controllers.is_empty() {
            meta.insert_str_array("controllers", &membership.controllers);
        }
        if let Some(unit) = &membership.service_unit {
            meta.insert_str("service_unit", unit);
            meta.insert_bool("service_inference", true);
        }
        out.push(RawObservation {
            source: ObservationSource::ProcCgroup,
            kind: ObservationKind::ProcessBelongsToCgroup,
            collector: CollectorName::new(COLLECTOR_NAME),
            subject: Some(RawIdentity::Process { pid: record.pid() }),
            object: Some(RawIdentity::Cgroup {
                path: membership.path.clone(),
            }),
            timestamp,
            raw_ref: Some(RawEvidenceRef::new(format!(
                "/proc/{}/cgroup",
                record.pid()
            ))),
            confidence_hint: ConfidenceHint::High,
            metadata: meta,
        });
    }

    out
}
