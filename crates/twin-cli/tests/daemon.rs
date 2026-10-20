mod support;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use serde_json::{json, Value};
use support::{stderr_utf8, stdout_utf8, TwinHome};
use twin_app::daemon::AccessPolicy;
use twin_app::TwinLayout;
use twin_cli::daemon::{DaemonServer, DaemonSocket};
use twin_cli::mcp::{McpServer, ServeMode, METHODS};

/// A home whose store holds the postgres/django fixture, scanned by the real binary.
fn scanned_home() -> TwinHome {
    let home = TwinHome::new();
    support::write_proc_fixture_with_active_connection(&home.proc_root);
    let init = home.run(&["init"]);
    assert!(init.status.success(), "init: {}", stderr_utf8(&init));
    let scan = home.run(&["scan"]);
    assert!(scan.status.success(), "scan: {}", stderr_utf8(&scan));
    home
}

fn layout(home: &TwinHome) -> TwinLayout {
    TwinLayout {
        data_dir: home.home_path().join(".local/share/twin"),
        config_dir: home.home_path().join(".config/twin"),
        state_dir: home.home_path().join(".local/state/twin"),
    }
}

struct RunningDaemon {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl RunningDaemon {
    fn start(home: &TwinHome, policy: AccessPolicy) -> Self {
        let dir = tempfile::TempDir::new().expect("socket dir");
        let socket = dir.path().join("twin.sock");
        let bound = DaemonSocket::bind(&socket).expect("bind");
        let server = DaemonServer::new(
            McpServer::new(layout(home), None, home.proc_root.clone(), 60),
            policy,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            server.serve(&bound, &flag).expect("serve");
        });
        Self {
            socket,
            stop,
            thread: Some(thread),
            _dir: dir,
        }
    }

    fn own(home: &TwinHome) -> Self {
        Self::start(home, AccessPolicy::current().expect("own identity"))
    }

    fn connect(&self) -> Session {
        let stream = UnixStream::connect(&self.socket).expect("connect");
        Session {
            reader: BufReader::new(stream.try_clone().expect("clone")),
            writer: stream,
            next_id: 1,
        }
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        twin_cli::daemon::wake(&self.socket);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("server thread");
        }
    }
}

struct Session {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Session {
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(
            self.writer,
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
        )
        .expect("send");
        self.read()
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("answer");
        serde_json::from_str(&line).expect("json answer")
    }
}

/// Every byte twin keeps for this home: a write anywhere in the store changes this.
fn store_bytes(home: &TwinHome) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for directory in [".local/share/twin", ".config/twin", ".local/state/twin"] {
        let Ok(entries) = std::fs::read_dir(home.home_path().join(directory)) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_file() {
                files.push((entry.path(), std::fs::read(entry.path()).expect("read")));
            }
        }
    }
    files.sort();
    files
}

fn is_root() -> bool {
    AccessPolicy::current().expect("identity").uid() == 0
}

fn tool_arguments(name: &str) -> Value {
    match name {
        "twin_overview" | "twin_changes" => json!({}),
        "twin_brief" => json!({ "target": "postgresql" }),
        "twin_preflight" => json!({ "command": "systemctl restart postgresql" }),
        "twin_why" => json!({ "target": "postgresql.service" }),
        "twin_find" => json!({ "query": "postgres" }),
        other => panic!("{other} has no read-only coverage in this test; add its arguments"),
    }
}

#[test]
fn every_handler_method_and_tool_leaves_the_store_untouched() {
    let home = scanned_home();
    let daemon = RunningDaemon::own(&home);
    let before = store_bytes(&home);
    let mut session = daemon.connect();

    let mut called = vec!["initialize", "ping", "tools/list", "tools/call"];
    assert!(session.request("initialize", json!({}))["result"]["serverInfo"].is_object());
    assert_eq!(session.request("ping", json!({}))["result"], json!({}));
    let tools = session.request("tools/list", json!({}))["result"]["tools"].clone();
    let tools = tools.as_array().expect("tools");
    assert!(!tools.is_empty());
    for tool in tools {
        let name = tool["name"].as_str().expect("name");
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
        assert_ne!(
            name, "twin_check",
            "the daemon must not list file-reading tools"
        );
        let answer = session.request(
            "tools/call",
            json!({ "name": name, "arguments": tool_arguments(name) }),
        );
        assert_ne!(answer["result"]["isError"], true, "{name}: {answer}");
    }

    let status = session.request("twin/status", json!({}));
    assert_eq!(status["result"]["is_daemon"], true);
    called.push("twin/status");
    let brief = session.request("twin/brief", json!({ "target": "postgresql" }));
    assert!(brief["result"]["title"].as_str().is_some(), "{brief}");
    called.push("twin/brief");
    let preflight = session.request(
        "twin/preflight",
        json!({ "command": "systemctl restart postgresql" }),
    );
    assert_eq!(preflight["result"]["action_performed"], false);
    called.push("twin/preflight");

    called.sort();
    let mut methods = METHODS.to_vec();
    methods.sort();
    assert_eq!(
        called, methods,
        "every handler method must be exercised here"
    );
    assert_eq!(
        session.request("tools/delete", json!({}))["error"]["code"],
        -32601
    );
    drop(session);
    assert_eq!(
        store_bytes(&home),
        before,
        "a daemon query wrote to twin's store"
    );
}

#[test]
fn file_reading_tools_and_scripts_are_never_read_for_a_client() {
    let home = scanned_home();
    let daemon = RunningDaemon::own(&home);
    let contract = home.home_path().join("twin.yaml");
    std::fs::write(&contract, "checks: []\n").expect("contract");
    let script = home.home_path().join("deploy.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nexport TOKEN=SENTINEL_SCRIPT_TOKEN\nsystemctl restart postgresql\n",
    )
    .expect("script");
    let mut session = daemon.connect();

    let check = session.request(
        "tools/call",
        json!({ "name": "twin_check", "arguments": { "path": contract } }),
    );
    assert_eq!(check["result"]["isError"], true, "{check}");
    assert!(check["result"]["content"][0]["text"]
        .as_str()
        .expect("text")
        .contains("never reads files for clients"));

    let preflight = session.request(
        "twin/preflight",
        json!({ "command": format!("sh {}", script.display()) }),
    );
    let text = preflight.to_string();
    assert!(!text.contains("SENTINEL_SCRIPT_TOKEN"), "{text}");
    assert!(!text.contains("restart postgresql"), "{text}");
    assert_eq!(preflight["result"]["verdict"], "caution", "{text}");
    assert!(text.contains("does not read files for clients"), "{text}");

    let request = twin_app::PreflightRequest {
        command: format!("sh {}", script.display()),
        file_changes: Vec::new(),
        cwd: None,
        config_override: None,
        max_graph_age_secs: 3600,
        should_read_scripts: true,
    };
    let refused = twin_cli::daemon::preflight(&daemon.socket, &request).expect_err("refused");
    assert!(refused.reason().contains("script"), "{refused}");
}

#[test]
fn a_uid_outside_the_policy_is_refused() {
    if is_root() {
        eprintln!("skipped: root is always allowed, so refusal needs an unprivileged run");
        return;
    }
    let home = scanned_home();
    let daemon = RunningDaemon::start(&home, AccessPolicy::new(u32::MAX - 7, u32::MAX - 7));
    let mut session = daemon.connect();
    let refusal = session.read();
    assert_eq!(refusal["error"]["code"], -32001, "{refusal}");
    assert!(refusal["error"]["message"]
        .as_str()
        .expect("message")
        .contains("may not query the twin daemon"));
    let status = twin_cli::daemon::status(&daemon.socket);
    assert!(!status.is_reachable);
}

#[test]
fn clients_answer_from_the_daemon_and_fall_back_with_a_note() {
    let served = scanned_home();
    let daemon = RunningDaemon::own(&served);
    let client = TwinHome::new();
    let socket = daemon.socket.display().to_string();

    let through = client.run(&["brief", "--daemon-socket", &socket, "postgresql"]);
    assert!(through.status.success(), "{}", stderr_utf8(&through));
    let text = stdout_utf8(&through);
    assert!(text.contains("postgresql.service"), "{text}");
    assert!(!text.contains("twin daemon not used"), "{text}");
    assert!(
        !client.home_path().join(".local/share/twin").exists(),
        "a daemon answer must not create a local store"
    );

    let preflight = client.run(&[
        "preflight",
        "--daemon-socket",
        &socket,
        "systemctl stop postgresql",
    ]);
    assert_eq!(
        preflight.status.code(),
        Some(4),
        "{}",
        stdout_utf8(&preflight)
    );
    assert!(stdout_utf8(&preflight).contains("django.service"));

    let absent = client.home_path().join("absent.sock").display().to_string();
    let fallback = client.run(&["brief", "--daemon-socket", &absent]);
    assert!(fallback.status.success(), "{}", stderr_utf8(&fallback));
    let text = stdout_utf8(&fallback);
    assert!(
        text.contains("twin daemon not used (no socket at"),
        "{text}"
    );
}

#[test]
fn a_stale_daemon_graph_is_not_used() {
    let home = scanned_home();
    let daemon = RunningDaemon::own(&home);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let request = twin_app::BriefRequest {
        target: None,
        config_override: None,
        max_graph_age_secs: 0,
        changes_since: "1h".to_string(),
    };
    let stale = twin_cli::daemon::brief(&daemon.socket, &request).expect_err("stale");
    assert!(stale.reason().contains("past the 0 s limit"), "{stale}");
    assert!(stale.coverage_note().contains("answered from a local scan"));
}

#[test]
fn a_local_mcp_server_forwards_graph_questions_to_the_daemon() {
    let served = scanned_home();
    let daemon = RunningDaemon::own(&served);
    let empty = TwinHome::new();
    let local = McpServer::new(layout(&empty), None, empty.proc_root.clone(), 3600).with_mode(
        ServeMode::Local {
            daemon_socket: Some(daemon.socket.clone()),
        },
    );
    let answer = local
        .handle_line(
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "twin_find", "arguments": { "query": "postgres" } }
            })
            .to_string(),
        )
        .expect("answer");
    assert!(answer.contains("service:postgresql.service"), "{answer}");
    assert!(
        !empty.home_path().join(".local/share/twin").exists(),
        "the forwarded call must not touch the local store"
    );
}

#[test]
fn binding_replaces_a_dead_socket_but_never_a_live_one_or_a_file() {
    let dir = tempfile::TempDir::new().expect("dir");
    let path = dir.path().join("twin.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).expect("leftover"));
    assert!(path.exists());
    let socket = DaemonSocket::bind(&path).expect("replaces the dead socket");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path)
            .expect("socket")
            .permissions()
            .mode()
            & 0o777,
        0o660
    );
    let live = DaemonSocket::bind(&path).err().expect("live socket kept");
    assert!(live.to_string().contains("already serving"), "{live}");
    drop(socket);
    assert!(!path.exists(), "the socket is removed on shutdown");

    let file = dir.path().join("not-a-socket");
    std::fs::write(&file, "data").expect("file");
    assert!(DaemonSocket::bind(&file).is_err());
    assert_eq!(std::fs::read(&file).expect("kept"), b"data");
}

#[test]
fn install_unit_only_prints_a_hardened_unit() {
    let home = TwinHome::new();
    let refused = home.run(&["daemon", "install-unit"]);
    assert!(!refused.status.success(), "install-unit without --print");
    let printed = home.run(&["daemon", "install-unit", "--print"]);
    assert!(printed.status.success(), "{}", stderr_utf8(&printed));
    let unit = stdout_utf8(&printed);
    for directive in [
        "ExecStart=/usr/bin/twin daemon run --state-dir /var/lib/twin --socket /run/twin/twin.sock",
        "Group=twin",
        "RuntimeDirectory=twin",
        "CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_BPF CAP_PERFMON",
        "NoNewPrivileges=yes",
        "ProtectSystem=strict",
        "RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6",
        "SystemCallFilter=@system-service bpf perf_event_open",
    ] {
        assert!(unit.contains(directive), "missing {directive}:\n{unit}");
    }
    assert!(!unit.contains("CAP_SYS_ADMIN") && !unit.contains("CAP_DAC_OVERRIDE"));
    exposure_is_below_three(&home, &unit);
}

/// `systemd-analyze security --offline` scores the unit without loading it. Hosts without
/// a systemd new enough to analyze offline skip only this part.
fn exposure_is_below_three(home: &TwinHome, unit: &str) {
    let path = home.home_path().join("twin.service");
    std::fs::write(&path, unit).expect("unit file");
    let Ok(output) = std::process::Command::new("systemd-analyze")
        .args(["security", "--offline=true", "--no-pager"])
        .arg(&path)
        .output()
    else {
        eprintln!("skipped exposure check: systemd-analyze not installed");
        return;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(score) = exposure_score(&text) else {
        eprintln!("skipped exposure check: systemd-analyze cannot analyze offline here");
        return;
    };
    assert!(score < 3.0, "exposure {score}:\n{text}");
}

fn exposure_score(text: &str) -> Option<f64> {
    let line = text
        .lines()
        .find_map(|line| line.split_once("Overall exposure level for "))?
        .1;
    line.split_once(": ")?
        .1
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[test]
fn status_reports_an_absent_daemon() {
    let home = TwinHome::new();
    let socket: &Path = &home.home_path().join("none.sock");
    let output = home.run(&[
        "daemon",
        "status",
        "--socket",
        &socket.display().to_string(),
    ]);
    assert_eq!(output.status.code(), Some(3));
    let text = stdout_utf8(&output);
    assert!(
        text.contains("not available") && text.contains("no socket at"),
        "{text}"
    );
}
