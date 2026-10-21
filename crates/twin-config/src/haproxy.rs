use crate::endpoint::{host_port, is_socket_path, DeclaredEndpoint};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HaproxyParse {
    pub servers: Vec<DeclaredEndpoint>,
    pub warnings: Vec<String>,
}

/// `server` lines of `backend` and `listen` sections. Keyed `backend NAME server SNAME`.
pub fn parse_haproxy(text: &str) -> HaproxyParse {
    let mut parsed = HaproxyParse::default();
    let mut section: Option<String> = None;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.split('#').next().unwrap_or_default().trim();
        let mut words = line.split_whitespace();
        let Some(first) = words.next() else {
            continue;
        };
        match first {
            "global" | "defaults" | "frontend" | "userlist" | "peers" | "resolvers" | "mailers"
            | "program" | "cache" | "ring" | "http-errors" => {
                section = None;
                continue;
            }
            "backend" | "listen" => {
                section = words.next().map(|name| format!("{first} {name}"));
                continue;
            }
            _ => {}
        }
        let Some(section) = &section else {
            continue;
        };
        if first == "server-template" {
            parsed.warnings.push(format!(
                "line {line_no}: server-template backends are resolved at runtime and not followed"
            ));
            continue;
        }
        if first != "server" {
            continue;
        }
        let (Some(name), Some(address)) = (words.next(), words.next()) else {
            parsed
                .warnings
                .push(format!("line {line_no}: server line without an address"));
            continue;
        };
        let key = format!("{section} server {name}");
        match server_address(&key, address) {
            Some(endpoint) => parsed.servers.push(endpoint.at_line(line_no)),
            None => parsed.warnings.push(format!(
                "line {line_no}: server address uses a variable, a runtime port or an unsupported form"
            )),
        }
    }
    parsed
}

fn server_address(key: &str, address: &str) -> Option<DeclaredEndpoint> {
    if address.contains('$') {
        return None;
    }
    let (family, rest) = match address.split_once('@') {
        Some((family, rest)) => (Some(family), rest),
        None => (None, address),
    };
    match family {
        Some("unix") => is_socket_path(rest).then(|| DeclaredEndpoint::unix(key, rest)),
        Some("ipv4" | "ipv6") | None if rest.starts_with('/') => {
            is_socket_path(rest).then(|| DeclaredEndpoint::unix(key, rest))
        }
        Some("ipv4" | "ipv6") | None => {
            if rest
                .rsplit(':')
                .next()
                .is_some_and(|port| port.starts_with(['+', '-']))
            {
                return None;
            }
            let (host, port) = host_port(rest, true)?;
            Some(DeclaredEndpoint::tcp(key, None, &host, port))
        }
        _ => None,
    }
}
