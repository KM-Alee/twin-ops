//! Endpoints declared in configuration values. Values are read transiently; only the
//! scheme, host and port (or socket path) of a declared endpoint are returned, never user
//! names, passwords, paths, queries or the rest of the value.

use std::fmt;

use twin_core::TransportProtocol;

use crate::ProxyTarget;

const MAX_HOST_LEN: usize = 253;
const MAX_ENDPOINTS_PER_VALUE: usize = 16;

/// A dependency a configuration file declares: where, and on what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredEndpoint {
    /// The setting that declares it, such as `DATABASE_URL` or `backend api server web1`.
    pub key: String,
    pub scheme: Option<String>,
    pub protocol: TransportProtocol,
    pub target: ProxyTarget,
    /// 1-based line in the declaring file, or 0 when the format has no lines.
    pub line: usize,
}

impl DeclaredEndpoint {
    pub fn tcp(key: impl Into<String>, scheme: Option<&str>, host: &str, port: u16) -> Self {
        Self {
            key: key.into(),
            scheme: scheme.map(str::to_string),
            protocol: protocol_for(scheme),
            target: ProxyTarget::Tcp {
                host: host.to_ascii_lowercase(),
                port,
            },
            line: 0,
        }
    }

    pub fn unix(key: impl Into<String>, path: &str) -> Self {
        Self {
            key: key.into(),
            scheme: None,
            protocol: TransportProtocol::Tcp,
            target: ProxyTarget::Unix { path: path.into() },
            line: 0,
        }
    }

    pub fn at_line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }

    /// The sanitized endpoint, e.g. `postgres://db.internal:5432` or `unix:/run/app.sock`.
    pub fn origin(&self) -> String {
        match &self.target {
            ProxyTarget::Tcp { host, port } => {
                let host = if host.contains(':') {
                    format!("[{host}]")
                } else {
                    host.clone()
                };
                match &self.scheme {
                    Some(scheme) => format!("{scheme}://{host}:{port}"),
                    None => format!("{host}:{port}"),
                }
            }
            ProxyTarget::Unix { path } => format!("unix:{path}"),
            ProxyTarget::Named { name } => name.clone(),
        }
    }
}

impl fmt::Display for DeclaredEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} → {}", self.key, self.origin())
    }
}

/// Names whose values are credentials. Their values are never parsed, even when they look
/// like endpoints.
pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const ANYWHERE: [&str; 6] = [
        "PASSWORD",
        "PASSWD",
        "SECRET",
        "TOKEN",
        "CREDENTIAL",
        "PRIVATE",
    ];
    const SEGMENTS: [&str; 7] = ["PASS", "PWD", "KEY", "APIKEY", "SALT", "COOKIE", "AUTH"];
    ANYWHERE.iter().any(|word| upper.contains(word))
        || upper
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .any(|segment| {
                SEGMENTS.contains(&segment) || segment.ends_with("PASS") || segment.ends_with("PWD")
            })
}

/// Names whose bare `name:port` or bare host values are endpoints rather than, say, times.
fn names_an_endpoint(name: &str) -> bool {
    const SUFFIXES: [&str; 14] = [
        "HOST", "HOSTS", "ADDR", "ADDRESS", "URL", "URLS", "URI", "DSN", "ENDPOINT", "SERVER",
        "SERVERS", "BROKER", "BROKERS", "UPSTREAM",
    ];
    let upper = name.to_ascii_uppercase();
    upper
        .rsplit(|ch: char| !ch.is_ascii_alphanumeric())
        .next()
        .is_some_and(|last| SUFFIXES.contains(&last))
}

/// Every endpoint in one `name=value` setting: URLs (any scheme, credentials dropped) and
/// `host:port` pairs, in lists separated by commas, semicolons or spaces.
pub fn endpoints_in_value(name: &str, value: &str) -> Vec<DeclaredEndpoint> {
    if is_secret_name(name) {
        return Vec::new();
    }
    let mut found: Vec<DeclaredEndpoint> = Vec::new();
    let bare_names = names_an_endpoint(name);
    let mut pending_scheme: Option<String> = None;
    for token in value.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';') {
        if found.len() >= MAX_ENDPOINTS_PER_VALUE {
            break;
        }
        let token = token.trim_matches(|ch| ch == '"' || ch == '\'');
        if token.is_empty() {
            continue;
        }
        let endpoint = if token.contains("://") {
            let endpoint = url_endpoint(name, token);
            pending_scheme = token
                .split_once("://")
                .map(|(scheme, _)| scheme.to_ascii_lowercase());
            endpoint
        } else {
            // `mongodb://a:1,b:2` lists further hosts after the first one.
            host_port(token, bare_names || pending_scheme.is_some()).map(|(host, port)| {
                DeclaredEndpoint::tcp(name, pending_scheme.as_deref(), &host, port)
            })
        };
        if let Some(endpoint) = endpoint {
            if !found.contains(&endpoint) {
                found.push(endpoint);
            }
        }
    }
    found
}

/// Endpoints from a whole environment: each value, plus `X_HOST` / `X_PORT` pairs.
pub fn endpoints_in_environment<'a>(
    assignments: impl IntoIterator<Item = (&'a str, &'a str)> + Clone,
) -> Vec<DeclaredEndpoint> {
    let mut found = Vec::new();
    for (name, value) in assignments.clone() {
        for endpoint in endpoints_in_value(name, value) {
            if !found.contains(&endpoint) {
                found.push(endpoint);
            }
        }
    }
    for (name, value) in assignments.clone() {
        let upper = name.to_ascii_uppercase();
        let Some(prefix) = upper.strip_suffix("_HOST") else {
            continue;
        };
        if is_secret_name(name) || value.contains(':') {
            continue;
        }
        let port_name = format!("{prefix}_PORT");
        let Some(port) = assignments
            .clone()
            .into_iter()
            .find(|(other, _)| other.eq_ignore_ascii_case(&port_name))
            .and_then(|(_, port)| port.trim().parse::<u16>().ok())
            .filter(|port| *port != 0)
        else {
            continue;
        };
        if let Some((host, port)) = host_port(&format!("{}:{port}", value.trim()), true) {
            let endpoint = DeclaredEndpoint::tcp(format!("{name}+{port_name}"), None, &host, port);
            if !found
                .iter()
                .any(|known: &DeclaredEndpoint| known.target == endpoint.target)
            {
                found.push(endpoint);
            }
        }
    }
    found
}

fn url_endpoint(name: &str, token: &str) -> Option<DeclaredEndpoint> {
    let (scheme, rest) = token.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return None;
    }
    if scheme == "unix" || scheme.ends_with("+unix") {
        let path = rest.split(['?', '#']).next().unwrap_or_default();
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        return is_socket_path(&path).then(|| DeclaredEndpoint::unix(name, &path));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if cut_inside_credentials(rest, authority) {
        return None;
    }
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = split_host_port(hostport)?;
    let port = match port {
        Some(port) => port,
        None => default_port(&scheme)?,
    };
    valid_host(&host, true).then(|| DeclaredEndpoint::tcp(name, Some(&scheme), &host, port))
}

/// `host:port`, `user@host:port` or `[v6]:port`, optionally followed by a path.
pub(crate) fn host_port(token: &str, allow_bare_names: bool) -> Option<(String, u16)> {
    let authority = token.split(['/', '?', '#']).next().unwrap_or_default();
    if cut_inside_credentials(token, authority) {
        return None;
    }
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = split_host_port(hostport)?;
    let port = port?;
    valid_host(&host, allow_bare_names).then_some((host, port))
}

/// An `@` past the authority means a `/`, `?` or `#` inside the credentials ended it
/// early, so what looks like `host:port` is really part of a user name and password.
fn cut_inside_credentials(text: &str, authority: &str) -> bool {
    text.matches('@').count() != authority.matches('@').count()
}

fn split_host_port(hostport: &str) -> Option<(String, Option<u16>)> {
    if hostport.is_empty() || hostport.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        let port = match suffix {
            "" => None,
            suffix => Some(parse_port(suffix.strip_prefix(':')?)?),
        };
        return Some((host.to_string(), port));
    }
    match hostport.rsplit_once(':') {
        Some((host, _)) if host.contains(':') => None,
        Some((host, port)) => Some((host.to_string(), Some(parse_port(port)?))),
        None => Some((hostport.to_string(), None)),
    }
}

fn parse_port(text: &str) -> Option<u16> {
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok().filter(|port| *port != 0)
}

/// IP literals, `localhost`, and DNS names with a dot. Single-label names (`db`, a
/// Compose service) count only where the setting's name says it holds an endpoint.
fn valid_host(host: &str, allow_bare_names: bool) -> bool {
    if host.is_empty() || host.len() > MAX_HOST_LEN {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let is_dns = host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    });
    let has_letter = host.chars().any(|ch| ch.is_ascii_alphabetic());
    is_dns
        && has_letter
        && (host.eq_ignore_ascii_case("localhost") || host.contains('.') || allow_bare_names)
}

pub(crate) fn is_socket_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() > 1
        && path.len() <= 107
        && !path.contains("..")
        && path
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '@'))
}

fn default_port(scheme: &str) -> Option<u16> {
    let base = scheme.split('+').next().unwrap_or(scheme);
    Some(match base {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "postgres" | "postgresql" => 5432,
        "mysql" | "mariadb" => 3306,
        "redis" | "rediss" | "valkey" => 6379,
        "amqp" => 5672,
        "amqps" => 5671,
        "mongodb" => 27017,
        "nats" => 4222,
        "memcached" => 11211,
        "ldap" => 389,
        "ldaps" => 636,
        "smtp" => 25,
        "smtps" => 465,
        "mqtt" => 1883,
        "mqtts" => 8883,
        "ftp" => 21,
        _ => return None,
    })
}

fn protocol_for(scheme: Option<&str>) -> TransportProtocol {
    match scheme {
        Some("udp") | Some("statsd") | Some("syslog+udp") => TransportProtocol::Udp,
        _ => TransportProtocol::Tcp,
    }
}
