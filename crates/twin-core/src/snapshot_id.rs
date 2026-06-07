use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SnapshotId(String);

impl SnapshotId {
    pub fn new(name: impl Into<String>) -> Result<Self, ParseError> {
        let name = name.into();
        validate_snapshot_name(&name)?;
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SnapshotId {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

pub fn validate_snapshot_name(name: &str) -> Result<(), ParseError> {
    if name.is_empty() {
        return Err(ParseError::SnapshotName {
            value: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }
    if name == "current" {
        return Err(ParseError::SnapshotName {
            value: name.to_string(),
            reason: "name is reserved".to_string(),
        });
    }
    if name.contains(':') || name.contains('/') || name.contains(char::is_whitespace) {
        return Err(ParseError::SnapshotName {
            value: name.to_string(),
            reason: "name contains invalid characters".to_string(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ParseError::SnapshotName {
            value: name.to_string(),
            reason: "name must use only letters, digits, '.', '_', or '-'".to_string(),
        });
    }
    Ok(())
}

pub fn parse_graph_ref(value: &str) -> Result<GraphRef, ParseError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ParseError::GraphRef {
            value: value.to_string(),
            reason: "ref cannot be empty".to_string(),
        });
    }
    if trimmed == "current" {
        return Ok(GraphRef::Current);
    }
    let Some(name) = trimmed.strip_prefix("snapshot:") else {
        return Err(ParseError::GraphRef {
            value: value.to_string(),
            reason: "expected `current` or `snapshot:NAME`".to_string(),
        });
    };
    if name.is_empty() {
        return Err(ParseError::GraphRef {
            value: value.to_string(),
            reason: "snapshot name missing after `snapshot:`".to_string(),
        });
    }
    Ok(GraphRef::Snapshot(SnapshotId::new(name)?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRef {
    Current,
    Snapshot(SnapshotId),
}

impl fmt::Display for GraphRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Current => write!(f, "current"),
            Self::Snapshot(id) => write!(f, "snapshot:{id}"),
        }
    }
}
