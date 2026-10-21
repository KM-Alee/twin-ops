#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NginxParse {
    pub proxies: Vec<ProxyPass>,
    pub upstreams: std::collections::BTreeMap<String, Vec<ProxyTarget>>,
    pub includes: Vec<String>,
    pub include_ranges: Vec<std::ops::Range<usize>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyPass {
    pub has_explicit_port: bool,
    pub is_stream: bool,
    pub protocol: twin_core::TransportProtocol,
    pub raw: String,
    pub target: ProxyTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTarget {
    Tcp { host: String, port: u16 },
    Unix { path: String },
    Named { name: String },
}

/// Directives that hand requests to a backend, with the scheme their raw evidence
/// records. Only `proxy_pass` takes a URL; the others take an address.
const PASS_DIRECTIVES: [(&str, Option<&str>); 5] = [
    ("proxy_pass", None),
    ("fastcgi_pass", Some("fastcgi")),
    ("uwsgi_pass", Some("uwsgi")),
    ("scgi_pass", Some("scgi")),
    ("grpc_pass", Some("grpc")),
];

pub fn content_fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn parse_nginx(text: &str) -> NginxParse {
    let mut parsed = NginxParse {
        proxies: Vec::new(),
        upstreams: std::collections::BTreeMap::new(),
        includes: Vec::new(),
        include_ranges: Vec::new(),
        warnings: Vec::new(),
    };
    let mut blocks: Vec<(Option<String>, bool, usize, bool)> = Vec::new();
    let mut duplicates = std::collections::BTreeSet::new();
    let mut statement = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut line_no = 1;
    let mut statement_line = 1;
    let mut statement_start = 0;
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            line_no += 1;
        }
        if comment {
            if ch == '\n' {
                comment = false;
                statement.push(' ');
            }
            continue;
        }
        if escaped {
            statement.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                statement.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch == '#' {
            comment = true;
            continue;
        }
        if matches!(ch, ';' | '{' | '}') {
            let is_stream = blocks.last().is_some_and(|(_, stream, _, _)| *stream);
            if ch == '{' {
                let entering_stream = statement.trim() == "stream" || is_stream;
                let mut words = statement.split_whitespace();
                let name = (words.next() == Some("upstream"))
                    .then(|| words.next())
                    .flatten()
                    .filter(|name| {
                        !name.is_empty()
                            && name
                                .chars()
                                .all(|ch| ch.is_ascii_alphanumeric() || "_.-".contains(ch))
                    });
                let name = name.map(|name| upstream_key(name, entering_stream));
                if let Some(name) = &name {
                    if parsed.upstreams.contains_key(name) {
                        duplicates.insert(name.clone());
                    }
                    parsed.upstreams.entry(name.clone()).or_default();
                }
                blocks.push((name, entering_stream, parsed.proxies.len(), false));
            } else if ch == '}' {
                if let Some((_, stream, first_proxy, udp)) = blocks.pop() {
                    if stream && udp {
                        for proxy in &mut parsed.proxies[first_proxy..] {
                            proxy.protocol = twin_core::TransportProtocol::Udp;
                            if let Some(address) = proxy.raw.strip_prefix("tcp://") {
                                proxy.raw = format!("udp://{address}");
                            }
                        }
                    }
                } else {
                    parsed
                        .warnings
                        .push(format!("line {line_no}: unmatched closing block"));
                }
            } else if let Some(name) = blocks.last().and_then(|(name, _, _, _)| name.as_deref()) {
                parse_upstream_server(&statement, name, statement_line, &mut parsed);
            }
            if is_stream
                && statement.split_whitespace().next() == Some("listen")
                && statement.split_whitespace().any(|word| word == "udp")
            {
                if let Some((_, _, _, udp)) = blocks.last_mut() {
                    *udp = true;
                }
            }
            let before = parsed.includes.len();
            parse_statement(
                &statement,
                statement_line,
                ch == ';',
                is_stream,
                &mut parsed,
            );
            if parsed.includes.len() > before {
                parsed.include_ranges.push(statement_start..offset + 1);
            }
            statement.clear();
            statement_line = line_no;
            continue;
        }
        if statement.trim().is_empty() && !ch.is_whitespace() {
            statement_line = line_no;
            statement_start = offset;
        }
        statement.push(ch);
    }
    if quote.is_some() || escaped || !blocks.is_empty() {
        parsed
            .warnings
            .push("incomplete quoted value or block".into());
    }
    parse_statement(&statement, statement_line, false, false, &mut parsed);
    for name in duplicates {
        parsed.upstreams.remove(&name);
        parsed
            .warnings
            .push(format!("duplicate upstream `{name}` is ambiguous"));
    }
    parsed
}

fn parse_upstream_server(statement: &str, name: &str, line: usize, parsed: &mut NginxParse) {
    let mut words = statement.split_whitespace();
    if words.next() != Some("server") {
        return;
    }
    let address = words.next().unwrap_or("");
    if words.any(|word| word == "down") {
        return;
    }
    let target = if address.starts_with("unix:") {
        parse_target(address)
    } else if !address.contains(['@', '/', '?', '#', '$']) {
        parse_target(&format!("http://{address}"))
    } else {
        None
    };
    match target {
        Some(target) => {
            parsed
                .upstreams
                .entry(name.into())
                .or_default()
                .push(target);
        }
        None => parsed.warnings.push(format!(
            "line {line}: upstream server address is invalid or unresolved"
        )),
    }
}

fn parse_statement(
    statement: &str,
    line: usize,
    terminated: bool,
    is_stream: bool,
    parsed: &mut NginxParse,
) {
    let statement = statement.trim();
    if let Some(rest) = statement
        .strip_prefix("include")
        .filter(|rest| rest.starts_with(char::is_whitespace))
    {
        let path = rest.trim();
        if path.is_empty() || !terminated || path.contains('$') {
            parsed.warnings.push(format!(
                "line {line}: include is malformed or uses an unresolved variable"
            ));
        } else {
            parsed.includes.push(path.into());
        }
        return;
    }
    let Some((directive, rest)) = PASS_DIRECTIVES.iter().find_map(|(directive, scheme)| {
        statement
            .strip_prefix(directive)
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            .map(|rest| ((*directive, *scheme), rest))
    }) else {
        return;
    };
    let (directive, gateway_scheme) = directive;
    let target = rest.trim();
    if target.is_empty() || !terminated {
        parsed.warnings.push(format!(
            "line {line}: {directive} is malformed or missing a target"
        ));
        return;
    }
    let grpc;
    let target = match gateway_scheme {
        Some("grpc") => {
            grpc = target
                .replacen("grpcs://", "https://", 1)
                .replacen("grpc://", "http://", 1);
            grpc.as_str()
        }
        _ => target,
    };
    let is_gateway = gateway_scheme.is_some();
    if is_stream && target.contains("://") {
        parsed.warnings.push(format!(
            "line {line}: stream proxy target has an unsupported URL scheme"
        ));
        return;
    }
    let stream_address = (is_stream || is_gateway)
        && !target.contains("://")
        && target.contains(':')
        && !target.starts_with("unix:");
    let address = if stream_address {
        format!("http://{target}")
    } else {
        target.to_string()
    };
    match parse_target(&address) {
        Some(target_kind) => {
            let raw = match &target_kind {
                ProxyTarget::Tcp { host, port } => {
                    let host = if host.contains(':') {
                        format!("[{host}]")
                    } else {
                        host.clone()
                    };
                    let scheme = if is_stream {
                        "tcp"
                    } else if let Some(scheme) = gateway_scheme {
                        scheme
                    } else if target.starts_with("https://") {
                        "https"
                    } else {
                        "http"
                    };
                    format!("{scheme}://{host}:{port}")
                }
                ProxyTarget::Unix { path } => format!("unix:{path}"),
                ProxyTarget::Named { name } => name.clone(),
            };
            let authority = target
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(target)
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("");
            let hostport = authority.rsplit('@').next().unwrap_or("");
            let has_explicit_port = if hostport.starts_with('[') {
                hostport
                    .split_once(']')
                    .is_some_and(|(_, suffix)| suffix.starts_with(':'))
            } else {
                hostport.contains(':') && !hostport.starts_with("unix:")
            };
            parsed.proxies.push(ProxyPass {
                has_explicit_port,
                is_stream,
                protocol: twin_core::TransportProtocol::Tcp,
                raw,
                target: target_kind,
            });
        }
        None => parsed.warnings.push(format!(
            "line {line}: {directive} target could not be parsed"
        )),
    }
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
    for prefix in ["http://unix:", "https://unix:"] {
        if let Some(rest) = statement.strip_prefix(prefix) {
            let path = rest.split(":/").next()?;
            if !path.starts_with('/') {
                return None;
            }
            return Some(ProxyTarget::Unix { path: path.into() });
        }
    }
    let (default_port, rest) = if let Some(rest) = statement.strip_prefix("https://") {
        (443, rest)
    } else if let Some(rest) = statement.strip_prefix("http://") {
        (80, rest)
    } else {
        return statement
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_-.$".contains(ch))
            .then(|| ProxyTarget::Named {
                name: statement.into(),
            });
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let hostport = authority.rsplit('@').next()?;
    if hostport.chars().any(char::is_whitespace) {
        return None;
    }
    if hostport.is_empty() {
        return None;
    }
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']')?;
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix.strip_prefix(':')?.parse().ok()?
        };
        if port == 0 {
            return None;
        }
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
        if port == 0 {
            return None;
        }
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

pub fn upstream_key(name: &str, is_stream: bool) -> String {
    if is_stream {
        format!("stream:{name}")
    } else {
        name.to_string()
    }
}
