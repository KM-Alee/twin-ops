use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};

use twin_collectors::dns::{DnsError, DnsReader};
use twin_core::TimestampNs;
use twin_test::{CheckKind, CheckStatus, TestDocument, TestRunReport};

pub(crate) struct ResolutionNotes {
    evidence: Vec<String>,
    failures: Vec<String>,
    fail_on_unknown: bool,
    historical: bool,
}

pub(crate) fn resolve_allowlists(
    document: &mut TestDocument,
    reader: &impl DnsReader,
) -> BTreeMap<String, ResolutionNotes> {
    let hosts = document
        .checks
        .iter()
        .flat_map(|check| match &check.kind {
            CheckKind::Outbound { allowed, .. } => allowed
                .iter()
                .filter_map(|value| {
                    let (host, _) = endpoint(value)?;
                    host.parse::<IpAddr>().is_err().then_some(host)
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<BTreeSet<_>>();
    let answers = reader.resolve(&hosts);
    let observed_at = TimestampNs::now().as_i64();
    let mut notes = BTreeMap::new();
    for check in &mut document.checks {
        let CheckKind::Outbound {
            allowed,
            fail_on_unknown,
            ..
        } = &mut check.kind
        else {
            continue;
        };
        let mut note = ResolutionNotes {
            evidence: Vec::new(),
            failures: Vec::new(),
            fail_on_unknown: *fail_on_unknown,
            historical: false,
        };
        let mut expanded = BTreeSet::new();
        for value in allowed.iter() {
            let (transport, value) = transport_endpoint(value);
            let Some((host, port)) = endpoint(value) else {
                note.failures.push(
                    "invalid outbound allowlist endpoint (expected IP:port or hostname:port)"
                        .into(),
                );
                continue;
            };
            if let Ok(ip) = host.parse::<IpAddr>() {
                expanded.insert(format!("{transport}{}", SocketAddr::new(ip, port)));
                continue;
            }
            match answers.get(&host).unwrap_or(&Err(DnsError::Unavailable)) {
                Ok(addresses) if !addresses.is_empty() => {
                    let endpoints: BTreeSet<_> = addresses
                        .iter()
                        .map(|ip| format!("{transport}{}", SocketAddr::new(*ip, port)))
                        .collect();
                    note.evidence.push(format!("system resolver observed {host} => {} at timestamp_ns={observed_at}; allowlist match inferred from current DNS, historical hostname ownership unknown", endpoints.iter().cloned().collect::<Vec<_>>().join(", ")));
                    expanded.extend(endpoints);
                }
                Ok(_) => note.failures.push(format!("{host}: {}", DnsError::Empty)),
                Err(error) => note.failures.push(format!("{host}: {error}")),
            }
        }
        *allowed = expanded.into_iter().collect();
        notes.insert(check.name.clone(), note);
    }
    notes
}

pub(crate) fn historical_allowlists(
    document: &mut TestDocument,
) -> BTreeMap<String, ResolutionNotes> {
    let mut notes = BTreeMap::new();
    for check in &mut document.checks {
        let CheckKind::Outbound {
            allowed,
            fail_on_unknown,
            ..
        } = &mut check.kind
        else {
            continue;
        };
        let mut normalized = BTreeSet::new();
        let mut note = ResolutionNotes {
            evidence: Vec::new(),
            failures: Vec::new(),
            fail_on_unknown: *fail_on_unknown || document.fail_on_unknown,
            historical: true,
        };
        for value in allowed.iter() {
            let (transport, address) = transport_endpoint(value);
            match endpoint(address) {
                Some((host, port)) => match host.parse::<IpAddr>() {
                    Ok(ip) => { normalized.insert(format!("{transport}{}", SocketAddr::new(ip, port))); }
                    Err(_) => note.failures.push(format!("historical DNS ownership of {host} is unavailable; current DNS is not evidence for a snapshot")),
                },
                None => note.failures.push("invalid historical outbound allowlist endpoint".into()),
            }
        }
        *allowed = normalized.into_iter().collect();
        notes.insert(check.name.clone(), note);
    }
    notes
}

pub(crate) fn apply_notes(report: &mut TestRunReport, notes: BTreeMap<String, ResolutionNotes>) {
    for check in &mut report.checks {
        let Some(note) = notes.get(&check.name) else {
            continue;
        };
        if !note.evidence.is_empty() {
            append(&mut check.evidence, &note.evidence.join("; "));
        }
        if !note.failures.is_empty() {
            if note.historical {
                check.detail = Some(format!("Unknown: {}", note.failures.join("; ")));
                check.status = if note.fail_on_unknown {
                    CheckStatus::Fail
                } else {
                    CheckStatus::Warn
                };
                continue;
            }
            append(&mut check.detail, &note.failures.join("; "));
            if check.status != CheckStatus::Fail {
                check.status = if note.fail_on_unknown {
                    CheckStatus::Fail
                } else {
                    CheckStatus::Warn
                };
            }
        }
    }
    report.passed = 0;
    report.warned = 0;
    report.failed = 0;
    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => report.passed += 1,
            CheckStatus::Warn => report.warned += 1,
            CheckStatus::Fail => report.failed += 1,
        }
    }
}

fn append(target: &mut Option<String>, text: &str) {
    match target {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(text);
        }
        None => *target = Some(text.to_string()),
    }
}

fn transport_endpoint(value: &str) -> (&str, &str) {
    if let Some(value) = value.strip_prefix("tcp:") {
        ("tcp:", value)
    } else if let Some(value) = value.strip_prefix("udp:") {
        ("udp:", value)
    } else {
        ("", value)
    }
}

fn endpoint(value: &str) -> Option<(String, u16)> {
    let (_, value) = transport_endpoint(value);
    if let Ok(address) = value.parse::<SocketAddr>() {
        return Some((address.ip().to_string(), address.port()));
    }
    let (host, port) = value.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some((ip.to_string(), port));
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty()
        || host.len() > 253
        || !host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return None;
    }
    Some((host.to_ascii_lowercase(), port))
}
