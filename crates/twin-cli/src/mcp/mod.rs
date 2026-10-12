//! Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Every tool is read-only: it reads twin's graph (rescanning the host when the graph
//! is stale) and never performs the operations it predicts.

mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;

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

pub struct McpServer {
    layout: TwinLayout,
    config: Option<PathBuf>,
    proc_root: PathBuf,
    max_graph_age_secs: u64,
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
            "tools/list" => Ok(json!({ "tools": tools::definitions() })),
            "tools/call" => Ok(self.call(&params)),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn call(&self, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let context = tools::ToolContext {
            layout: &self.layout,
            config: self.config.clone(),
            proc_root: &self.proc_root,
            max_graph_age_secs: self.max_graph_age_secs,
        };
        match tools::call(&context, name, &arguments) {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(message) => json!({
                "content": [{ "type": "text", "text": message }],
                "isError": true,
            }),
        }
    }
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

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}
