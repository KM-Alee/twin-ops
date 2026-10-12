use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NetworkNamespaceId(u64);

impl NetworkNamespaceId {
    pub fn new(inode: u64) -> Result<Self, ParseError> {
        if inode == 0 {
            return Err(ParseError::Enum {
                kind: "NetworkNamespaceId",
                value: inode.to_string(),
            });
        }
        Ok(Self(inode))
    }
}

impl fmt::Display for NetworkNamespaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NetworkNamespaceId {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let inode = value.parse().map_err(|_| ParseError::Enum {
            kind: "NetworkNamespaceId",
            value: value.into(),
        })?;
        let id = Self::new(inode)?;
        if id.to_string() != value {
            return Err(ParseError::Enum {
                kind: "NetworkNamespaceId",
                value: value.into(),
            });
        }
        Ok(id)
    }
}
