use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use serde_json::{json, Value};
use twin_app::{GraphRequest, GraphResult, TwinLayout};
use twin_core::{ChangeCategory, NodeKind};

/// Hard cap on one tool result so a single call never floods an agent's context.
const MAX_TEXT: usize = 24_000;
const DEFAULT_CHANGE_LIMIT: usize = 30;
const MAX_CHANGE_LIMIT: usize = 200;
const MAX_WHY_RELATIONSHIPS: usize = 30;
const FIND_LIMIT_PER_KIND: usize = 10;

pub struct ToolContext<'a> {
    pub layout: &'a TwinLayout,
    pub config: Option<PathBuf>,
    pub proc_root: &'a Path,
    pub max_graph_age_secs: u64,
}

pub fn definitions() -> Value {
    let read_only = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    });
    let tool = |name: &str, title: &str, description: &str, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": read_only,
        })
    };
    json!([
        tool(
            "twin_overview",
            "Host overview",
            "What runs on this Linux host: active/failed services, network-exposed listeners and their owners, the most depended-on services, running containers, and significant recent changes. Call this first when you start working on a machine.",
            json!({
                "type": "object",
                "properties": {
                    "since": { "type": "string", "description": "Change window, e.g. 1h or 24h (default 1h)" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_brief",
            "Briefing on one target",
            "Everything about one service, container, port, file, package or pid in a single call: state, processes, listeners, what it depends on, what depends on it, config files, recent changes, and a predicted verdict for restarting it. Replaces systemctl status + ss + lsof + docker inspect + journalctl digging.",
            json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "e.g. nginx, postgresql.service, redis (container), :5432, port:tcp:0.0.0.0:443, /etc/nginx/nginx.conf, 1234 (pid)" },
                    "since": { "type": "string", "description": "Change window (default 24h)" }
                },
                "required": ["target"],
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_preflight",
            "Predict a command's blast radius",
            "Before you run a shell command that changes this host, ask what it would break. Understands systemctl/service, docker/podman, rm/sed -i/tee/redirection, apt/dnf/pacman/zypper/apk, kill/pkill, kubectl, ufw/iptables and reboot, including sudo, env prefixes, pipelines and bash -c. Returns go / caution / stop with affected services, reasons and safer alternatives. Nothing is executed. On stop, ask the user before proceeding.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The exact shell command you intend to run" },
                    "cwd": { "type": "string", "description": "Working directory for relative paths" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_changes",
            "What changed recently",
            "Chronological operational changes in a window, optionally scoped to a target and its direct dependencies: service restarts, config edits, new/removed listeners, package version changes, container/image changes. Use it to investigate an incident. Temporal association is not causation.",
            json!({
                "type": "object",
                "properties": {
                    "since": { "type": "string", "description": "Window, e.g. 30m, 2h, 1d (default 1h)" },
                    "target": { "type": "string", "description": "Optional service/node to scope to" },
                    "categories": {
                        "type": "array",
                        "items": { "type": "string", "enum": [
                            "process_started", "process_disappeared", "service_restarted", "config_changed",
                            "listening_port_added", "listening_port_disappeared", "outbound_endpoint_added",
                            "container_added", "image_changed", "package_version_changed", "kubernetes_object_changed"
                        ]},
                        "description": "Optional category filter"
                    },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_CHANGE_LIMIT, "description": "Newest entries to return (default 30)" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_why",
            "Evidence behind relationships",
            "Explain why twin believes a target is connected to other nodes: bounded proof chains (config → endpoint → listener → process → service) with observed/inferred class and evidence strength.",
            json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Service name or node id" }
                },
                "required": ["target"],
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_find",
            "Find nodes by name",
            "Search the host graph for services, containers, ports, files and packages whose name contains the query; returns exact ids for the other tools.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Substring to match, e.g. postgres or 5432" },
                    "kind": { "type": "string", "enum": ["service", "container", "port", "file", "package", "process", "image", "directory", "mount", "library", "unix_socket", "cgroup"] }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "twin_check",
            "Run health contracts",
            "Evaluate a twin.yaml contract file (service active, listener owned by service, container healthy, Kubernetes ready, outbound allowlist) against a fresh scan. Missing evidence is reported as unknown, never as pass.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to twin.yaml" },
                    "baseline": { "type": "string", "description": "Optional snapshot name to compare against" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
    ])
}

pub fn call(context: &ToolContext<'_>, name: &str, arguments: &Value) -> Result<String, String> {
    if !definitions()
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
    {
        return Err(format!("unknown tool: {name}"));
    }
    twin_app::ensure_initialized_in(context.layout, context.config.as_deref())
        .map_err(|error| format!("twin could not prepare its store: {error}"))?;
    let text = match name {
        "twin_overview" => brief(context, None, optional(arguments, "since").unwrap_or("1h")),
        "twin_brief" => brief(
            context,
            Some(required(arguments, "target")?),
            optional(arguments, "since").unwrap_or("24h"),
        ),
        "twin_preflight" => preflight(context, arguments),
        "twin_changes" => changes(context, arguments),
        "twin_why" => why(context, required(arguments, "target")?),
        "twin_find" => find(context, arguments),
        "twin_check" => check(context, arguments),
        _ => unreachable!("tool names are validated against definitions() above"),
    }?;
    Ok(bounded(text))
}

fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    optional(arguments, key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing required argument `{key}`"))
}

fn optional<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(Value::as_str)
}

fn to_json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn bounded(text: String) -> String {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let cut = text
        .char_indices()
        .take_while(|(index, _)| *index < MAX_TEXT)
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    format!(
        "{}\n…[truncated {} of {} bytes; narrow the request]",
        &text[..cut],
        text.len() - cut,
        text.len()
    )
}

fn refresh(context: &ToolContext<'_>) -> Result<u64, String> {
    twin_app::refresh_graph_in(
        context.layout,
        context.config.as_deref(),
        context.max_graph_age_secs,
        context.proc_root,
    )
    .map_err(|error| error.to_string())
}

fn brief(context: &ToolContext<'_>, target: Option<&str>, since: &str) -> Result<String, String> {
    let result = twin_app::brief_in(
        context.layout,
        &twin_app::BriefRequest {
            target: target.map(str::to_string),
            config_override: context.config.clone(),
            max_graph_age_secs: context.max_graph_age_secs,
            changes_since: since.to_string(),
        },
        context.proc_root,
    )
    .map_err(|error| error.to_string())?;
    to_json(&result)
}

fn preflight(context: &ToolContext<'_>, arguments: &Value) -> Result<String, String> {
    let result = twin_app::preflight_in(
        context.layout,
        &twin_app::PreflightRequest {
            command: required(arguments, "command")?.to_string(),
            file_changes: Vec::new(),
            cwd: optional(arguments, "cwd").map(PathBuf::from),
            config_override: context.config.clone(),
            max_graph_age_secs: context.max_graph_age_secs,
        },
        context.proc_root,
    )
    .map_err(|error| error.to_string())?;
    let steps = result
        .steps
        .iter()
        .filter(|step| step.is_assessed || step.verdict != twin_app::PreflightVerdict::Go)
        .collect::<Vec<_>>();
    to_json(&json!({
        "verdict": result.verdict,
        "headline": result.headline,
        "steps": steps,
        "coverage_notes": result.coverage_notes,
        "graph_age_secs": result.graph_age_secs,
        "action_performed": false,
    }))
}

fn changes(context: &ToolContext<'_>, arguments: &Value) -> Result<String, String> {
    refresh(context)?;
    let categories = arguments
        .get("categories")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(|value| ChangeCategory::from_str(value).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|limit| (limit as usize).clamp(1, MAX_CHANGE_LIMIT))
        .unwrap_or(DEFAULT_CHANGE_LIMIT);
    let result = twin_app::what_changed_in(
        context.layout,
        twin_app::WhatChangedRequest {
            config_override: context.config.clone(),
            since: optional(arguments, "since").unwrap_or("1h").to_string(),
            verbose: false,
            target_query: optional(arguments, "target").map(str::to_string),
            categories,
            limit,
            max_depth: 2,
        },
    )
    .map_err(|error| error.to_string())?;
    let changes = result
        .operational_changes
        .iter()
        .rev()
        .map(|change| {
            json!({
                "at": change.recorded_at_label(),
                "category": change.category,
                "entity": change.entity.to_string(),
                "observation": change.observation,
                "class": change.class.to_string(),
                "evidence": change.evidence_strength.label(),
            })
        })
        .collect::<Vec<_>>();
    to_json(&json!({
        "since": result.since_label,
        "target": result.target,
        "changes_newest_first": changes,
        "omitted_older": result.omitted_operational_changes,
        "unknowns": result.unknowns,
        "note": "temporal association is not causation",
    }))
}

fn why(context: &ToolContext<'_>, target: &str) -> Result<String, String> {
    refresh(context)?;
    let request =
        twin_app::why_request(target, context.config.clone()).map_err(|error| error.to_string())?;
    let result = twin_app::why_in(context.layout, request).map_err(|error| error.to_string())?;
    let relationships = result
        .relationships
        .iter()
        .take(MAX_WHY_RELATIONSHIPS)
        .map(|relationship| {
            let latest = relationship.observations.last();
            json!({
                "from": relationship.from,
                "kind": relationship.kind,
                "to": relationship.to,
                "class": relationship.class,
                "evidence": format!("{} ({}/100)", relationship.evidence_strength, relationship.evidence_score),
                "latest_observation": latest.map(|observation| json!({
                    "source": observation.source,
                    "kind": observation.kind,
                    "at": observation.timestamp_label(),
                    "seen_times": observation.repeat_count,
                    "facts": observation.facts,
                })),
            })
        })
        .collect::<Vec<_>>();
    let mut unknowns = result.unknowns.clone();
    for relationship in &result.relationships {
        for unknown in &relationship.unknowns {
            if !unknowns.contains(unknown) {
                unknowns.push(unknown.clone());
            }
        }
    }
    to_json(&json!({
        "target": result.target,
        "label": result.label,
        "proof_chains": result.proof_chains,
        "relationships": relationships,
        "omitted_relationships": result.relationships.len().saturating_sub(MAX_WHY_RELATIONSHIPS),
        "unknowns": unknowns,
    }))
}

fn find(context: &ToolContext<'_>, arguments: &Value) -> Result<String, String> {
    refresh(context)?;
    let query = required(arguments, "query")?;
    let kinds = match optional(arguments, "kind") {
        Some(kind) => vec![NodeKind::from_str(kind).map_err(|error| error.to_string())?],
        None => vec![
            NodeKind::Service,
            NodeKind::Container,
            NodeKind::Port,
            NodeKind::File,
            NodeKind::Package,
        ],
    };
    let mut matches = Vec::new();
    for kind in kinds {
        let result = twin_app::graph_in(
            context.layout,
            GraphRequest {
                config_override: context.config.clone(),
                kind: Some(kind),
                label: Some(query.to_string()),
                limit: Some(FIND_LIMIT_PER_KIND),
                ..GraphRequest::default()
            },
        )
        .map_err(|error| error.to_string())?;
        if let GraphResult::List(list) = result {
            matches.extend(list.nodes.into_iter().map(
                |node| json!({ "id": node.id, "label": node.label, "kind": kind.to_string() }),
            ));
        }
    }
    to_json(&json!({ "query": query, "matches": matches }))
}

fn check(context: &ToolContext<'_>, arguments: &Value) -> Result<String, String> {
    let path = PathBuf::from(required(arguments, "path")?);
    refresh(context)?;
    let options = twin_app::TestRunOptions {
        refresh: false,
        max_age_seconds: Some(context.max_graph_age_secs),
        snapshot: None,
        baseline: optional(arguments, "baseline").map(str::to_string),
    };
    let report = twin_app::run_file_with_options(context.layout, &path, &options)
        .map_err(|error| error.to_string())?;
    to_json(&report)
}
