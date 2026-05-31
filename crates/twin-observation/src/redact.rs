use crate::raw::RawObservation;
use crate::vocab::{ObservationKind, RedactionState};

pub trait Redactor {
    fn redact(&self, raw: &mut RawObservation) -> RedactionState;
}

pub struct BasicRedactor;

const SENSITIVE_FLAG_NAMES: &[&str] = &[
    "password", "passwd", "token", "secret", "key", "api-key", "api_key",
];

const SENSITIVE_SUBSTRINGS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "credential",
    "authorization",
    "private_key",
];

impl Redactor for BasicRedactor {
    fn redact(&self, raw: &mut RawObservation) -> RedactionState {
        let mut partial = false;
        let mut full = false;
        if raw.kind == ObservationKind::ProcessCommandSeen
            && redact_argv_metadata(&mut raw.metadata)
        {
            raw.metadata.insert_bool("command_redacted", true);
            partial = true;
        }
        let keys = raw.metadata.keys();
        for key in keys {
            let key_lower = key.to_lowercase();
            if SENSITIVE_SUBSTRINGS
                .iter()
                .any(|needle| key_lower.contains(needle))
            {
                raw.metadata.insert_str(&key, "<redacted>");
                raw.metadata.insert_bool("value_redacted", true);
                full = true;
                continue;
            }
            if let Some(value) = raw
                .metadata
                .get(&key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                if redact_connection_string(raw, &key, &value).is_some() {
                    full = true;
                }
            }
        }
        if full {
            RedactionState::Redacted
        } else if partial {
            RedactionState::Partial
        } else {
            RedactionState::None
        }
    }
}

fn redact_argv_metadata(metadata: &mut crate::observation::ObservationMetadata) -> bool {
    let Some(argv) = metadata
        .get("argv_json")
        .and_then(|v| v.as_array())
        .cloned()
    else {
        return false;
    };
    let strings: Vec<String> = argv
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if strings.is_empty() {
        return false;
    }
    let (redacted_argv, changed) = redact_argv(&strings);
    if changed {
        metadata.insert_str_array("argv_json", &redacted_argv);
    }
    changed
}

fn redact_argv(argv: &[String]) -> (Vec<String>, bool) {
    let mut out = Vec::with_capacity(argv.len());
    let mut changed = false;
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            out.push("<redacted>".to_string());
            changed = true;
            redact_next = false;
            continue;
        }
        if let Some(redacted) = redact_flag_value(arg) {
            out.push(redacted);
            changed = true;
            continue;
        }
        if is_sensitive_flag(arg) {
            out.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_uri_userinfo(arg) {
            out.push(redacted);
            changed = true;
            continue;
        }
        out.push(arg.clone());
    }
    (out, changed)
}

fn redact_flag_value(arg: &str) -> Option<String> {
    let (name, _value) = arg.split_once('=')?;
    if !is_sensitive_flag(name) {
        return None;
    }
    Some(format!("{name}=<redacted>"))
}

fn is_sensitive_flag(arg: &str) -> bool {
    let name = arg.trim_start_matches('-');
    let name = name.trim_start_matches('-');
    let name_lower = name.to_ascii_lowercase();
    SENSITIVE_FLAG_NAMES
        .iter()
        .any(|needle| name_lower == *needle || name_lower.starts_with(&format!("{needle}=")))
}

fn redact_uri_userinfo(arg: &str) -> Option<String> {
    let scheme_end = arg.find("://")?;
    let scheme = &arg[..scheme_end];
    if scheme.is_empty() || scheme == "file" {
        return None;
    }
    let after = &arg[scheme_end + 3..];
    let at = after.find('@')?;
    let userinfo = &after[..at];
    if !userinfo.contains(':') {
        return None;
    }
    let host_part = &after[at + 1..];
    Some(format!("{scheme}://<redacted>@{host_part}"))
}

fn redact_connection_string(raw: &mut RawObservation, key: &str, value: &str) -> Option<String> {
    let scheme_end = value.find("://")?;
    let scheme = &value[..scheme_end];
    if scheme.is_empty() || scheme == "file" {
        return None;
    }
    let after_scheme = &value[scheme_end + 3..];
    let endpoint = if let Some(at) = after_scheme.find('@') {
        let host_part = &after_scheme[at + 1..];
        host_part.split('/').next().unwrap_or(host_part)
    } else {
        after_scheme.split('/').next().unwrap_or(after_scheme)
    };
    if endpoint.is_empty() {
        return None;
    }
    raw.metadata.insert_str(key, "<redacted>");
    raw.metadata.insert_bool("value_redacted", true);
    raw.metadata.insert_str("maybe_endpoint", endpoint);
    Some(endpoint.to_string())
}
