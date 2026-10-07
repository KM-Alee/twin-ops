use serde::Serialize;
use twin_core::EvidenceLabel;

use crate::document::{CheckKind, TestCheck, TestDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestRunReport {
    pub name: String,
    pub checks: Vec<CheckResult>,
    pub passed: u32,
    pub warned: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFact {
    pub id: String,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeFact {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub evidence: EvidenceLabel,
    pub stale: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GraphFacts {
    pub nodes: Vec<NodeFact>,
    pub edges: Vec<EdgeFact>,
}

pub fn evaluate(document: &TestDocument, facts: &GraphFacts) -> TestRunReport {
    let checks: Vec<CheckResult> = document
        .checks
        .iter()
        .map(|check| evaluate_check(document, check, facts))
        .collect();
    let mut passed = 0;
    let mut warned = 0;
    let mut failed = 0;
    for check in &checks {
        match check.status {
            CheckStatus::Pass => passed += 1,
            CheckStatus::Warn => warned += 1,
            CheckStatus::Fail => failed += 1,
        }
    }
    TestRunReport {
        name: document.name.clone(),
        checks,
        passed,
        warned,
        failed,
    }
}

fn evaluate_check(document: &TestDocument, check: &TestCheck, facts: &GraphFacts) -> CheckResult {
    match &check.kind {
        CheckKind::Node(id) | CheckKind::Port(id) | CheckKind::Service(id) => {
            node_result(&check.name, id.as_str(), check.exists, facts)
        }
        CheckKind::Dependency { from, to } => dependency_result(
            &check.name,
            from.as_str(),
            to.as_str(),
            check.exists,
            check.min_evidence.or(document.default_min_evidence),
            facts,
        ),
    }
}

fn node_result(name: &str, id: &str, exists: bool, facts: &GraphFacts) -> CheckResult {
    match facts.nodes.iter().find(|node| node.id == id) {
        Some(node) if exists && node.stale => CheckResult {
            name: name.to_string(),
            status: CheckStatus::Warn,
            detail: Some(format!("{id} is stale")),
        },
        Some(_) if exists => pass(name),
        Some(_) => CheckResult {
            name: name.to_string(),
            status: CheckStatus::Fail,
            detail: Some(format!("{id} exists")),
        },
        None if exists => CheckResult {
            name: name.to_string(),
            status: CheckStatus::Fail,
            detail: Some(format!("{id} is not in the graph")),
        },
        None => pass(name),
    }
}

fn dependency_result(
    name: &str,
    from: &str,
    to: &str,
    exists: bool,
    min_evidence: Option<EvidenceLabel>,
    facts: &GraphFacts,
) -> CheckResult {
    let mut matches: Vec<&EdgeFact> = facts
        .edges
        .iter()
        .filter(|edge| edge.from == from && edge.to == to)
        .filter(|edge| edge.kind == "depends_on" || edge.kind == "connects_to")
        .collect();
    if matches.is_empty() {
        return if exists {
            CheckResult {
                name: name.to_string(),
                status: CheckStatus::Fail,
                detail: Some(format!("no dependency from {from} to {to}")),
            }
        } else {
            pass(name)
        };
    }
    if !exists {
        return CheckResult {
            name: name.to_string(),
            status: CheckStatus::Fail,
            detail: Some(format!("dependency from {from} to {to} exists")),
        };
    }
    matches.sort_by(|left, right| {
        evidence_rank(right.evidence)
            .cmp(&evidence_rank(left.evidence))
            .then(left.stale.cmp(&right.stale))
    });
    let best = matches[0];
    if let Some(minimum) = min_evidence {
        if evidence_rank(best.evidence) < evidence_rank(minimum) {
            return CheckResult {
                name: name.to_string(),
                status: CheckStatus::Fail,
                detail: Some(format!(
                    "{} {} {} evidence is {}, minimum is {}",
                    from, best.kind, to, best.evidence, minimum
                )),
            };
        }
    }
    if best.stale {
        return CheckResult {
            name: name.to_string(),
            status: CheckStatus::Warn,
            detail: Some(format!(
                "{} {} {} is stale ({})",
                from, best.kind, to, best.evidence
            )),
        };
    }
    pass(name)
}

fn pass(name: &str) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        status: CheckStatus::Pass,
        detail: None,
    }
}

fn evidence_rank(label: EvidenceLabel) -> u8 {
    match label {
        EvidenceLabel::Weak => 0,
        EvidenceLabel::Moderate => 1,
        EvidenceLabel::Strong => 2,
        EvidenceLabel::VeryStrong => 3,
    }
}
