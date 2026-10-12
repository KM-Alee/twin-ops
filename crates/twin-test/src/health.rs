use crate::evaluate::{outcome, unknown};
use crate::{CheckResult, CheckStatus, GraphFacts};

#[derive(Debug, Clone)]
pub struct ContainerFact {
    pub id: String,
    pub is_running: Option<bool>,
    pub is_healthy: Option<bool>,
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub struct K8sReadyFact {
    pub id: String,
    pub ready: Option<u32>,
    pub desired: Option<u32>,
    pub is_current_generation: Option<bool>,
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub struct RoutingFact {
    pub id: String,
    pub ready_workloads: Vec<String>,
    pub is_known: bool,
    pub stale: bool,
}

pub(crate) fn container_result(
    name: &str,
    id: &str,
    running: bool,
    healthy: bool,
    facts: &GraphFacts,
) -> CheckResult {
    let Some(fact) = facts.containers.iter().find(|fact| fact.id == id) else {
        return unknown(name, "container runtime state not observed", facts);
    };
    if fact.stale || fact.is_running.is_none() || (healthy && fact.is_healthy.is_none()) {
        return unknown(
            name,
            "container runtime/health evidence missing or stale",
            facts,
        );
    }
    if fact.is_running != Some(running) || (healthy && fact.is_healthy != Some(true)) {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!(
                "{id} does not meet running={running}, healthy={healthy}"
            )),
            None,
        );
    }
    outcome(
        name,
        CheckStatus::Pass,
        None,
        Some(format!(
            "Docker list/inspect observed {id} running={running}, healthy={healthy}"
        )),
    )
}

pub(crate) fn k8s_result(
    name: &str,
    id: &str,
    min_ready: Option<u32>,
    service: Option<&str>,
    facts: &GraphFacts,
) -> CheckResult {
    let Some(fact) = facts.workloads.iter().find(|fact| fact.id == id) else {
        return unknown(name, "deployment readiness not observed", facts);
    };
    let minimum = min_ready.or(fact.desired);
    if fact.stale
        || fact.ready.is_none()
        || minimum.is_none()
        || fact.is_current_generation.is_none()
    {
        return unknown(
            name,
            "deployment readiness/generation evidence missing or stale",
            facts,
        );
    }
    let minimum = minimum.unwrap_or(0);
    if fact.is_current_generation != Some(true) || fact.ready.unwrap_or(0) < minimum {
        return outcome(
            name,
            CheckStatus::Fail,
            Some(format!(
                "{id} has {} ready replicas, requires {minimum}, current generation={}",
                fact.ready.unwrap_or(0),
                fact.is_current_generation.unwrap_or(false)
            )),
            None,
        );
    }
    if let Some(service) = service {
        let Some(routing) = facts.routing.iter().find(|fact| fact.id == service) else {
            return unknown(name, "service endpoint routing not observed", facts);
        };
        if routing.stale || !routing.is_known {
            return unknown(name, "service endpoint routing missing or stale", facts);
        }
        let routed = routing
            .ready_workloads
            .iter()
            .filter(|deployment| deployment.as_str() == id)
            .count();
        if routed < minimum as usize {
            return outcome(
                name,
                CheckStatus::Fail,
                Some(format!(
                    "{service} routes to {routed} ready pods of {id}, requires {minimum}"
                )),
                None,
            );
        }
    }
    outcome(name,CheckStatus::Pass,None,Some(format!("Kubernetes get/list observed {id}: {} ready replicas, current generation; endpoint routing {}",fact.ready.unwrap_or(0),if service.is_some() { "verified" } else { "not required" })))
}
