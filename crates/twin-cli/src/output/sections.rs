pub const NEW: &str = "new";
pub const DISAPPEARED: &str = "disappeared";
pub const STALE: &str = "stale";
pub const CHANGED: &str = "changed";
pub const REAPPEARED: &str = "reappeared";
pub const ADDED: &str = "added";
pub const REMOVED: &str = "removed";
pub const TARGET: &str = "target";
pub const RISK: &str = "risk";
pub const EVIDENCE_STRENGTH: &str = "evidence strength";
pub const SCAN_HEALTH: &str = "scan health";
pub const UNKNOWNS: &str = "unknowns";
pub const OVERLAY: &str = "overlay";
pub const OVERLAY_SERVICE: &str = "service";
pub const OVERLAY_TCP_LISTENERS: &str = "tcp listeners";
pub const OVERLAY_UNIX_LISTENERS: &str = "unix listeners";
pub const TRANSIENT_IMPACT: &str = "transient impact";
pub const CONFIGURED_CONTEXT: &str = "configured context";
pub const RUNTIME_IMPACT: &str = "runtime impact";
pub const RESTART_IMPACT: &str = "restart impact";
pub const PERSISTENT_IMPACT: &str = "persistent impact";
pub const UNKNOWN_IMPACT: &str = "unknown impact";
pub const SAFETY: &str = "safety";
pub const EVIDENCE: &str = "evidence";
pub const OWNED_BY: &str = "owned by";
pub const DIRECT_DEPENDENTS_RUNTIME: &str = "direct dependents (runtime)";
pub const IMPACT_PATHS: &str = "impact paths";
pub const CONFIGURED_DEPENDENTS: &str = "configured dependents (inactive)";
pub const EMULATION_SCORING_NOTE_DIRECT: &str =
    "emulation uses direct-only scoring; twin impact applies broader thresholds";
pub const EMULATION_SCORING_NOTE_WITH_PATHS: &str =
    "emulation scoring includes transitive path evidence; twin impact applies broader risk thresholds";

pub fn emulation_scoring_note(paths_requested: bool) -> &'static str {
    if paths_requested {
        EMULATION_SCORING_NOTE_WITH_PATHS
    } else {
        EMULATION_SCORING_NOTE_DIRECT
    }
}
