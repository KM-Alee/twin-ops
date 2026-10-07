use std::fs;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use twin_core::{EvidenceLabel, NodeId, NodeKind};

use crate::error::TestError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestDocument {
    pub name: String,
    pub checks: Vec<TestCheck>,
    pub default_min_evidence: Option<EvidenceLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestCheck {
    pub name: String,
    pub kind: CheckKind,
    pub exists: bool,
    pub min_evidence: Option<EvidenceLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Node(NodeId),
    Port(NodeId),
    Service(NodeId),
    Dependency { from: NodeId, to: NodeId },
}

pub fn parse_file(path: &Path) -> Result<TestDocument, TestError> {
    let text = fs::read_to_string(path).map_err(|source| TestError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_str(&text, path)
}

pub fn parse_str(text: &str, path: &Path) -> Result<TestDocument, TestError> {
    if mentions_shell(text) {
        return Err(TestError::ShellNotSupported);
    }
    let raw: RawDocument = serde_yaml::from_str(text).map_err(|err| TestError::Invalid {
        path: path.to_path_buf(),
        reason: err.to_string(),
    })?;
    compile(raw, path)
}

fn compile(raw: RawDocument, path: &Path) -> Result<TestDocument, TestError> {
    if raw.version != 1 {
        return Err(invalid(
            path,
            format!(
                "unsupported version {}; only version 1 is supported",
                raw.version
            ),
        ));
    }
    let name = raw.name.trim();
    if name.is_empty() {
        return Err(invalid(path, "name must not be empty"));
    }
    if raw.checks.is_empty() {
        return Err(invalid(path, "checks must not be empty"));
    }
    let default_min_evidence = match raw.defaults {
        Some(defaults) => Some(parse_label(path, &defaults.min_evidence)?),
        None => None,
    };
    let mut checks = Vec::new();
    let mut seen = Vec::new();
    for check in raw.checks {
        let check_name = check.name.trim();
        if check_name.is_empty() {
            return Err(invalid(path, "check name must not be empty"));
        }
        if seen.iter().any(|existing: &String| existing == check_name) {
            return Err(invalid(
                path,
                format!("duplicate check name `{check_name}`"),
            ));
        }
        seen.push(check_name.to_string());
        checks.push(compile_check(path, check_name, check.assert)?);
    }
    Ok(TestDocument {
        name: name.to_string(),
        checks,
        default_min_evidence,
    })
}

fn compile_check(path: &Path, name: &str, assert: RawAssert) -> Result<TestCheck, TestError> {
    let targets = [
        assert.node.is_some(),
        assert.port.is_some(),
        assert.service.is_some(),
        assert.dependency.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if targets != 1 {
        return Err(invalid(
            path,
            format!("check `{name}` must set exactly one of node, port, service, or dependency"),
        ));
    }
    let exists = assert.exists.unwrap_or(true);
    if assert.dependency.is_none() && assert.min_evidence.is_some() {
        return Err(invalid(
            path,
            format!("check `{name}` sets min_evidence without a dependency"),
        ));
    }
    let min_evidence = match assert.min_evidence {
        Some(label) => Some(parse_label(path, &label)?),
        None => None,
    };
    let kind = if let Some(id) = assert.node {
        CheckKind::Node(parse_id(path, name, &id)?)
    } else if let Some(id) = assert.port {
        let id = parse_id(path, name, &id)?;
        require_kind(path, name, &id, NodeKind::Port, "port")?;
        CheckKind::Port(id)
    } else if let Some(id) = assert.service {
        let id = parse_id(path, name, &id)?;
        require_kind(path, name, &id, NodeKind::Service, "service")?;
        CheckKind::Service(id)
    } else if let Some(dependency) = assert.dependency {
        CheckKind::Dependency {
            from: parse_id(path, name, &dependency.from)?,
            to: parse_id(path, name, &dependency.to)?,
        }
    } else {
        return Err(invalid(path, format!("check `{name}` has no assertion")));
    };
    Ok(TestCheck {
        name: name.to_string(),
        kind,
        exists,
        min_evidence,
    })
}

fn parse_id(path: &Path, check: &str, value: &str) -> Result<NodeId, TestError> {
    NodeId::from_str(value).map_err(|err| {
        invalid(
            path,
            format!("check `{check}` has invalid id `{value}`: {err}"),
        )
    })
}

fn require_kind(
    path: &Path,
    check: &str,
    id: &NodeId,
    expected: NodeKind,
    field: &str,
) -> Result<(), TestError> {
    if id.kind() == Some(expected) {
        Ok(())
    } else {
        Err(invalid(
            path,
            format!("check `{check}` {field} `{id}` is not a {expected} id"),
        ))
    }
}

fn parse_label(path: &Path, value: &str) -> Result<EvidenceLabel, TestError> {
    EvidenceLabel::from_str(value).map_err(|_| {
        invalid(
            path,
            format!("unknown evidence label `{value}`; use weak, moderate, strong, or very_strong"),
        )
    })
}

fn invalid(path: &Path, reason: impl Into<String>) -> TestError {
    TestError::Invalid {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

fn mentions_shell(text: &str) -> bool {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let key = trimmed.split(':').next().unwrap_or("").trim();
        if matches!(key, "shell" | "command" | "exec" | "script") {
            return true;
        }
    }
    false
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    version: u32,
    name: String,
    #[serde(default)]
    defaults: Option<RawDefaults>,
    checks: Vec<RawCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefaults {
    min_evidence: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    name: String,
    assert: RawAssert,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAssert {
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    dependency: Option<RawDependency>,
    #[serde(default)]
    exists: Option<bool>,
    #[serde(default)]
    min_evidence: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependency {
    from: String,
    to: String,
}
