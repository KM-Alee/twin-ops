use serde_json::Value;
use twin_app::{PreflightAffected, PreflightResult, PreflightStep, PreflightVerdict};
use twin_cli::hook::{fail_open, parse_input, response, HookAgent, StopPolicy};

fn result(verdict: PreflightVerdict) -> PreflightResult {
    PreflightResult {
        command: "systemctl stop postgresql".into(),
        verdict,
        headline: "stop postgresql — 1 dependent(s) stay down".into(),
        has_operations: true,
        steps: vec![PreflightStep {
            segment: "systemctl stop postgresql".into(),
            operation: "stop postgresql".into(),
            target: Some("service:postgresql.service".into()),
            verdict,
            is_assessed: true,
            risk: None,
            evidence: None,
            reasons: vec!["1 dependent(s) stay down until it is back: django.service".into()],
            affected: vec![PreflightAffected {
                id: "service:django.service".into(),
                label: "django.service".into(),
                effect: "loses its database".into(),
            }],
            affected_total: 1,
            advice: vec![],
        }],
        graph_age_secs: Some(3),
        coverage_notes: vec![],
        action_performed: false,
    }
}

fn json(text: Option<String>) -> Value {
    serde_json::from_str(&text.expect("hook response")).expect("valid json")
}

#[test]
fn every_agent_input_shape_yields_the_command_and_cwd() {
    let claude = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/srv","tool_input":{"command":"systemctl restart nginx","description":"x"}}"#;
    let gemini = r#"{"tool_name":"run_shell_command","cwd":"/srv","tool_input":{"command":"systemctl restart nginx"}}"#;
    let cursor = r#"{"command":"systemctl restart nginx","cwd":"/srv","sandbox":false}"#;
    let argv = r#"{"tool_input":{"command":["systemctl","restart","my nginx"]}}"#;
    for input in [claude, gemini, cursor] {
        let parsed = parse_input(input).expect("parsed");
        assert_eq!(parsed.command, "systemctl restart nginx");
        assert_eq!(parsed.cwd.as_deref(), Some(std::path::Path::new("/srv")));
    }
    assert_eq!(
        parse_input(argv).expect("argv").command,
        "systemctl restart 'my nginx'"
    );
    assert!(parse_input("not json").is_none());
    assert!(parse_input(r#"{"tool_name":"Read","tool_input":{"file_path":"/x"}}"#).is_none());
}

#[test]
fn go_is_silent_except_where_the_agent_requires_an_answer() {
    let go = result(PreflightVerdict::Go);
    for agent in [HookAgent::Claude, HookAgent::Codex, HookAgent::Gemini] {
        assert!(response(agent, StopPolicy::Ask, &go).is_none());
        assert!(fail_open(agent).is_none());
    }
    assert_eq!(
        json(response(HookAgent::Cursor, StopPolicy::Ask, &go))["permission"],
        "allow"
    );
}

#[test]
fn stop_maps_to_each_agents_confirmation_or_block_shape() {
    let stop = result(PreflightVerdict::Stop);
    let claude = json(response(HookAgent::Claude, StopPolicy::Ask, &stop));
    assert_eq!(claude["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(claude["hookSpecificOutput"]["permissionDecision"], "ask");
    assert!(claude["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .contains("django.service"));
    let claude_deny = json(response(HookAgent::Claude, StopPolicy::Deny, &stop));
    assert_eq!(
        claude_deny["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );

    let codex = json(response(HookAgent::Codex, StopPolicy::Ask, &stop));
    assert_eq!(codex["hookSpecificOutput"]["permissionDecision"], "deny");

    let cursor = json(response(HookAgent::Cursor, StopPolicy::Ask, &stop));
    assert_eq!(cursor["permission"], "ask");
    assert!(cursor["agent_message"].as_str().unwrap().contains("STOP"));

    let gemini = json(response(HookAgent::Gemini, StopPolicy::Ask, &stop));
    assert_eq!(gemini["decision"], "deny");
    assert!(gemini["reason"].as_str().is_some());
}

#[test]
fn caution_and_warn_policy_add_context_without_blocking() {
    let caution = result(PreflightVerdict::Caution);
    let claude = json(response(HookAgent::Claude, StopPolicy::Ask, &caution));
    assert!(claude["hookSpecificOutput"]["permissionDecision"].is_null());
    assert!(claude["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("CAUTION"));
    let warned = json(response(
        HookAgent::Claude,
        StopPolicy::Warn,
        &result(PreflightVerdict::Stop),
    ));
    assert!(warned["hookSpecificOutput"]["permissionDecision"].is_null());
    let cursor = json(response(HookAgent::Cursor, StopPolicy::Ask, &caution));
    assert_eq!(cursor["permission"], "allow");
}

#[test]
fn agent_text_is_bounded() {
    let mut big = result(PreflightVerdict::Stop);
    big.steps[0].reasons = (0..200)
        .map(|i| format!("reason {i} {}", "x".repeat(80)))
        .collect();
    big.steps = std::iter::repeat_n(big.steps[0].clone(), 20).collect();
    let text = twin_cli::output::preflight::render_brief(&big);
    assert!(text.len() <= 1900, "{}", text.len());
}

#[test]
fn agent_file_tools_become_file_changes_not_commands() {
    use twin_app::FileOperation;
    let claude_edit = parse_input(
        r#"{"tool_name":"Edit","cwd":"/srv","tool_input":{"file_path":"/etc/nginx/nginx.conf","old_string":"a","new_string":"b"}}"#,
    )
    .expect("edit");
    assert!(claude_edit.command.is_empty());
    assert_eq!(claude_edit.file_changes.len(), 1);
    assert_eq!(
        claude_edit.file_changes[0].path,
        std::path::Path::new("/etc/nginx/nginx.conf")
    );
    assert_eq!(claude_edit.file_changes[0].operation, FileOperation::Edit);

    let notebook = parse_input(
        r#"{"tool_name":"NotebookEdit","tool_input":{"notebook_path":"/srv/a.ipynb","new_source":"x"}}"#,
    )
    .expect("notebook");
    assert_eq!(
        notebook.file_changes[0].path,
        std::path::Path::new("/srv/a.ipynb")
    );

    let gemini = parse_input(
        r#"{"tool_name":"write_file","tool_input":{"file_path":"/etc/motd","content":"hi"}}"#,
    )
    .expect("gemini");
    assert_eq!(
        gemini.file_changes[0].path,
        std::path::Path::new("/etc/motd")
    );
}

#[test]
fn codex_apply_patch_envelopes_yield_every_touched_file() {
    use twin_app::FileOperation;
    let patch = "*** Begin Patch\n*** Update File: /etc/hosts\n@@\n-a\n+b\n*** Add File: src/new.rs\n+x\n*** Delete File: /etc/old.conf\n*** Update File: a.txt\n*** Move to: b.txt\n*** End Patch";
    let tool = parse_input(
        &serde_json::json!({"tool_name":"apply_patch","tool_input":{"input": patch}}).to_string(),
    )
    .expect("apply_patch");
    let changes = tool
        .file_changes
        .iter()
        .map(|change| (change.path.display().to_string(), change.operation))
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec![
            ("/etc/hosts".to_string(), FileOperation::Edit),
            ("src/new.rs".to_string(), FileOperation::Create),
            ("/etc/old.conf".to_string(), FileOperation::Delete),
            ("a.txt".to_string(), FileOperation::Edit),
            ("b.txt".to_string(), FileOperation::Create),
        ]
    );
    let heredoc = parse_input(
        &serde_json::json!({"tool_name":"Bash","tool_input":{"command": format!("apply_patch <<'EOF'\n{patch}\nEOF")}})
            .to_string(),
    )
    .expect("shell apply_patch");
    assert!(!heredoc.command.is_empty());
    assert_eq!(heredoc.file_changes.len(), 5);
}
