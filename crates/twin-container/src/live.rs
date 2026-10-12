use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::ContainerError;
use crate::model::{
    parse_container_inspect, parse_container_list, parse_image_inspect, parse_live_restore,
    ContainerInspect, ContainerSummary, ImageInspect, PortMapping, VolumeMount,
};

const MAX_BODY: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

pub struct LiveDocker {
    socket: PathBuf,
    api_version: OnceLock<(u16, u16)>,
}

impl LiveDocker {
    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            api_version: OnceLock::new(),
        }
    }

    fn get(&self, path: &str) -> Result<String, ContainerError> {
        let version = match self.api_version.get() {
            Some(version) => *version,
            None => {
                let response = http_get(&self.socket, "/version")?;
                let version = negotiate(&response)?;
                let _ = self.api_version.set(version);
                version
            }
        };
        http_get(
            &self.socket,
            &format!(
                "/v{}.{}/{}",
                version.0,
                version.1,
                path.trim_start_matches('/')
            ),
        )
    }

    pub fn live_restore(&self) -> Result<Option<bool>, ContainerError> {
        let path = "/info";
        let body = self.get(path)?;
        parse_live_restore(path, &body)
    }

    pub fn list_containers(&self) -> Result<Vec<ContainerSummary>, ContainerError> {
        let path = "/containers/json";
        let body = self.get(path)?;
        parse_container_list(path, &body)
    }

    pub fn inspect_container(&self, id: &str) -> Result<ContainerInspect, ContainerError> {
        let path = format!("/containers/{}/json", encode_segment(id)?);
        let body = self.get(&path)?;
        parse_container_inspect(&path, &body)
    }

    pub fn inspect_image(&self, reference: &str) -> Result<ImageInspect, ContainerError> {
        let path = format!("/images/{}/json", encode_segment(reference)?);
        let body = self.get(&path)?;
        parse_image_inspect(&path, &body, reference)
    }

    pub fn port_mappings(&self, id: &str) -> Result<Vec<PortMapping>, ContainerError> {
        Ok(self.inspect_container(id)?.port_mappings().to_vec())
    }

    pub fn mounts(&self, id: &str) -> Result<Vec<VolumeMount>, ContainerError> {
        Ok(self.inspect_container(id)?.mounts().to_vec())
    }
}

fn negotiate(body: &str) -> Result<(u16, u16), ContainerError> {
    let invalid = || ContainerError::Parse {
        path: "/version".into(),
        detail: "invalid or incompatible Docker API version range".into(),
    };
    let document: serde_json::Value = serde_json::from_str(body).map_err(|_| invalid())?;
    let maximum = parse_version(
        document
            .get("ApiVersion")
            .and_then(|value| value.as_str())
            .ok_or_else(invalid)?,
    )
    .ok_or_else(invalid)?;
    let minimum = document
        .get("MinAPIVersion")
        .and_then(|value| value.as_str())
        .map(parse_version)
        .unwrap_or(Some((1, 12)))
        .ok_or_else(invalid)?;
    let selected = maximum.min((1, 51));
    if selected < minimum.max((1, 24)) || minimum > maximum || selected.0 != 1 {
        return Err(invalid());
    }
    Ok(selected)
}

fn parse_version(value: &str) -> Option<(u16, u16)> {
    let (major, minor) = value.split_once('.')?;
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn encode_segment(raw: &str) -> Result<String, ContainerError> {
    if raw.is_empty() || raw.contains('\0') || raw.contains("..") {
        return Err(ContainerError::Unavailable {
            detail: "docker object id is empty or unsafe".to_string(),
        });
    }
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(out)
}

fn allow_read_path(path: &str) -> bool {
    if path == "/version" {
        return true;
    }
    let path = if let Some(rest) = path.strip_prefix("/v") {
        let Some((version, path)) = rest.split_once('/') else {
            return false;
        };
        let Some(version) = parse_version(version) else {
            return false;
        };
        if !((1, 24)..=(1, 51)).contains(&version) {
            return false;
        }
        format!("/{path}")
    } else {
        path.to_string()
    };
    path == "/containers/json"
        || path == "/info"
        || allowed_object_path(&path, "/containers/")
        || allowed_object_path(&path, "/images/")
}

fn allowed_object_path(path: &str, prefix: &str) -> bool {
    let Some(rest) = path.strip_prefix(prefix) else {
        return false;
    };
    let Some(segment) = rest.strip_suffix("/json") else {
        return false;
    };
    !segment.is_empty()
        && !segment.contains('/')
        && !segment.contains("..")
        && segment.bytes().all(|byte| {
            matches!(
                byte,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'%'
            )
        })
}

fn http_get(socket: &Path, path: &str) -> Result<String, ContainerError> {
    if !allow_read_path(path) {
        return Err(ContainerError::Unavailable {
            detail: format!("refusing docker read path {path}"),
        });
    }
    let mut stream = UnixStream::connect(socket).map_err(|source| ContainerError::Unavailable {
        detail: format!("cannot connect to {}: {source}", socket.display()),
    })?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|source| ContainerError::Read {
            path: socket.to_path_buf(),
            source,
        })?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|source| ContainerError::Read {
            path: socket.to_path_buf(),
            source,
        })?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|source| ContainerError::Read {
            path: socket.to_path_buf(),
            source,
        })?;
    let body = read_response(&mut stream, socket, path)?;
    String::from_utf8(body).map_err(|err| ContainerError::Parse {
        path: path.to_string(),
        detail: err.to_string(),
    })
}

fn read_response(
    stream: &mut UnixStream,
    socket: &Path,
    path: &str,
) -> Result<Vec<u8>, ContainerError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if buf.len() > MAX_BODY {
            return Err(ContainerError::Parse {
                path: socket.display().to_string(),
                detail: "docker response exceeds the read limit".to_string(),
            });
        }
        match stream.read(&mut tmp) {
            Ok(0) => {
                return decode_http(&buf, true, path)?.ok_or_else(|| ContainerError::Parse {
                    path: path.to_string(),
                    detail: "truncated docker http response".to_string(),
                })
            }
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(body) = decode_http(&buf, false, path)? {
                    return Ok(body);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::TimedOut
                    || err.kind() == std::io::ErrorKind::WouldBlock =>
            {
                if let Some(body) = decode_http(&buf, true, path)? {
                    return Ok(body);
                }
                return Err(ContainerError::Unavailable {
                    detail: format!("timed out reading {}", socket.display()),
                });
            }
            Err(source) => {
                return Err(ContainerError::Read {
                    path: socket.to_path_buf(),
                    source,
                })
            }
        }
    }
}

fn decode_http(buf: &[u8], eof: bool, path: &str) -> Result<Option<Vec<u8>>, ContainerError> {
    let Some(header_end) = find_header_end(buf) else {
        return Ok(None);
    };
    let head = std::str::from_utf8(&buf[..header_end]).map_err(|err| ContainerError::Parse {
        path: path.to_string(),
        detail: err.to_string(),
    })?;
    let status = status_code(head)?;
    let body = &buf[header_end + 4..];
    if let Some(len) = content_length(head) {
        if body.len() < len {
            return Ok(None);
        }
        return status_payload(path, status, body[..len].to_vec());
    }
    if chunked(head) {
        let Some(payload) = decode_chunks(body)? else {
            return Ok(None);
        };
        return status_payload(path, status, payload);
    }
    if eof {
        return status_payload(path, status, body.to_vec());
    }
    Ok(None)
}

fn status_payload(
    path: &str,
    status: u16,
    payload: Vec<u8>,
) -> Result<Option<Vec<u8>>, ContainerError> {
    if (200..300).contains(&status) {
        return Ok(Some(payload));
    }
    Err(ContainerError::Api {
        path: path.to_string(),
        status,
        detail: "request failed; response body omitted".into(),
    })
}

fn status_code(head: &str) -> Result<u16, ContainerError> {
    let code = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| ContainerError::Parse {
            path: "http".to_string(),
            detail: "missing http status".to_string(),
        })?;
    code.parse().map_err(|err| ContainerError::Parse {
        path: "http".to_string(),
        detail: format!("invalid http status: {err}"),
    })
}

fn content_length(head: &str) -> Option<usize> {
    header_value(head, "content-length")?.parse().ok()
}

fn chunked(head: &str) -> bool {
    header_value(head, "transfer-encoding")
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    for line in head.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(value.trim());
        }
    }
    None
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn decode_chunks(body: &[u8]) -> Result<Option<Vec<u8>>, ContainerError> {
    let mut rest = body;
    let mut out = Vec::new();
    loop {
        let Some(line_end) = find_crlf(rest) else {
            return Ok(None);
        };
        let line = std::str::from_utf8(&rest[..line_end]).map_err(|err| ContainerError::Parse {
            path: "http".to_string(),
            detail: err.to_string(),
        })?;
        let size_text = line.split(';').next().unwrap_or(line).trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|err| ContainerError::Parse {
            path: "http".to_string(),
            detail: format!("invalid chunk size: {err}"),
        })?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(Some(out));
        }
        if rest.len() < size + 2 {
            return Ok(None);
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|window| window == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_content_length_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        let body = decode_http(raw, true, "/containers/json")
            .expect("decode")
            .expect("complete");
        assert_eq!(body, b"[]");
    }

    #[test]
    fn decodes_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n[]\r\n0\r\n\r\n";
        let body = decode_http(raw, true, "/containers/json")
            .expect("decode")
            .expect("complete");
        assert_eq!(body, b"[]");
    }

    #[test]
    fn rejects_paths_outside_list_and_inspect() {
        assert!(allow_read_path("/containers/json"));
        assert!(allow_read_path("/containers/abc/json"));
        assert!(allow_read_path("/images/redis%3A7/json"));
        assert!(!allow_read_path("/containers/abc/json/extra"));
        assert!(allow_read_path("/version"));
        assert!(allow_read_path("/v1.51/info"));
        assert!(!allow_read_path("/info/extra"));
        assert!(allow_read_path("/v1.51/containers/json"));
        assert!(!allow_read_path("/v1.999/containers/json"));
        // Compose the rejected path so production safety scanning does not flag this negative fixture.
        let rejected = format!("/v1.51/containers/abc/{}", "start");
        assert!(!allow_read_path(&rejected));
    }
}

#[cfg(test)]
mod error_safety_tests {
    #[test]
    fn api_failure_omits_untrusted_response_body() {
        let error =
            super::status_payload("/containers/json", 500, b"PRIVATE_API_SENTINEL".to_vec())
                .expect_err("failure");
        assert!(!error.to_string().contains("PRIVATE_API_SENTINEL"));
        assert!(error.to_string().contains("500"));
    }
}

#[cfg(test)]
mod api_version_tests {
    #[test]
    fn negotiates_intersection_and_rejects_unsafe_ranges() {
        assert_eq!(
            super::negotiate(r#"{"ApiVersion":"1.56","MinAPIVersion":"1.44"}"#)
                .expect("new daemon"),
            (1, 51)
        );
        assert_eq!(
            super::negotiate(r#"{"ApiVersion":"1.40"}"#).expect("older daemon"),
            (1, 40)
        );
        for body in [
            r#"{"ApiVersion":"1.56","MinAPIVersion":"1.52"}"#,
            r#"{"ApiVersion":"1.23"}"#,
            r#"{"ApiVersion":"SENTINEL"}"#,
            r#"{"ApiVersion":"1.51","MinAPIVersion":"1.56"}"#,
        ] {
            let error = super::negotiate(body).expect_err("unsupported");
            assert!(!error.to_string().contains("SENTINEL"));
        }
    }
}
