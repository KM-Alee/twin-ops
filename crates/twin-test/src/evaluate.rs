use serde::Serialize;
use twin_core::{EvidenceLabel, RiskLevel};

use crate::document::{CheckKind, EmulateAction, TestCheck, TestDocument};

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
    pub evidence: Option<String>,
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
pub struct EndpointFact {
    pub endpoint: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFact {
    pub mount: String,
    pub used_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFact {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulationFact {
    pub action: String,
    pub target: String,
    pub risk: RiskLevel,
    pub evidence_label: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GraphFacts {
    pub nodes: Vec<NodeFact>,
    pub edges: Vec<EdgeFact>,
    pub endpoints: Vec<EndpointFact>,
    pub disks: Vec<DiskFact>,
    pub unknowns: Vec<UnknownFact>,
    pub emulations: Vec<EmulationFact>,
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
        CheckKind::Emulate {
            action,
            target,
            max_risk,
            require_evidence,
        } => emulate_result(
            &check.name,
            *action,
            target,
            *max_risk,
            *require_evidence,
            facts,
        ),
        CheckKind::Outbound {
            allowed,
            fail_on_unknown,
        } => outbound_result(&check.name, allowed, *fail_on_unknown, facts),
        CheckKind::Disk {
            mount,
            max_used_percent,
        } => disk_result(&check.name, mount, *max_used_percent, facts),
        CheckKind::Unknowns { max_count, forbid } => {
            unknowns_result(&check.name, *max_count, forbid, facts)
        }
    }
}

fn node_result(name: &str, id: &str, exists: bool, facts: &GraphFacts) -> CheckResult {
    match facts.nodes.iter().find(|node| node.id == id) {
        Some(node) if exists && node.stale => outcome(
            name,
            CheckStatus::Warn,
            Some(format!("{id} is stale")),
            None,
        ),
        Some(_) if exists => pass(name),
        Some(_) => outcome(name, CheckStatus::Fail, Some(format!("{id} exists")), None),
        None if exists => outcome(
            name,
            CheckStatus::Fail,
            Some(format!("{id} is not in the graph")),
            None,
        ),
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
            outcome(
                name,
                CheckStatus::Fail,
                Some(format!("no dependency from {from} to {to}")),
                None,
            )
        } else {
            pass(name)
        };
    }
    if !exists {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!("dependency from {from} to {to} exists")),
            None,
        );
    }
    matches.sort_by(|left, right| {
        evidence_rank(right.evidence)
            .cmp(&evidence_rank(left.evidence))
            .then(left.stale.cmp(&right.stale))
    });
    let best = matches[0];
    if let Some(minimum) = min_evidence {
        if evidence_rank(best.evidence) < evidence_rank(minimum) {
            return outcome(
                name,
                CheckStatus::Fail,
                Some(format!(
                    "{} {} {} evidence is {}, minimum is {}",
                    from, best.kind, to, best.evidence, minimum
                )),
                Some(format!("evidence is {}", best.evidence)),
            );
        }
    }
    if best.stale {
        return outcome(
            name,
            CheckStatus::Warn,
            Some(format!(
                "{} {} {} is stale ({})",
                from, best.kind, to, best.evidence
            )),
            Some(format!("evidence is {}", best.evidence)),
        );
    }
    pass(name)
}

fn emulate_result(
    name: &str,
    action: EmulateAction,
    target: &str,
    max_risk: RiskLevel,
    require_evidence: bool,
    facts: &GraphFacts,
) -> CheckResult {
    let Some(fact) = facts
        .emulations
        .iter()
        .find(|fact| fact.action == action.as_str() && fact.target == target)
    else {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!(
                "emulation {action_name} {target} did not run",
                action_name = action.as_str()
            )),
            None,
        );
    };
    let risk_line = format!("Risk: {}", fact.risk.to_string().to_ascii_uppercase());
    let evidence = emulation_evidence(fact);
    if require_evidence && evidence.is_none() {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(risk_line),
            Some("no evidence recorded".to_string()),
        );
    }
    let status = if risk_rank(fact.risk) > risk_rank(max_risk) {
        CheckStatus::Fail
    } else if risk_rank(fact.risk) == risk_rank(max_risk) {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };
    outcome(name, status, Some(risk_line), evidence)
}

fn emulation_evidence(fact: &EmulationFact) -> Option<String> {
    let mut parts = Vec::new();
    if !fact.evidence_label.is_empty() {
        parts.push(format!(
            "Evidence: {}",
            fact.evidence_label.to_ascii_uppercase()
        ));
    }
    parts.extend(fact.evidence.iter().cloned());
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn outbound_result(
    name: &str,
    allowed: &[String],
    fail_on_unknown: bool,
    facts: &GraphFacts,
) -> CheckResult {
    let unexpected: Vec<&EndpointFact> = facts
        .endpoints
        .iter()
        .filter(|fact| !allowed.iter().any(|item| item == &fact.endpoint))
        .collect();
    if unexpected.is_empty() {
        return pass(name);
    }
    let listed = unexpected
        .iter()
        .map(|fact| fact.endpoint.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let evidence = unexpected
        .iter()
        .map(|fact| fact.evidence.as_str())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let status = if fail_on_unknown {
        CheckStatus::Fail
    } else {
        CheckStatus::Warn
    };
    outcome(
        name,
        status,
        Some(format!("Unexpected endpoint: {listed}")),
        if evidence.is_empty() {
            None
        } else {
            Some(evidence)
        },
    )
}

fn disk_result(name: &str, mount: &str, max_used_percent: u8, facts: &GraphFacts) -> CheckResult {
    let Some(fact) = facts.disks.iter().find(|fact| fact.mount == mount) else {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!("mount {mount} usage is unknown")),
            None,
        );
    };
    if fact.used_percent > max_used_percent {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!(
                "mount {mount} is {}% used, maximum is {max_used_percent}",
                fact.used_percent
            )),
            Some(format!("{}% used", fact.used_percent)),
        );
    }
    if fact.used_percent == max_used_percent {
        return outcome(
            name,
            CheckStatus::Warn,
            Some(format!(
                "mount {mount} is {}% used, at the maximum",
                fact.used_percent
            )),
            Some(format!("{}% used", fact.used_percent)),
        );
    }
    pass(name)
}

fn unknowns_result(
    name: &str,
    max_count: Option<u32>,
    forbid: &[String],
    facts: &GraphFacts,
) -> CheckResult {
    let forbidden: Vec<&UnknownFact> = facts
        .unknowns
        .iter()
        .filter(|fact| forbid.iter().any(|kind| kind == &fact.kind))
        .collect();
    if !forbidden.is_empty() {
        let kinds = forbidden
            .iter()
            .map(|fact| fact.kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let evidence = forbidden
            .iter()
            .map(|fact| fact.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!("forbidden unknowns: {kinds}")),
            Some(evidence),
        );
    }
    if let Some(max_count) = max_count {
        let count = u32::try_from(facts.unknowns.len()).unwrap_or(u32::MAX);
        if count > max_count {
            let evidence = facts
                .unknowns
                .iter()
                .map(|fact| fact.detail.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return outcome(
                name,
                CheckStatus::Fail,
                Some(format!("{count} unknowns, maximum is {max_count}")),
                if evidence.is_empty() {
                    None
                } else {
                    Some(evidence)
                },
            );
        }
    }
    pass(name)
}

fn pass(name: &str) -> CheckResult {
    outcome(name, CheckStatus::Pass, None, None)
}

fn outcome(
    name: &str,
    status: CheckStatus,
    detail: Option<String>,
    evidence: Option<String>,
) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        status,
        detail,
        evidence,
    }
}

fn risk_rank(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
        RiskLevel::Unknown => 5,
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
