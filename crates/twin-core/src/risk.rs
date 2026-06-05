use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
    Unknown,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        })
    }
}

impl FromStr for RiskLevel {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            "unknown" => Ok(Self::Unknown),
            other => Err(ParseError::Enum {
                kind: "RiskLevel",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLabel {
    Weak,
    Moderate,
    Strong,
    VeryStrong,
}

impl fmt::Display for EvidenceLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Weak => "weak",
            Self::Moderate => "moderate",
            Self::Strong => "strong",
            Self::VeryStrong => "very_strong",
        })
    }
}

impl FromStr for EvidenceLabel {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "weak" => Ok(Self::Weak),
            "moderate" => Ok(Self::Moderate),
            "strong" => Ok(Self::Strong),
            "very_strong" => Ok(Self::VeryStrong),
            other => Err(ParseError::Enum {
                kind: "EvidenceLabel",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceStrength {
    score: u8,
    label: EvidenceLabel,
}

impl EvidenceStrength {
    pub fn new(score: u8) -> Self {
        let score = score.min(100);
        Self {
            score,
            label: label_for_score(score),
        }
    }

    pub fn weak() -> Self {
        Self::new(15)
    }

    pub fn moderate() -> Self {
        Self::new(45)
    }

    pub fn strong() -> Self {
        Self::new(75)
    }

    pub fn very_strong() -> Self {
        Self::new(95)
    }

    pub fn score(&self) -> u8 {
        self.score
    }

    pub fn label(&self) -> EvidenceLabel {
        self.label
    }
}

fn label_for_score(score: u8) -> EvidenceLabel {
    match score {
        0..=30 => EvidenceLabel::Weak,
        31..=60 => EvidenceLabel::Moderate,
        61..=85 => EvidenceLabel::Strong,
        86..=100 => EvidenceLabel::VeryStrong,
        _ => EvidenceLabel::Weak,
    }
}
