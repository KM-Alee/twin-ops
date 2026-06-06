use std::str::FromStr;

use twin_observation::{ObservationKind, ObservationSource};

#[test]
fn observation_sources_round_trip() {
    for source in [
        ObservationSource::Proc,
        ObservationSource::ProcNetTcp,
        ObservationSource::ProcNetUnix,
        ObservationSource::ProcCgroup,
        ObservationSource::SystemdUnitFile,
        ObservationSource::SystemdDBus,
        ObservationSource::SystemdEnableSymlink,
        ObservationSource::ConfigFileDiscovery,
    ] {
        let s = source.to_string();
        let parsed = ObservationSource::from_str(&s).expect("parse");
        assert_eq!(parsed, source);
    }
}

#[test]
fn process_observation_kinds_round_trip() {
    for kind in [
        ObservationKind::ProcessSeen,
        ObservationKind::ProcessCommandSeen,
        ObservationKind::ProcessExeSeen,
        ObservationKind::ProcessParentSeen,
    ] {
        let s = kind.to_string();
        let parsed = ObservationKind::from_str(&s).expect("parse");
        assert_eq!(parsed, kind);
    }
}

#[test]
fn systemd_observation_kinds_round_trip() {
    for kind in [
        ObservationKind::SystemdUnitSeen,
        ObservationKind::SystemdUnitRequires,
        ObservationKind::SystemdUnitWants,
        ObservationKind::UnixSocketSeen,
        ObservationKind::UnixConnectionSeen,
        ObservationKind::SystemdUnitStateSeen,
        ObservationKind::SystemdUnitWantedBy,
        ObservationKind::SystemdSocketSeen,
        ObservationKind::SystemdSocketActivates,
        ObservationKind::SystemdCgroupCorrection,
        ObservationKind::ConfigFileSeen,
        ObservationKind::ServiceConfiguredByFile,
    ] {
        let s = kind.to_string();
        let parsed = ObservationKind::from_str(&s).expect("parse");
        assert_eq!(parsed, kind);
    }
}
