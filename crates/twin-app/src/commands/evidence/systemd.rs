use twin_observation::{Observation, ObservationKind};

pub struct GraphSystemdDepLine {
    pub source: String,
    pub statement: String,
    pub strength: String,
}

pub fn graph_systemd_dep_line(obs: &Observation) -> Option<GraphSystemdDepLine> {
    if !matches!(
        obs.kind(),
        ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants
    ) {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str().to_string();
    let from_unit = obs.metadata().get("from_unit").and_then(|v| v.as_str())?;
    let to_unit = obs.metadata().get("to_unit").and_then(|v| v.as_str())?;
    let key = obs
        .metadata()
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("Requires");
    Some(GraphSystemdDepLine {
        source: raw_ref,
        statement: format!("{key}={to_unit} in unit file for {from_unit}"),
        strength: systemd_dep_strength_label(key).to_string(),
    })
}

pub fn systemd_impact_statement(obs: &Observation) -> Option<(String, String)> {
    if !matches!(
        obs.kind(),
        ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants
    ) {
        return None;
    }
    let from_unit = obs.metadata().get("from_unit").and_then(|v| v.as_str())?;
    let to_unit = obs.metadata().get("to_unit").and_then(|v| v.as_str())?;
    let key = obs
        .metadata()
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("Requires");
    Some((
        format!("Configured: {key}={to_unit} in unit file for {from_unit}"),
        systemd_dep_strength_label(key).to_string(),
    ))
}

pub fn systemd_dep_strength_label(key: &str) -> &'static str {
    if key == "Wants" {
        "moderate"
    } else {
        "strong"
    }
}
