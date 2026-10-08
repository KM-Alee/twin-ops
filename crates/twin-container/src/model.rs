use std::str::FromStr;

use serde_json::Value;
use twin_core::NodeId;

use crate::error::ContainerError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    pub container_port: u16,
    pub host_ip: String,
    pub host_port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMount {
    pub kind: String,
    pub name: String,
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    pub image: String,
    pub ports: Vec<PortMapping>,
    pub mounts: Vec<VolumeMount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInspect {
    pub id: String,
    pub name: String,
    pub image: String,
    pub pid: Option<u32>,
    ports: Vec<PortMapping>,
    mounts: Vec<VolumeMount>,
}

impl ContainerInspect {
    pub fn port_mappings(&self) -> &[PortMapping] {
        &self.ports
    }

    pub fn mounts(&self) -> &[VolumeMount] {
        &self.mounts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInspect {
    pub id: String,
    pub reference: String,
}

pub(crate) fn parse_container_list(
    path: &str,
    body: &str,
) -> Result<Vec<ContainerSummary>, ContainerError> {
    let value = parse_json(path, body)?;
    let items = value.as_array().ok_or_else(|| ContainerError::Parse {
        path: path.to_string(),
        detail: "container list is not an array".to_string(),
    })?;
    let mut out = Vec::new();
    for item in items {
        let Some(summary) = summary_from_value(item) else {
            continue;
        };
        out.push(summary);
    }
    Ok(out)
}

pub(crate) fn parse_container_inspect(
    path: &str,
    body: &str,
) -> Result<ContainerInspect, ContainerError> {
    let value = parse_json(path, body)?;
    inspect_from_value(&value).ok_or_else(|| ContainerError::Parse {
        path: path.to_string(),
        detail: "container inspect is missing an id".to_string(),
    })
}

pub(crate) fn parse_image_inspect(
    path: &str,
    body: &str,
    requested: &str,
) -> Result<ImageInspect, ContainerError> {
    let value = parse_json(path, body)?;
    let id = value
        .get("Id")
        .and_then(|item| item.as_str())
        .unwrap_or(requested)
        .to_string();
    let reference = value
        .get("RepoTags")
        .and_then(|item| item.as_array())
        .and_then(|tags| tags.iter().find_map(|tag| tag.as_str()))
        .filter(|tag| usable_image(tag))
        .unwrap_or(requested)
        .to_string();
    if !usable_image(&reference) {
        return Err(ContainerError::Parse {
            path: path.to_string(),
            detail: format!("image reference `{reference}` is not a graph id"),
        });
    }
    Ok(ImageInspect { id, reference })
}

fn summary_from_value(value: &Value) -> Option<ContainerSummary> {
    let id = value.get("Id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    let name = choose_name(value, &id)?;
    let image = image_reference(value)?;
    Some(ContainerSummary {
        id,
        name,
        image,
        ports: ports_from_list(value),
        mounts: mounts_from_value(value),
    })
}

fn inspect_from_value(value: &Value) -> Option<ContainerInspect> {
    let id = value.get("Id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    let name = choose_name(value, &id)?;
    let image = image_reference(value)?;
    let pid = value
        .get("State")
        .and_then(|state| state.get("Pid"))
        .and_then(|pid| pid.as_u64())
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0);
    let ports = ports_from_inspect(value);
    let ports = if ports.is_empty() {
        ports_from_list(value)
    } else {
        ports
    };
    Some(ContainerInspect {
        id,
        name,
        image,
        pid,
        ports,
        mounts: mounts_from_value(value),
    })
}

fn choose_name(value: &Value, id: &str) -> Option<String> {
    if let Some(name) = value.get("Name").and_then(|item| item.as_str()) {
        let trimmed = name.trim_start_matches('/');
        if usable_container(trimmed) {
            return Some(trimmed.to_string());
        }
    }
    if let Some(names) = value.get("Names").and_then(|item| item.as_array()) {
        for name in names {
            let Some(text) = name.as_str() else {
                continue;
            };
            let trimmed = text.trim_start_matches('/');
            if usable_container(trimmed) {
                return Some(trimmed.to_string());
            }
        }
    }
    let short = id.chars().take(12).collect::<String>();
    usable_container(&short).then_some(short)
}

fn image_reference(value: &Value) -> Option<String> {
    let candidates = [
        value
            .pointer("/Config/Image")
            .and_then(|item| item.as_str()),
        value.get("Image").and_then(|item| item.as_str()),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.starts_with("sha256:") {
            continue;
        }
        if usable_image(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn ports_from_list(value: &Value) -> Vec<PortMapping> {
    let Some(ports) = value.get("Ports").and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for port in ports {
        let Some(protocol) = port.get("Type").and_then(|item| item.as_str()) else {
            continue;
        };
        if protocol != "tcp" {
            continue;
        }
        let Some(container_port) = port.get("PrivatePort").and_then(json_u16) else {
            continue;
        };
        let Some(host_port) = port.get("PublicPort").and_then(json_u16) else {
            continue;
        };
        if host_port == 0 {
            continue;
        }
        let host_ip = port
            .get("IP")
            .and_then(|item| item.as_str())
            .filter(|ip| !ip.is_empty())
            .unwrap_or("0.0.0.0");
        out.push(PortMapping {
            container_port,
            host_ip: host_ip.to_string(),
            host_port,
            protocol: protocol.to_string(),
        });
    }
    out
}

fn ports_from_inspect(value: &Value) -> Vec<PortMapping> {
    let Some(ports) = value.pointer("/NetworkSettings/Ports") else {
        return ports_from_bindings(value);
    };
    let Some(map) = ports.as_object() else {
        return ports_from_bindings(value);
    };
    let mut out = Vec::new();
    for (key, binding) in map {
        let Some((container_port, protocol)) = split_port_key(key) else {
            continue;
        };
        if protocol != "tcp" {
            continue;
        }
        let Some(bindings) = binding.as_array() else {
            continue;
        };
        for item in bindings {
            let Some(host_port) = item.get("HostPort").and_then(json_u16) else {
                continue;
            };
            if host_port == 0 {
                continue;
            }
            let host_ip = item
                .get("HostIp")
                .and_then(|ip| ip.as_str())
                .filter(|ip| !ip.is_empty())
                .unwrap_or("0.0.0.0");
            out.push(PortMapping {
                container_port,
                host_ip: host_ip.to_string(),
                host_port,
                protocol: protocol.to_string(),
            });
        }
    }
    if out.is_empty() {
        ports_from_bindings(value)
    } else {
        out
    }
}

fn ports_from_bindings(value: &Value) -> Vec<PortMapping> {
    let Some(map) = value
        .pointer("/HostConfig/PortBindings")
        .and_then(|item| item.as_object())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, binding) in map {
        let Some((container_port, protocol)) = split_port_key(key) else {
            continue;
        };
        if protocol != "tcp" {
            continue;
        }
        let Some(bindings) = binding.as_array() else {
            continue;
        };
        for item in bindings {
            let Some(host_port) = item.get("HostPort").and_then(json_u16) else {
                continue;
            };
            if host_port == 0 {
                continue;
            }
            let host_ip = item
                .get("HostIp")
                .and_then(|ip| ip.as_str())
                .filter(|ip| !ip.is_empty())
                .unwrap_or("0.0.0.0");
            out.push(PortMapping {
                container_port,
                host_ip: host_ip.to_string(),
                host_port,
                protocol: protocol.to_string(),
            });
        }
    }
    out
}

fn mounts_from_value(value: &Value) -> Vec<VolumeMount> {
    let Some(mounts) = value.get("Mounts").and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for mount in mounts {
        let Some(destination) = mount.get("Destination").and_then(|item| item.as_str()) else {
            continue;
        };
        if destination.is_empty() {
            continue;
        }
        out.push(VolumeMount {
            kind: mount
                .get("Type")
                .and_then(|item| item.as_str())
                .unwrap_or("volume")
                .to_string(),
            name: mount
                .get("Name")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .to_string(),
            source: mount
                .get("Source")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .to_string(),
            destination: destination.to_string(),
        });
    }
    out
}

fn split_port_key(key: &str) -> Option<(u16, &str)> {
    let (port, protocol) = key.split_once('/')?;
    let port = port.parse().ok()?;
    Some((port, protocol))
}

fn json_u16(value: &Value) -> Option<u16> {
    match value {
        Value::Number(number) => u16::try_from(number.as_u64()?).ok(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn parse_json(path: &str, body: &str) -> Result<Value, ContainerError> {
    serde_json::from_str(body).map_err(|err| ContainerError::Parse {
        path: path.to_string(),
        detail: err.to_string(),
    })
}

fn usable_container(name: &str) -> bool {
    NodeId::from_str(NodeId::container(name).as_str()).is_ok()
}

fn usable_image(reference: &str) -> bool {
    NodeId::from_str(NodeId::image(reference).as_str()).is_ok()
}
