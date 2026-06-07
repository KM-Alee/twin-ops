use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    New,
    Changed,
    Stale,
    Gone,
    Reappeared,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Changed => "changed",
            Self::Stale => "stale",
            Self::Gone => "gone",
            Self::Reappeared => "reappeared",
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ChangeKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "new" => Ok(Self::New),
            "changed" => Ok(Self::Changed),
            "stale" => Ok(Self::Stale),
            "gone" => Ok(Self::Gone),
            "reappeared" => Ok(Self::Reappeared),
            _ => Err(ParseError::Enum {
                kind: "ChangeKind",
                value: s.to_string(),
            }),
        }
    }
}
