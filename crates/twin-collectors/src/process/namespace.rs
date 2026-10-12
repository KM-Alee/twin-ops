use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use twin_core::{NetworkNamespaceId, TimestampNs};
use twin_observation::{RawEvidenceRef, RawIdentity, RawObservation};

use super::{
    collector::{collect_inet_sockets, collect_unix_sockets},
    InetConnectionRecord, InetSocketRecord, ProcReader, ProcessRecord, ProcessWarning,
    ProcessWarningKind, SocketOwner, UnixConnectionRecord, UnixSocketRecord,
};

#[derive(Debug)]
pub struct NetworkNamespaceBatch {
    pub namespace: NetworkNamespaceId,
    pub inet_listeners: Vec<InetSocketRecord>,
    pub inet_connections: Vec<InetConnectionRecord>,
    pub unix_listeners: Vec<UnixSocketRecord>,
    pub unix_connections: Vec<UnixConnectionRecord>,
    pub owners: HashMap<u64, Vec<SocketOwner>>,
}

pub(super) fn read_namespace(reader: &impl ProcReader, path: &Path) -> Option<NetworkNamespaceId> {
    let link = reader.read_link(path).ok()?;
    let value = link.to_str()?.strip_prefix("net:[")?.strip_suffix(']')?;
    value.parse().ok()
}

pub(super) fn collect(
    reader: &impl ProcReader,
    proc_root: &Path,
    records: &[ProcessRecord],
    host: Option<NetworkNamespaceId>,
    at: TimestampNs,
    observations: &mut Vec<RawObservation>,
    warnings: &mut Vec<ProcessWarning>,
) -> Vec<NetworkNamespaceBatch> {
    let mut groups: BTreeMap<NetworkNamespaceId, Vec<ProcessRecord>> = BTreeMap::new();
    for record in records {
        if let Some(namespace) = record
            .network_namespace()
            .filter(|namespace| Some(*namespace) != host)
        {
            groups.entry(namespace).or_default().push(record.clone());
        }
    }
    let mut batches = Vec::new();
    for (namespace, group) in groups {
        let Some(representative) = group.first() else {
            continue;
        };
        let table_root = proc_root.join(representative.pid().to_string());
        let (inet_listeners, inet_connections, mut owners, mut raw) =
            collect_inet_sockets(reader, proc_root, &table_root, &group, at, warnings);
        let (unix_listeners, unix_connections, unix_raw, extra) = collect_unix_sockets(
            reader,
            proc_root,
            &table_root,
            &group,
            at,
            &owners,
            warnings,
        );
        raw.extend(unix_raw);
        for (inode, extra_owners) in extra {
            owners.entry(inode).or_default().extend(extra_owners);
        }
        let stable = group.iter().all(|record| {
            read_namespace(
                reader,
                &proc_root.join(record.pid().to_string()).join("ns/net"),
            ) == Some(namespace)
        });
        if !stable {
            warnings.push(ProcessWarning::new(
                ProcessWarningKind::PermissionDenied,
                table_root.join("ns/net"),
                "network namespace changed during collection; namespace evidence discarded",
            ));
            continue;
        }
        for observation in &mut raw {
            observation
                .metadata
                .insert_str("network_namespace", &namespace.to_string());
            if let Some(line) = observation
                .metadata
                .get("raw_line")
                .and_then(|value| value.as_u64())
            {
                let table = observation
                    .metadata
                    .get("table")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unix");
                observation.raw_ref = Some(RawEvidenceRef::new(format!(
                    "{}:{line}",
                    table_root.join("net").join(table).display()
                )));
            }
            for identity in [&mut observation.subject, &mut observation.object] {
                *identity = identity.take().map(|identity| match identity {
                    RawIdentity::TcpEndpoint { ip, port } => RawIdentity::NamespacedEndpoint {
                        namespace,
                        protocol: twin_core::TransportProtocol::Tcp,
                        ip,
                        port,
                    },
                    RawIdentity::UdpEndpoint { ip, port } => RawIdentity::NamespacedEndpoint {
                        namespace,
                        protocol: twin_core::TransportProtocol::Udp,
                        ip,
                        port,
                    },
                    RawIdentity::UnixSocket { path } => {
                        RawIdentity::NamespacedUnixSocket { namespace, path }
                    }
                    other => other,
                });
            }
        }
        observations.extend(raw);
        batches.push(NetworkNamespaceBatch {
            namespace,
            inet_listeners,
            inet_connections,
            unix_listeners,
            unix_connections,
            owners,
        });
    }
    batches
}
