pub const UNIX_FLAG_LISTEN: u32 = 0x0001_0000;
pub const UNIX_ST_CONNECTED: u8 = 0x03;
pub const UNIX_TYPE_STREAM: u32 = 0x0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixSocketRecord {
    pub path: String,
    pub flags: u32,
    pub socket_type: u32,
    pub st: u8,
    pub inode: u64,
    pub raw_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixConnectionRecord {
    pub path: String,
    pub flags: u32,
    pub socket_type: u32,
    pub st: u8,
    pub inode: u64,
    pub raw_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseUnixWarning {
    pub line: usize,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnixTableParse {
    pub listeners: Vec<UnixSocketRecord>,
    pub connections: Vec<UnixConnectionRecord>,
    pub warnings: Vec<ParseUnixWarning>,
}

impl UnixSocketRecord {
    pub fn is_listener(&self) -> bool {
        self.flags & UNIX_FLAG_LISTEN != 0
    }
}

impl UnixConnectionRecord {
    pub fn is_connected_stream(&self) -> bool {
        self.flags & UNIX_FLAG_LISTEN == 0
            && self.st == UNIX_ST_CONNECTED
            && self.socket_type == UNIX_TYPE_STREAM
    }
}

pub fn parse_unix_table(content: &str) -> UnixTableParse {
    let mut result = UnixTableParse::default();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_unix_line(trimmed, line_no) {
            Ok(Some(UnixParsedRow::Listener(record))) => result.listeners.push(record),
            Ok(Some(UnixParsedRow::Connection(record))) => result.connections.push(record),
            Ok(None) => {}
            Err(detail) => result.warnings.push(ParseUnixWarning {
                line: line_no,
                detail,
            }),
        }
    }
    result
}

enum UnixParsedRow {
    Listener(UnixSocketRecord),
    Connection(UnixConnectionRecord),
}

fn parse_unix_line(line: &str, raw_line: usize) -> Result<Option<UnixParsedRow>, String> {
    let mut parts = line.split_whitespace();
    let _slot = parts.next().ok_or_else(|| "missing slot".to_string())?;
    let refcount = parts.next().ok_or_else(|| "missing refcount".to_string())?;
    let _protocol = parts.next().ok_or_else(|| "missing protocol".to_string())?;
    let flags_str = parts.next().ok_or_else(|| "missing flags".to_string())?;
    let type_str = parts.next().ok_or_else(|| "missing type".to_string())?;
    let st_str = parts.next().ok_or_else(|| "missing st".to_string())?;
    let inode_str = parts.next().ok_or_else(|| "missing inode".to_string())?;
    let path = parts.collect::<Vec<_>>().join(" ");

    let _ = refcount;
    let flags = u32::from_str_radix(flags_str, 16).map_err(|e| format!("flags: {e}"))?;
    let socket_type = u32::from_str_radix(type_str, 16).map_err(|e| format!("type: {e}"))?;
    let st = u8::from_str_radix(st_str, 16).map_err(|e| format!("st: {e}"))?;
    if inode_str == "0" {
        return Ok(None);
    }
    let inode = inode_str
        .parse::<u64>()
        .map_err(|e| format!("inode: {e}"))?;

    let base = UnixSocketRecord {
        path: path.clone(),
        flags,
        socket_type,
        st,
        inode,
        raw_line,
    };

    if base.is_listener() {
        if path.is_empty() {
            return Ok(None);
        }
        return Ok(Some(UnixParsedRow::Listener(base)));
    }

    let conn = UnixConnectionRecord {
        path,
        flags,
        socket_type,
        st,
        inode,
        raw_line,
    };
    if conn.is_connected_stream() {
        return Ok(Some(UnixParsedRow::Connection(conn)));
    }
    Ok(None)
}
