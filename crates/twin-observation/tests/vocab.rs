use std::str::FromStr;

use twin_observation::ObservationKind;

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
