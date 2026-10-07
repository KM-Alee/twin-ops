use twin_core::{
    factors_from_lines, score_capped_evidence, score_evidence, EvidenceAdjustments,
    EvidenceFactors, EvidenceLabel, EvidenceLineRef, EvidenceRecency, EvidenceSourceKind,
};

fn ebpf_repeated() -> EvidenceFactors {
    EvidenceFactors {
        source: EvidenceSourceKind::Ebpf,
        runtime_confirmed: false,
        static_confirmed: false,
        recency: EvidenceRecency::Unknown,
        repeat_count: 2,
        independent_sources: 1,
        permission_gaps: 0,
        conflicting: false,
        dropped_ebpf: false,
    }
}

#[test]
fn repeated_ebpf_connects_are_very_strong() {
    let explained = score_evidence(ebpf_repeated());
    assert_eq!(explained.strength.score(), 86);
    assert_eq!(explained.strength.label(), EvidenceLabel::VeryStrong);
    assert!(explained
        .reasons
        .iter()
        .any(|reason| reason == "eBPF connect observed"));
    assert!(explained
        .reasons
        .iter()
        .any(|reason| reason == "relationship observed repeatedly"));
}

#[test]
fn one_ebpf_connect_stays_strong() {
    let mut factors = ebpf_repeated();
    factors.repeat_count = 1;
    let explained = score_evidence(factors);
    assert_eq!(explained.strength.score(), 70);
    assert_eq!(explained.strength.label(), EvidenceLabel::Strong);
}

#[test]
fn socket_table_connection_stays_strong() {
    let explained = score_evidence(EvidenceFactors {
        source: EvidenceSourceKind::SocketTable,
        repeat_count: 1,
        independent_sources: 1,
        ..EvidenceFactors::none()
    });
    assert_eq!(explained.strength.score(), 75);
    assert_eq!(explained.strength.label(), EvidenceLabel::Strong);
    assert_eq!(
        explained.reasons,
        vec!["socket inode mapped to process".to_string()]
    );
}

#[test]
fn config_only_dependency_stays_moderate() {
    let explained = score_evidence(EvidenceFactors {
        source: EvidenceSourceKind::Config,
        static_confirmed: true,
        repeat_count: 1,
        independent_sources: 1,
        ..EvidenceFactors::none()
    });
    assert_eq!(explained.strength.score(), 45);
    assert_eq!(explained.strength.label(), EvidenceLabel::Moderate);
}

#[test]
fn dropped_ebpf_events_weaken_an_ebpf_claim_one_band() {
    let mut repeated = ebpf_repeated();
    repeated.dropped_ebpf = true;
    let explained = score_evidence(repeated);
    assert_eq!(explained.strength.score(), 70);
    assert_eq!(explained.strength.label(), EvidenceLabel::Strong);
    assert!(explained
        .reasons
        .iter()
        .any(|reason| reason == "dropped eBPF events weaken this claim"));

    let mut once = ebpf_repeated();
    once.repeat_count = 1;
    once.dropped_ebpf = true;
    let explained = score_evidence(once);
    assert_eq!(explained.strength.score(), 54);
    assert_eq!(explained.strength.label(), EvidenceLabel::Moderate);
}

#[test]
fn dropped_events_do_not_weaken_a_socket_table_claim() {
    let explained = score_evidence(EvidenceFactors {
        source: EvidenceSourceKind::SocketTable,
        repeat_count: 1,
        independent_sources: 1,
        dropped_ebpf: true,
        ..EvidenceFactors::none()
    });
    assert_eq!(explained.strength.score(), 75);
    assert_eq!(explained.strength.label(), EvidenceLabel::Strong);
}

#[test]
fn full_confirmation_with_permission_gaps_scores_87() {
    let explained = score_evidence(EvidenceFactors {
        source: EvidenceSourceKind::Ebpf,
        runtime_confirmed: true,
        static_confirmed: true,
        recency: EvidenceRecency::Unknown,
        repeat_count: 2,
        independent_sources: 3,
        permission_gaps: 8,
        conflicting: false,
        dropped_ebpf: false,
    });
    assert_eq!(explained.strength.score(), 87);
    assert_eq!(explained.strength.label(), EvidenceLabel::VeryStrong);
    assert_eq!(
        explained.reasons,
        vec![
            "eBPF connect observed".to_string(),
            "socket inode mapped to process".to_string(),
            "process mapped to service".to_string(),
            "relationship observed repeatedly".to_string(),
            "permission gaps reduce confidence".to_string(),
        ]
    );
}

#[test]
fn permission_gaps_reduce_the_score_without_panicking() {
    let open = score_evidence(ebpf_repeated());
    let mut hidden = ebpf_repeated();
    hidden.permission_gaps = 8;
    let hidden = score_evidence(hidden);
    assert_eq!(open.strength.score() - hidden.strength.score(), 8);
    assert!(hidden.strength.score() <= 100);
}

#[test]
fn same_factors_always_produce_the_same_score() {
    let factors = EvidenceFactors {
        source: EvidenceSourceKind::Ebpf,
        runtime_confirmed: true,
        static_confirmed: true,
        recency: EvidenceRecency::Fresh,
        repeat_count: 4,
        independent_sources: 3,
        permission_gaps: 2,
        conflicting: true,
        dropped_ebpf: true,
    };
    assert_eq!(
        score_evidence(factors).strength.score(),
        score_evidence(factors).strength.score()
    );
}

#[test]
fn recency_uses_injected_timestamps() {
    let fresh = EvidenceRecency::from_timestamps(1_000, 1_000 + 60 * 1_000_000_000);
    let recent = EvidenceRecency::from_timestamps(1_000, 1_000 + 30 * 60 * 1_000_000_000);
    let stale = EvidenceRecency::from_timestamps(1_000, 1_000 + 2 * 60 * 60 * 1_000_000_000);
    assert_eq!(fresh, EvidenceRecency::Fresh);
    assert_eq!(recent, EvidenceRecency::Recent);
    assert_eq!(stale, EvidenceRecency::Stale);
    assert_eq!(
        EvidenceRecency::from_timestamps(0, 5),
        EvidenceRecency::Unknown
    );

    let mut factors = ebpf_repeated();
    factors.recency = EvidenceRecency::Fresh;
    assert_eq!(score_evidence(factors).strength.score(), 90);
    factors.recency = EvidenceRecency::Recent;
    assert_eq!(score_evidence(factors).strength.score(), 88);
    factors.recency = EvidenceRecency::Stale;
    assert_eq!(score_evidence(factors).strength.score(), 86);
}

#[test]
fn conflicting_evidence_and_extra_sources_are_bounded() {
    let mut factors = ebpf_repeated();
    factors.conflicting = true;
    assert_eq!(score_evidence(factors).strength.score(), 66);

    factors.conflicting = false;
    factors.independent_sources = 6;
    assert_eq!(score_evidence(factors).strength.score(), 92);
}

#[test]
fn line_parser_keeps_socket_and_config_bands() {
    let socket = factors_from_lines(
        &[EvidenceLineRef {
            source: "/proc/net/tcp:2",
            statement: "Observed: inode 456 established from 127.0.0.1:1 to 127.0.0.1:5432",
            relationship: "Inferred: service dependency inferred from active connection",
            strength_score: 75,
        }],
        EvidenceAdjustments::default(),
    );
    let explained = score_evidence(socket);
    assert_eq!(explained.strength.label(), EvidenceLabel::Strong);

    let config = factors_from_lines(
        &[EvidenceLineRef {
            source: "config_file_discovery",
            statement: "known nginx config path",
            relationship: "configured_by",
            strength_score: 70,
        }],
        EvidenceAdjustments::default(),
    );
    assert_eq!(
        score_evidence(config).strength.label(),
        EvidenceLabel::Moderate
    );
}

#[test]
fn extra_path_line_raises_the_score_and_caps_still_apply() {
    let one = factors_from_lines(
        &[EvidenceLineRef {
            source: "/proc/net/tcp:2",
            statement: "Observed: direct connection",
            relationship: "inferred",
            strength_score: 70,
        }],
        EvidenceAdjustments::default(),
    );
    let two = factors_from_lines(
        &[
            EvidenceLineRef {
                source: "/proc/net/tcp:2",
                statement: "Observed: direct connection",
                relationship: "inferred",
                strength_score: 70,
            },
            EvidenceLineRef {
                source: "/proc/net/tcp:1",
                statement: "Observed: active connection",
                relationship: "inferred",
                strength_score: 95,
            },
        ],
        EvidenceAdjustments::default(),
    );
    let low = score_capped_evidence(one, true, true, false, true);
    let high = score_capped_evidence(two, true, true, false, true);
    assert!(high.strength.score() > low.strength.score());

    let weakened = score_capped_evidence(two, true, true, true, true);
    assert!(weakened.strength.score() <= 60);
    assert!(weakened
        .reasons
        .iter()
        .any(|reason| reason.contains("cap confidence")));
}
