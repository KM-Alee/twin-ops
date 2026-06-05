use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanQuality {
    Good,
    Partial,
    Degraded,
}

impl ScanQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Partial => "partial",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanQualityAssessment {
    pub quality: ScanQuality,
    pub reasons: Vec<String>,
    pub impact_reliable: bool,
    pub ephemeral_capture_recommended: bool,
}
