use thiserror::Error;
use twin_core::TimestampNs;
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};
use zbus::blocking::Connection;
use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

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
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdUnit {
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn sub_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn requires(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property)]
    fn wants(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property)]
    fn control_group(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn fragment_path(&self) -> zbus::Result<String>;
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
            let unit_proxy = SystemdUnitProxyBlocking::builder(&self.connection)
                .path(path.clone())
                .map_err(|e| SystemdDBusError::CallFailed {
                    detail: e.to_string(),
                })?
                .build()
                .map_err(|e| SystemdDBusError::CallFailed {
                    detail: e.to_string(),
                })?;
            let requires = unit_proxy.requires().unwrap_or_default();
            let wants = unit_proxy.wants().unwrap_or_default();
            let control_group = unit_proxy.control_group().unwrap_or_default();
            let fragment_path = unit_proxy.fragment_path().unwrap_or_default();
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
        let unit_proxy = SystemdUnitProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?
            .build()
            .map_err(|e| SystemdDBusError::CallFailed {
                detail: e.to_string(),
            })?;
        Ok(unit_proxy.id().ok())
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
    let mut meta = ObservationMetadata::new();
    meta.insert_str("from_unit", from_unit);
    meta.insert_str("to_unit", to_unit);
    meta.insert_str("key", key);
    meta.insert_str("source", "systemd_dbus");
    RawObservation {
        source: ObservationSource::SystemdDBus,
        kind,
        collector: CollectorName::new(DBUS_COLLECTOR_NAME),
        subject: Some(RawIdentity::Service {
            unit: from_unit.to_string(),
        }),
        object: Some(RawIdentity::Service {
            unit: to_unit.to_string(),
        }),
        timestamp,
        raw_ref: Some(RawEvidenceRef::new("org.freedesktop.systemd1")),
        confidence_hint: hint,
        metadata: meta,
    }
}

pub fn try_connect_dbus() -> Option<StdSystemdDBusReader> {
    StdSystemdDBusReader::connect().ok()
}
