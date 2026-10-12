use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InetTableKind {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
}

impl InetTableKind {
    pub fn protocol(self) -> twin_core::TransportProtocol {
        match self {
            Self::Tcp | Self::Tcp6 => twin_core::TransportProtocol::Tcp,
            Self::Udp | Self::Udp6 => twin_core::TransportProtocol::Udp,
        }
    }

    pub fn net_file_name(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tcp6 => "tcp6",
            Self::Udp => "udp",
            Self::Udp6 => "udp6",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketState {
    Listen,
    Established,
    Bound,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InetSocketRecord {
    pub table: InetTableKind,
    pub local_ip: String,
    pub local_port: u16,
    pub state: SocketState,
    pub inode: u64,
    pub raw_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InetConnectionRecord {
    pub table: InetTableKind,
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub state: SocketState,
    pub inode: u64,
    pub raw_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketOwner {
    pub pid: u32,
    pub fd: u32,
    pub inode: u64,
    pub fd_path: String,
    pub is_fdinfo_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseInetWarning {
    pub table: InetTableKind,
    pub line: usize,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InetTableParse {
    pub listeners: Vec<InetSocketRecord>,
    pub connections: Vec<InetConnectionRecord>,
    pub warnings: Vec<ParseInetWarning>,
}

pub fn parse_inet_table(table: InetTableKind, content: &str) -> InetTableParse {
    let mut result = InetTableParse::default();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_inet_line(table, trimmed, line_no) {
            Ok(Some(InetParsedRow::Listener(record))) => result.listeners.push(record),
            Ok(Some(InetParsedRow::Connection(record))) => result.connections.push(record),
            Ok(None) => {}
            Err(detail) => result.warnings.push(ParseInetWarning {
                table,
                line: line_no,
                detail,
            }),
        }
    }
    result
}

enum InetParsedRow {
    Listener(InetSocketRecord),
    Connection(InetConnectionRecord),
}

fn parse_inet_line(
    table: InetTableKind,
    line: &str,
    raw_line: usize,
) -> Result<Option<InetParsedRow>, String> {
    let mut fields = line.split_whitespace();
    let _sl = fields.next().ok_or_else(|| "missing sl".to_string())?;
    let local = fields
        .next()
        .ok_or_else(|| "missing local_address".to_string())?;
    let remote = fields
        .next()
        .ok_or_else(|| "missing rem_address".to_string())?;
    let state_hex = fields.next().ok_or_else(|| "missing st".to_string())?;
    for _ in 0..3 {
        fields.next();
    }
    let _uid = fields.next();
    let _timeout = fields.next();
    let inode_str = fields.next().ok_or_else(|| "missing inode".to_string())?;
    if inode_str == "0" {
        return Ok(None);
    }
    let inode = inode_str
        .parse::<u64>()
        .map_err(|e| format!("invalid inode: {e}"))?;
    let state = if table.protocol() == twin_core::TransportProtocol::Udp {
        match state_hex {
            "07" => SocketState::Bound,
            "01" => SocketState::Established,
            _ => return Ok(None),
        }
    } else {
        parse_state(state_hex)?
    };
    let (local_ip, local_port) = parse_endpoint(table, local)?;
    match state {
        SocketState::Listen | SocketState::Bound => {
            Ok(Some(InetParsedRow::Listener(InetSocketRecord {
                table,
                local_ip,
                local_port,
                state,
                inode,
                raw_line,
            })))
        }
        SocketState::Established => {
            if is_empty_remote(table, remote) {
                return Ok(None);
            }
            let (remote_ip, remote_port) = parse_endpoint(table, remote)?;
            Ok(Some(InetParsedRow::Connection(InetConnectionRecord {
                table,
                local_ip,
                local_port,
                remote_ip,
                remote_port,
                state,
                inode,
                raw_line,
            })))
        }
        SocketState::Other(_) => Ok(None),
    }
}

fn parse_state(hex: &str) -> Result<SocketState, String> {
    let upper = hex.to_ascii_uppercase();
    match upper.as_str() {
        "0A" => Ok(SocketState::Listen),
        "01" => Ok(SocketState::Established),
        other => Ok(SocketState::Other(other.to_string())),
    }
}

fn parse_endpoint(table: InetTableKind, endpoint: &str) -> Result<(String, u16), String> {
    let (ip_hex, port_hex) = endpoint
        .split_once(':')
        .ok_or_else(|| "invalid address:port".to_string())?;
    let port = u16::from_str_radix(port_hex, 16).map_err(|e| format!("invalid port: {e}"))?;
    let ip = match table {
        InetTableKind::Tcp | InetTableKind::Udp => parse_ipv4_hex(ip_hex)?,
        InetTableKind::Tcp6 | InetTableKind::Udp6 => parse_ipv6_hex(ip_hex)?,
    };
    Ok((ip, port))
}

fn is_empty_remote(table: InetTableKind, remote: &str) -> bool {
    match table {
        InetTableKind::Tcp | InetTableKind::Udp => remote == "00000000:0000",
        InetTableKind::Tcp6 | InetTableKind::Udp6 => {
            remote == "00000000000000000000000000000000:0000"
                || remote.starts_with("00000000000000000000000000000000:")
        }
    }
}

fn parse_ipv4_hex(hex: &str) -> Result<String, String> {
    if hex.len() != 8 || !hex.is_ascii() {
        return Err(format!("expected 8 hex digits for IPv4, got {}", hex.len()));
    }
    let mut octets = [0u8; 4];
    for (i, oct) in octets.iter_mut().enumerate() {
        let start = (3 - i) * 2;
        *oct = u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|e| format!("invalid IPv4 octet: {e}"))?;
    }
    Ok(Ipv4Addr::from(octets).to_string())
}

fn parse_ipv6_hex(hex: &str) -> Result<String, String> {
    if hex.len() != 32 || !hex.is_ascii() {
        return Err(format!(
            "expected 32 hex digits for IPv6, got {}",
            hex.len()
        ));
    }
    let mut bytes = [0u8; 16];
    for (word_idx, chunk) in hex.as_bytes().chunks(8).enumerate() {
        let word = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
        for (pair_idx, oct) in bytes[word_idx * 4..word_idx * 4 + 4].iter_mut().enumerate() {
            let start = (3 - pair_idx) * 2;
            *oct = u8::from_str_radix(&word[start..start + 2], 16)
                .map_err(|e| format!("invalid IPv6 byte: {e}"))?;
        }
    }
    Ok(Ipv6Addr::from(bytes).to_string())
}

pub fn parse_socket_fd_target(target: &str) -> Option<u64> {
    let rest = target.strip_prefix("socket:[")?;
    let num = rest.strip_suffix(']')?;
    num.parse().ok()
}

pub fn owners_by_inode(owners: &[SocketOwner]) -> HashMap<u64, Vec<SocketOwner>> {
    let mut map: HashMap<u64, Vec<SocketOwner>> = HashMap::new();
    for owner in owners {
        map.entry(owner.inode).or_default().push(owner.clone());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

    #[test]
    fn parses_ipv4_listen_row() {
        let content = format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp, &content);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.listeners.len(), 1);
        assert_eq!(parsed.connections.len(), 0);
        assert_eq!(parsed.listeners[0].local_ip, "127.0.0.1");
        assert_eq!(parsed.listeners[0].local_port, 5432);
        assert_eq!(parsed.listeners[0].inode, 12345);
        assert_eq!(parsed.listeners[0].state, SocketState::Listen);
    }

    #[test]
    fn parses_ipv4_established_row() {
        let content = format!(
            "{TCP_HEADER}\n   0: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp, &content);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.listeners.len(), 0);
        assert_eq!(parsed.connections.len(), 1);
        let conn = &parsed.connections[0];
        assert_eq!(conn.local_ip, "127.0.0.1");
        assert_eq!(conn.local_port, 50122);
        assert_eq!(conn.remote_ip, "127.0.0.1");
        assert_eq!(conn.remote_port, 5432);
        assert_eq!(conn.inode, 456);
        assert_eq!(conn.state, SocketState::Established);
    }

    #[test]
    fn parses_ipv4_wildcard_listen() {
        let content = format!(
            "{TCP_HEADER}\n   0: 00000000:0050 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 99 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp, &content);
        assert_eq!(parsed.listeners[0].local_ip, "0.0.0.0");
        assert_eq!(parsed.listeners[0].local_port, 80);
    }

    #[test]
    fn skips_non_listen_non_established_rows() {
        let content = format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 0100007F:0050 02 00000000:00000000 00000000:00000000  00000000       0        0 1 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp, &content);
        assert!(parsed.listeners.is_empty());
        assert!(parsed.connections.is_empty());
    }

    #[test]
    fn malformed_row_is_warning_not_panic() {
        let content = format!("{TCP_HEADER}\n   0: badaddr 00000000:0000 0A");
        let parsed = parse_inet_table(InetTableKind::Tcp, &content);
        assert!(parsed.listeners.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn parses_ipv6_loopback_listen() {
        let content = format!(
            "{TCP_HEADER}\n   0: 00000000000000000000000001000000:1538 00000000000000000000000000000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 54321 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp6, &content);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.listeners.len(), 1);
        assert_eq!(parsed.listeners[0].local_ip, "::1");
        assert_eq!(parsed.listeners[0].local_port, 5432);
    }

    #[test]
    fn parses_ipv6_established_without_panic() {
        let content = format!(
            "{TCP_HEADER}\n   0: 00000000000000000000000001000000:C453 00000000000000000000000001000000:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 789 1 0000000000000000 100 0 0 10 0"
        );
        let parsed = parse_inet_table(InetTableKind::Tcp6, &content);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.connections.len(), 1);
        assert_eq!(parsed.connections[0].remote_ip, "::1");
        assert_eq!(parsed.connections[0].remote_port, 5432);
    }

    #[test]
    fn parses_socket_fd_target() {
        assert_eq!(parse_socket_fd_target("socket:[12345]"), Some(12345));
        assert_eq!(parse_socket_fd_target("pipe:[1]"), None);
    }
}
