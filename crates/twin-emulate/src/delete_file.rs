use twin_core::{EdgeKind, NodeId, NodeState};

use crate::action::{EmulationAction, DELETE_ACTION, DELETE_SAFETY_STATEMENT, SAFETY_STATEMENT};
use crate::effective_view::EffectiveGraphView;
use crate::input::{DeleteFileInput, EmulationConfiguredService};
use crate::overlay::{GraphOverlay, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
};
use crate::scoring::{score_delete_evidence, score_delete_risk};

pub fn emulate_delete_file(input: DeleteFileInput) -> EmulationDomainReport {
    let target_id = input.target.id.clone();
    let target_label = input.target.label.clone();
    let overlay = build_overlay(&input);
    let base_nodes = std::slice::from_ref(&input.target);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);

    let (risk_level, risk_reasons) = score_delete_risk(
        &input.configured_services,
        input.file_in_graph,
        input.file_exists,
        &input.unknowns,
    );
    let evidence_strength =
        score_delete_evidence(&input.configured_services, &input.evidence, &input.unknowns);

    let runtime_impacts = input
        .configured_services
        .iter()
        .map(|s| runtime_impact(s, &target_label))
        .collect();
    let restart_impacts = input
        .configured_services
        .iter()
        .map(|s| restart_impact(s, &target_label))
        .collect();
    let persistent_impacts = persistent_impacts_for(
        &target_id,
        &target_label,
        !input.configured_services.is_empty(),
    );
    let unknown_impacts = unknown_impacts_for(&input);

    let overlay_summary = overlay_summary_from(&view, &input);

    EmulationDomainReport {
        action: DELETE_ACTION.to_string(),
        target: target_id.to_string(),
        target_label,
        action_performed: false,
        safety_statement: DELETE_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level,
        risk_reasons,
        evidence_strength,
        overlay_summary,
        overlay,
        transient_impacts: Vec::new(),
        configured_impacts: Vec::new(),
        runtime_impacts,
        restart_impacts,
        persistent_impacts,
        unknown_impacts,
        evidence_lines: input.evidence.iter().map(|e| e.statement.clone()).collect(),
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 0,
    }
}

fn build_overlay(input: &DeleteFileInput) -> GraphOverlay {
    let target_id = input.target.id.clone();
    let target_label = input.target.label.clone();
    let node_overrides = vec![NodeOverlay {
        node_id: target_id.clone(),
        state: OverlayNodeState::HypotheticallyDeleted,
        reason: deleted_reason(&target_label),
    }];
    GraphOverlay {
        action: EmulationAction::DeleteFile { target: target_id },
        node_overrides,
        interrupted_relationships: Vec::new(),
    }
}

fn deleted_reason(label: &str) -> String {
    format!("{label} would be hypothetically deleted")
}

fn configured_by_path(service_id: &NodeId, file_id: &NodeId) -> String {
    format!(
        "{} {} {}",
        service_id.as_str(),
        EdgeKind::ConfiguredBy,
        file_id.as_str()
    )
}

fn runtime_impact(service: &EmulationConfiguredService, file_label: &str) -> EmulationImpact {
    EmulationImpact {
        id: service.id.to_string(),
        label: service.label.clone(),
        statement: format!(
            "Low immediate impact: running service is not assumed to reread {file_label} immediately"
        ),
        path: configured_by_path(&service.id, &service.file_id),
        evidence: service
            .evidence
            .iter()
            .map(|line| line.statement.clone())
            .collect(),
    }
}

fn restart_impact(service: &EmulationConfiguredService, file_label: &str) -> EmulationImpact {
    EmulationImpact {
        id: service.id.to_string(),
        label: service.label.clone(),
        statement: format!("May fail to reload or restart without {file_label}"),
        path: configured_by_path(&service.id, &service.file_id),
        evidence: service
            .evidence
            .iter()
            .map(|line| line.statement.clone())
            .collect(),
    }
}

fn persistent_impacts_for(
    target_id: &NodeId,
    file_label: &str,
    has_configured_services: bool,
) -> Vec<EmulationImpact> {
    if !has_configured_services {
        return Vec::new();
    }
    vec![EmulationImpact {
        id: target_id.to_string(),
        label: file_label.to_string(),
        statement: "Missing file remains a risk until restored".to_string(),
        path: String::new(),
        evidence: Vec::new(),
    }]
}

fn unknown_impacts_for(input: &DeleteFileInput) -> Vec<EmulationImpact> {
    if !input.configured_services.is_empty() {
        return Vec::new();
    }
    let statement = if !input.file_in_graph {
        "File is not in the graph; configured services are unknown".to_string()
    } else {
        "No configured services were found for this file".to_string()
    };
    vec![EmulationImpact {
        id: input.target.id.to_string(),
        label: input.target.label.clone(),
        statement,
        path: String::new(),
        evidence: Vec::new(),
    }]
}

fn overlay_summary_from(
    view: &EffectiveGraphView<'_>,
    input: &DeleteFileInput,
) -> EmulationOverlaySummary {
    let deleted_nodes = view
        .overlay()
        .node_overrides
        .iter()
        .filter(|n| view.is_hypothetically_deleted(&n.node_id))
        .map(|n| EmulationOverlayNode {
            id: n.node_id.to_string(),
            label: input.target.label.clone(),
            reason: n.reason.clone(),
        })
        .collect();
    EmulationOverlaySummary {
        unavailable_nodes: deleted_nodes,
        interrupted_relationships: Vec::new(),
    }
}
