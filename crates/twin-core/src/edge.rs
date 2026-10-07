use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::node::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    ParentOf,
    Owns,
    InCgroup,
    ListensOn,
    ConnectsTo,
    ConfiguredBy,
    DependsOn,
    ProxiesTo,
    References,
    MountedOn,
    LogsTo,
    Uses,
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ParentOf => "parent_of",
            Self::Owns => "owns",
            Self::InCgroup => "in_cgroup",
            Self::ListensOn => "listens_on",
            Self::ConnectsTo => "connects_to",
            Self::ConfiguredBy => "configured_by",
            Self::DependsOn => "depends_on",
            Self::ProxiesTo => "proxies_to",
            Self::References => "references",
            Self::MountedOn => "mounted_on",
            Self::LogsTo => "logs_to",
            Self::Uses => "uses",
        })
    }
}

impl FromStr for EdgeKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "parent_of" => Ok(Self::ParentOf),
            "owns" => Ok(Self::Owns),
            "in_cgroup" => Ok(Self::InCgroup),
            "listens_on" => Ok(Self::ListensOn),
            "connects_to" => Ok(Self::ConnectsTo),
            "configured_by" => Ok(Self::ConfiguredBy),
            "depends_on" => Ok(Self::DependsOn),
            "proxies_to" => Ok(Self::ProxiesTo),
            "references" => Ok(Self::References),
            "mounted_on" => Ok(Self::MountedOn),
            "logs_to" => Ok(Self::LogsTo),
            "uses" => Ok(Self::Uses),
            other => Err(ParseError::Enum {
                kind: "EdgeKind",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeClass {
    Observed,
    Inferred,
    Predicted,
}

impl fmt::Display for EdgeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
            Self::Predicted => "predicted",
        })
    }
}

impl FromStr for EdgeClass {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "observed" => Ok(Self::Observed),
            "inferred" => Ok(Self::Inferred),
            "predicted" => Ok(Self::Predicted),
            other => Err(ParseError::Enum {
                kind: "EdgeClass",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeState {
    Active,
    Stale,
    Gone,
}

impl fmt::Display for EdgeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Gone => "gone",
        })
    }
}

impl FromStr for EdgeState {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "stale" => Ok(Self::Stale),
            "gone" => Ok(Self::Gone),
            other => Err(ParseError::Enum {
                kind: "EdgeState",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeId(String);

impl EdgeId {
    pub fn new(from: &NodeId, kind: EdgeKind, to: &NodeId) -> Self {
        Self(format!("{}|{}|{}", from.as_str(), kind, to.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for EdgeId {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_edge_id(s)?;
        Ok(Self(s.to_string()))
    }
}

fn validate_edge_id(s: &str) -> Result<(), ParseError> {
    let invalid = || ParseError::InvalidEdgeId {
        value: s.to_string(),
    };
    if s.is_empty() {
        return Err(invalid());
    }
    let mut parts = s.split('|');
    let from = parts.next().ok_or_else(invalid)?;
    let kind = parts.next().ok_or_else(invalid)?;
    let to = parts.next().ok_or_else(invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    let from_id = NodeId::from_str(from)?;
    let edge_kind = EdgeKind::from_str(kind)?;
    let to_id = NodeId::from_str(to)?;
    let canonical = EdgeId::new(&from_id, edge_kind, &to_id);
    if canonical.as_str() != s {
        return Err(invalid());
    }
    Ok(())
}
