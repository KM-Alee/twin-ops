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
        let label = item.split(':').next().unwrap_or("object");
        out.tree_leaf(index + 1 == items.len(), label, item);
    }
    out.blank();
}
