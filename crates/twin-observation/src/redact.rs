use crate::raw::RawObservation;
use crate::vocab::RedactionState;

pub trait Redactor {
    fn redact(&self, raw: &mut RawObservation) -> RedactionState;
}

pub struct BasicRedactor;

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
        let mut redacted = false;
        let keys = raw.metadata.keys();
        for key in keys {
            let key_lower = key.to_lowercase();
            if SENSITIVE_SUBSTRINGS
                .iter()
                .any(|needle| key_lower.contains(needle))
            {
                raw.metadata.insert_str(&key, "<redacted>");
                raw.metadata.insert_bool("value_redacted", true);
                redacted = true;
                continue;
            }
            if let Some(value) = raw
                .metadata
                .get(&key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                if redact_connection_string(raw, &key, &value).is_some() {
                    redacted = true;
                }
            }
        }
        if redacted {
            RedactionState::Redacted
        } else {
            RedactionState::None
        }
    }
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
