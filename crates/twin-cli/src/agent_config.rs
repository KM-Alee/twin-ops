use clap::ValueEnum;
use serde::Serialize;
use serde_json::json;

pub const SKILL: &str = include_str!("../assets/SKILL.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentTarget {
    /// Claude Code: MCP server, PreToolUse hook and skill
    ClaudeCode,
    /// OpenAI Codex CLI: MCP server and PreToolUse hook
    Codex,
    /// Cursor: MCP server and beforeShellExecution hook
    Cursor,
    /// Gemini CLI: MCP server and BeforeTool hook
    Gemini,
    /// VS Code (Copilot agent mode): MCP server
    Vscode,
    /// Any agent that reads AGENTS.md and runs shell commands
    Generic,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFile {
    pub path: String,
    pub purpose: String,
    pub format: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConfig {
    pub agent: String,
    pub files: Vec<ConfigFile>,
    pub notes: Vec<String>,
}

fn file(path: &str, purpose: &str, format: &'static str, content: String) -> ConfigFile {
    ConfigFile {
        path: path.to_string(),
        purpose: purpose.to_string(),
        format,
        content,
    }
}

fn pretty(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

fn agents_md(twin: &str) -> String {
    format!(
        "## Host safety (twin)\n\n\
Before running any command that restarts, stops, kills, deletes, edits, upgrades, removes, \
firewalls or reboots something on this machine, run:\n\n\
    {twin} preflight \"<the exact command>\"\n\n\
Exit 0 = go, 3 = caution (read the reasons), 4 = stop (do not run it; ask the user). \
To orient yourself on this machine, run `{twin} brief`. For one service, container or port, \
run `{twin} brief <name>`. Use `{twin} what-changed --since 1h --target <name>` when \
investigating a failure. twin is read-only and never changes the host.\n"
    )
}

/// Configuration snippets only; twin never edits agent configuration files itself.
pub fn render(agent: AgentTarget, twin: &str) -> AgentConfig {
    let mcp_entry = json!({ "command": twin, "args": ["mcp"] });
    let (name, files, notes) = match agent {
        AgentTarget::ClaudeCode => (
            "claude-code",
            vec![
                file(
                    ".mcp.json (project) or `claude mcp add --scope user twin -- <twin> mcp`",
                    "MCP server: twin_overview, twin_brief, twin_preflight, twin_changes, twin_why, twin_find, twin_check",
                    "json",
                    pretty(json!({ "mcpServers": { "twin": mcp_entry } })),
                ),
                file(
                    "~/.claude/settings.json (merge into \"hooks\")",
                    "Checks every shell command and file edit before it runs; stop verdicts ask you to confirm",
                    "json",
                    pretty(json!({ "hooks": { "PreToolUse": [{
                        "matcher": "Bash|Write|Edit|MultiEdit|NotebookEdit",
                        "hooks": [{ "type": "command", "command": format!("{twin} preflight --hook claude"), "timeout": 30 }]
                    }]}})),
                ),
                file(
                    "~/.claude/skills/twin/SKILL.md",
                    "Teaches the agent when to reach for twin",
                    "markdown",
                    SKILL.to_string(),
                ),
            ],
            vec!["Use `--on-stop deny` in the hook command to block instead of asking.".into()],
        ),
        AgentTarget::Codex => (
            "codex",
            vec![
                file(
                    "~/.codex/config.toml",
                    "MCP server",
                    "toml",
                    format!("[mcp_servers.twin]\ncommand = \"{twin}\"\nargs = [\"mcp\"]\n"),
                ),
                file(
                    "~/.codex/hooks.json",
                    "Checks shell commands and apply_patch edits before they run. Codex cannot ask, so stop verdicts block with an explanation",
                    "json",
                    pretty(json!({ "hooks": { "PreToolUse": [{
                        "matcher": "^(Bash|apply_patch)$",
                        "hooks": [{ "type": "command", "command": format!("{twin} preflight --hook codex"), "timeout": 30 }]
                    }]}})),
                ),
                file("AGENTS.md (append)", "Standing instructions", "markdown", agents_md(twin)),
            ],
            vec!["Review and trust the new hook with /hooks in Codex.".into()],
        ),
        AgentTarget::Cursor => (
            "cursor",
            vec![
                file(
                    "~/.cursor/mcp.json",
                    "MCP server",
                    "json",
                    pretty(json!({ "mcpServers": { "twin": mcp_entry } })),
                ),
                file(
                    "~/.cursor/hooks.json",
                    "Checks shell commands before they run; stop verdicts ask you to confirm",
                    "json",
                    pretty(json!({ "version": 1, "hooks": { "beforeShellExecution": [{
                        "command": format!("{twin} preflight --hook cursor"),
                        "timeout": 30
                    }]}})),
                ),
            ],
            vec![
                "Cursor requires an explicit hook answer, so go verdicts reply \"allow\" for that command only.".into(),
                "Cursor offers no hook before its own file edits, so those are not checked in advance; run `twin preflight --file PATH` for sensitive files.".into(),
            ],
        ),
        AgentTarget::Gemini => (
            "gemini",
            vec![file(
                "~/.gemini/settings.json (merge)",
                "MCP server and BeforeTool hook for shell commands and file writes",
                "json",
                pretty(json!({
                    "mcpServers": { "twin": mcp_entry },
                    "hooks": { "BeforeTool": [{
                        "matcher": "run_shell_command|write_file|replace",
                        "hooks": [{ "name": "twin-preflight", "type": "command", "command": format!("{twin} preflight --hook gemini"), "timeout": 30000 }]
                    }]}
                })),
            )],
            vec!["Gemini CLI hooks cannot ask, so stop verdicts block with an explanation.".into()],
        ),
        AgentTarget::Vscode => (
            "vscode",
            vec![file(
                ".vscode/mcp.json",
                "MCP server for Copilot agent mode",
                "json",
                pretty(json!({ "servers": { "twin": { "type": "stdio", "command": twin, "args": ["mcp"] } } })),
            )],
            Vec::new(),
        ),
        AgentTarget::Generic => (
            "generic",
            vec![
                file("AGENTS.md (append)", "Standing instructions for any shell-capable agent", "markdown", agents_md(twin)),
                file(
                    "any MCP client",
                    "stdio MCP server command",
                    "text",
                    format!("{twin} mcp"),
                ),
            ],
            Vec::new(),
        ),
    };
    let mut notes = notes;
    notes.push(
        "twin only prints these snippets; merge them into the files yourself. Run `twin init && twin scan` once first."
            .into(),
    );
    AgentConfig {
        agent: name.to_string(),
        files,
        notes,
    }
}

pub fn render_text(config: &AgentConfig) -> String {
    let mut out = vec![
        format!("twin agent-config · {}", config.agent),
        "═".repeat(20 + config.agent.len()),
        String::new(),
    ];
    for file in &config.files {
        out.push(format!("── {}", file.path));
        out.push(format!("   {}", file.purpose));
        out.push(String::new());
        out.push(file.content.trim_end().to_string());
        out.push(String::new());
    }
    for note in &config.notes {
        out.push(format!("note: {note}"));
    }
    out.join("\n")
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupSummary {
    pub store_created: bool,
    pub processes: usize,
    pub services: usize,
    pub listeners: usize,
    pub hidden_processes: usize,
    pub visibility_tip: Option<String>,
    pub config: AgentConfig,
}

impl SetupSummary {
    pub fn new(
        store_created: bool,
        scan: &twin_app::ScanResult,
        config: AgentConfig,
        twin: &str,
    ) -> Self {
        let hidden = scan.coverage.restricted_processes;
        Self {
            store_created,
            processes: scan.process_count,
            services: scan.service_count,
            listeners: scan.tcp_listener_count + scan.udp_listener_count,
            hidden_processes: hidden,
            visibility_tip: (hidden > 0).then(|| {
                format!(
                    "{hidden} processes are hidden without privileges; for full visibility run: sudo setcap cap_dac_read_search,cap_sys_ptrace+ep {twin}"
                )
            }),
            config,
        }
    }
}

pub fn render_setup(summary: &SetupSummary) -> String {
    use crate::output::format::{Lines, Status};
    let mut out = Lines::new();
    out.title(&format!("twin setup · {}", summary.config.agent));
    out.status_row(
        Status::Ok,
        "store",
        if summary.store_created {
            "created"
        } else {
            "already present"
        },
    );
    out.status_row(
        Status::Ok,
        "first scan",
        &format!(
            "{} processes, {} services, {} listening sockets",
            summary.processes, summary.services, summary.listeners
        ),
    );
    match &summary.visibility_tip {
        Some(tip) => out.status_row(Status::Warn, "visibility", tip),
        None => out.status_row(Status::Ok, "visibility", "all processes readable"),
    }
    out.blank();
    out.section("paste these into your agent's configuration (twin writes none of them)");
    let mut text = out.into_string();
    text.push_str("\n\n");
    text.push_str(&render_text(&summary.config));
    text.push_str("\n\nthen try: twin brief   ·   twin preflight \"<a command you plan to run>\"");
    text
}
