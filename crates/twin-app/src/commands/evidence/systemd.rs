use twin_observation::{Observation, ObservationKind};

pub struct GraphSystemdDepLine {
    pub source: String,
    pub statement: String,
    pub strength: String,
}

struct SystemdUnitDepFields {
    raw_ref: String,
    from_unit: String,
    to_unit: String,
    key: String,
}

fn systemd_unit_dep_fields(obs: &Observation) -> Option<SystemdUnitDepFields> {
    if !matches!(
        obs.kind(),
        ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants
    ) {
        return None;
    }
    Some(SystemdUnitDepFields {
        raw_ref: obs.raw_ref()?.as_str().to_string(),
        from_unit: obs
            .metadata()
            .get("from_unit")
            .and_then(|v| v.as_str())?
            .to_string(),
        to_unit: obs
            .metadata()
            .get("to_unit")
            .and_then(|v| v.as_str())?
            .to_string(),
        key: obs
            .metadata()
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("Requires")
            .to_string(),
    })
}

pub fn graph_systemd_dep_line(obs: &Observation) -> Option<GraphSystemdDepLine> {
    let fields = systemd_unit_dep_fields(obs)?;
    Some(GraphSystemdDepLine {
        source: fields.raw_ref,
        statement: format!(
            "{}={} in unit file for {}",
            fields.key, fields.to_unit, fields.from_unit
        ),
        strength: systemd_dep_strength_label(&fields.key).to_string(),
    })
}

pub fn systemd_impact_statement(obs: &Observation) -> Option<(String, String)> {
    let fields = systemd_unit_dep_fields(obs)?;
    Some((
        format!(
            "Configured: {}={} in unit file for {}",
            fields.key, fields.to_unit, fields.from_unit
        ),
        systemd_dep_strength_label(&fields.key).to_string(),
    ))
}

pub fn systemd_dep_strength_label(key: &str) -> &'static str {
    if key == "Wants" {
        "moderate"
    } else {
        "strong"
    }
}

pub fn is_runtime_active_state(state: Option<&str>) -> bool {
    matches!(
        state,
        Some("active") | Some("activating") | Some("reloading") | Some("deactivating")
    )
}

use twin_core::{DependentImpactKind, EdgeClass, GraphEdge, GraphNode};

/// `is_dependent_running` is true when the dependent service is observed running: D-Bus
/// reports it active, or it owns live processes. The process signal keeps declared
/// dependencies of visibly running services runtime on hosts where D-Bus is unreadable.
pub fn classify_dependent_impact_kind(
    edge: &GraphEdge,
    is_dependent_running: bool,
) -> DependentImpactKind {
    if edge.class() == EdgeClass::Inferred {
        // A declared endpoint is a dependency only while its client runs.
        let is_declared = serde_json::from_str::<serde_json::Value>(edge.metadata().as_str())
            .ok()
            .is_some_and(|metadata| metadata["source"] == "config_declared");
        return if is_declared && !is_dependent_running {
            DependentImpactKind::Configured
        } else {
            DependentImpactKind::Runtime
        };
    }
    if edge.kind() == twin_core::EdgeKind::DependsOn
        && serde_json::from_str::<serde_json::Value>(edge.metadata().as_str())
            .ok()
            .and_then(|metadata| {
                metadata
                    .get("key")
                    .and_then(|key| key.as_str())
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("Wants")
    {
        return DependentImpactKind::Configured;
    }
    if is_dependent_running {
        DependentImpactKind::Runtime
    } else {
        DependentImpactKind::Configured
    }
}

/// Whether a service is observed running: active per D-Bus, or owning a live process.
pub fn is_service_running(
    store: &twin_store::Store,
    service: &GraphNode,
) -> Result<bool, twin_store::StoreError> {
    if is_runtime_active_metadata(service.metadata().as_str()) {
        return Ok(true);
    }
    for row in store.list_active_edges_from(service.id().as_str())? {
        let Ok(edge) = GraphEdge::try_from(&row) else {
            continue;
        };
        if edge.kind() == twin_core::EdgeKind::Owns
            && edge.to().kind() == Some(twin_core::NodeKind::Process)
            && store
                .get_node_typed(edge.to())?
                .is_some_and(|process| process.state() == twin_core::NodeState::Active)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn is_runtime_active_metadata(metadata: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return false;
    };
    is_runtime_active_state(value.get("active_state").and_then(|v| v.as_str()))
}
