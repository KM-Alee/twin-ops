use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpTableKind {
    Tcp,
    Tcp6,
}

impl TcpTableKind {
    pub fn net_file_name(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tcp6 => "tcp6",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketState {
    Listen,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpSocketRecord {
    pub table: TcpTableKind,
    pub local_ip: String,
    pub local_port: u16,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTcpWarning {
    pub table: TcpTableKind,
    pub line: usize,
    pub detail: String,
}

pub fn parse_tcp_table(
    table: TcpTableKind,
    content: &str,
) -> (Vec<TcpSocketRecord>, Vec<ParseTcpWarning>) {
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_tcp_line(table, trimmed, line_no) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {}
            Err(detail) => warnings.push(ParseTcpWarning {
                table,
                line: line_no,
                detail,
            }),
        }
    }
    (records, warnings)
}

fn parse_tcp_line(
    table: TcpTableKind,
    line: &str,
    raw_line: usize,
) -> Result<Option<TcpSocketRecord>, String> {
    let mut fields = line.split_whitespace();
    let _sl = fields.next().ok_or_else(|| "missing sl".to_string())?;
    let local = fields
        .next()
        .ok_or_else(|| "missing local_address".to_string())?;
    let _rem = fields
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
    let state = parse_state(state_hex)?;
    if state != SocketState::Listen {
        return Ok(None);
    }
    let (ip_hex, port_hex) = local
        .split_once(':')
        .ok_or_else(|| "invalid local_address".to_string())?;
    let local_port = u16::from_str_radix(port_hex, 16).map_err(|e| format!("invalid port: {e}"))?;
    let local_ip = match table {
        TcpTableKind::Tcp => parse_ipv4_hex(ip_hex)?,
        TcpTableKind::Tcp6 => parse_ipv6_hex(ip_hex)?,
    };
    Ok(Some(TcpSocketRecord {
        table,
        local_ip,
        local_port,
        state,
        inode,
        raw_line,
    }))
}

fn parse_state(hex: &str) -> Result<SocketState, String> {
    let upper = hex.to_ascii_uppercase();
    if upper == "0A" {
        return Ok(SocketState::Listen);
    }
    Ok(SocketState::Other(upper))
}

fn parse_ipv4_hex(hex: &str) -> Result<String, String> {
    if hex.len() != 8 {
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
    if hex.len() != 32 {
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
        let (records, warnings) = parse_tcp_table(TcpTableKind::Tcp, &content);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].local_ip, "127.0.0.1");
        assert_eq!(records[0].local_port, 5432);
        assert_eq!(records[0].inode, 12345);
        assert_eq!(records[0].state, SocketState::Listen);
    }

    #[test]
    fn parses_ipv4_wildcard_listen() {
        let content = format!(
            "{TCP_HEADER}\n   0: 00000000:0050 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 99 1 0000000000000000 100 0 0 10 0"
        );
        let (records, _) = parse_tcp_table(TcpTableKind::Tcp, &content);
        assert_eq!(records[0].local_ip, "0.0.0.0");
        assert_eq!(records[0].local_port, 80);
    }

    #[test]
    fn skips_non_listen_rows() {
        let content = format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 0100007F:0050 01 00000000:00000000 00000000:00000000  00000000       0        0 1 1 0000000000000000 100 0 0 10 0"
        );
        let (records, _) = parse_tcp_table(TcpTableKind::Tcp, &content);
        assert!(records.is_empty());
    }

    #[test]
    fn malformed_row_is_warning_not_panic() {
        let content = format!("{TCP_HEADER}\n   0: badaddr 00000000:0000 0A");
        let (records, warnings) = parse_tcp_table(TcpTableKind::Tcp, &content);
        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn parses_ipv6_loopback_listen() {
        let content = format!(
            "{TCP_HEADER}\n   0: 00000000000000000000000001000000:1538 00000000000000000000000000000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 54321 1 0000000000000000 100 0 0 10 0"
        );
        let (records, warnings) = parse_tcp_table(TcpTableKind::Tcp6, &content);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].local_ip, "::1");
        assert_eq!(records[0].local_port, 5432);
    }

    #[test]
    fn parses_socket_fd_target() {
        assert_eq!(parse_socket_fd_target("socket:[12345]"), Some(12345));
        assert_eq!(parse_socket_fd_target("pipe:[1]"), None);
    }
}
