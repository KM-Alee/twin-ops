//! Replication sources declared by database servers. These settings carry credentials, so
//! each parser reads only the host and port fields and returns nothing else.

use crate::endpoint::{host_port, DeclaredEndpoint};

/// PostgreSQL `primary_conninfo` (in `postgresql.conf`, `postgresql.auto.conf` or a
/// legacy `recovery.conf`): libpq `key=value` pairs, of which only `host`/`hostaddr` and
/// `port` are read.
pub fn postgres_primary(text: &str) -> Option<DeclaredEndpoint> {
    let mut found = None;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "primary_conninfo" {
            continue;
        }
        let value = value.trim();
        let inner = value
            .strip_prefix('\'')
            .and_then(|rest| rest.rsplit_once('\'').map(|(inner, _)| inner))
            .unwrap_or(value);
        found = conninfo_endpoint(inner).map(|endpoint| endpoint.at_line(index + 1));
    }
    found
}

fn conninfo_endpoint(conninfo: &str) -> Option<DeclaredEndpoint> {
    let mut host = None;
    let mut port = 5432u16;
    for pair in conninfo.split_whitespace() {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim_matches('\'');
        match key {
            "host" | "hostaddr" if host.is_none() || key == "hostaddr" => {
                host = Some(value.split(',').next().unwrap_or_default().to_string())
            }
            "port" => port = value.split(',').next()?.parse().ok().filter(|p| *p != 0)?,
            _ => {}
        }
    }
    let host = host?;
    if host.starts_with('/') {
        return Some(DeclaredEndpoint::unix(
            "primary_conninfo",
            &format!("{host}/.s.PGSQL.{port}"),
        ));
    }
    let (host, port) = host_port(&format!("{host}:{port}"), true)?;
    Some(DeclaredEndpoint::tcp(
        "primary_conninfo",
        Some("postgres"),
        &host,
        port,
    ))
}

/// Redis/Valkey `replicaof HOST PORT` (or the older `slaveof`).
pub fn redis_primary(text: &str) -> Option<DeclaredEndpoint> {
    let mut found = None;
    for (index, raw) in text.lines().enumerate() {
        let mut words = raw.split_whitespace();
        let Some(directive) = words.next() else {
            continue;
        };
        if !matches!(
            directive.to_ascii_lowercase().as_str(),
            "replicaof" | "slaveof"
        ) {
            continue;
        }
        let (Some(host), Some(port)) = (words.next(), words.next()) else {
            continue;
        };
        found = host_port(&format!("{host}:{port}"), true).map(|(host, port)| {
            DeclaredEndpoint::tcp(directive, Some("redis"), &host, port).at_line(index + 1)
        });
    }
    found
}

/// MariaDB/MySQL `master.info`: line 1 holds the line count, line 4 the host and line 7
/// the port. Lines 5 and 6 (user and password) are never read into anything.
pub fn mysql_master_info(text: &str) -> Option<DeclaredEndpoint> {
    let mut host = None;
    let mut port = None;
    for (index, line) in text.lines().enumerate() {
        match index {
            3 => host = Some(line.trim()),
            6 => port = line.trim().parse::<u16>().ok(),
            _ if index > 6 => break,
            _ => {}
        }
    }
    let (host, port) = host_port(&format!("{}:{}", host?, port?), true)?;
    Some(DeclaredEndpoint::tcp(
        "master_host",
        Some("mysql"),
        &host,
        port,
    ))
}
