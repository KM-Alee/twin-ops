use std::collections::BTreeSet;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<EvaluationContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineComparison>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvaluationContext {
    pub evaluated_at_ns: i64,
    pub graph_observed_at_ns: Option<i64>,
    pub snapshot: Option<String>,
    pub max_age_seconds: Option<u64>,
    pub live_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineComparison {
    pub snapshot: String,
    pub newly_failing: Vec<String>,
    pub newly_uncertain: Vec<String>,
    pub recovered: Vec<String>,
    pub checks: Vec<CheckResult>,
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
    pub supporting_observations: usize,
    pub stale: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EndpointFact {
    pub endpoint: String,
    pub evidence: String,
    pub from: String,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFact {
    pub mount: String,
    pub used_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFact {
    pub source: Option<crate::coverage::FactSource>,
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
    pub supporting_observations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFact {
    pub id: String,
    pub active_state: Option<String>,
    pub stale: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GraphFacts {
    pub nodes: Vec<NodeFact>,
    pub services: Vec<ServiceFact>,
    pub containers: Vec<crate::health::ContainerFact>,
    pub workloads: Vec<crate::health::K8sReadyFact>,
    pub routing: Vec<crate::health::RoutingFact>,
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
        context: None,
        baseline: None,
    }
}

fn evaluate_check(document: &TestDocument, check: &TestCheck, facts: &GraphFacts) -> CheckResult {
    let mut result = match &check.kind {
        CheckKind::Node(id) | CheckKind::Port(id) | CheckKind::Service(id) => {
            node_result(&check.name, id.as_str(), check.exists, facts)
        }
        CheckKind::ServiceState {
            service,
            active_state,
        } => service_state_result(&check.name, service.as_str(), active_state, facts),
        CheckKind::Listener { port, owned_by } => listener_result(
            &check.name,
            port.as_str(),
            owned_by.as_str(),
            check.exists,
            facts,
        ),
        CheckKind::ContainerHealth {
            container,
            running,
            healthy,
        } => crate::health::container_result(
            &check.name,
            container.as_str(),
            *running,
            *healthy,
            facts,
        ),
        CheckKind::K8sReady {
            deployment,
            min_ready,
            routed_service,
        } => crate::health::k8s_result(
            &check.name,
            deployment.as_str(),
            *min_ready,
            routed_service.as_ref().map(|id| id.as_str()),
            facts,
        ),
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
            from,
        } => outbound_result(
            &check.name,
            allowed,
            *fail_on_unknown,
            from.as_ref().map(|id| id.as_str()),
            facts,
        ),
        CheckKind::Disk {
            mount,
            max_used_percent,
        } => disk_result(&check.name, mount, *max_used_percent, facts),
        CheckKind::Unknowns { max_count, forbid } => {
            unknowns_result(&check.name, *max_count, forbid, facts)
        }
    };
    if document.fail_on_unknown
        && result.status == CheckStatus::Warn
        && result
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("Unknown:"))
    {
        result.status = CheckStatus::Fail;
    }
    result
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
        None if crate::coverage::has_node_gaps(facts, id) => unknown(
            name,
            "absence cannot be established with incomplete coverage",
            facts,
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
        } else if crate::coverage::has_node_gaps(facts, from)
            || crate::coverage::has_node_gaps(facts, to)
        {
            unknown(
                name,
                "dependency absence cannot be established with incomplete coverage",
                facts,
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
    if best.supporting_observations == 0 {
        return unknown(
            name,
            "dependency supporting observations are not retained",
            facts,
        );
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
    if require_evidence && fact.supporting_observations == 0 {
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
    from: Option<&str>,
    facts: &GraphFacts,
) -> CheckResult {
    if let Some(from) = from {
        if !facts
            .nodes
            .iter()
            .any(|node| node.id == from && !node.stale)
        {
            let mut result = unknown(name, "outbound workload is missing or stale", facts);
            if fail_on_unknown {
                result.status = CheckStatus::Fail;
            }
            return result;
        }
    }
    let unexpected: Vec<&EndpointFact> = facts
        .endpoints
        .iter()
        .filter(|fact| from.is_none_or(|from| from == fact.from))
        .filter(|fact| {
            !allowed
                .iter()
                .any(|item| endpoint_matches(item, &fact.endpoint))
        })
        .collect();
    if unexpected.is_empty() {
        let stale = facts
            .endpoints
            .iter()
            .any(|fact| from.is_none_or(|from| from == fact.from) && fact.stale);
        if stale || crate::coverage::has_network_gaps(facts, from) {
            let mut result = unknown(name, "outbound coverage is incomplete or stale", facts);
            if fail_on_unknown {
                result.status = CheckStatus::Fail;
            }
            return result;
        }
        return pass(name);
    }
    let listed = unexpected
        .iter()
        .map(|fact| fact.endpoint.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let evidence = unexpected
        .iter()
        .flat_map(|fact| fact.evidence.split("; "))
        .filter(|line| !line.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
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

pub(crate) fn pass(name: &str) -> CheckResult {
    outcome(name, CheckStatus::Pass, None, None)
}

pub(crate) fn outcome(
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

fn endpoint_matches(allowed: &str, endpoint: &str) -> bool {
    if allowed.starts_with("tcp:") || allowed.starts_with("udp:") {
        allowed == endpoint
    } else {
        allowed
            == endpoint
                .strip_prefix("tcp:")
                .or_else(|| endpoint.strip_prefix("udp:"))
                .unwrap_or(endpoint)
    }
}

pub(crate) fn unknown(name: &str, detail: &str, facts: &GraphFacts) -> CheckResult {
    outcome(
        name,
        CheckStatus::Warn,
        Some(format!("Unknown: {detail}")),
        Some(
            facts
                .unknowns
                .iter()
                .map(|fact| fact.detail.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ),
    )
}

fn service_state_result(name: &str, id: &str, expected: &str, facts: &GraphFacts) -> CheckResult {
    match facts.services.iter().find(|fact| fact.id == id) {
        Some(fact) if !fact.stale && fact.active_state.as_deref() == Some(expected) => outcome(
            name,
            CheckStatus::Pass,
            None,
            Some(format!(
                "systemd runtime state observed: {id} active_state={expected}"
            )),
        ),
        Some(fact) if fact.stale || fact.active_state.is_none() => {
            unknown(name, "service runtime state is unavailable or stale", facts)
        }
        Some(fact) => outcome(
            name,
            CheckStatus::Fail,
            Some(format!(
                "{id} active_state is {}, expected {expected}",
                fact.active_state.as_deref().unwrap_or("unknown")
            )),
            None,
        ),
        None => unknown(name, "service runtime state is not observed", facts),
    }
}

fn listener_result(
    name: &str,
    port: &str,
    owner: &str,
    exists: bool,
    facts: &GraphFacts,
) -> CheckResult {
    let matches = facts
        .edges
        .iter()
        .filter(|edge| edge.from == owner && edge.to == port && edge.kind == "listens_on")
        .collect::<Vec<_>>();
    if matches
        .iter()
        .any(|edge| !edge.stale && edge.supporting_observations > 0)
    {
        return if exists {
            let best = matches
                .iter()
                .filter(|edge| !edge.stale && edge.supporting_observations > 0)
                .max_by_key(|edge| evidence_rank(edge.evidence));
            outcome(name,CheckStatus::Pass,None,best.map(|edge| format!("socket ownership observed: {owner} listens on {port}; {} supporting observations; evidence {}",edge.supporting_observations,edge.evidence)))
        } else {
            outcome(
                name,
                CheckStatus::Fail,
                Some(format!("{owner} listens on {port}")),
                None,
            )
        };
    }
    if !matches.is_empty()
        || crate::coverage::has_node_gaps(facts, port)
        || crate::coverage::has_node_gaps(facts, owner)
    {
        return unknown(name, "listener ownership is unavailable or stale", facts);
    }
    if exists {
        outcome(
            name,
            CheckStatus::Fail,
            Some(format!("no observed listener {port} owned by {owner}")),
            None,
        )
    } else {
        pass(name)
    }
}
