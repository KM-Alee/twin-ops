use std::collections::HashMap;
use thiserror::Error;
use twin_core::TimestampNs;
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use zbus::blocking::Connection;
use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::systemd::warning::{SystemdWarning, SystemdWarningKind};
use twin_core::CollectorName;

pub const DBUS_COLLECTOR_NAME: &str = "systemd_dbus";

#[derive(Debug, Error)]
pub enum SystemdDBusError {
    #[error("systemd D-Bus unavailable: {detail}")]
    Unavailable { detail: String },
    #[error("systemd D-Bus call failed: {detail}")]
    CallFailed { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitDBusSnapshot {
    pub name: String,
    pub active_state: String,
    pub load_state: String,
    pub sub_state: String,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub control_group: String,
    pub fragment_path: String,
}

pub trait SystemdDBusReader {
    fn list_units(&self) -> Result<Vec<UnitDBusSnapshot>, SystemdDBusError>;

    fn get_unit_by_control_group(
        &self,
        cgroup_path: &str,
    ) -> Result<Option<String>, SystemdDBusError> {
        let _ = cgroup_path;
        Ok(None)
    }
}

pub struct StdSystemdDBusReader {
    connection: Connection,
}

impl StdSystemdDBusReader {
    pub fn connect() -> Result<Self, SystemdDBusError> {
        let connection = Connection::system().map_err(|e| SystemdDBusError::Unavailable {
            detail: e.to_string(),
        })?;
        Ok(Self { connection })
    }
}

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn list_units(&self) -> zbus::Result<Vec<UnitListTuple>>;

    fn get_unit_by_control_group(&self, cgroup: &str) -> zbus::Result<OwnedObjectPath>;
}

type UnitListTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

#[proxy(
    interface = "org.freedesktop.DBus.Properties",
    default_service = "org.freedesktop.systemd1"
)]
trait UnitProperties {
    fn get_all(&self, interface_name: &str) -> zbus::Result<HashMap<String, OwnedValue>>;
}

fn unit_props_from_get_all(
    props: HashMap<String, OwnedValue>,
) -> (Vec<String>, Vec<String>, String, String) {
    (
        string_vec_prop(&props, "Requires"),
        string_vec_prop(&props, "Wants"),
        string_prop(&props, "ControlGroup"),
        string_prop(&props, "FragmentPath"),
    )
}

fn string_prop(props: &HashMap<String, OwnedValue>, key: &str) -> String {
    props
        .get(key)
        .and_then(|v| v.downcast_ref::<String>().ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn string_vec_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Vec<String> {
    let Some(value) = props.get(key) else {
        return Vec::new();
    };
    let Ok(values) = <Vec<OwnedValue>>::try_from(value.clone()) else {
        return Vec::new();
    };
    values
        .into_iter()
        .filter_map(|v| String::try_from(v).ok())
        .collect()
}

impl SystemdDBusReader for StdSystemdDBusReader {
    fn list_units(&self) -> Result<Vec<UnitDBusSnapshot>, SystemdDBusError> {
        let manager = SystemdManagerProxyBlocking::new(&self.connection).map_err(|e| {
            SystemdDBusError::CallFailed {
                detail: e.to_string(),
            }
        })?;
        let rows = manager
            .list_units()
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?;
        let mut out = Vec::with_capacity(rows.len());
        for (
            name,
            _desc,
            load_state,
            active_state,
            sub_state,
            _following,
            path,
            _job_id,
            _job_type,
            _job_path,
        ) in rows
        {
            let props_proxy = UnitPropertiesProxyBlocking::builder(&self.connection)
                .path(path.clone())
                .map_err(|e| SystemdDBusError::CallFailed {
                    detail: e.to_string(),
                })?
                .build()
                .map_err(|e| SystemdDBusError::CallFailed {
                    detail: e.to_string(),
                })?;
            let props = props_proxy
                .get_all("org.freedesktop.systemd1.Unit")
                .unwrap_or_default();
            let (requires, wants, control_group, fragment_path) = unit_props_from_get_all(props);
            out.push(UnitDBusSnapshot {
                name,
                active_state,
                load_state,
                sub_state,
                requires,
                wants,
                control_group,
                fragment_path,
            });
        }
        Ok(out)
    }

    fn get_unit_by_control_group(
        &self,
        cgroup_path: &str,
    ) -> Result<Option<String>, SystemdDBusError> {
        if cgroup_path.is_empty() || !cgroup_path.starts_with('/') {
            return Err(SystemdDBusError::CallFailed {
                detail: "cgroup path must be absolute".to_string(),
            });
        }
        let manager = SystemdManagerProxyBlocking::new(&self.connection).map_err(|e| {
            SystemdDBusError::CallFailed {
                detail: e.to_string(),
            }
        })?;
        let path = manager
            .get_unit_by_control_group(cgroup_path)
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?;
        let props_proxy = UnitPropertiesProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?
            .build()
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?;
        let props = props_proxy
            .get_all("org.freedesktop.systemd1.Unit")
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?;
        let id = string_prop(&props, "Id");
        if id.is_empty() {
            Ok(None)
        } else {
            Ok(Some(id))
        }
    }
}

pub struct FixtureSystemdDBusReader {
    units: Vec<UnitDBusSnapshot>,
    cgroup_to_unit: std::collections::HashMap<String, String>,
}

impl FixtureSystemdDBusReader {
    pub fn new(units: Vec<UnitDBusSnapshot>) -> Self {
        Self {
            units,
            cgroup_to_unit: std::collections::HashMap::new(),
        }
    }

    pub fn with_cgroup_map(
        units: Vec<UnitDBusSnapshot>,
        cgroup_to_unit: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            units,
            cgroup_to_unit,
        }
    }
}

impl SystemdDBusReader for FixtureSystemdDBusReader {
    fn list_units(&self) -> Result<Vec<UnitDBusSnapshot>, SystemdDBusError> {
        Ok(self.units.clone())
    }

    fn get_unit_by_control_group(
        &self,
        cgroup_path: &str,
    ) -> Result<Option<String>, SystemdDBusError> {
        Ok(self.cgroup_to_unit.get(cgroup_path).cloned())
    }
}

pub fn collect_dbus_observations<R: SystemdDBusReader>(
    reader: &R,
    started_at: TimestampNs,
    warnings: &mut Vec<SystemdWarning>,
) -> Result<Vec<RawObservation>, SystemdDBusError> {
    let units = match reader.list_units() {
        Ok(units) => units,
        Err(SystemdDBusError::Unavailable { detail }) => {
            warnings.push(SystemdWarning::new(
                SystemdWarningKind::DbusUnavailable,
                std::path::PathBuf::from("/org/freedesktop/systemd1"),
                detail,
            ));
            return Ok(Vec::new());
        }
        Err(err) => return Err(err),
    };

    let mut observations = Vec::new();
    for unit in units {
        let mut state_meta = ObservationMetadata::new();
        state_meta.insert_str("unit", &unit.name);
        let unit_type = if unit.name.ends_with(".socket") {
            "socket"
        } else {
            "service"
        };
        state_meta.insert_str("unit_type", unit_type);
        state_meta.insert_str("active_state", &unit.active_state);
        state_meta.insert_str("load_state", &unit.load_state);
        state_meta.insert_str("sub_state", &unit.sub_state);
        if !unit.control_group.is_empty() {
            state_meta.insert_str("control_group", &unit.control_group);
        }
        if !unit.fragment_path.is_empty() {
            state_meta.insert_str("fragment_path", &unit.fragment_path);
        }
        observations.push(RawObservation {
            source: ObservationSource::SystemdDBus,
            kind: ObservationKind::SystemdUnitStateSeen,
            collector: CollectorName::new(DBUS_COLLECTOR_NAME),
            subject: Some(RawIdentity::Service {
                unit: unit.name.clone(),
            }),
            object: None,
            timestamp: started_at,
            raw_ref: Some(RawEvidenceRef::new("org.freedesktop.systemd1")),
            confidence_hint: ConfidenceHint::High,
            metadata: state_meta,
        });

        for dep in &unit.requires {
            observations.push(dep_observation(
                &unit.name,
                dep,
                "Requires",
                started_at,
                ConfidenceHint::High,
            ));
        }
        for dep in &unit.wants {
            observations.push(dep_observation(
                &unit.name,
                dep,
                "Wants",
                started_at,
                ConfidenceHint::Moderate,
            ));
        }
    }
    Ok(observations)
}

fn dep_observation(
    from_unit: &str,
    to_unit: &str,
    key: &str,
    timestamp: TimestampNs,
    hint: ConfidenceHint,
) -> RawObservation {
    let kind = if key == "Requires" {
        ObservationKind::SystemdUnitRequires
    } else {
        ObservationKind::SystemdUnitWants
    };
    super::observation::unit_dependency_raw(super::observation::UnitDependencyRawInput {
        source: ObservationSource::SystemdDBus,
        collector: DBUS_COLLECTOR_NAME,
        kind,
        from_unit,
        to_unit,
        key,
        timestamp,
        raw_ref: RawEvidenceRef::new("org.freedesktop.systemd1"),
        hint,
        extra_metadata: &[("source", "systemd_dbus")],
    })
}

pub fn try_connect_dbus() -> Option<StdSystemdDBusReader> {
    StdSystemdDBusReader::connect().ok()
}
