use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

/// Why a client could not use the daemon. The text becomes a coverage note on the local
/// answer that replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonUnavailable {
    reason: String,
}

impl DaemonUnavailable {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The note a fallback answer carries.
    pub fn coverage_note(&self) -> String {
        format!(
            "twin daemon not used ({}); answered from a local scan, which cannot see processes other users own",
            self.reason
        )
    }
}

impl std::fmt::Display for DaemonUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

/// One connection to the daemon's socket; several calls may share it.
pub struct DaemonClient {
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl DaemonClient {
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Self, DaemonUnavailable> {
        let stream = UnixStream::connect(socket).map_err(|error| {
            DaemonUnavailable::new(match error.kind() {
                ErrorKind::NotFound => format!("no socket at {}", socket.display()),
                ErrorKind::ConnectionRefused => format!("not running at {}", socket.display()),
                ErrorKind::PermissionDenied => format!(
                    "no permission for {}; join group twin to use it",
                    socket.display()
                ),
                _ => format!("cannot connect to {}: {error}", socket.display()),
            })
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| DaemonUnavailable::new(format!("socket setup failed: {error}")))?;
        Ok(Self {
            reader: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Sends one request and returns its `result`, or the daemon's error as the reason.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, DaemonUnavailable> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stream = self.reader.get_mut();
        writeln!(stream, "{request}")
            .and_then(|()| stream.flush())
            .map_err(|error| DaemonUnavailable::new(format!("request failed: {error}")))?;
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => return Err(DaemonUnavailable::new("the daemon closed the connection")),
            Ok(_) => {}
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(DaemonUnavailable::new("the daemon did not answer in time"))
            }
            Err(error) => return Err(DaemonUnavailable::new(format!("read failed: {error}"))),
        }
        let mut response: Value = serde_json::from_str(&line)
            .map_err(|error| DaemonUnavailable::new(format!("unreadable answer: {error}")))?;
        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(DaemonUnavailable::new(format!(
                "the daemon said: {message}"
            )));
        }
        if response.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(DaemonUnavailable::new("answer for another request"));
        }
        Ok(response["result"].take())
    }

    /// Calls a method whose result is a typed twin result.
    pub fn call_typed<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<T, DaemonUnavailable> {
        let value = self.call(method, params)?;
        serde_json::from_value(value)
            .map_err(|error| DaemonUnavailable::new(format!("unexpected answer: {error}")))
    }
}
