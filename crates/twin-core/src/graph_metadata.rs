use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphMetadata(String);

impl GraphMetadata {
    pub fn empty() -> Self {
        Self("{}".to_string())
    }

    pub fn from_json(json: impl Into<String>) -> Self {
        Self(json.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GraphMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
