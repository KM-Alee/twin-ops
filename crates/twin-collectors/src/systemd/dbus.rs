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
    pub invocation_id: Option<String>,
    pub triggered_by: Vec<String>,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub control_group: String,
    pub fragment_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketListenAddress {
    kind: SocketListenKind,
    address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketListenKind {
    Stream,
    Datagram,
    SequentialPacket,
    Fifo,
}

impl SocketListenKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "Stream" => Some(Self::Stream),
            "Datagram" => Some(Self::Datagram),
            "SequentialPacket" => Some(Self::SequentialPacket),
            "FIFO" => Some(Self::Fifo),
            _ => None,
        }
    }

    fn metadata_key(self) -> &'static str {
        match self {
            Self::Stream => "socket_config_streams",
            Self::Datagram => "socket_config_datagrams",
            Self::SequentialPacket => "socket_config_sequential_packets",
            Self::Fifo => "socket_config_fifos",
        }
    }
}

impl SocketListenAddress {
    pub fn new(kind: SocketListenKind, address: String) -> Result<Self, SystemdDBusError> {
        if address.is_empty() || address.len() > 4096 || address.chars().any(char::is_control) {
            return Err(SystemdDBusError::CallFailed {
                detail: "invalid socket Listen address".into(),
            });
        }
        Ok(Self { kind, address })
    }

    pub fn kind(&self) -> SocketListenKind {
        self.kind
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}

/// When a timer last fired and will next fire, as wall-clock nanoseconds since the epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimerSchedule {
    pub last_trigger_ns: Option<i64>,
    pub next_elapse_ns: Option<i64>,
}

impl TimerSchedule {
    /// systemd reports microseconds, with 0 (and `u64::MAX` for the next run) meaning none.
    pub fn from_usec(last_trigger_usec: u64, next_elapse_usec: u64) -> Self {
        let to_ns = |usec: u64| {
            (usec != 0 && usec != u64::MAX)
                .then(|| i64::try_from(usec).ok()?.checked_mul(1_000))
                .flatten()
        };
        Self {
            last_trigger_ns: to_ns(last_trigger_usec),
            next_elapse_ns: to_ns(next_elapse_usec),
        }
    }
}

pub trait SystemdDBusReader {
    fn list_units(&self) -> Result<Vec<UnitDBusSnapshot>, SystemdDBusError>;

    fn timer_schedule(&self, _unit: &str) -> Result<TimerSchedule, SystemdDBusError> {
        Err(SystemdDBusError::Unavailable {
            detail: "timer properties unavailable".into(),
        })
    }

    fn socket_listen(&self, _unit: &str) -> Result<Vec<SocketListenAddress>, SystemdDBusError> {
        Err(SystemdDBusError::Unavailable {
            detail: "socket Listen property unavailable".into(),
        })
    }

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

    fn get_unit(&self, name: &str) -> zbus::Result<OwnedObjectPath>;

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

fn invocation_id_prop(props: &HashMap<String, OwnedValue>) -> Option<String> {
    let values = Vec::<OwnedValue>::try_from(props.get("InvocationID")?.clone()).ok()?;
    let bytes: Vec<u8> = values
        .into_iter()
        .map(u8::try_from)
        .collect::<Result<_, _>>()
        .ok()?;
    if bytes.len() != 16 || bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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
            let triggered_by = string_vec_prop(&props, "TriggeredBy");
            let invocation_id = invocation_id_prop(&props);
            let (requires, wants, control_group, fragment_path) = unit_props_from_get_all(props);
            out.push(UnitDBusSnapshot {
                name,
                active_state,
                load_state,
                sub_state,
                invocation_id,
                triggered_by,
                requires,
                wants,
                control_group,
                fragment_path,
            });
        }
        Ok(out)
    }

    fn timer_schedule(&self, unit: &str) -> Result<TimerSchedule, SystemdDBusError> {
        let failed = || SystemdDBusError::CallFailed {
            detail: "timer properties unreadable".into(),
        };
        let manager = SystemdManagerProxyBlocking::new(&self.connection).map_err(|_| failed())?;
        let path = manager.get_unit(unit).map_err(|_| failed())?;
        let props = UnitPropertiesProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(|_| failed())?
            .build()
            .map_err(|_| failed())?
            .get_all("org.freedesktop.systemd1.Timer")
            .map_err(|_| failed())?;
        let usec = |key: &str| {
            props
                .get(key)
                .and_then(|value| u64::try_from(value.clone()).ok())
                .ok_or_else(failed)
        };
        Ok(TimerSchedule::from_usec(
            usec("LastTriggerUSec")?,
            usec("NextElapseUSecRealtime")?,
        ))
    }

    fn socket_listen(&self, unit: &str) -> Result<Vec<SocketListenAddress>, SystemdDBusError> {
        let manager = SystemdManagerProxyBlocking::new(&self.connection)
            .map_err(|_| socket_property_error())?;
        let path = manager
            .get_unit(unit)
            .map_err(|_| socket_property_error())?;
        let proxy = UnitPropertiesProxyBlocking::builder(&self.connection)
            .path(path)
            .map_err(|_| socket_property_error())?
            .build()
            .map_err(|_| socket_property_error())?;
        let props = proxy
            .get_all("org.freedesktop.systemd1.Socket")
            .map_err(|_| socket_property_error())?;
        let value = props.get("Listen").ok_or_else(socket_property_error)?;
        let rows = Vec::<(String, String)>::try_from(value.clone())
            .map_err(|_| socket_property_error())?;
        if rows.len() > 256 {
            return Err(socket_property_error());
        }
        rows.into_iter()
            .map(|(kind, address)| {
                let kind = SocketListenKind::parse(&kind).ok_or_else(socket_property_error)?;
                SocketListenAddress::new(kind, address)
            })
            .collect()
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

fn socket_property_error() -> SystemdDBusError {
    SystemdDBusError::CallFailed {
        detail: "socket Listen property unreadable or unsupported".into(),
    }
}

pub struct FixtureSystemdDBusReader {
    units: Vec<UnitDBusSnapshot>,
    socket_listen: HashMap<String, Vec<SocketListenAddress>>,
    timer_schedules: HashMap<String, TimerSchedule>,
    cgroup_to_unit: std::collections::HashMap<String, String>,
}

impl FixtureSystemdDBusReader {
    pub fn with_socket_listen(mut self, listen: HashMap<String, Vec<SocketListenAddress>>) -> Self {
        self.socket_listen = listen;
        self
    }

    pub fn with_timer_schedules(mut self, schedules: HashMap<String, TimerSchedule>) -> Self {
        self.timer_schedules = schedules;
        self
    }

    pub fn new(units: Vec<UnitDBusSnapshot>) -> Self {
        Self {
            units,
            socket_listen: HashMap::new(),
            timer_schedules: HashMap::new(),
            cgroup_to_unit: std::collections::HashMap::new(),
        }
    }

    pub fn with_cgroup_map(
        units: Vec<UnitDBusSnapshot>,
        cgroup_to_unit: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            units,
            socket_listen: HashMap::new(),
            timer_schedules: HashMap::new(),
            cgroup_to_unit,
        }
    }
}

impl SystemdDBusReader for FixtureSystemdDBusReader {
    fn timer_schedule(&self, unit: &str) -> Result<TimerSchedule, SystemdDBusError> {
        self.timer_schedules
            .get(unit)
            .copied()
            .ok_or_else(|| SystemdDBusError::CallFailed {
                detail: "timer properties unreadable".into(),
            })
    }

    fn socket_listen(&self, unit: &str) -> Result<Vec<SocketListenAddress>, SystemdDBusError> {
        self.socket_listen
            .get(unit)
            .cloned()
            .ok_or_else(socket_property_error)
    }

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
        } else if unit.name.ends_with(".timer") {
            "timer"
        } else {
            "service"
        };
        state_meta.insert_str("unit_type", unit_type);
        if unit_type == "timer" {
            match reader.timer_schedule(&unit.name) {
                Ok(schedule) => {
                    state_meta.insert_bool("timer_schedule_known", true);
                    if let Some(last) = schedule.last_trigger_ns {
                        state_meta.insert_str("timer_last_trigger_ns", &last.to_string());
                    }
                    if let Some(next) = schedule.next_elapse_ns {
                        state_meta.insert_str("timer_next_elapse_ns", &next.to_string());
                    }
                }
                Err(_) => state_meta.insert_bool("timer_schedule_known", false),
            }
        }
        if unit_type == "socket" {
            match reader.socket_listen(&unit.name) {
                Ok(listen) if listen.len() <= 256 => {
                    state_meta.insert_bool("socket_config_known", true);
                    state_meta.insert_str(
                        "socket_config_source",
                        "org.freedesktop.systemd1.Socket.Listen",
                    );
                    for kind in [
                        SocketListenKind::Stream,
                        SocketListenKind::Datagram,
                        SocketListenKind::SequentialPacket,
                        SocketListenKind::Fifo,
                    ] {
                        let addresses: Vec<String> = listen
                            .iter()
                            .filter(|entry| entry.kind == kind)
                            .map(|entry| entry.address.clone())
                            .collect();
                        state_meta.insert_str_array(kind.metadata_key(), &addresses);
                    }
                }
                _ => {
                    state_meta.insert_bool("socket_config_known", false);
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::SocketPropertyUnreadable,
                        std::path::PathBuf::from(&unit.name),
                        "socket Listen property unreadable or unsupported",
                    ));
                }
            }
        }
        state_meta.insert_str("active_state", &unit.active_state);
        state_meta.insert_str("load_state", &unit.load_state);
        state_meta.insert_str("sub_state", &unit.sub_state);
        if let Some(id) = &unit.invocation_id {
            state_meta.insert_str("invocation_id", id);
        }
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

        for socket in unit
            .triggered_by
            .iter()
            .filter(|unit| unit.ends_with(".socket"))
        {
            let mut metadata = ObservationMetadata::new();
            metadata.insert_str("socket_unit", socket);
            metadata.insert_str("service_unit", &unit.name);
            metadata.insert_str("source", "systemd_dbus");
            observations.push(RawObservation {
                source: ObservationSource::SystemdDBus,
                kind: ObservationKind::SystemdSocketActivates,
                collector: CollectorName::new(DBUS_COLLECTOR_NAME),
                subject: Some(RawIdentity::Service {
                    unit: socket.clone(),
                }),
                object: Some(RawIdentity::Service {
                    unit: unit.name.clone(),
                }),
                timestamp: started_at,
                raw_ref: Some(RawEvidenceRef::new(
                    "org.freedesktop.systemd1.Unit.TriggeredBy",
                )),
                confidence_hint: ConfidenceHint::High,
                metadata,
            });
        }
        for timer in unit
            .triggered_by
            .iter()
            .filter(|unit| unit.ends_with(".timer"))
        {
            let mut metadata = ObservationMetadata::new();
            metadata.insert_str("timer_unit", timer);
            metadata.insert_str("service_unit", &unit.name);
            metadata.insert_str("source", "systemd_dbus");
            observations.push(RawObservation {
                source: ObservationSource::SystemdDBus,
                kind: ObservationKind::SystemdTimerActivates,
                collector: CollectorName::new(DBUS_COLLECTOR_NAME),
                subject: Some(RawIdentity::Service {
                    unit: timer.clone(),
                }),
                object: Some(RawIdentity::Service {
                    unit: unit.name.clone(),
                }),
                timestamp: started_at,
                raw_ref: Some(RawEvidenceRef::new(
                    "org.freedesktop.systemd1.Unit.TriggeredBy",
                )),
                confidence_hint: ConfidenceHint::High,
                metadata,
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_property_requires_a_nonzero_128_bit_identity() {
        let mut props = HashMap::new();
        assert_eq!(invocation_id_prop(&props), None);
        for bytes in [vec![0u8; 16], vec![1u8; 15], vec![1u8; 16]] {
            props.insert(
                "InvocationID".into(),
                OwnedValue::try_from(zbus::zvariant::Value::from(bytes.clone())).expect("property"),
            );
            let identity = invocation_id_prop(&props);
            if bytes == vec![1; 16] {
                assert_eq!(
                    identity.as_deref(),
                    Some("01010101010101010101010101010101")
                );
            } else {
                assert_eq!(identity, None);
            }
        }
    }
}
