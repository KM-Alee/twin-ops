use std::fs;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use twin_core::{EvidenceLabel, NodeId, NodeKind, RiskLevel};

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
    Dependency {
        from: NodeId,
        to: NodeId,
    },
    Emulate {
        action: EmulateAction,
        target: String,
        max_risk: RiskLevel,
        require_evidence: bool,
    },
    Outbound {
        allowed: Vec<String>,
        fail_on_unknown: bool,
    },
    Disk {
        mount: String,
        max_used_percent: u8,
    },
    Unknowns {
        max_count: Option<u32>,
        forbid: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulateAction {
    Restart,
    Delete,
}

impl EmulateAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Delete => "delete",
        }
    }
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
        let check_name = check.name.trim().to_string();
        if check_name.is_empty() {
            return Err(invalid(path, "check name must not be empty"));
        }
        if seen.iter().any(|existing: &String| existing == &check_name) {
            return Err(invalid(
                path,
                format!("duplicate check name `{check_name}`"),
            ));
        }
        seen.push(check_name.clone());
        checks.push(compile_check(path, &check_name, check)?);
    }
    Ok(TestDocument {
        name: name.to_string(),
        checks,
        default_min_evidence,
    })
}

fn compile_check(path: &Path, name: &str, check: RawCheck) -> Result<TestCheck, TestError> {
    if check.expect.is_some() && check.emulate.is_none() {
        return Err(invalid(
            path,
            format!("check `{name}` sets expect without emulate"),
        ));
    }
    match (check.assert, check.emulate) {
        (Some(assert), None) => compile_assert(path, name, assert),
        (None, Some(emulate)) => compile_emulate(path, name, emulate, check.expect),
        (Some(_), Some(_)) => Err(invalid(
            path,
            format!("check `{name}` must set assert or emulate, not both"),
        )),
        (None, None) => Err(invalid(
            path,
            format!("check `{name}` must set assert or emulate"),
        )),
    }
}

fn compile_emulate(
    path: &Path,
    name: &str,
    emulate: RawEmulate,
    expect: Option<RawExpect>,
) -> Result<TestCheck, TestError> {
    let Some(expect) = expect else {
        return Err(invalid(
            path,
            format!("check `{name}` emulate check needs expect"),
        ));
    };
    let action = match emulate.action.as_str() {
        "restart" => EmulateAction::Restart,
        "delete" => EmulateAction::Delete,
        other => {
            return Err(invalid(
                path,
                format!("check `{name}` has unknown emulate action `{other}`"),
            ))
        }
    };
    let target = emulate.target.trim();
    if target.is_empty() {
        return Err(invalid(
            path,
            format!("check `{name}` emulate target is empty"),
        ));
    }
    if action == EmulateAction::Restart {
        let id = parse_id(path, name, target)?;
        require_kind(path, name, &id, NodeKind::Service, "emulate target")?;
    }
    let max_risk = RiskLevel::from_str(&expect.max_risk).map_err(|_| {
        invalid(
            path,
            format!("check `{name}` has unknown max_risk `{}`", expect.max_risk),
        )
    })?;
    Ok(TestCheck {
        name: name.to_string(),
        kind: CheckKind::Emulate {
            action,
            target: target.to_string(),
            max_risk,
            require_evidence: expect.require_evidence,
        },
        exists: true,
        min_evidence: None,
    })
}

fn compile_assert(path: &Path, name: &str, assert: RawAssert) -> Result<TestCheck, TestError> {
    let targets = [
        assert.node.is_some(),
        assert.port.is_some(),
        assert.service.is_some(),
        assert.dependency.is_some(),
        assert.outbound_endpoints.is_some(),
        assert.disk.is_some(),
        assert.unknowns.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if targets != 1 {
        return Err(invalid(
            path,
            format!(
                "check `{name}` must set exactly one of node, port, service, dependency, outbound_endpoints, disk, or unknowns"
            ),
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
    } else if let Some(outbound) = assert.outbound_endpoints {
        if outbound.allowed.is_empty() && !outbound.fail_on_unknown {
            return Err(invalid(
                path,
                format!("check `{name}` outbound allowlist is empty"),
            ));
        }
        CheckKind::Outbound {
            allowed: outbound.allowed,
            fail_on_unknown: outbound.fail_on_unknown,
        }
    } else if let Some(disk) = assert.disk {
        let mount = disk.mount.trim();
        if mount.is_empty() {
            return Err(invalid(path, format!("check `{name}` disk mount is empty")));
        }
        if disk.max_used_percent > 100 {
            return Err(invalid(
                path,
                format!("check `{name}` max_used_percent cannot exceed 100"),
            ));
        }
        CheckKind::Disk {
            mount: mount.to_string(),
            max_used_percent: disk.max_used_percent,
        }
    } else if let Some(unknowns) = assert.unknowns {
        if unknowns.max_count.is_none() && unknowns.forbid.is_empty() {
            return Err(invalid(
                path,
                format!("check `{name}` unknowns policy needs max_count or forbid"),
            ));
        }
        CheckKind::Unknowns {
            max_count: unknowns.max_count,
            forbid: unknowns.forbid,
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
    #[serde(default)]
    assert: Option<RawAssert>,
    #[serde(default)]
    emulate: Option<RawEmulate>,
    #[serde(default)]
    expect: Option<RawExpect>,
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
    #[serde(default)]
    outbound_endpoints: Option<RawOutbound>,
    #[serde(default)]
    disk: Option<RawDisk>,
    #[serde(default)]
    unknowns: Option<RawUnknowns>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEmulate {
    action: String,
    target: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpect {
    max_risk: String,
    #[serde(default)]
    require_evidence: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOutbound {
    allowed: Vec<String>,
    #[serde(default)]
    fail_on_unknown: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisk {
    mount: String,
    max_used_percent: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUnknowns {
    #[serde(default)]
    max_count: Option<u32>,
    #[serde(default)]
    forbid: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependency {
    from: String,
    to: String,
}
