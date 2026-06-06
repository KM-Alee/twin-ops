use twin_core::{EdgeKind, NodeId, NodeState};

use crate::action::{EmulationAction, RESTART_ACTION, SAFETY_STATEMENT};
use crate::effective_view::EffectiveGraphView;
use crate::input::{EmulationDependent, RestartServiceInput};
use crate::overlay::{GraphOverlay, InterruptedRelationship, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayInterrupted, EmulationOverlayNode,
    EmulationOverlaySummary,
};
use crate::scoring::{score_restart_evidence, score_restart_risk};

pub fn emulate_restart_service(input: RestartServiceInput) -> EmulationDomainReport {
    let target_id = input.target.id.clone();
    let target_label = input.target.label.clone();
    let overlay = build_overlay(&input);
    let base_nodes: Vec<_> = std::iter::once(input.target.clone())
        .chain(input.unavailable_nodes.iter().cloned())
        .collect();
    let view = EffectiveGraphView::new(&base_nodes, NodeState::Active, &overlay);

    let (risk_level, risk_reasons) = score_restart_risk(
        &input.runtime_dependents,
        &input.configured_dependents,
        &input.unknowns,
    );
    let evidence_strength = score_restart_evidence(&input.runtime_dependents, &input.unknowns);

    let transient_impacts = input
        .runtime_dependents
        .iter()
        .map(|d| runtime_dependent_to_impact(d, &target_id, &target_label))
        .collect();
    let configured_impacts = input
        .configured_dependents
        .iter()
        .map(|d| configured_dependent_to_impact(d, &target_label))
        .collect();
    let overlay_summary = overlay_summary_from(&view, &input);

    EmulationDomainReport {
        action: RESTART_ACTION.to_string(),
        target: target_id.to_string(),
        target_label,
        action_performed: false,
        safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level,
        risk_reasons,
        evidence_strength,
        overlay_summary,
        overlay,
        transient_impacts,
        configured_impacts,
    }
}

fn build_overlay(input: &RestartServiceInput) -> GraphOverlay {
    let target_id = input.target.id.clone();
    let target_label = input.target.label.clone();
    let mut node_overrides = Vec::new();
    let mut seen_nodes = std::collections::HashSet::new();

    node_overrides.push(NodeOverlay {
        node_id: target_id.clone(),
        state: OverlayNodeState::TemporarilyUnavailable,
        reason: unavailable_reason(&target_label),
    });
    seen_nodes.insert(target_id.as_str().to_string());

    for node in &input.unavailable_nodes {
        if !seen_nodes.insert(node.id.as_str().to_string()) {
            continue;
        }
        node_overrides.push(NodeOverlay {
            node_id: node.id.clone(),
            state: OverlayNodeState::TemporarilyUnavailable,
            reason: listener_unavailable_reason(&node.label),
        });
    }

    let mut interrupted_relationships = Vec::new();
    for dependent in &input.runtime_dependents {
        let Some(step) = dependent.path.first() else {
            continue;
        };
        interrupted_relationships.push(InterruptedRelationship {
            edge_id: step.edge_id.clone(),
            from: dependent.id.clone(),
            to: target_id.clone(),
            kind: EdgeKind::DependsOn,
            reason: runtime_interrupted_reason(&dependent.label, &target_label),
        });
    }

    node_overrides.sort_by(|a, b| a.node_id.as_str().cmp(b.node_id.as_str()));
    interrupted_relationships.sort_by(|a, b| a.edge_id.as_str().cmp(b.edge_id.as_str()));

    GraphOverlay {
        action: EmulationAction::RestartService { target: target_id },
        node_overrides,
        interrupted_relationships,
    }
}

fn unavailable_reason(label: &str) -> String {
    format!("{label} would be unavailable during restart")
}

fn listener_unavailable_reason(label: &str) -> String {
    format!("owned listener {label} would be unavailable during restart")
}

fn runtime_interrupted_reason(dependent_label: &str, target_label: &str) -> String {
    format!("{dependent_label} may lose dependency while {target_label} restarts")
}

fn dependency_path(from: &NodeId, to: &NodeId) -> String {
    format!("{} {} {}", from.as_str(), EdgeKind::DependsOn, to.as_str())
}

fn overlay_summary_from(
    view: &EffectiveGraphView<'_>,
    input: &RestartServiceInput,
) -> EmulationOverlaySummary {
    let label_for = |id: &NodeId| -> String {
        if id == &input.target.id {
            return input.target.label.clone();
        }
        input
            .unavailable_nodes
            .iter()
            .find(|n| &n.id == id)
            .map(|n| n.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let unavailable_nodes = view
        .overlay()
        .node_overrides
        .iter()
        .filter(|n| view.is_temporarily_unavailable(&n.node_id))
        .map(|n| EmulationOverlayNode {
            id: n.node_id.to_string(),
            label: label_for(&n.node_id),
            reason: n.reason.clone(),
        })
        .collect();
    let interrupted_relationships = view
        .overlay()
        .interrupted_relationships
        .iter()
        .map(|r| EmulationOverlayInterrupted {
            edge_id: r.edge_id.to_string(),
            from: r.from.to_string(),
            to: r.to.to_string(),
            kind: r.kind.to_string(),
            reason: r.reason.clone(),
        })
        .collect();
    EmulationOverlaySummary {
        unavailable_nodes,
        interrupted_relationships,
    }
}

fn runtime_dependent_to_impact(
    dependent: &EmulationDependent,
    target_id: &NodeId,
    target_label: &str,
) -> EmulationImpact {
    let path = dependency_path(&dependent.id, target_id);
    let statement = runtime_interrupted_reason(&dependent.label, target_label);
    let evidence: Vec<String> = dependent
        .evidence
        .iter()
        .map(|line| line.statement.clone())
        .collect();
    EmulationImpact {
        id: dependent.id.to_string(),
        label: dependent.label.clone(),
        statement,
        path,
        evidence,
    }
}

fn configured_dependent_to_impact(
    dependent: &EmulationDependent,
    target_label: &str,
) -> EmulationImpact {
    let path = dependent
        .path
        .first()
        .map(|step| dependency_path(&step.from_id, &step.to_id))
        .unwrap_or_else(|| dependent.id.to_string());
    let statement = configured_impact_statement(&dependent.label, target_label);
    let evidence: Vec<String> = dependent
        .evidence
        .iter()
        .map(|line| line.statement.clone())
        .collect();
    EmulationImpact {
        id: dependent.id.to_string(),
        label: dependent.label.clone(),
        statement,
        path,
        evidence,
    }
}

fn configured_impact_statement(dependent_label: &str, target_label: &str) -> String {
    format!(
        "{dependent_label} declares dependency on {target_label} but no active runtime use was observed"
    )
}
