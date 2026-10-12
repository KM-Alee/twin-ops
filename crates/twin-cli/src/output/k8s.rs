use twin_app::{K8sGraphResult, K8sImpactResult, K8sScanResult};

use crate::output::format::{status_tag, Lines, Status};

pub fn render_scan(result: &K8sScanResult) -> String {
    let mut out = Lines::new();
    out.title("twin k8s scan");
    let status = if result.coverage_gap.is_some() {
        Status::Warn
    } else if result.warnings.is_empty() {
        Status::Ok
    } else {
        Status::Warn
    };
    let summary = if let Some(gap) = &result.coverage_gap {
        gap.clone()
    } else {
        format!(
            "{} namespaces, {} deployments, {} replicasets, {} pods, {} services",
            result.namespaces, result.deployments, result.replicasets, result.pods, result.services
        )
    };
    out.status_row(status, "summary", &summary);
    out.status_row(Status::Neutral, "source", &result.source);
    out.blank();
    out.section("resources");
    let rows = [
        ("namespaces", result.namespaces),
        ("deployments", result.deployments),
        ("replicasets", result.replicasets),
        ("pods", result.pods),
        ("services", result.services),
        ("endpoints", result.endpoints),
        ("ingresses", result.ingresses),
        ("configmaps", result.configmaps),
        ("secret refs", result.secret_refs),
        ("pvcs", result.pvcs),
        ("events", result.events),
    ];
    for (index, (label, count)) in rows.iter().enumerate() {
        out.tree_leaf(index + 1 == rows.len(), label, &count.to_string());
    }
    if !result.warnings.is_empty() {
        out.blank();
        out.section("warnings");
        for (index, warning) in result.warnings.iter().enumerate() {
            out.tree_leaf(index + 1 == result.warnings.len(), "document", warning);
        }
    }
    let kube_status = if result.kubeconfig_present {
        Status::Ok
    } else {
        Status::Warn
    };
    out.blank();
    out.tree_leaf(
        true,
        "kubeconfig",
        &format!("{}  {}", status_tag(kube_status), result.kubeconfig_path),
    );
    out.into_string()
}

pub fn render_graph(result: &K8sGraphResult) -> String {
    let mut out = Lines::new();
    out.title(&result.id);
    render_group(&mut out, "Owns", &result.owns);
    render_group(&mut out, "Selects", &result.selects);
    render_group(&mut out, "Uses", &result.uses);
    render_group(&mut out, "Routes to", &result.routes_to);
    render_group(&mut out, "Routed by", &result.routed_by);
    if result.owns.is_empty()
        && result.selects.is_empty()
        && result.uses.is_empty()
        && result.routes_to.is_empty()
        && result.routed_by.is_empty()
    {
        out.section("Links");
        out.tree_leaf(true, "status", "no kubernetes edges stored for this object");
    }
    out.into_string()
}

pub fn render_impact(result: &K8sImpactResult) -> String {
    let mut out = Lines::new();
    out.title(&result.id);
    render_group(&mut out, "Selects", &result.selects);
    render_group(&mut out, "Affected", &result.affected);
    render_group(&mut out, "Owned by", &result.owned_by);
    render_group(&mut out, "Routed by", &result.routed_by);
    if result.selects.is_empty()
        && result.affected.is_empty()
        && result.owned_by.is_empty()
        && result.routed_by.is_empty()
    {
        out.section("Impact");
        out.tree_leaf(
            true,
            "status",
            "no kubernetes neighbors stored for this object",
        );
    }
    out.into_string()
}

fn render_group(out: &mut Lines, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.section(title);
    for (index, item) in items.iter().enumerate() {
        let (label, value) = item
            .strip_prefix("k8s:")
            .unwrap_or(item)
            .split_once(':')
            .unwrap_or(("object", item));
        out.tree_leaf(index + 1 == items.len(), label, value);
    }
    out.blank();
}

pub fn render_watch(result: &twin_app::K8sWatchResult) -> String {
    let mut out = Lines::new();
    out.title("twin k8s watch");
    let summary = &result.summary;
    out.status_row(
        if summary.warnings.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        },
        "summary",
        &format!(
            "{} events, {} graph refreshes; {}s duration limit",
            summary.observed_events, summary.refreshes, result.duration_seconds
        ),
    );
    out.section("watch continuity");
    out.tree_leaf(false, "relists", &summary.relists.to_string());
    out.tree_leaf(true, "reconnects", &summary.reconnects.to_string());
    out.section("resource changes");
    for (index, notice) in summary.notices.iter().enumerate() {
        let target = match (&notice.namespace, &notice.name) {
            (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
            (_, Some(name)) => name.clone(),
            _ => "resource collection".into(),
        };
        out.tree_leaf(
            index + 1 == summary.notices.len(),
            &format!("{} {}", notice.resource, notice.event),
            &target,
        );
    }
    if summary.notices.is_empty() {
        out.tree_leaf(true, "status", "no resource changes retained");
    }
    if summary.omitted_notices > 0 {
        out.status_row(
            Status::Warn,
            "omitted events",
            &summary.omitted_notices.to_string(),
        );
    }
    if summary.limit_reached {
        out.status_row(
            Status::Warn,
            "event limit",
            "watch stopped at the requested event limit",
        );
    }
    if !summary.warnings.is_empty() {
        out.section("coverage gaps");
        for (index, warning) in summary.warnings.iter().enumerate() {
            out.tree_leaf(index + 1 == summary.warnings.len(), "watch", warning);
        }
    }
    if summary.omitted_warnings > 0 {
        out.status_row(
            Status::Warn,
            "omitted warnings",
            &summary.omitted_warnings.to_string(),
        );
    }
    out.into_string()
}

pub fn render_logs(result: &twin_app::PodLogSummary) -> String {
    let mut out = Lines::new();
    out.title("twin k8s logs");
    out.status_row(
        if result.coverage_gap.is_some() || result.limit_reached {
            Status::Warn
        } else {
            Status::Ok
        },
        "summary",
        &format!(
            "{}/{}: {} bytes, {} line breaks",
            result.namespace, result.pod, result.received_bytes, result.line_breaks
        ),
    );
    out.section("read bounds");
    out.tree_leaf(
        false,
        "container",
        result.container.as_deref().unwrap_or("pod default"),
    );
    out.tree_leaf(
        false,
        "previous container",
        if result.previous { "yes" } else { "no" },
    );
    out.tree_leaf(false, "tail lines", &result.tail_lines.to_string());
    out.tree_leaf(true, "byte limit", &result.max_bytes.to_string());
    if result.limit_reached {
        out.status_row(
            Status::Warn,
            "partial",
            "byte limit reached; total log volume is unknown",
        );
    }
    if let Some(gap) = &result.coverage_gap {
        out.status_row(Status::Warn, "coverage gap", gap);
    }
    out.section("observed phrase matches");
    for (index, count) in result.phrase_matches.iter().enumerate() {
        let label = match count.signal {
            twin_app::LogSignal::ConnectionRefused => "connection refused",
            twin_app::LogSignal::Timeout => "timeout / timed out",
            twin_app::LogSignal::PermissionDenied => "permission denied",
            twin_app::LogSignal::OutOfMemory => "out of memory / oom-kill",
            twin_app::LogSignal::ErrorMarker => "error marker",
            twin_app::LogSignal::WarningMarker => "warn marker",
        };
        out.tree_leaf(
            index + 1 == result.phrase_matches.len(),
            label,
            &count.matches.to_string(),
        );
    }
    out.status_row(
        Status::Neutral,
        "interpretation",
        "case-insensitive phrase matches; diagnostic causes remain unknown",
    );
    out.status_row(
        Status::Neutral,
        "content policy",
        "log bodies are discarded; only volume metadata is returned",
    );
    out.into_string()
}
