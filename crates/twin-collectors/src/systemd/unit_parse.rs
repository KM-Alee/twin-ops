use std::path::Path;

use crate::error::CollectorError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitDependencies {
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub binds_to: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocketUnitConfig {
    pub listen_streams: Vec<String>,
    pub listen_datagrams: Vec<String>,
    pub listen_sequential_packets: Vec<String>,
    pub service: Option<String>,
}

pub fn parse_unit_file(path: &Path, content: &str) -> Result<UnitDependencies, CollectorError> {
    let section = unit_section(content).ok_or_else(|| CollectorError::SystemdParse {
        path: path.to_path_buf(),
        detail: "missing [Unit] section".to_string(),
    })?;
    let mut deps = UnitDependencies::default();
    for (key, value) in section {
        let targets = split_unit_list(&value);
        match key.as_str() {
            "Requires" => deps.requires.extend(targets),
            "Wants" => deps.wants.extend(targets),
            "BindsTo" => deps.binds_to.extend(targets),
            _ => {}
        }
    }
    Ok(deps)
}

pub fn merge_dependencies(base: &mut UnitDependencies, overlay: UnitDependencies) {
    base.requires.extend(overlay.requires);
    base.wants.extend(overlay.wants);
    base.binds_to.extend(overlay.binds_to);
}

pub fn normalize_unit_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('*') || trimmed.contains('?') {
        return None;
    }
    if trimmed.contains('@') {
        let (_, after_at) = trimmed.split_once('@')?;
        let instance = after_at.split('.').next().unwrap_or("");
        if instance.is_empty() {
            return None;
        }
    }
    if trimmed.ends_with(".service") || trimmed.ends_with(".target") || trimmed.ends_with(".socket")
    {
        return Some(trimmed.to_string());
    }
    if trimmed.contains('.') {
        return None;
    }
    Some(format!("{trimmed}.service"))
}

pub fn parse_socket_unit(path: &Path, content: &str) -> Result<SocketUnitConfig, CollectorError> {
    let section = socket_section(content).ok_or_else(|| CollectorError::SystemdParse {
        path: path.to_path_buf(),
        detail: "missing [Socket] section".to_string(),
    })?;
    let mut config = SocketUnitConfig::default();
    for (key, value) in section {
        let values = split_socket_list(&value);
        match key.as_str() {
            "ListenStream" => config.listen_streams.extend(values),
            "ListenDatagram" => config.listen_datagrams.extend(values),
            "ListenSequentialPacket" => config.listen_sequential_packets.extend(values),
            "Service" => {
                if let Some(service) = values.into_iter().next() {
                    config.service = Some(service);
                }
            }
            _ => {}
        }
    }
    Ok(config)
}

pub fn socket_activation_target(socket_unit: &str, config: &SocketUnitConfig) -> String {
    if let Some(service) = &config.service {
        return service.clone();
    }
    if let Some(base) = socket_unit.strip_suffix(".socket") {
        return format!("{base}.service");
    }
    format!("{socket_unit}.service")
}

pub fn template_unit_name_for_instance(instance_unit: &str) -> Option<String> {
    let (prefix, rest) = instance_unit.split_once('@')?;
    let suffix = rest.split_once('.').map(|(_, s)| s)?;
    Some(format!("{prefix}@.{suffix}"))
}

fn socket_section(content: &str) -> Option<Vec<(String, String)>> {
    let mut in_socket = false;
    let mut pairs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_socket = line.eq_ignore_ascii_case("[Socket]");
            continue;
        }
        if !in_socket {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    if pairs.is_empty() && !content.contains("[Socket]") {
        return None;
    }
    Some(pairs)
}

fn split_socket_list(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn unit_section(content: &str) -> Option<Vec<(String, String)>> {
    let mut in_unit = false;
    let mut pairs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_unit = line.eq_ignore_ascii_case("[Unit]");
            continue;
        }
        if !in_unit {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    if pairs.is_empty() && !content.contains("[Unit]") {
        return None;
    }
    Some(pairs)
}

fn split_unit_list(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(normalize_unit_name)
        .collect()
}

pub fn effective_unit_name(unit_path: &Path) -> Option<String> {
    let file_name = unit_path.file_name()?.to_str()?;
    if file_name.contains('*') || file_name.contains('?') {
        return None;
    }
    if file_name.contains('@') {
        let (_, after_at) = file_name.split_once('@')?;
        let instance = after_at.split('.').next().unwrap_or("");
        if instance.is_empty() {
            return None;
        }
    }
    normalize_unit_name(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requires_and_wants() {
        let content = r#"
[Unit]
Description=Docker
Requires=containerd.service
Wants=network-online.target
"#;
        let deps = parse_unit_file(Path::new("docker.service"), content).expect("parse");
        assert_eq!(deps.requires, vec!["containerd.service"]);
        assert_eq!(deps.wants, vec!["network-online.target"]);
    }

    #[test]
    fn parses_socket_listen_and_service() {
        let content = r#"
[Socket]
ListenStream=/run/docker.sock
Service=docker.service
"#;
        let cfg = parse_socket_unit(Path::new("docker.socket"), content).expect("parse");
        assert_eq!(cfg.listen_streams, vec!["/run/docker.sock"]);
        assert_eq!(cfg.service.as_deref(), Some("docker.service"));
        assert_eq!(
            socket_activation_target("docker.socket", &cfg),
            "docker.service"
        );
    }

    #[test]
    fn allows_instance_unit_names() {
        assert_eq!(
            normalize_unit_name("getty@tty1.service").as_deref(),
            Some("getty@tty1.service")
        );
        assert!(normalize_unit_name("getty@.service").is_none());
    }

    #[test]
    fn merge_appends_dependencies() {
        let mut base = UnitDependencies {
            requires: vec!["a.service".to_string()],
            ..Default::default()
        };
        merge_dependencies(
            &mut base,
            UnitDependencies {
                requires: vec!["b.service".to_string()],
                wants: vec!["c.service".to_string()],
                ..Default::default()
            },
        );
        assert_eq!(base.requires, vec!["a.service", "b.service"]);
        assert_eq!(base.wants, vec!["c.service"]);
    }
}
