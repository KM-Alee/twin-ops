use std::fmt;

/// One `NAME=value` line. The value can be a credential: it exists only for the caller to
/// look for endpoints in, and `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct EnvAssignment {
    pub name: String,
    pub value: String,
    pub line: usize,
}

impl fmt::Debug for EnvAssignment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvAssignment")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("line", &self.line)
            .finish()
    }
}

/// Parses environment files in the shared subset of systemd `EnvironmentFile=`, Docker
/// `env_file` and shell syntax: `NAME=value`, optional `export`, quotes and `#` comments.
pub fn parse_env_file(text: &str) -> Vec<EnvAssignment> {
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !is_env_name(name) {
            continue;
        }
        out.push(EnvAssignment {
            name: name.to_string(),
            value: unquote(value.trim()),
            line: index + 1,
        });
    }
    out
}

pub(crate) fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.split_once(quote).map(|(inner, _)| inner))
        {
            return inner.to_string();
        }
    }
    value
        .split_once(" #")
        .map(|(value, _)| value)
        .unwrap_or(value)
        .trim()
        .to_string()
}
