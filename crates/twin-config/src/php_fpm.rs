use crate::endpoint::{host_port, is_socket_path, DeclaredEndpoint};

/// The `listen` address of every pool in a PHP-FPM pool file, keyed `pool NAME listen`.
/// A bare port listens on every address, reported as `0.0.0.0`.
pub fn parse_fpm_pools(text: &str) -> Vec<DeclaredEndpoint> {
    let mut pools = Vec::new();
    let mut pool: Option<String> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split(';').next().unwrap_or_default().trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            pool = (name != "global").then(|| name.trim().to_string());
            continue;
        }
        let Some(name) = &pool else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "listen" {
            continue;
        }
        let value = value.trim().trim_matches(|ch| ch == '"' || ch == '\'');
        let key = format!("pool {name} listen");
        let endpoint = if value.starts_with('/') {
            is_socket_path(value).then(|| DeclaredEndpoint::unix(&key, value))
        } else if let Ok(port) = value.parse::<u16>() {
            (port != 0).then(|| DeclaredEndpoint::tcp(&key, Some("fastcgi"), "0.0.0.0", port))
        } else {
            host_port(value, true)
                .map(|(host, port)| DeclaredEndpoint::tcp(&key, Some("fastcgi"), &host, port))
        };
        if let Some(endpoint) = endpoint {
            pools.push(endpoint.at_line(index + 1));
        }
    }
    pools
}
