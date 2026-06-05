use std::collections::HashMap;
use std::path::PathBuf;

use twin_core::TimestampNs;
use twin_observation::RawObservation;

use crate::systemd::dbus::{
    collect_dbus_observations, try_connect_dbus, FixtureSystemdDBusReader, UnitDBusSnapshot,
    DBUS_COLLECTOR_NAME,
};
use crate::systemd::enable_symlinks::{collect_enable_symlinks, ENABLE_COLLECTOR_NAME};
use crate::systemd::warning::SystemdWarning;
use crate::CollectorError;

pub const RUNTIME_COLLECTOR_NAME: &str = "systemd_runtime";

pub struct SystemdRuntimeBatch {
    observations: Vec<RawObservation>,
    warnings: Vec<SystemdWarning>,
    dbus_available: bool,
    dbus_unit_count: usize,
    enable_symlink_count: usize,
    cgroup_dbus_reader: Option<FixtureSystemdDBusReader>,
    started_at: TimestampNs,
    ended_at: TimestampNs,
}

impl SystemdRuntimeBatch {
    pub fn observations(&self) -> &[RawObservation] {
        &self.observations
    }

    pub fn warnings(&self) -> &[SystemdWarning] {
        &self.warnings
    }

    pub fn dbus_available(&self) -> bool {
        self.dbus_available
    }

    pub fn dbus_unit_count(&self) -> usize {
        self.dbus_unit_count
    }

    pub fn enable_symlink_count(&self) -> usize {
        self.enable_symlink_count
    }

    pub fn started_at(&self) -> TimestampNs {
        self.started_at
    }

    pub fn ended_at(&self) -> TimestampNs {
        self.ended_at
    }

    pub fn drain_observations(&mut self) -> Vec<RawObservation> {
        std::mem::take(&mut self.observations)
    }

    pub fn cgroup_dbus_reader(&self) -> Option<&FixtureSystemdDBusReader> {
        self.cgroup_dbus_reader.as_ref()
    }
}

pub struct SystemdRuntimeCollector {
    search_roots: Vec<PathBuf>,
    dbus_fixture: Option<Vec<UnitDBusSnapshot>>,
    cgroup_fixture: Option<HashMap<String, String>>,
}

impl SystemdRuntimeCollector {
    pub fn new(search_roots: Vec<PathBuf>) -> Self {
        Self {
            search_roots,
            dbus_fixture: None,
            cgroup_fixture: None,
        }
    }

    pub fn with_dbus_fixture(search_roots: Vec<PathBuf>, units: Vec<UnitDBusSnapshot>) -> Self {
        Self {
            search_roots,
            dbus_fixture: Some(units),
            cgroup_fixture: None,
        }
    }

    pub fn with_dbus_and_cgroup_fixture(
        search_roots: Vec<PathBuf>,
        units: Vec<UnitDBusSnapshot>,
        cgroup_to_unit: HashMap<String, String>,
    ) -> Self {
        Self {
            search_roots,
            dbus_fixture: Some(units),
            cgroup_fixture: Some(cgroup_to_unit),
        }
    }

    pub fn from_default_paths() -> Self {
        Self::new(crate::systemd::default_search_paths())
    }

    pub fn collect(&self, started_at: TimestampNs) -> Result<SystemdRuntimeBatch, CollectorError> {
        let mut warnings = Vec::new();
        let mut observations = Vec::new();

        let enable_obs = collect_enable_symlinks(&self.search_roots, started_at, &mut warnings)?;
        let enable_symlink_count = enable_obs.len();
        observations.extend(enable_obs);

        let mut dbus_available = false;
        let mut dbus_unit_count = 0usize;
        if self.search_roots.is_empty()
            && self.dbus_fixture.is_none()
            && self.cgroup_fixture.is_none()
        {
            let _ = (
                DBUS_COLLECTOR_NAME,
                ENABLE_COLLECTOR_NAME,
                RUNTIME_COLLECTOR_NAME,
            );
            return Ok(SystemdRuntimeBatch {
                observations,
                warnings,
                dbus_available,
                dbus_unit_count,
                enable_symlink_count,
                cgroup_dbus_reader: None,
                started_at,
                ended_at: TimestampNs::now(),
            });
        }

        let mut cgroup_dbus_reader = None;
        let skip_live_dbus = std::env::var("TWIN_SYSTEMD_UNIT_ROOT").is_ok();
        if let Some(fixture) = &self.dbus_fixture {
            let reader = match &self.cgroup_fixture {
                Some(map) => {
                    let r = FixtureSystemdDBusReader::with_cgroup_map(fixture.clone(), map.clone());
                    cgroup_dbus_reader = Some(FixtureSystemdDBusReader::with_cgroup_map(
                        fixture.clone(),
                        map.clone(),
                    ));
                    r
                }
                None => FixtureSystemdDBusReader::new(fixture.clone()),
            };
            let dbus_obs = collect_dbus_observations(&reader, started_at, &mut warnings)?;
            dbus_unit_count = dbus_obs
                .iter()
                .filter(|o| o.kind == twin_observation::ObservationKind::SystemdUnitStateSeen)
                .count();
            dbus_available = true;
            observations.extend(dbus_obs);
        } else if !skip_live_dbus {
            if let Some(reader) = try_connect_dbus() {
                if let Ok(dbus_obs) = collect_dbus_observations(&reader, started_at, &mut warnings)
                {
                    dbus_unit_count = dbus_obs
                        .iter()
                        .filter(|o| {
                            o.kind == twin_observation::ObservationKind::SystemdUnitStateSeen
                        })
                        .count();
                    dbus_available = dbus_unit_count > 0;
                    observations.extend(dbus_obs);
                }
            }
        }

        let _ = (
            DBUS_COLLECTOR_NAME,
            ENABLE_COLLECTOR_NAME,
            RUNTIME_COLLECTOR_NAME,
        );

        Ok(SystemdRuntimeBatch {
            observations,
            warnings,
            dbus_available,
            dbus_unit_count,
            enable_symlink_count,
            cgroup_dbus_reader,
            started_at,
            ended_at: TimestampNs::now(),
        })
    }
}
