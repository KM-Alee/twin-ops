#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NginxParse {
    pub proxies: Vec<ProxyPass>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyPass {
    pub raw: String,
    pub target: ProxyTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTarget {
    Tcp { host: String, port: u16 },
    Unix { path: String },
    Named { name: String },
}

pub fn content_fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn parse_nginx(text: &str) -> NginxParse {
    let mut proxies = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let stripped = strip_comment(line).trim().to_string();
        if stripped.is_empty() {
            continue;
        }
        let Some(rest) = directive_arg(&stripped, "proxy_pass") else {
            continue;
        };
        if rest.is_empty() {
            warnings.push(format!("line {line_no}: proxy_pass is missing a target"));
            continue;
        }
        let statement = rest.trim_end_matches(';').trim();
        if statement.is_empty() || !rest.trim_end().ends_with(';') {
            warnings.push(format!("line {line_no}: proxy_pass is malformed"));
            continue;
        }
        match parse_target(statement) {
            Some(target) => proxies.push(ProxyPass {
                raw: statement.to_string(),
                target,
            }),
            None => warnings.push(format!(
                "line {line_no}: proxy_pass target `{statement}` could not be parsed"
            )),
        }
    }
    NginxParse { proxies, warnings }
}

fn directive_arg<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?;
    if rest.is_empty() {
        return Some("");
    }
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some(rest.trim())
    } else {
        None
    }
}

fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    for ch in line.chars() {
        if let Some(open) = quote {
            out.push(ch);
            if ch == open {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            out.push(ch);
            continue;
        }
        if ch == '#' {
            break;
        }
        out.push(ch);
    }
    out
}

fn parse_target(statement: &str) -> Option<ProxyTarget> {
    if let Some(path) = statement.strip_prefix("unix:") {
        if path.is_empty() {
            return None;
        }
        return Some(ProxyTarget::Unix {
            path: path.to_string(),
        });
    }
    let (default_port, rest) = if let Some(rest) = statement.strip_prefix("https://") {
        (443, rest)
    } else if let Some(rest) = statement.strip_prefix("http://") {
        (80, rest)
    } else {
        return Some(ProxyTarget::Named {
            name: statement.to_string(),
        });
    };
    let hostport = rest.split(['/', '?']).next().unwrap_or(rest);
    if hostport.is_empty() {
        return None;
    }
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        let port = port.parse().ok()?;
        return Some(ProxyTarget::Tcp {
            host: host.to_string(),
            port,
        });
    }
    if let Some((host, port)) = hostport.rsplit_once(':') {
        if host.contains(':') {
            return None;
        }
        let port = port.parse().ok()?;
        return Some(ProxyTarget::Tcp {
            host: host.to_string(),
            port,
        });
    }
    Some(ProxyTarget::Tcp {
        host: hostport.to_string(),
        port: default_port,
    })
}
