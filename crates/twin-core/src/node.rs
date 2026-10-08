use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Host,
    Process,
    Service,
    Port,
    UnixSocket,
    File,
    Cgroup,
    Mount,
    Directory,
    Library,
    Package,
    Container,
    Image,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Host => "host",
            Self::Process => "process",
            Self::Service => "service",
            Self::Port => "port",
            Self::UnixSocket => "unix_socket",
            Self::File => "file",
            Self::Cgroup => "cgroup",
            Self::Mount => "mount",
            Self::Directory => "directory",
            Self::Library => "library",
            Self::Package => "package",
            Self::Container => "container",
            Self::Image => "image",
        })
    }
}

impl FromStr for NodeKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "host" => Ok(Self::Host),
            "process" => Ok(Self::Process),
            "service" => Ok(Self::Service),
            "port" => Ok(Self::Port),
            "unix_socket" => Ok(Self::UnixSocket),
            "file" => Ok(Self::File),
            "cgroup" => Ok(Self::Cgroup),
            "mount" => Ok(Self::Mount),
            "directory" => Ok(Self::Directory),
            "library" => Ok(Self::Library),
            "package" => Ok(Self::Package),
            "container" => Ok(Self::Container),
            "image" => Ok(Self::Image),
            other => Err(ParseError::Enum {
                kind: "NodeKind",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeState {
    Active,
    Stale,
    Gone,
}

impl fmt::Display for NodeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Gone => "gone",
        })
    }
}

impl FromStr for NodeState {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "stale" => Ok(Self::Stale),
            "gone" => Ok(Self::Gone),
            other => Err(ParseError::Enum {
                kind: "NodeState",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    pub fn host(hostname: &str) -> Self {
        Self(format!("host:{hostname}"))
    }

    pub fn process(pid: u32) -> Self {
        Self(format!("process:pid:{pid}"))
    }

    pub fn service(unit: &str) -> Self {
        Self(format!("service:{}", normalize_unit(unit)))
    }

    pub fn port_tcp(ip: &str, port: u16) -> Result<Self, ParseError> {
        let ip = normalize_ip(ip)?;
        let id = if ip.contains(':') {
            format!("port:tcp:[{ip}]:{port}")
        } else {
            format!("port:tcp:{ip}:{port}")
        };
        Ok(Self(id))
    }

    pub fn unix_socket(path: &str) -> Result<Self, ParseError> {
        let path = path.trim();
        if path.is_empty() {
            return Err(ParseError::InvalidNodeId {
                value: "unix:".to_string(),
            });
        }
        let canonical = if path.starts_with('@') {
            path.to_string()
        } else {
            lexical_canonical(path)
        };
        Ok(Self(format!("unix:{canonical}")))
    }

    pub fn file(path: &str) -> Self {
        Self(format!("file:{}", lexical_canonical(path)))
    }

    pub fn mount(path: &str) -> Self {
        Self(format!("mount:{}", lexical_canonical(path)))
    }

    pub fn directory(path: &str) -> Self {
        Self(format!("directory:{}", lexical_canonical(path)))
    }

    pub fn library(path: &str) -> Self {
        Self(format!("library:{}", lexical_canonical(path)))
    }

    pub fn package(name: &str) -> Self {
        Self(format!("package:{name}"))
    }

    pub fn container(name: &str) -> Self {
        Self(format!("container:{name}"))
    }

    pub fn image(reference: &str) -> Self {
        Self(format!("image:{reference}"))
    }

    pub fn cgroup(path: &str) -> Self {
        Self(format!("cgroup:{}", lexical_canonical(path)))
    }

    pub fn kind(&self) -> Option<NodeKind> {
        let s = self.0.as_str();
        if s.starts_with("host:") && s.len() > "host:".len() {
            return Some(NodeKind::Host);
        }
        if s.starts_with("process:pid:") {
            return Some(NodeKind::Process);
        }
        if s.starts_with("service:") && s.len() > "service:".len() {
            return Some(NodeKind::Service);
        }
        if s.starts_with("port:tcp:") {
            return Some(NodeKind::Port);
        }
        if s.starts_with("unix:") && s.len() > "unix:".len() {
            return Some(NodeKind::UnixSocket);
        }
        if s.starts_with("file:") && s.len() > "file:".len() {
            return Some(NodeKind::File);
        }
        if s.starts_with("mount:") && s.len() > "mount:".len() {
            return Some(NodeKind::Mount);
        }
        if s.starts_with("directory:") && s.len() > "directory:".len() {
            return Some(NodeKind::Directory);
        }
        if s.starts_with("library:") && s.len() > "library:".len() {
            return Some(NodeKind::Library);
        }
        if s.starts_with("package:") && s.len() > "package:".len() {
            return Some(NodeKind::Package);
        }
        if s.starts_with("container:") && s.len() > "container:".len() {
            return Some(NodeKind::Container);
        }
        if s.starts_with("image:") && s.len() > "image:".len() {
            return Some(NodeKind::Image);
        }
        if s.starts_with("cgroup:") && s.len() > "cgroup:".len() {
            return Some(NodeKind::Cgroup);
        }
        None
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn process_pid(&self) -> Option<u32> {
        self.0.strip_prefix("process:pid:")?.parse().ok()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NodeId {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_node_id(s)?;
        Ok(Self(s.to_string()))
    }
}

fn validate_node_id(s: &str) -> Result<(), ParseError> {
    let invalid = || ParseError::InvalidNodeId {
        value: s.to_string(),
    };

    if s.is_empty() {
        return Err(invalid());
    }

    if let Some(hostname) = s.strip_prefix("host:") {
        if hostname.is_empty() {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(pid) = s.strip_prefix("process:pid:") {
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            return Err(invalid());
        }
        return pid.parse::<u32>().map(|_| ()).map_err(|_| invalid());
    }

    if let Some(unit) = s.strip_prefix("service:") {
        if unit.is_empty() {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("file:") {
        if path.is_empty() {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("cgroup:") {
        if path.is_empty() {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("mount:") {
        if path.is_empty() {
            return Err(invalid());
        }
        let canonical = NodeId::mount(path);
        if canonical.as_str() != s {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("directory:") {
        if path.is_empty() {
            return Err(invalid());
        }
        let canonical = NodeId::directory(path);
        if canonical.as_str() != s {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("library:") {
        if path.is_empty() || !path.starts_with('/') {
            return Err(invalid());
        }
        let canonical = NodeId::library(path);
        if canonical.as_str() != s {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(name) = s.strip_prefix("package:") {
        if !is_package_name(name) {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(name) = s.strip_prefix("container:") {
        if !is_container_name(name) {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(reference) = s.strip_prefix("image:") {
        if !is_image_reference(reference) {
            return Err(invalid());
        }
        return Ok(());
    }

    if s.starts_with("port:tcp:") {
        let rest = s.strip_prefix("port:tcp:").ok_or_else(invalid)?;
        let (ip, port) = split_port_host_port(rest).map_err(|_| invalid())?;
        let canonical = NodeId::port_tcp(&ip, port).map_err(|_| invalid())?;
        if canonical.as_str() != s {
            return Err(invalid());
        }
        return Ok(());
    }

    if let Some(path) = s.strip_prefix("unix:") {
        if path.is_empty() {
            return Err(invalid());
        }
        let canonical = NodeId::unix_socket(path).map_err(|_| invalid())?;
        if canonical.as_str() != s {
            return Err(invalid());
        }
        return Ok(());
    }

    Err(invalid())
}

fn split_port_host_port(rest: &str) -> Result<(String, u16), ParseError> {
    let invalid = || ParseError::IpAddr {
        value: rest.to_string(),
    };
    if let Some(bracketed) = rest.strip_prefix('[') {
        let end = bracketed.find("]:").ok_or_else(invalid)?;
        let ip = &bracketed[..end];
        let port_str = &bracketed[end + 2..];
        let port = port_str.parse::<u16>().map_err(|_| invalid())?;
        return Ok((ip.to_string(), port));
    }
    let (ip, port_str) = rest.rsplit_once(':').ok_or_else(invalid)?;
    if ip.is_empty() {
        return Err(invalid());
    }
    let port = port_str.parse::<u16>().map_err(|_| invalid())?;
    Ok((ip.to_string(), port))
}

fn is_container_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn is_image_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 256
        && !reference.chars().any(|c| c.is_whitespace() || c == '|')
}

fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '|' | ':' | '/'))
}

fn normalize_unit(unit: &str) -> String {
    let after_slice = if let Some(pos) = unit.rfind(".slice/") {
        &unit[pos + ".slice/".len()..]
    } else {
        unit
    };
    after_slice
        .rsplit('/')
        .next()
        .unwrap_or(after_slice)
        .to_string()
}

fn normalize_ip(ip: &str) -> Result<String, ParseError> {
    if ip.contains('.') && !ip.contains(':') {
        if let Ok(v4) = normalize_ipv4(ip) {
            return Ok(v4);
        }
    }
    ip.parse::<IpAddr>()
        .map(|addr| addr.to_string())
        .map_err(|_| ParseError::IpAddr {
            value: ip.to_string(),
        })
}

fn normalize_ipv4(ip: &str) -> Result<String, ParseError> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return Err(ParseError::IpAddr {
            value: ip.to_string(),
        });
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 3 || !part.chars().all(|c| c.is_ascii_digit()) {
            return Err(ParseError::IpAddr {
                value: ip.to_string(),
            });
        }
        let trimmed = part.trim_start_matches('0');
        let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
        octets[i] = trimmed.parse::<u8>().map_err(|_| ParseError::IpAddr {
            value: ip.to_string(),
        })?;
    }
    Ok(format!(
        "{}.{}.{}.{}",
        octets[0], octets[1], octets[2], octets[3]
    ))
}

pub fn lexical_canonical(path: &str) -> String {
    let absolute = path.starts_with('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut stack: Vec<&str> = Vec::new();
    for seg in segments {
        match seg {
            "." => {}
            ".." => {
                if absolute || !stack.is_empty() {
                    let _ = stack.pop();
                }
            }
            _ => stack.push(seg),
        }
    }
    let body = stack.join("/");
    if absolute {
        if body.is_empty() {
            "/".to_string()
        } else {
            format!("/{body}")
        }
    } else {
        body
    }
}
