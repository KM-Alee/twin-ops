use twin_app::ScanResult;
use twin_collectors::{ProcessWarningKind, SystemdWarningKind};

use crate::output::format::{format_duration_ns, Lines, Status};

pub fn render(result: &ScanResult) -> String {
    let mut out = Lines::new();
    out.title("twin scan");

    let status = if result.warning_count > 0 {
        Status::Warn
    } else {
        Status::Ok
    };
    out.status_row(
        status,
        "result",
        &format!(
            "{} processes, {} services, {} parent edges",
            result.process_count, result.service_count, result.parent_edge_count
        ),
    );
    out.status_row(
        Status::Neutral,
        "duration",
        &format_duration_ns(result.started_at_ns, result.ended_at_ns),
    );
    if result.samples_requested > 1 {
        out.status_row(
            Status::Neutral,
            "samples",
            &format!(
                "{}/{} · interval {}s",
                result.samples_completed, result.samples_requested, result.interval_secs
            ),
        );
        let new_edges: usize = result.edges_first_seen_by_sample.iter().sum();
        if new_edges > 0 {
            out.status_row(
                Status::Neutral,
                "new edges",
                &format!("{new_edges} edge(s) first seen across samples"),
            );
        }
    }
    out.blank();
    out.section("persisted");
    out.tree_leaf(false, "processes", &result.process_count.to_string());
    out.tree_leaf(false, "cgroups", &result.cgroup_count.to_string());
    out.tree_leaf(false, "services", &result.service_count.to_string());
    out.tree_leaf(false, "ports", &result.port_count.to_string());
    out.tree_leaf(false, "parent-of", &result.parent_edge_count.to_string());
    out.tree_leaf(false, "in-cgroup", &result.in_cgroup_edge_count.to_string());
    out.tree_leaf(
        false,
        "service-owns",
        &result.service_owns_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "process-listens-on",
        &result.process_listens_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-listens-on",
        &result.service_listens_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "active-connections",
        &result.tcp_connection_count.to_string(),
    );
    out.tree_leaf(
        false,
        "process-connects-to",
        &result.process_connects_to_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-connects-to",
        &result.service_connects_to_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-depends-on",
        &result.service_depends_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "declared-depends-on",
        &result.declared_depends_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "systemd-units",
        &result.systemd_unit_count.to_string(),
    );
    out.tree_leaf(false, "unix-sockets", &result.unix_socket_count.to_string());
    out.tree_leaf(
        false,
        "unix-listeners",
        &result.unix_listener_count.to_string(),
    );
    out.tree_leaf(
        false,
        "process-listens-on-unix",
        &result.process_listens_on_unix_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-listens-on-unix",
        &result.service_listens_on_unix_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "unmapped-unix-listeners",
        &result.unmapped_unix_listener_count.to_string(),
    );
    out.tree_leaf(
        false,
        "unix-connections",
        &result.unix_connection_count.to_string(),
    );
    out.tree_leaf(
        false,
        "process-connects-to-unix",
        &result.process_connects_to_unix_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-connects-to-unix",
        &result.service_connects_to_unix_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "service-depends-on-unix",
        &result.service_depends_on_unix_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "enable-depends-on",
        &result.enable_depends_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "dbus-depends-on",
        &result.dbus_depends_on_edge_count.to_string(),
    );
    out.tree_leaf(
        false,
        "dbus-available",
        if result.dbus_available { "yes" } else { "no" },
    );
    out.tree_leaf(true, "observations", &result.observation_count.to_string());

    out.blank();
    out.section("coverage");
    let coverage = &result.coverage;
    out.tree_leaf(
        false,
        "readable processes",
        &coverage.readable_processes.to_string(),
    );
    out.tree_leaf(
        false,
        "restricted processes",
        &coverage.restricted_processes.to_string(),
    );
    out.tree_leaf(
        false,
        "unmapped sockets",
        &coverage.unmapped_sockets.to_string(),
    );
    let unavailable = if coverage.unavailable_collectors.is_empty() {
        "none".to_string()
    } else {
        coverage.unavailable_collectors.join(", ")
    };
    out.tree_leaf(false, "unavailable collectors", &unavailable);
    out.tree_leaf(
        false,
        "eBPF",
        if coverage.ebpf_available {
            "available"
        } else {
            "unavailable"
        },
    );
    out.tree_leaf(
        false,
        "Docker",
        if coverage.docker_available {
            "available"
        } else {
            "unavailable"
        },
    );
    out.tree_leaf(
        true,
        "Kubernetes",
        if coverage.kubernetes_available {
            "available"
        } else {
            "unavailable"
        },
    );
    let gaps = coverage.unknown_lines();
    if !gaps.is_empty() {
        out.blank();
        out.section("unknowns");
        for (i, gap) in gaps.iter().enumerate() {
            out.tree_leaf(i + 1 == gaps.len(), "gap", &gap.detail);
        }
    }

    if result.warning_count > 0 {
        out.blank();
        let kind_count = result.warnings.len();
        let section = if kind_count == 1 {
            format!("warnings (1 kind, {} events)", result.warning_count)
        } else {
            format!(
                "warnings ({kind_count} kinds, {} events)",
                result.warning_count
            )
        };
        out.section(&section);
        let warnings = &result.warnings;
        for (i, warning) in warnings.iter().enumerate() {
            let is_last = i + 1 == warnings.len();
            let (kind, detail) = warning_parts(&warning.kind, warning.count);
            out.tree_leaf(is_last, kind, &detail);
        }
    }

    out.into_string()
}

fn warning_parts(kind: &str, count: usize) -> (&'static str, String) {
    if let Some(kind) = ProcessWarningKind::from_aggregate_key(kind) {
        return kind.cli_summary(count);
    }
    if let Some(kind) = SystemdWarningKind::from_aggregate_key(kind) {
        return kind.cli_summary(count);
    }
    if kind == "package_manager_unsupported" {
        return (
            "package manager",
            format!("{count} package manager unsupported"),
        );
    }
    ("other", format!("{count} {}", kind.replace('_', " ")))
}
