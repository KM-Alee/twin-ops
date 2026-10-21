use serde_yaml::Value;
use twin_core::TransportProtocol;

use crate::endpoint::{endpoints_in_value, DeclaredEndpoint};
use crate::env_file::is_env_name;

const MAX_SERVICES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposePort {
    pub host_ip: Option<String>,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: TransportProtocol,
}

/// What one Compose service declares about its dependencies. Environment values are read
/// only for endpoints; `env_file` names are returned for the caller to read the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeService {
    pub name: String,
    pub depends_on: Vec<String>,
    pub ports: Vec<ComposePort>,
    pub endpoints: Vec<DeclaredEndpoint>,
    pub env_files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeParse {
    pub services: Vec<ComposeService>,
    pub warnings: Vec<String>,
}

/// Parses a Compose file. YAML errors report only their location, never the text.
pub fn parse_compose(text: &str) -> Result<ComposeParse, String> {
    let document: Value = serde_yaml::from_str(text).map_err(|error| match error.location() {
        Some(location) => format!(
            "compose file is not valid YAML (line {}, column {})",
            location.line(),
            location.column()
        ),
        None => "compose file is not valid YAML".to_string(),
    })?;
    let mut parsed = ComposeParse::default();
    let Some(services) = document.get("services").and_then(Value::as_mapping) else {
        return Ok(parsed);
    };
    for (name, service) in services.iter().take(MAX_SERVICES) {
        let Some(name) = name.as_str().filter(|name| is_service_name(name)) else {
            parsed
                .warnings
                .push("a service name is not a valid Compose name".into());
            continue;
        };
        let mut declared = ComposeService {
            name: name.to_string(),
            ..ComposeService::default()
        };
        declared.depends_on = depends_on(service);
        for link in strings(service.get("links")) {
            let target = link.split(':').next().unwrap_or_default();
            if is_service_name(target) && !declared.depends_on.iter().any(|known| known == target) {
                declared.depends_on.push(target.to_string());
            }
        }
        for port in sequence(service.get("ports")) {
            match compose_port(port) {
                Some(Some(port)) => declared.ports.push(port),
                Some(None) => {}
                None => parsed.warnings.push(format!(
                    "service {name}: a port uses a range, a variable or an unsupported form"
                )),
            }
        }
        for (key, value) in environment(service.get("environment")) {
            for endpoint in endpoints_in_value(&key, &value) {
                if !declared.endpoints.contains(&endpoint) {
                    declared.endpoints.push(endpoint);
                }
            }
        }
        declared.env_files = env_files(service.get("env_file"));
        parsed.services.push(declared);
    }
    Ok(parsed)
}

fn is_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn sequence(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value.and_then(Value::as_sequence).into_iter().flatten()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    sequence(value)
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn depends_on(service: &Value) -> Vec<String> {
    let value = service.get("depends_on");
    let names: Vec<String> = match value.and_then(Value::as_mapping) {
        Some(mapping) => mapping
            .keys()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        None => strings(value),
    };
    names
        .into_iter()
        .filter(|name| is_service_name(name))
        .collect()
}

/// `Some(None)` for a port that publishes nothing on the host.
fn compose_port(value: &Value) -> Option<Option<ComposePort>> {
    if let Some(mapping) = value.as_mapping() {
        let field = |key: &str| mapping.get(key);
        let container_port = number(field("target")?)?;
        let Some(published) = field("published") else {
            return Some(None);
        };
        return Some(Some(ComposePort {
            host_ip: field("host_ip").and_then(Value::as_str).map(str::to_string),
            host_port: number(published)?,
            container_port,
            protocol: protocol(field("protocol").and_then(Value::as_str)),
        }));
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    if text.contains('$') || text.contains('-') {
        return None;
    }
    let (spec, protocol_name) = match text.split_once('/') {
        Some((spec, protocol)) => (spec, Some(protocol)),
        None => (text.as_str(), None),
    };
    let (host_ip, ports) = if let Some(rest) = spec.strip_prefix('[') {
        let (ip, rest) = rest.split_once("]:")?;
        (Some(ip.to_string()), rest)
    } else {
        match spec.matches(':').count() {
            2 => {
                let (ip, rest) = spec.split_once(':')?;
                (Some(ip.to_string()), rest)
            }
            _ => (None, spec),
        }
    };
    let Some((host_port, container_port)) = ports.split_once(':') else {
        ports.parse::<u16>().ok()?;
        return Some(None);
    };
    Some(Some(ComposePort {
        host_ip: host_ip.filter(|ip| ip.parse::<std::net::IpAddr>().is_ok()),
        host_port: host_port.parse().ok().filter(|port| *port != 0)?,
        container_port: container_port.parse().ok().filter(|port| *port != 0)?,
        protocol: protocol(protocol_name),
    }))
}

fn number(value: &Value) -> Option<u16> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|n| u16::try_from(n).ok()),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
    .filter(|port| *port != 0)
}

fn protocol(name: Option<&str>) -> TransportProtocol {
    match name {
        Some(name) if name.eq_ignore_ascii_case("udp") => TransportProtocol::Udp,
        _ => TransportProtocol::Tcp,
    }
}

fn environment(value: Option<&Value>) -> Vec<(String, String)> {
    let pairs: Vec<(String, String)> = match value.and_then(Value::as_mapping) {
        Some(mapping) => mapping
            .iter()
            .filter_map(|(key, value)| {
                let value = match value {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => number.to_string(),
                    _ => return None,
                };
                Some((key.as_str()?.to_string(), value))
            })
            .collect(),
        None => strings(value)
            .into_iter()
            .filter_map(|line| {
                line.split_once('=')
                    .map(|(key, value)| (key.to_string(), value.to_string()))
            })
            .collect(),
    };
    pairs
        .into_iter()
        .filter(|(key, _)| is_env_name(key))
        .collect()
}

fn env_files(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(path) => Some(path.clone()),
                Value::Mapping(mapping) => mapping
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
