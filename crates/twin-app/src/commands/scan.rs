use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;

use twin_collectors::{
    ProcessBatch, ProcessCollector, ProcessWarning, ProcessWarningKind, COLLECTOR_NAME,
};
use twin_core::{
    EdgeClass, EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind, ObservationId,
    TimestampNs,
};
use twin_observation::{Observation, ObservationKind};
use twin_store::{CollectorRunRow, Store, StoreError};

use crate::error::{AppError, ScanError};
use crate::model::{ScanResult, ScanWarning, ScanWarningDetail};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::ScanRequest;

pub fn run_home(request: ScanRequest, proc_root: &Path) -> Result<ScanResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    scan_at(&paths.layout, &request, proc_root, &paths.db_path)
}

pub fn run(
    layout: &TwinLayout,
    request: &ScanRequest,
    proc_root: &Path,
) -> Result<ScanResult, AppError> {
    scan_at(layout, request, proc_root, &layout.db_file())
}

fn scan_at(
    layout: &TwinLayout,
    request: &ScanRequest,
    proc_root: &Path,
    db_path: &Path,
) -> Result<ScanResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let started_at = TimestampNs::now();
    let batch = ProcessCollector::new(proc_root).collect(started_at)?;
    persist_scan(db_path, batch)
}

fn persist_scan(db_path: &Path, mut batch: ProcessBatch) -> Result<ScanResult, AppError> {
    let mut store = Store::open(db_path).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }

    let pipeline = twin_observation::Pipeline::default();
    let mut observations: Vec<Observation> = Vec::new();
    for raw in batch.drain_observations() {
        observations.push(pipeline.process(raw).map_err(ScanError::Observation)?);
    }

    let parent_obs_by_child = parent_observation_ids(&observations);
    let cgroup_obs_by_key = cgroup_observation_ids(&observations);
    let tcp_obs_by_inode = tcp_socket_observation_ids(&observations);
    let tcp_conn_obs_by_key = tcp_connection_observation_ids(&observations);
    let scan_time = batch.ended_at();
    let mut process_count = 0usize;
    let mut parent_edge_count = 0usize;
    let mut cgroup_count = 0usize;
    let mut service_count = 0usize;
    let mut in_cgroup_edge_count = 0usize;
    let mut service_owns_edge_count = 0usize;
    let mut port_count = 0usize;
    let mut process_listens_on_edge_count = 0usize;
    let mut service_listens_on_edge_count = 0usize;
    let mut unmapped_listener_socket_count = 0usize;
    let mut tcp_connection_count = 0usize;
    let mut process_connects_to_edge_count = 0usize;
    let mut service_connects_to_edge_count = 0usize;
    let mut service_depends_on_edge_count = 0usize;
    let mut unmapped_active_socket_count = 0usize;
    let mut seen_cgroups: HashSet<NodeId> = HashSet::new();
    let mut seen_services: HashSet<NodeId> = HashSet::new();
    let mut seen_ports: HashSet<NodeId> = HashSet::new();
    let mut seen_service_listeners: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut seen_process_listeners: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut seen_process_connects: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut seen_service_connects: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut seen_service_depends: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut listeners_by_port: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut service_connect_targets: Vec<(NodeId, String, u16, Option<ObservationId>)> = Vec::new();

    let pid_to_services = services_by_pid(batch.records());
    let owners_by_inode = batch.owners_by_inode();
    let listener_by_inode: HashMap<u64, &twin_collectors::TcpSocketRecord> =
        batch.tcp_listeners().iter().map(|l| (l.inode, l)).collect();
    let mut resolved_listeners = Vec::new();
    for listener in batch.tcp_listeners() {
        let port_id = NodeId::port_tcp(&listener.local_ip, listener.local_port)
            .map_err(ScanError::InvalidPortId)?;
        resolved_listeners.push((listener.inode, port_id));
    }

    store
        .with_transaction(|store| {
            let mut listener_obs_by_port: HashMap<NodeId, ObservationId> = HashMap::new();
            let run_id = store.insert_collector_run(&CollectorRunRow {
                id: None,
                collector: COLLECTOR_NAME.to_string(),
                started_at_ns: batch.started_at().as_i64(),
                ended_at_ns: batch.ended_at().as_i64(),
                status: "success".to_string(),
                observation_count: observations.len() as i64,
                warning_count: batch.warnings().len() as i64,
                error_message: None,
            })?;

            for obs in &observations {
                store.insert_observation_typed_for_run(obs, run_id)?;
            }

            for record in batch.records() {
                let node_id = NodeId::process(record.pid());
                let existing = store.get_node_typed(&node_id)?;
                let node =
                    GraphNode::process(record.pid(), record.label(), scan_time, existing.as_ref());
                store.upsert_node_typed(&node)?;
                process_count += 1;

                if let Some(exe) = record.exe() {
                    let exe_str = exe.to_string_lossy();
                    let file_id = NodeId::file(&exe_str);
                    let file_existing = store.get_node_typed(&file_id)?;
                    let file_node = GraphNode::file(&exe_str, scan_time, file_existing.as_ref());
                    store.upsert_node_typed(&file_node)?;
                }

                let process_id = NodeId::process(record.pid());
                for membership in record.cgroup_memberships() {
                    let cgroup_id = NodeId::cgroup(&membership.path);
                    if upsert_node_once(store, &mut seen_cgroups, cgroup_id.clone(), |existing| {
                        GraphNode::cgroup(&membership.path, scan_time, existing)
                    })? {
                        cgroup_count += 1;
                    }

                    let cgroup_obs_id =
                        cgroup_obs_by_key.get(&(record.pid(), membership.path.clone()));
                    let cgroup_obs_support = cgroup_obs_id.map(|id| (id, "support"));
                    let cgroup_obs_direct = cgroup_obs_id.map(|id| (id, "direct"));

                    if let Some(unit) = &membership.service_unit {
                        let service_id = NodeId::service(unit);
                        if upsert_node_once(
                            store,
                            &mut seen_services,
                            service_id.clone(),
                            |existing| GraphNode::service(unit, scan_time, existing),
                        )? {
                            service_count += 1;
                        }

                        for owned in [&process_id, &cgroup_id] {
                            let edge = GraphEdge::inferred_service_owns(
                                &service_id,
                                owned,
                                scan_time,
                                load_existing_edge(store, &service_id, EdgeKind::Owns, owned)?
                                    .as_ref(),
                            );
                            upsert_edge_with_link(store, &edge, cgroup_obs_support)?;
                            service_owns_edge_count += 1;
                        }
                    }

                    let in_cgroup = GraphEdge::observed_in_cgroup(
                        &process_id,
                        &cgroup_id,
                        scan_time,
                        load_existing_edge(store, &process_id, EdgeKind::InCgroup, &cgroup_id)?
                            .as_ref(),
                    );
                    upsert_edge_with_link(store, &in_cgroup, cgroup_obs_direct)?;
                    in_cgroup_edge_count += 1;
                }

                let Some(ppid) = record.ppid() else {
                    continue;
                };
                if ppid == 0 {
                    continue;
                }
                let parent_id = NodeId::process(ppid);
                let child_id = NodeId::process(record.pid());
                let parent_existing = store.get_node_typed(&parent_id)?;
                if parent_existing.is_none() {
                    let parent_node =
                        GraphNode::process(ppid, format!("pid:{ppid}"), scan_time, None);
                    store.upsert_node_typed(&parent_node)?;
                }
                let edge_id = EdgeId::new(&parent_id, EdgeKind::ParentOf, &child_id);
                let edge_existing = store.get_edge(edge_id.as_str())?;
                let existing_edge = edge_existing
                    .as_ref()
                    .and_then(|row| GraphEdge::try_from(row).ok());
                let edge = GraphEdge::observed_parent(
                    &parent_id,
                    &child_id,
                    scan_time,
                    existing_edge.as_ref(),
                );
                store.upsert_edge_typed(&edge)?;
                parent_edge_count += 1;

                if let Some(obs_id) = parent_obs_by_child.get(&record.pid()) {
                    store.link_edge_observation(
                        edge.id().as_str(),
                        &obs_id.to_string(),
                        "support",
                    )?;
                }
            }

            for (inode, port_id) in &resolved_listeners {
                let listener = listener_by_inode[inode];
                let port_existing = store.get_node_typed(port_id)?;
                let port_node = GraphNode::tcp_port(
                    &listener.local_ip,
                    listener.local_port,
                    scan_time,
                    port_existing.as_ref(),
                )
                .map_err(|e| StoreError::Decode {
                    detail: e.to_string(),
                })?;
                store.upsert_node_typed(&port_node)?;
                if seen_ports.insert(port_id.clone()) {
                    port_count += 1;
                }

                let owners = owners_by_inode
                    .get(&listener.inode)
                    .cloned()
                    .unwrap_or_default();
                let socket_obs = tcp_obs_by_inode
                    .get(&listener.inode)
                    .map(|id| (id, "direct"));
                if let Some(obs_id) = tcp_obs_by_inode.get(&listener.inode) {
                    listener_obs_by_port.insert(port_id.clone(), *obs_id);
                }
                if owners.is_empty() {
                    unmapped_listener_socket_count += 1;
                    continue;
                }

                for owner in &owners {
                    let process_id = NodeId::process(owner.pid);
                    let process_existing = store.get_node_typed(&process_id)?;
                    if process_existing.is_none() {
                        let label = batch
                            .records()
                            .iter()
                            .find(|r| r.pid() == owner.pid)
                            .and_then(|r| r.comm().map(str::to_string))
                            .unwrap_or_else(|| format!("pid:{}", owner.pid));
                        store.upsert_node_typed(&GraphNode::process(
                            owner.pid, label, scan_time, None,
                        ))?;
                    }
                    let edge = GraphEdge::observed_process_listens_on(
                        &process_id,
                        port_id,
                        scan_time,
                        load_existing_edge(store, &process_id, EdgeKind::ListensOn, port_id)?
                            .as_ref(),
                    );
                    upsert_edge_with_link(store, &edge, socket_obs)?;
                    if seen_process_listeners.insert((process_id.clone(), port_id.clone())) {
                        process_listens_on_edge_count += 1;
                    }

                    if let Some(services) = pid_to_services.get(&owner.pid) {
                        for service_id in services {
                            if !seen_service_listeners.insert((service_id.clone(), port_id.clone()))
                            {
                                continue;
                            }
                            let edge = GraphEdge::inferred_service_listens_on(
                                service_id,
                                port_id,
                                scan_time,
                                load_existing_edge(
                                    store,
                                    service_id,
                                    EdgeKind::ListensOn,
                                    port_id,
                                )?
                                .as_ref(),
                            );
                            upsert_edge_with_link(store, &edge, socket_obs)?;
                            service_listens_on_edge_count += 1;
                            let listeners = listeners_by_port.entry(port_id.clone()).or_default();
                            if !listeners.contains(service_id) {
                                listeners.push(service_id.clone());
                            }
                        }
                    }
                }
            }

            for connection in batch.tcp_connections() {
                tcp_connection_count += 1;
                let remote_port_id =
                    NodeId::port_tcp(&connection.remote_ip, connection.remote_port).map_err(
                        |e| StoreError::Decode {
                            detail: e.to_string(),
                        },
                    )?;
                let port_existing = store.get_node_typed(&remote_port_id)?;
                let port_node = GraphNode::tcp_port(
                    &connection.remote_ip,
                    connection.remote_port,
                    scan_time,
                    port_existing.as_ref(),
                )
                .map_err(|e| StoreError::Decode {
                    detail: e.to_string(),
                })?;
                store.upsert_node_typed(&port_node)?;
                if seen_ports.insert(remote_port_id.clone()) {
                    port_count += 1;
                }

                let conn_key = ConnectionObservationKey {
                    inode: connection.inode,
                    remote_ip: connection.remote_ip.clone(),
                    remote_port: connection.remote_port,
                    raw_line: connection.raw_line,
                    table: connection.table.net_file_name().to_string(),
                };
                let conn_obs = tcp_conn_obs_by_key.get(&conn_key).map(|id| (id, "direct"));

                let owners = owners_by_inode
                    .get(&connection.inode)
                    .cloned()
                    .unwrap_or_default();
                if owners.is_empty() {
                    unmapped_active_socket_count += 1;
                    continue;
                }

                for owner in &owners {
                    let process_id = NodeId::process(owner.pid);
                    let process_existing = store.get_node_typed(&process_id)?;
                    if process_existing.is_none() {
                        let label = batch
                            .records()
                            .iter()
                            .find(|r| r.pid() == owner.pid)
                            .and_then(|r| r.comm().map(str::to_string))
                            .unwrap_or_else(|| format!("pid:{}", owner.pid));
                        store.upsert_node_typed(&GraphNode::process(
                            owner.pid, label, scan_time, None,
                        ))?;
                    }
                    let edge = GraphEdge::observed_process_connects_to(
                        &process_id,
                        &remote_port_id,
                        scan_time,
                        load_existing_edge(
                            store,
                            &process_id,
                            EdgeKind::ConnectsTo,
                            &remote_port_id,
                        )?
                        .as_ref(),
                    );
                    upsert_edge_with_link(store, &edge, conn_obs)?;
                    if seen_process_connects.insert((process_id.clone(), remote_port_id.clone())) {
                        process_connects_to_edge_count += 1;
                    }

                    if let Some(services) = pid_to_services.get(&owner.pid) {
                        for service_id in services {
                            let edge = GraphEdge::inferred_service_connects_to(
                                service_id,
                                &remote_port_id,
                                scan_time,
                                load_existing_edge(
                                    store,
                                    service_id,
                                    EdgeKind::ConnectsTo,
                                    &remote_port_id,
                                )?
                                .as_ref(),
                            );
                            upsert_edge_with_link(
                                store,
                                &edge,
                                conn_obs.map(|(id, _)| (id, "support")),
                            )?;
                            if seen_service_connects
                                .insert((service_id.clone(), remote_port_id.clone()))
                            {
                                service_connects_to_edge_count += 1;
                                service_connect_targets.push((
                                    service_id.clone(),
                                    connection.remote_ip.clone(),
                                    connection.remote_port,
                                    conn_obs.map(|(id, _)| *id),
                                ));
                            }
                        }
                    }
                }
            }

            for (source_service, remote_ip, remote_port, conn_obs_id) in &service_connect_targets {
                let candidate_ports = listener_port_candidates(remote_ip, *remote_port);
                for listener_port in candidate_ports {
                    let target_services =
                        listener_services_for_port(store, &listeners_by_port, &listener_port)?;
                    for target_service in &target_services {
                        if source_service == target_service {
                            continue;
                        }
                        let listener_obs = listener_obs_by_port
                            .get(&listener_port)
                            .copied()
                            .map(|id| (id, "support"))
                            .or_else(|| {
                                listener_obs_from_store(store, &listener_port, target_service)
                            });
                        let conn_obs = conn_obs_id.as_ref().map(|id| (id, "direct"));
                        let edge = GraphEdge::inferred_service_depends_on(
                            source_service,
                            target_service,
                            scan_time,
                            load_existing_edge(
                                store,
                                source_service,
                                EdgeKind::DependsOn,
                                target_service,
                            )?
                            .as_ref(),
                        );
                        upsert_edge_with_link(store, &edge, conn_obs)?;
                        if let Some(link) = listener_obs {
                            store.link_edge_observation(
                                edge.id().as_str(),
                                &link.0.to_string(),
                                link.1,
                            )?;
                        }
                        if seen_service_depends
                            .insert((source_service.clone(), target_service.clone()))
                        {
                            service_depends_on_edge_count += 1;
                        }
                    }
                }
            }

            Ok(())
        })
        .map_err(ScanError::Store)?;

    let collector_warnings = batch.warnings();
    Ok(ScanResult {
        started_at_ns: batch.started_at().as_i64(),
        ended_at_ns: batch.ended_at().as_i64(),
        process_count,
        parent_edge_count,
        cgroup_count,
        service_count,
        in_cgroup_edge_count,
        service_owns_edge_count,
        tcp_listener_count: resolved_listeners.len(),
        port_count,
        process_listens_on_edge_count,
        service_listens_on_edge_count,
        unmapped_listener_socket_count,
        tcp_connection_count,
        process_connects_to_edge_count,
        service_connects_to_edge_count,
        service_depends_on_edge_count,
        unmapped_active_socket_count,
        socket_owner_inode_count: batch.owners_by_inode().len(),
        observation_count: observations.len(),
        warning_count: collector_warnings.len(),
        warnings: aggregate_warnings(collector_warnings),
        warning_details: detailed_warnings(collector_warnings),
    })
}

fn services_by_pid(records: &[twin_collectors::ProcessRecord]) -> HashMap<u32, Vec<NodeId>> {
    let mut map: HashMap<u32, Vec<NodeId>> = HashMap::new();
    for record in records {
        for membership in record.cgroup_memberships() {
            let Some(unit) = &membership.service_unit else {
                continue;
            };
            let service_id = NodeId::service(unit);
            map.entry(record.pid()).or_default().push(service_id);
        }
    }
    for services in map.values_mut() {
        services.sort();
        services.dedup();
    }
    map
}

fn parent_observation_ids(observations: &[Observation]) -> HashMap<u32, ObservationId> {
    let mut map = HashMap::new();
    for obs in observations {
        if obs.kind() != ObservationKind::ProcessParentSeen {
            continue;
        }
        let Some(child) = obs.object() else {
            continue;
        };
        let Some(pid) = child.process_pid() else {
            continue;
        };
        map.insert(pid, obs.id());
    }
    map
}

fn cgroup_observation_ids(observations: &[Observation]) -> HashMap<(u32, String), ObservationId> {
    let mut map = HashMap::new();
    for obs in observations {
        if obs.kind() != ObservationKind::ProcessBelongsToCgroup {
            continue;
        }
        let Some(subject) = obs.subject() else {
            continue;
        };
        let Some(pid) = subject.process_pid() else {
            continue;
        };
        let path = obs
            .metadata()
            .get("cgroup_path")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let Some(path) = path else {
            continue;
        };
        map.insert((pid, path), obs.id());
    }
    map
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConnectionObservationKey {
    inode: u64,
    remote_ip: String,
    remote_port: u16,
    raw_line: usize,
    table: String,
}

fn tcp_connection_observation_ids(
    observations: &[Observation],
) -> HashMap<ConnectionObservationKey, ObservationId> {
    let mut map = HashMap::new();
    for obs in observations {
        if obs.kind() != ObservationKind::TcpConnectionSeen {
            continue;
        }
        let Some(inode) = obs
            .metadata()
            .get("inode")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        let remote_ip = obs
            .metadata()
            .get("remote_ip")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let remote_port = obs
            .metadata()
            .get("remote_port")
            .and_then(|v| v.as_u64())
            .map(|n| n as u16);
        let raw_line = obs
            .metadata()
            .get("raw_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let table = obs
            .metadata()
            .get("table")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let (Some(remote_ip), Some(remote_port), Some(raw_line), Some(table)) =
            (remote_ip, remote_port, raw_line, table)
        else {
            continue;
        };
        map.insert(
            ConnectionObservationKey {
                inode,
                remote_ip,
                remote_port,
                raw_line,
                table,
            },
            obs.id(),
        );
    }
    map
}

fn listener_services_for_port(
    store: &Store,
    listeners_by_port: &HashMap<NodeId, Vec<NodeId>>,
    port_id: &NodeId,
) -> Result<Vec<NodeId>, StoreError> {
    let mut out = listeners_by_port.get(port_id).cloned().unwrap_or_default();
    for row in store.list_edges_to(port_id.as_str())? {
        let Ok(edge) = GraphEdge::try_from(&row) else {
            continue;
        };
        if edge.kind() != EdgeKind::ListensOn || edge.class() != EdgeClass::Inferred {
            continue;
        }
        let service_id = edge.from();
        if service_id.kind() == Some(NodeKind::Service) && !out.contains(service_id) {
            out.push(service_id.clone());
        }
    }
    Ok(out)
}

fn listener_obs_from_store(
    store: &Store,
    port_id: &NodeId,
    service_id: &NodeId,
) -> Option<(ObservationId, &'static str)> {
    let edge_id = EdgeId::new(service_id, EdgeKind::ListensOn, port_id);
    let row = store.get_edge(edge_id.as_str()).ok()??;
    let edge = GraphEdge::try_from(&row).ok()?;
    let obs_links = store.list_observations_for_edge(edge.id().as_str()).ok()?;
    let (obs_id, _) = obs_links.first()?;
    ObservationId::from_str(obs_id)
        .ok()
        .map(|id| (id, "support"))
}

fn listener_port_candidates(remote_ip: &str, remote_port: u16) -> Vec<NodeId> {
    let mut out = Vec::new();
    if let Ok(exact) = NodeId::port_tcp(remote_ip, remote_port) {
        out.push(exact);
    }
    if remote_ip.parse::<Ipv4Addr>().is_ok() {
        if let Ok(wildcard) = NodeId::port_tcp("0.0.0.0", remote_port) {
            if !out.contains(&wildcard) {
                out.push(wildcard);
            }
        }
    }
    if remote_ip.parse::<Ipv6Addr>().is_ok() {
        if let Ok(wildcard) = NodeId::port_tcp("::", remote_port) {
            if !out.contains(&wildcard) {
                out.push(wildcard);
            }
        }
    }
    out
}

fn tcp_socket_observation_ids(observations: &[Observation]) -> HashMap<u64, ObservationId> {
    let mut map = HashMap::new();
    for obs in observations {
        if obs.kind() != ObservationKind::TcpSocketSeen {
            continue;
        }
        let Some(inode) = obs
            .metadata()
            .get("inode")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        map.insert(inode, obs.id());
    }
    map
}

fn aggregate_warnings(warnings: &[ProcessWarning]) -> Vec<ScanWarning> {
    let mut vanished = 0usize;
    let mut permission = 0usize;
    let mut malformed = 0usize;
    let mut exe = 0usize;
    let mut cgroup_missing = 0usize;
    let mut cgroup_permission = 0usize;
    let mut cgroup_malformed = 0usize;
    let mut tcp_table_missing = 0usize;
    let mut tcp_table_malformed = 0usize;
    let mut fd_permission = 0usize;
    let mut fd_malformed = 0usize;
    let mut fd_vanished = 0usize;
    let mut socket_unmapped = 0usize;
    let mut active_socket_unmapped = 0usize;
    for w in warnings {
        match w.kind() {
            ProcessWarningKind::Vanished => vanished += 1,
            ProcessWarningKind::PermissionDenied => permission += 1,
            ProcessWarningKind::Malformed => malformed += 1,
            ProcessWarningKind::ExeUnreadable => exe += 1,
            ProcessWarningKind::CgroupMissing => cgroup_missing += 1,
            ProcessWarningKind::CgroupPermissionDenied => cgroup_permission += 1,
            ProcessWarningKind::CgroupMalformed => cgroup_malformed += 1,
            ProcessWarningKind::TcpTableMissing => tcp_table_missing += 1,
            ProcessWarningKind::TcpTableMalformed => tcp_table_malformed += 1,
            ProcessWarningKind::FdPermissionDenied => fd_permission += 1,
            ProcessWarningKind::FdMalformed => fd_malformed += 1,
            ProcessWarningKind::FdVanished => fd_vanished += 1,
            ProcessWarningKind::SocketUnmapped => socket_unmapped += 1,
            ProcessWarningKind::ActiveSocketUnmapped => active_socket_unmapped += 1,
        }
    }
    let mut out = Vec::new();
    push_aggregate(&mut out, ProcessWarningKind::Vanished, vanished);
    push_aggregate(&mut out, ProcessWarningKind::PermissionDenied, permission);
    push_aggregate(&mut out, ProcessWarningKind::Malformed, malformed);
    push_aggregate(&mut out, ProcessWarningKind::ExeUnreadable, exe);
    push_aggregate(&mut out, ProcessWarningKind::CgroupMissing, cgroup_missing);
    push_aggregate(
        &mut out,
        ProcessWarningKind::CgroupPermissionDenied,
        cgroup_permission,
    );
    push_aggregate(
        &mut out,
        ProcessWarningKind::CgroupMalformed,
        cgroup_malformed,
    );
    push_aggregate(
        &mut out,
        ProcessWarningKind::TcpTableMissing,
        tcp_table_missing,
    );
    push_aggregate(
        &mut out,
        ProcessWarningKind::TcpTableMalformed,
        tcp_table_malformed,
    );
    push_aggregate(
        &mut out,
        ProcessWarningKind::FdPermissionDenied,
        fd_permission,
    );
    push_aggregate(&mut out, ProcessWarningKind::FdMalformed, fd_malformed);
    push_aggregate(&mut out, ProcessWarningKind::FdVanished, fd_vanished);
    push_aggregate(
        &mut out,
        ProcessWarningKind::SocketUnmapped,
        socket_unmapped,
    );
    push_aggregate(
        &mut out,
        ProcessWarningKind::ActiveSocketUnmapped,
        active_socket_unmapped,
    );
    out
}

fn push_aggregate(out: &mut Vec<ScanWarning>, kind: ProcessWarningKind, count: usize) {
    if count > 0 {
        out.push(ScanWarning {
            kind: kind.aggregate_key().to_string(),
            count,
        });
    }
}

fn detailed_warnings(warnings: &[ProcessWarning]) -> Vec<ScanWarningDetail> {
    warnings
        .iter()
        .filter(|w| w.kind().includes_json_detail())
        .map(|w| ScanWarningDetail {
            kind: w.kind().detail_key().to_string(),
            path: w.path().display().to_string(),
            detail: w.detail().to_string(),
        })
        .collect()
}

fn upsert_node_once(
    store: &mut Store,
    seen: &mut HashSet<NodeId>,
    id: NodeId,
    build: impl FnOnce(Option<&GraphNode>) -> GraphNode,
) -> Result<bool, StoreError> {
    if !seen.insert(id.clone()) {
        return Ok(false);
    }
    let existing = store.get_node_typed(&id)?;
    store.upsert_node_typed(&build(existing.as_ref()))?;
    Ok(true)
}

fn load_existing_edge(
    store: &Store,
    from: &NodeId,
    kind: EdgeKind,
    to: &NodeId,
) -> Result<Option<GraphEdge>, StoreError> {
    let id = EdgeId::new(from, kind, to);
    Ok(store
        .get_edge(id.as_str())?
        .as_ref()
        .and_then(|row| GraphEdge::try_from(row).ok()))
}

fn upsert_edge_with_link(
    store: &mut Store,
    edge: &GraphEdge,
    link: Option<(&ObservationId, &str)>,
) -> Result<(), StoreError> {
    store.upsert_edge_typed(edge)?;
    if let Some((obs_id, role)) = link {
        store.link_edge_observation(edge.id().as_str(), &obs_id.to_string(), role)?;
    }
    Ok(())
}
