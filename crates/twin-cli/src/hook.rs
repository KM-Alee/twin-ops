use std::path::PathBuf;

use clap::ValueEnum;
use serde_json::{json, Value};
use twin_app::{FileChange, FileOperation, PreflightResult, PreflightVerdict};

use crate::output::preflight::render_brief;

/// Agent hook dialects. Each agent sends the pending shell command or file change on
/// stdin and expects its own response shape; twin never answers "allow" on an agent's
/// behalf where the protocol lets it stay silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookAgent {
    /// Claude Code PreToolUse (Bash and file-edit tools)
    Claude,
    /// OpenAI Codex CLI PreToolUse (Bash and apply_patch)
    Codex,
    /// Cursor beforeShellExecution
    Cursor,
    /// Gemini CLI BeforeTool (run_shell_command, write_file, replace)
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StopPolicy {
    /// Ask the human to confirm (falls back to deny where the agent cannot ask)
    Ask,
    /// Block the command and tell the agent why
    Deny,
    /// Never block; only add context for the agent
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInput {
    pub command: String,
    pub file_changes: Vec<FileChange>,
    pub cwd: Option<PathBuf>,
}

/// Agent tools that change a file. Read-only tools such as Read or read_file also carry
/// a path and must never be mistaken for edits.
const FILE_EDIT_TOOLS: [&str; 6] = [
    "Write",
    "Edit",
    "MultiEdit",
    "NotebookEdit",
    "write_file",
    "replace",
];

/// Field names agents use for the file a file-editing tool targets.
const FILE_PATH_FIELDS: [&str; 3] = ["file_path", "notebook_path", "path"];

pub fn parse_input(stdin: &str) -> Option<HookInput> {
    let value: Value = serde_json::from_str(stdin).ok()?;
    let tool_input = value.get("tool_input");
    let command = tool_input
        .and_then(|input| input.get("command"))
        .or_else(|| value.get("command"))
        .and_then(command_text)
        .unwrap_or_default();
    let tool_name = value.get("tool_name").and_then(Value::as_str);
    let mut file_changes = Vec::new();
    if let Some(input) = tool_input {
        collect_patch_changes(input, &mut file_changes);
        let is_edit_tool = tool_name.is_some_and(|name| FILE_EDIT_TOOLS.contains(&name));
        if is_edit_tool && command.trim().is_empty() && file_changes.is_empty() {
            if let Some(path) = FILE_PATH_FIELDS
                .iter()
                .find_map(|field| input.get(field).and_then(Value::as_str))
                .filter(|path| !path.is_empty())
            {
                file_changes.push(FileChange {
                    path: PathBuf::from(path),
                    operation: FileOperation::Edit,
                });
            }
        }
    }
    if tool_input.is_none() && command.contains("*** Begin Patch") {
        file_changes.extend(patch_changes(&command));
    }
    if command.trim().is_empty() && file_changes.is_empty() {
        return None;
    }
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !cwd.is_empty())
        .map(PathBuf::from);
    Some(HookInput {
        command,
        file_changes,
        cwd,
    })
}

/// Codex `apply_patch` sends a patch envelope; its field name has varied, so every
/// string value in the tool input is checked for envelope headers.
fn collect_patch_changes(value: &Value, out: &mut Vec<FileChange>) {
    match value {
        Value::String(text) if text.contains("*** Begin Patch") => {
            out.extend(patch_changes(text));
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_patch_changes(item, out)),
        Value::Object(map) => map
            .values()
            .for_each(|item| collect_patch_changes(item, out)),
        _ => {}
    }
}

fn patch_changes(patch: &str) -> Vec<FileChange> {
    let mut changes: Vec<FileChange> = Vec::new();
    for line in patch.lines() {
        let line = line.trim_start();
        let parsed = [
            ("*** Add File: ", FileOperation::Create),
            ("*** Update File: ", FileOperation::Edit),
            ("*** Delete File: ", FileOperation::Delete),
            ("*** Move to: ", FileOperation::Create),
        ]
        .into_iter()
        .find_map(|(header, operation)| {
            line.strip_prefix(header)
                .map(|path| (path.trim(), operation))
        });
        if let Some((path, operation)) = parsed.filter(|(path, _)| !path.is_empty()) {
            let change = FileChange {
                path: PathBuf::from(path),
                operation,
            };
            if !changes.contains(&change) {
                changes.push(change);
            }
        }
    }
    changes
}

fn command_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let words = parts
                .iter()
                .map(|part| part.as_str().map(shell_quote))
                .collect::<Option<Vec<_>>>()?;
            Some(words.join(" "))
        }
        _ => None,
    }
}

fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The JSON the agent expects on stdout, or `None` when twin has no opinion and the
/// agent should continue through its normal permission flow.
pub fn response(agent: HookAgent, policy: StopPolicy, result: &PreflightResult) -> Option<String> {
    let verdict = match (result.verdict, policy) {
        (PreflightVerdict::Stop, StopPolicy::Warn) => PreflightVerdict::Caution,
        (verdict, _) => verdict,
    };
    let text = render_brief(result);
    let value = match (agent, verdict) {
        (HookAgent::Cursor, PreflightVerdict::Go) => json!({ "permission": "allow" }),
        (HookAgent::Cursor, PreflightVerdict::Caution) => {
            json!({ "permission": "allow", "agent_message": text })
        }
        (HookAgent::Cursor, PreflightVerdict::Stop) => json!({
            "permission": if policy == StopPolicy::Deny { "deny" } else { "ask" },
            "user_message": text,
            "agent_message": text,
        }),
        (_, PreflightVerdict::Go) => return None,
        (HookAgent::Claude | HookAgent::Codex, PreflightVerdict::Caution) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": text,
            }
        }),
        (HookAgent::Claude, PreflightVerdict::Stop) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": if policy == StopPolicy::Deny { "deny" } else { "ask" },
                "permissionDecisionReason": text,
            }
        }),
        (HookAgent::Codex, PreflightVerdict::Stop) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": format!(
                    "{text}\nAsk the user to confirm before running this command."
                ),
            }
        }),
        (HookAgent::Gemini, PreflightVerdict::Caution) => json!({ "systemMessage": text }),
        (HookAgent::Gemini, PreflightVerdict::Stop) => json!({
            "decision": "deny",
            "reason": format!("{text}\nAsk the user to confirm before running this command."),
            "systemMessage": text,
        }),
    };
    Some(value.to_string())
}

/// Response for a hook call twin could not evaluate. Hooks fail open: an unavailable
/// twin must never block an agent's unrelated work.
pub fn fail_open(agent: HookAgent) -> Option<String> {
    match agent {
        HookAgent::Cursor => Some(json!({ "permission": "allow" }).to_string()),
        _ => None,
    }
}
