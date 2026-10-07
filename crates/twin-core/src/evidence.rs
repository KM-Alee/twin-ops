use crate::risk::{cap_dependent_evidence_score, EvidenceStrength};

pub const SOURCE_NONE: i32 = 15;
pub const SOURCE_CONFIG: i32 = 45;
pub const SOURCE_SOCKET: i32 = 75;
pub const SOURCE_EBPF: i32 = 70;
pub const RUNTIME_CONFIRMATION: i32 = 5;
pub const STATIC_CONFIRMATION: i32 = 4;
pub const REPEAT_AT_LEAST_TWO: i32 = 16;
pub const RECENCY_FRESH: i32 = 4;
pub const RECENCY_RECENT: i32 = 2;
pub const INDEPENDENT_SOURCE: i32 = 3;
pub const INDEPENDENT_SOURCE_CAP: i32 = 2;
pub const PERMISSION_GAP: i32 = 1;
pub const PERMISSION_GAP_CAP: i32 = 8;
pub const CONFLICTING: i32 = 20;
pub const DROPPED_EBPF: i32 = 16;

const FRESH_NS: i64 = 10 * 60 * 1_000_000_000;
const RECENT_NS: i64 = 60 * 60 * 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSourceKind {
    None,
    Config,
    SocketTable,
    Ebpf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRecency {
    Unknown,
    Stale,
    Recent,
    Fresh,
}

impl EvidenceRecency {
    pub fn from_timestamps(observed_ns: i64, reference_ns: i64) -> Self {
        if observed_ns <= 0 || reference_ns <= 0 {
            return Self::Unknown;
        }
        let age = reference_ns.saturating_sub(observed_ns);
        if age <= FRESH_NS {
            Self::Fresh
        } else if age <= RECENT_NS {
            Self::Recent
        } else {
            Self::Stale
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceFactors {
    pub source: EvidenceSourceKind,
    pub runtime_confirmed: bool,
    pub static_confirmed: bool,
    pub recency: EvidenceRecency,
    pub repeat_count: u32,
    pub independent_sources: u32,
    pub permission_gaps: u32,
    pub conflicting: bool,
    pub dropped_ebpf: bool,
}

impl EvidenceFactors {
    pub fn none() -> Self {
        Self {
            source: EvidenceSourceKind::None,
            runtime_confirmed: false,
            static_confirmed: false,
            recency: EvidenceRecency::Unknown,
            repeat_count: 0,
            independent_sources: 0,
            permission_gaps: 0,
            conflicting: false,
            dropped_ebpf: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceExplanation {
    pub strength: EvidenceStrength,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceLineRef<'a> {
    pub source: &'a str,
    pub statement: &'a str,
    pub relationship: &'a str,
    pub strength_score: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceAdjustments {
    pub permission_gaps: u32,
    pub dropped_ebpf: bool,
    pub recency: EvidenceRecency,
    pub conflicting: bool,
}

impl Default for EvidenceAdjustments {
    fn default() -> Self {
        Self {
            permission_gaps: 0,
            dropped_ebpf: false,
            recency: EvidenceRecency::Unknown,
            conflicting: false,
        }
    }
}

pub fn score_evidence(factors: EvidenceFactors) -> EvidenceExplanation {
    let mut score = match factors.source {
        EvidenceSourceKind::None => SOURCE_NONE,
        EvidenceSourceKind::Config => SOURCE_CONFIG,
        EvidenceSourceKind::SocketTable => SOURCE_SOCKET,
        EvidenceSourceKind::Ebpf => SOURCE_EBPF,
    };
    let runtime_bonus = factors.runtime_confirmed && factors.source == EvidenceSourceKind::Ebpf;
    if runtime_bonus {
        score += RUNTIME_CONFIRMATION;
    }
    let static_bonus = factors.static_confirmed && factors.source != EvidenceSourceKind::Config;
    if static_bonus {
        score += STATIC_CONFIRMATION;
    }
    score += match factors.recency {
        EvidenceRecency::Fresh => RECENCY_FRESH,
        EvidenceRecency::Recent => RECENCY_RECENT,
        EvidenceRecency::Stale | EvidenceRecency::Unknown => 0,
    };
    let repeated = factors.repeat_count >= 2;
    if repeated {
        score += REPEAT_AT_LEAST_TWO;
    }
    score += independent_bonus(&factors);
    let permission_penalty =
        (factors.permission_gaps as i32).min(PERMISSION_GAP_CAP) * PERMISSION_GAP;
    score -= permission_penalty;
    if factors.conflicting {
        score -= CONFLICTING;
    }
    let dropped = factors.dropped_ebpf && factors.source == EvidenceSourceKind::Ebpf;
    if dropped {
        score -= DROPPED_EBPF;
    }
    let score = score.clamp(0, 100) as u8;

    let mut reasons = Vec::new();
    match factors.source {
        EvidenceSourceKind::Ebpf => reasons.push("eBPF connect observed".to_string()),
        EvidenceSourceKind::SocketTable => {
            reasons.push("socket inode mapped to process".to_string())
        }
        EvidenceSourceKind::Config => {
            reasons.push("static configuration confirms the relationship".to_string())
        }
        EvidenceSourceKind::None => reasons.push("no supporting observations".to_string()),
    }
    if runtime_bonus {
        reasons.push("socket inode mapped to process".to_string());
    }
    if static_bonus {
        reasons.push("process mapped to service".to_string());
    }
    if repeated {
        reasons.push("relationship observed repeatedly".to_string());
    }
    match factors.recency {
        EvidenceRecency::Fresh | EvidenceRecency::Recent => {
            reasons.push("observation is recent".to_string())
        }
        EvidenceRecency::Stale => reasons.push("observation is stale".to_string()),
        EvidenceRecency::Unknown => {}
    }
    if independent_bonus(&factors) > 0 {
        reasons.push("independent sources agree".to_string());
    }
    if permission_penalty > 0 {
        reasons.push("permission gaps reduce confidence".to_string());
    }
    if factors.conflicting {
        reasons.push("conflicting evidence lowers confidence".to_string());
    }
    if dropped {
        reasons.push("dropped eBPF events weaken this claim".to_string());
    }

    EvidenceExplanation {
        strength: EvidenceStrength::new(score),
        reasons,
    }
}

pub fn score_capped_evidence(
    factors: EvidenceFactors,
    has_observation_links: bool,
    only_inferred: bool,
    has_weakening_unknowns: bool,
    has_dependents_or_evidence: bool,
) -> EvidenceExplanation {
    if !has_observation_links && !has_dependents_or_evidence {
        return EvidenceExplanation {
            strength: EvidenceStrength::weak(),
            reasons: vec!["no supporting observations".to_string()],
        };
    }
    let explained = score_evidence(factors);
    let inferred_cap = only_inferred && factors.source != EvidenceSourceKind::Ebpf;
    let capped = cap_dependent_evidence_score(
        explained.strength.score(),
        has_observation_links,
        inferred_cap,
        has_weakening_unknowns,
        has_dependents_or_evidence,
    );
    let mut reasons = explained.reasons;
    if capped.score() < explained.strength.score() {
        reasons.push("missing links or coverage gaps cap confidence".to_string());
    }
    EvidenceExplanation {
        strength: capped,
        reasons,
    }
}

pub fn factors_from_lines(
    lines: &[EvidenceLineRef<'_>],
    adjustments: EvidenceAdjustments,
) -> EvidenceFactors {
    let mut ebpf = 0u32;
    let mut socket = false;
    let mut config = false;
    let mut static_confirmed = false;
    let mut best = 0u8;
    for line in lines {
        best = best.max(line.strength_score);
        if let Some(count) = ebpf_repeat(line.statement) {
            ebpf = ebpf.max(count);
        } else if line.source == "ebpf" || line.statement.contains("eBPF") {
            ebpf = ebpf.max(1);
        }
        if line.statement.contains("inode") {
            socket = true;
        }
        if line.statement.contains("ownership")
            || line.statement.contains("cgroup")
            || line.relationship.contains("service dependency inferred")
            || line
                .relationship
                .contains("runtime connection observed by eBPF")
        {
            static_confirmed = true;
        }
        if line.source.contains("systemd")
            || line.source == "config_file_discovery"
            || line.relationship.contains("systemd")
            || line.relationship.contains("configured_by")
            || line.statement.contains("Requires")
            || line.statement.contains("Wants")
            || line.statement.contains("configured_by")
        {
            config = true;
        }
    }

    let source = if ebpf > 0 {
        EvidenceSourceKind::Ebpf
    } else if socket {
        EvidenceSourceKind::SocketTable
    } else if config {
        EvidenceSourceKind::Config
    } else if best >= 86 {
        EvidenceSourceKind::Ebpf
    } else if best >= 61 {
        EvidenceSourceKind::SocketTable
    } else if best >= 31 {
        EvidenceSourceKind::Config
    } else {
        EvidenceSourceKind::None
    };

    let repeat_count = if ebpf > 0 {
        ebpf
    } else if source == EvidenceSourceKind::Ebpf {
        2
    } else if source == EvidenceSourceKind::None {
        0
    } else {
        1
    };

    let mut kinds = 0u32;
    if ebpf > 0 || (ebpf == 0 && source == EvidenceSourceKind::Ebpf && best >= 86) {
        kinds += 1;
    }
    if socket {
        kinds += 1;
    }
    if config || (source == EvidenceSourceKind::Config && ebpf == 0 && !socket) {
        kinds += 1;
    }
    if static_confirmed && source != EvidenceSourceKind::Config {
        kinds += 1;
    }
    let line_count = lines.len() as u32;
    let independent_sources = if line_count > kinds {
        line_count
    } else {
        kinds
    };

    EvidenceFactors {
        source,
        runtime_confirmed: socket && source == EvidenceSourceKind::Ebpf,
        static_confirmed: static_confirmed && source != EvidenceSourceKind::Config,
        recency: adjustments.recency,
        repeat_count,
        independent_sources,
        permission_gaps: adjustments.permission_gaps,
        conflicting: adjustments.conflicting,
        dropped_ebpf: adjustments.dropped_ebpf,
    }
}

fn independent_bonus(factors: &EvidenceFactors) -> i32 {
    let mut accounted = 1u32;
    if factors.runtime_confirmed && factors.source == EvidenceSourceKind::Ebpf {
        accounted += 1;
    }
    if factors.static_confirmed && factors.source != EvidenceSourceKind::Config {
        accounted += 1;
    }
    let extra = factors.independent_sources.saturating_sub(accounted) as i32;
    extra.min(INDEPENDENT_SOURCE_CAP) * INDEPENDENT_SOURCE
}

fn ebpf_repeat(statement: &str) -> Option<u32> {
    let rest = statement.strip_prefix("eBPF observed ")?;
    let count = rest.split_whitespace().next()?;
    let parsed = count.parse().ok()?;
    if parsed == 0 {
        None
    } else {
        Some(parsed)
    }
}
