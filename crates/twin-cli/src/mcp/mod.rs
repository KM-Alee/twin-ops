//! Model Context Protocol handler (newline-delimited JSON-RPC 2.0), served over stdio by
//! `twin mcp` and over the daemon's Unix socket by `twin daemon run`.
//!
//! Every method is read-only: it reads twin's graph (a local server rescans the host when
//! the graph is stale) and never performs the operations it predicts.

mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use twin_app::TwinLayout;

const SUPPORTED_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "twin is a read-only map of this Linux host: services, processes, \
listening ports, connections, containers, config files, packages and what depends on what. \
Nothing it does changes the host.\n\
Use it instead of chaining systemctl/ss/lsof/docker/journalctl calls:\n\
- twin_overview first, to learn what runs here and what is exposed.\n\
- twin_brief for everything about one service, container, port, file or package.\n\
- twin_preflight BEFORE running any command that restarts, stops, kills, deletes, edits, \
upgrades or firewalls something; follow its verdict (stop = ask the user first).\n\
- twin_changes when something broke: what changed recently around a target.\n\
- twin_why to see the evidence behind a relationship.\n\
- twin_find to discover exact names/ids.\n\
Results separate observed facts from inferences and list coverage gaps; treat gaps as unknowns, \
not as absence.";

/// Every JSON-RPC method the handler answers. Tools are listed by `tools/list`.
pub const METHODS: [&str; 7] = [
    "initialize",
    "ping",
    "tools/list",
    "tools/call",
    "twin/status",
    "twin/brief",
    "twin/preflight",
];

/// Tools that read a file the caller names; the daemon never runs them for a client.
const FILE_READING_TOOLS: [&str; 1] = ["twin_check"];

#[derive(Debug, Clone)]
pub enum ServeMode {
    /// One agent's own server: rescans when the graph is stale, and asks the daemon first
    /// when a socket is given.
    Local { daemon_socket: Option<PathBuf> },
    /// Behind the daemon's socket: answers from the graph the daemon's watch loop keeps
    /// fresh, never scans on a client's behalf and never reads files a client names.
    Daemon,
}

pub struct McpServer {
    layout: TwinLayout,
    config: Option<PathBuf>,
    proc_root: PathBuf,
    max_graph_age_secs: u64,
    mode: ServeMode,
}

impl McpServer {
    pub fn new(
        layout: TwinLayout,
        config: Option<PathBuf>,
        proc_root: PathBuf,
        max_graph_age_secs: u64,
    ) -> Self {
        Self {
            layout,
            config,
            proc_root,
            max_graph_age_secs,
            mode: ServeMode::Local {
                daemon_socket: None,
            },
        }
    }

    pub fn with_mode(mut self, mode: ServeMode) -> Self {
        self.mode = mode;
        self
    }

    fn is_daemon(&self) -> bool {
        matches!(self.mode, ServeMode::Daemon)
    }

    /// The daemon never rescans: its watch loop owns freshness and reports the age.
    fn graph_age_limit(&self) -> u64 {
        if self.is_daemon() {
            u64::MAX
        } else {
            self.max_graph_age_secs
        }
    }

    /// Serve until stdin closes.
    pub fn serve(&self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle_line(&line) {
                writeln!(output, "{response}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One JSON-RPC message in, at most one response out (notifications get none).
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("parse error: {error}"),
                ))
            }
        };
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = id?;
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tool_definitions() })),
            "tools/call" => Ok(self.call(&params)),
            "twin/status" => self.status(),
            "twin/brief" => self.brief(&params),
            "twin/preflight" => self.preflight(&params),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn tool_definitions(&self) -> Vec<Value> {
        let tools = tools::definitions();
        tools
            .as_array()
            .into_iter()
            .flatten()
            .filter(|tool| {
                !self.is_daemon()
                    || !FILE_READING_TOOLS.contains(&tool["name"].as_str().unwrap_or_default())
            })
            .cloned()
            .collect()
    }

    fn call(&self, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(result) = self.forward_to_daemon(name, params) {
            return result;
        }
        if self.is_daemon() && FILE_READING_TOOLS.contains(&name) {
            return tool_error(format!(
                "{name} reads a file you name, and the twin daemon never reads files for clients; run it through a local `twin mcp --no-daemon`"
            ));
        }
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let context = tools::ToolContext {
            layout: &self.layout,
            config: self.config.clone(),
            proc_root: &self.proc_root,
            max_graph_age_secs: self.graph_age_limit(),
            should_read_scripts: !self.is_daemon(),
        };
        match tools::call(&context, name, &arguments) {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(message) => tool_error(message),
        }
    }

    /// A local server hands graph questions to a fresh daemon, which sees the whole host.
    /// Any failure falls back to answering locally.
    fn forward_to_daemon(&self, name: &str, params: &Value) -> Option<Value> {
        let ServeMode::Local {
            daemon_socket: Some(socket),
        } = &self.mode
        else {
            return None;
        };
        if FILE_READING_TOOLS.contains(&name) || reads_files(name, params) {
            return None;
        }
        let mut client = crate::daemon::DaemonClient::connect(socket, DAEMON_TIMEOUT).ok()?;
        let status = client.call("twin/status", &json!({})).ok()?;
        let age = status.get("graph_age_secs").and_then(Value::as_u64)?;
        if age > self.max_graph_age_secs {
            return None;
        }
        client.call("tools/call", params).ok()
    }

    fn status(&self) -> Result<Value, (i64, String)> {
        let age = twin_app::ensure_initialized_in(&self.layout, self.config.as_deref())
            .and_then(|()| {
                twin_app::refresh_graph_in(
                    &self.layout,
                    self.config.as_deref(),
                    self.graph_age_limit(),
                    &self.proc_root,
                )
            })
            .map_err(app_error)?;
        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "graph_age_secs": age,
            "is_daemon": self.is_daemon(),
        }))
    }

    fn brief(&self, params: &Value) -> Result<Value, (i64, String)> {
        let request = twin_app::BriefRequest {
            target: params
                .get("target")
                .and_then(Value::as_str)
                .map(str::to_string),
            config_override: self.config.clone(),
            max_graph_age_secs: self.graph_age_limit(),
            changes_since: params
                .get("since")
                .and_then(Value::as_str)
                .unwrap_or("24h")
                .to_string(),
        };
        let result = twin_app::ensure_initialized_in(&self.layout, self.config.as_deref())
            .and_then(|()| twin_app::brief_in(&self.layout, &request, &self.proc_root))
            .map_err(app_error)?;
        serde_json::to_value(result).map_err(|error| (-32603, error.to_string()))
    }

    fn preflight(&self, params: &Value) -> Result<Value, (i64, String)> {
        let file_changes = match params.get("file_changes") {
            Some(changes) => serde_json::from_value(changes.clone())
                .map_err(|error| (-32602, format!("invalid file_changes: {error}")))?,
            None => Vec::new(),
        };
        let request = twin_app::PreflightRequest {
            command: params
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            file_changes,
            cwd: params.get("cwd").and_then(Value::as_str).map(PathBuf::from),
            config_override: self.config.clone(),
            max_graph_age_secs: self.graph_age_limit(),
            should_read_scripts: !self.is_daemon(),
        };
        let result = twin_app::ensure_initialized_in(&self.layout, self.config.as_deref())
            .and_then(|()| twin_app::preflight_in(&self.layout, &request, &self.proc_root))
            .map_err(app_error)?;
        serde_json::to_value(result).map_err(|error| (-32603, error.to_string()))
    }
}

fn reads_files(name: &str, params: &Value) -> bool {
    name == "twin_preflight"
        && params
            .pointer("/arguments/command")
            .and_then(Value::as_str)
            .is_some_and(twin_app::preflight_reads_files)
}

const DAEMON_TIMEOUT: Duration = Duration::from_secs(10);

fn app_error(error: twin_app::AppError) -> (i64, String) {
    (-32000, error.to_string())
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

fn initialize(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_VERSION);
    let version = if SUPPORTED_VERSIONS.contains(&requested) {
        requested
    } else {
        DEFAULT_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "twin", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

pub(crate) fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}
