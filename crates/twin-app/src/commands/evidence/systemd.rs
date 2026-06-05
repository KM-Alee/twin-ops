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

pub fn is_runtime_active_metadata(metadata: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return false;
    };
    is_runtime_active_state(value.get("active_state").and_then(|v| v.as_str()))
}
