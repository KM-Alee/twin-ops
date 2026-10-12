use crate::raw::RawObservation;
use crate::vocab::{ObservationKind, RedactionState};

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
    let argv = metadata.remove("argv_json");
    let other = metadata.remove("argv");
    let Some(value) = argv.or(other) else {
        return false;
    };
    if let Some(values) = value.as_array() {
        metadata.insert_u64("argc", values.len() as u64);
    }
    true
}

fn redact_connection_string(raw: &mut RawObservation, key: &str, value: &str) -> Option<String> {
    let scheme_end = value.find("://")?;
    let scheme = &value[..scheme_end];
    if scheme.is_empty() || scheme == "file" {
        return None;
    }
    let after_scheme = &value[scheme_end + 3..];
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let endpoint = authority.rsplit('@').next().unwrap_or_default();
    if endpoint.is_empty() {
        return None;
    }
    raw.metadata.insert_str(key, "<redacted>");
    raw.metadata.insert_bool("value_redacted", true);
    raw.metadata.insert_str("maybe_endpoint", endpoint);
    Some(endpoint.to_string())
}
