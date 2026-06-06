use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependentImpactKind {
    Runtime,
    Configured,
}

impl DependentImpactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Configured => "configured",
        }
    }

    pub fn is_runtime(self) -> bool {
        matches!(self, Self::Runtime)
    }
}

impl fmt::Display for DependentImpactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DependentImpactKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "runtime" => Ok(Self::Runtime),
            "configured" => Ok(Self::Configured),
            other => Err(ParseError::Enum {
                kind: "DependentImpactKind",
                value: other.to_string(),
            }),
        }
    }
}
