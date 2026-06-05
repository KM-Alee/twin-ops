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

    pub fn bool_field(&self, key: &str) -> bool {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.0) else {
            return false;
        };
        value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    pub fn socket_activation(&self) -> bool {
        self.bool_field("socket_activation")
    }
}

impl fmt::Display for GraphMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
