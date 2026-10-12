mod support;

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use support::{stderr_utf8, stdout_utf8, twin_bin, TwinHome};

fn scanned_home() -> TwinHome {
    let home = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&home.proc_root);
    let init = home.run(&["init"]);
    assert!(init.status.success(), "init: {}", stderr_utf8(&init));
    let scan = home.run(&["scan"]);
    assert!(scan.status.success(), "scan: {}", stderr_utf8(&scan));
    home
}

fn run_with_stdin(home: &TwinHome, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(twin_bin())
        .env("HOME", home.home_path())
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .env("TWIN_PROC_ROOT", &home.proc_root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn twin");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("twin output")
}

#[test]
fn preflight_exit_codes_distinguish_go_caution_and_stop() {
    let home = scanned_home();
    let go = home.run(&["preflight", "ls -la"]);
    assert_eq!(go.status.code(), Some(0), "{}", stderr_utf8(&go));
    let stop = home.run(&[
        "preflight",
        "--max-age",
        "3600",
        "systemctl stop postgresql",
    ]);
    assert_eq!(stop.status.code(), Some(4), "{}", stdout_utf8(&stop));
    let text = stdout_utf8(&stop);
    for expected in [
        "twin preflight",
        "STOP",
        "django.service",
        "Nothing was executed",
    ] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
    let caution = home.run(&[
        "preflight",
        "--",
        "sed",
        "-i",
        "s/a/b/",
        "/etc/untracked.conf",
    ]);
    assert_eq!(caution.status.code(), Some(3), "{}", stdout_utf8(&caution));
    let json_out = home.run(&[
        "--json",
        "preflight",
        "--max-age",
        "3600",
        "systemctl stop postgresql",
    ]);
    let value: Value = serde_json::from_slice(&json_out.stdout).expect("json");
    assert_eq!(value["verdict"], "stop");
    assert_eq!(value["action_performed"], false);
}

#[test]
fn hook_mode_answers_in_the_agents_protocol_and_fails_open() {
    let home = scanned_home();
    let stop = run_with_stdin(
        &home,
        &["preflight", "--hook", "claude", "--max-age", "3600"],
        &json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/tmp",
                "tool_input":{"command":"sudo systemctl stop postgresql"}})
        .to_string(),
    );
    assert_eq!(stop.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&stop.stdout).expect("hook json");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");

    let go = run_with_stdin(
        &home,
        &["preflight", "--hook", "claude"],
        r#"{"tool_name":"Bash","tool_input":{"command":"cargo test"}}"#,
    );
    assert_eq!(go.status.code(), Some(0));
    assert!(go.stdout.is_empty());

    let garbage = run_with_stdin(&home, &["preflight", "--hook", "cursor"], "not json");
    assert_eq!(garbage.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&garbage.stdout).expect("cursor json");
    assert_eq!(value["permission"], "allow");
}

#[test]
fn mcp_server_handshakes_lists_read_only_tools_and_answers_calls() {
    let home = scanned_home();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"twin_brief","arguments":{"target":"postgresql"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"twin_preflight","arguments":{"command":"systemctl stop postgresql"}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"twin_find","arguments":{"query":"django"}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"twin_brief","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":7,"method":"no/such"}),
    ];
    let input = messages
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let output = run_with_stdin(&home, &["mcp", "--max-age", "3600"], &input);
    assert!(output.status.success(), "{}", stderr_utf8(&output));
    let responses = stdout_utf8(&output)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("json-rpc line"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 7, "notifications get no response");
    let by_id = |id: i64| {
        responses
            .iter()
            .find(|response| response["id"] == id)
            .unwrap_or_else(|| panic!("no response {id}"))
    };
    assert_eq!(by_id(1)["result"]["serverInfo"]["name"], "twin");
    assert_eq!(by_id(1)["result"]["protocolVersion"], "2025-06-18");
    let tools = by_id(2)["result"]["tools"].as_array().expect("tools");
    assert!(tools.len() >= 6);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["readOnlyHint"] == true
            && tool["annotations"]["destructiveHint"] == false));
    let text = |id: i64| {
        by_id(id)["result"]["content"][0]["text"]
            .as_str()
            .expect("text content")
            .to_string()
    };
    let brief: Value = serde_json::from_str(&text(3)).expect("brief json");
    assert_eq!(brief["target"], "service:postgresql.service");
    let preflight: Value = serde_json::from_str(&text(4)).expect("preflight json");
    assert_eq!(preflight["verdict"], "stop");
    assert!(text(5).contains("service:django.service"));
    assert_eq!(by_id(6)["result"]["isError"], true);
    assert_eq!(by_id(7)["error"]["code"], -32601);
}

#[test]
fn mcp_initializes_its_own_store_on_first_use() {
    let home = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&home.proc_root);
    let input = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"twin_overview","arguments":{}}}).to_string();
    let output = run_with_stdin(&home, &["mcp"], &input);
    let response: Value = serde_json::from_str(stdout_utf8(&output).trim()).expect("response");
    assert!(response["result"]["isError"].is_null(), "{response}");
    assert!(home.home_path().join(".local/share/twin/twin.db").exists());
}

#[test]
fn agent_config_snippets_are_valid_for_every_agent_and_embed_the_binary_path() {
    let home = TwinHome::new();
    for agent in [
        "claude-code",
        "codex",
        "cursor",
        "gemini",
        "vscode",
        "generic",
    ] {
        let output = home.run(&["--json", "agent-config", agent, "--twin-path", "/opt/twin"]);
        assert!(output.status.success(), "{agent}: {}", stderr_utf8(&output));
        let config: Value = serde_json::from_slice(&output.stdout).expect("agent config json");
        let files = config["files"].as_array().expect("files");
        assert!(!files.is_empty(), "{agent}");
        for file in files {
            let content = file["content"].as_str().expect("content");
            if file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("SKILL.md"))
            {
                continue;
            }
            assert!(content.contains("/opt/twin"), "{agent}: {content}");
            match file["format"].as_str() {
                Some("json") => {
                    serde_json::from_str::<Value>(content)
                        .unwrap_or_else(|error| panic!("{agent} json: {error}"));
                }
                Some("toml") => assert!(content.contains("[mcp_servers.twin]")),
                _ => {}
            }
        }
    }
    let skill = home.run(&["agent-config", "claude-code"]);
    assert!(stdout_utf8(&skill).contains("name: twin"));
}

#[test]
fn setup_creates_the_store_scans_and_prints_snippets_and_commands_auto_initialize() {
    let home = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&home.proc_root);
    let output = home.run(&["setup", "codex", "--twin-path", "/opt/twin"]);
    assert!(output.status.success(), "{}", stderr_utf8(&output));
    let text = stdout_utf8(&output);
    for expected in [
        "twin setup · codex",
        "store  created",
        "first scan",
        "[mcp_servers.twin]",
        "/opt/twin preflight --hook codex",
        "twin writes none of them",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    let again = home.run(&["--json", "setup", "codex"]);
    let value: Value = serde_json::from_slice(&again.stdout).expect("setup json");
    assert_eq!(value["store_created"], false);

    let fresh = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&fresh.proc_root);
    let preflight = fresh.run(&["preflight", "systemctl stop postgresql"]);
    assert_eq!(
        preflight.status.code(),
        Some(4),
        "{}",
        stderr_utf8(&preflight)
    );
    let brief = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&brief.proc_root);
    assert!(brief.run(&["brief"]).status.success());
}
