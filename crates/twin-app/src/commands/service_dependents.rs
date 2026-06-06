use twin_core::{DependentImpactKind, EdgeClass, EdgeId, EdgeKind, NodeId};
use twin_emulate::{
    EmulationDependent, EmulationEvidenceLine, EmulationPathStep, EmulationUnknown,
};

use crate::model::{
    ImpactDependent, ImpactEvidenceLine, ImpactNodeSummary, ImpactPathStep, ImpactUnknown,
};

#[derive(Debug, Clone)]
pub(crate) struct TypedServiceDependent {
    pub id: NodeId,
    pub label: String,
    pub edge_kind: EdgeKind,
    pub edge_class: EdgeClass,
    pub impact_kind: DependentImpactKind,
    pub reason: String,
    pub to_id: NodeId,
    pub to_label: String,
    pub edge_id: EdgeId,
    pub evidence: Vec<ImpactEvidenceLine>,
    pub observation_ids: Vec<String>,
}

impl TypedServiceDependent {
    pub fn to_impact_dependent(&self) -> ImpactDependent {
        ImpactDependent {
            id: self.id.to_string(),
            label: self.label.clone(),
            relationship: self.edge_kind.to_string(),
            edge_class: self.edge_class.to_string(),
            impact_kind: self.impact_kind.to_string(),
            reason: self.reason.clone(),
            path: vec![ImpactPathStep {
                from: ImpactNodeSummary {
                    id: self.id.to_string(),
                    label: self.label.clone(),
                },
                edge_kind: self.edge_kind.to_string(),
                edge_class: self.edge_class.to_string(),
                to: ImpactNodeSummary {
                    id: self.to_id.to_string(),
                    label: self.to_label.clone(),
                },
                edge_id: self.edge_id.to_string(),
            }],
            evidence: self.evidence.clone(),
            observation_ids: self.observation_ids.clone(),
        }
    }

    pub fn to_emulation_dependent(&self) -> EmulationDependent {
        EmulationDependent {
            id: self.id.clone(),
            label: self.label.clone(),
            edge_kind: self.edge_kind,
            edge_class: self.edge_class,
            reason: self.reason.clone(),
            path: vec![EmulationPathStep {
                from_id: self.id.clone(),
                from_label: self.label.clone(),
                edge_kind: self.edge_kind,
                edge_class: self.edge_class,
                to_id: self.to_id.clone(),
                to_label: self.to_label.clone(),
                edge_id: self.edge_id.clone(),
            }],
            evidence: self
                .evidence
                .iter()
                .map(|line| EmulationEvidenceLine {
                    source: line.source.clone(),
                    statement: line.statement.clone(),
                    relationship: line.relationship.clone(),
                    strength_score: line.strength_score(),
                })
                .collect(),
            observation_ids: self.observation_ids.clone(),
        }
    }
}

pub(crate) fn impact_unknown_to_emulation(unknown: &ImpactUnknown) -> EmulationUnknown {
    EmulationUnknown {
        kind: unknown.kind.clone(),
        detail: unknown.detail.clone(),
        source: unknown.source.clone(),
        weakens_evidence: unknown.weakens_evidence,
    }
}
