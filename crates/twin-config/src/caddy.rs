use crate::endpoint::{host_port, is_socket_path, DeclaredEndpoint};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaddyParse {
    pub upstreams: Vec<DeclaredEndpoint>,
    pub warnings: Vec<String>,
}

const PROXY_DIRECTIVES: [&str; 2] = ["reverse_proxy", "php_fastcgi"];

/// Upstreams of `reverse_proxy` and `php_fastcgi`, inline or in a `to` subdirective.
pub fn parse_caddy(text: &str) -> CaddyParse {
    let mut parsed = CaddyParse::default();
    let mut proxy_block: Option<(&str, usize)> = None;
    let mut depth = 0usize;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.split(" #").next().unwrap_or_default().trim();
        if line.starts_with('#') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let opens = words.last() == Some(&"{");
        let closes = line == "}";
        match (proxy_block, words.first()) {
            (Some((directive, block_depth)), Some(&"to")) if depth == block_depth + 1 => {
                for word in &words[1..] {
                    push_upstream(&mut parsed, directive, word, line_no);
                }
            }
            (None, Some(first)) if PROXY_DIRECTIVES.contains(first) => {
                let directive = *first;
                let arguments = &words[1..words.len() - usize::from(opens)];
                for word in arguments.iter().skip_while(|word| is_matcher(word)) {
                    push_upstream(&mut parsed, directive, word, line_no);
                }
                if opens {
                    proxy_block = Some((directive, depth));
                }
            }
            _ => {}
        }
        if opens {
            depth += 1;
        }
        if closes {
            depth = depth.saturating_sub(1);
            if proxy_block.is_some_and(|(_, block_depth)| block_depth == depth) {
                proxy_block = None;
            }
        }
    }
    parsed
}

fn is_matcher(word: &str) -> bool {
    word.starts_with('/') || word.starts_with('@') || word == "*"
}

fn push_upstream(parsed: &mut CaddyParse, directive: &str, word: &str, line_no: usize) {
    if word.contains('{') {
        parsed.warnings.push(format!(
            "line {line_no}: {directive} upstream uses a placeholder and is not followed"
        ));
        return;
    }
    match upstream(directive, word) {
        Some(endpoint) => parsed.upstreams.push(endpoint.at_line(line_no)),
        None => parsed.warnings.push(format!(
            "line {line_no}: {directive} upstream could not be parsed"
        )),
    }
}

fn upstream(directive: &str, word: &str) -> Option<DeclaredEndpoint> {
    if let Some(path) = word.strip_prefix("unix/") {
        return is_socket_path(path).then(|| DeclaredEndpoint::unix(directive, path));
    }
    let (scheme, rest) = match word.split_once("://") {
        Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
        None => (None, word),
    };
    let rest = rest.split('/').next().unwrap_or_default();
    let (host, port) = if let Some(port) = rest.strip_prefix(':') {
        (
            "localhost".to_string(),
            port.parse::<u16>().ok().filter(|port| *port != 0)?,
        )
    } else if let Some(found) = host_port(rest, true) {
        found
    } else {
        let port = match scheme.as_deref() {
            Some("https") => 443,
            _ if directive == "php_fastcgi" => 9000,
            _ => 80,
        };
        let (host, _) = host_port(&format!("{rest}:{port}"), true)?;
        (host, port)
    };
    let scheme = scheme.or_else(|| (directive == "php_fastcgi").then(|| "fastcgi".to_string()));
    Some(DeclaredEndpoint::tcp(
        directive,
        scheme.as_deref(),
        &host,
        port,
    ))
}
