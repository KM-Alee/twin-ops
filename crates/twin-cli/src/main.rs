use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{CommandFactory, Parser};
use twin_app::{
    AppError, GraphError, InitRequest, ScanRequest, WatchCommand, WatchExecEvent, WatchSink,
    WatchTcpEvent, WatchTick,
};
use twin_cli::cli::args::{
    AgentConfigArgs, BriefArgs, Cli, Command, DaemonAction, DaemonArgs, DiffArgs, DoctorArgs,
    EmulateActionArgs, EmulateArgs, GlobalArgs, GraphArgs, ImpactArgs, InitArgs, K8sAction,
    McpArgs, PreflightArgs, ScanArgs, SetupArgs, SnapshotActionArgs, SnapshotArgs, TestAction,
    TestArgs, WatchArgs, WhatChangedArgs,
};
use twin_cli::cli::diff;
use twin_cli::cli::emulate;
use twin_cli::cli::graph;
use twin_cli::cli::impact;
use twin_cli::cli::snapshot;
use twin_cli::cli::what_changed;
use twin_cli::output;
use twin_cli::output::format::ScanFreshness;
use twin_cli::output::json::JsonMode;

fn main() {
    let cli = Cli::parse();
    if let Some(root) = &cli.global.host_root {
        if !root.is_absolute() || !root.join("proc").is_dir() {
            eprintln!(
                "Error: --host-root {} must be an absolute directory holding the host's proc",
                root.display()
            );
            process::exit(1);
        }
        // Set once before any thread starts; every host read resolves through it.
        std::env::set_var("TWIN_HOST_ROOT", root);
    }
    process::exit(dispatch(cli));
}

fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        Command::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "twin", &mut script);
            use std::io::Write;
            match std::io::stdout().write_all(&script) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        Command::Init(args) => run_init(&cli.global, &args),
        Command::Doctor(args) => run_doctor(&cli.global, &args),
        Command::Scan(args) => run_scan(&cli.global, &args),
        Command::Graph(args) => run_graph(&cli.global, &args),
        Command::Why(args) => run_why(&cli.global, &args),
        Command::Impact(args) => run_impact(&cli.global, &args),
        Command::Emulate(args) => run_emulate(&cli.global, &args),
        Command::WhatChanged(args) => run_what_changed(&cli.global, &args),
        Command::Snapshot(args) => run_snapshot(&cli.global, &args),
        Command::Diff(args) => run_diff(&cli.global, &args),
        Command::Watch(args) => run_watch(&cli.global, &args),
        Command::Test(args) => run_test(&cli.global, &args),
        Command::K8s(args) => run_k8s(&cli.global, &args.action),
        Command::Preflight(args) => run_preflight(&cli.global, &args),
        Command::Brief(args) => run_brief(&cli.global, &args),
        Command::Mcp(args) => run_mcp(&args),
        Command::AgentConfig(args) => run_agent_config(&cli.global, &args),
        Command::Setup(args) => run_setup(&cli.global, &args),
        Command::Daemon(args) => run_daemon(&cli.global, &args),
    }
}

fn refresh_scan(config_override: Option<std::path::PathBuf>) -> Result<ScanFreshness, AppError> {
    let request = ScanRequest::default_with_config(config_override);
    let result = match std::env::var_os("TWIN_PROC_ROOT") {
        Some(root) => {
            let paths = twin_app::paths::resolve_command_paths(request.config_override.as_deref())?;
            twin_app::scan_in(&paths.layout, request, Path::new(&root))?
        }
        None => twin_app::scan(request)?,
    };
    Ok(ScanFreshness {
        started_at_ns: result.started_at_ns,
        ended_at_ns: result.ended_at_ns,
    })
}

fn proc_root() -> std::path::PathBuf {
    std::env::var_os("TWIN_PROC_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(twin_app::paths::default_proc_root)
}

fn run_preflight(global: &GlobalArgs, args: &PreflightArgs) -> i32 {
    use std::io::Read;
    use twin_cli::hook;

    let mut command = args.command.join(" ");
    let mut cwd = args.cwd.clone();
    let mut file_changes = args
        .file
        .iter()
        .map(|path| twin_app::FileChange {
            path: path.clone(),
            operation: args.op.into(),
        })
        .collect::<Vec<_>>();
    if let Some(agent) = args.hook {
        let mut stdin = String::new();
        if std::io::stdin().read_to_string(&mut stdin).is_err() {
            return emit_hook(hook::fail_open(agent), 0);
        }
        let Some(input) = hook::parse_input(&stdin) else {
            return emit_hook(hook::fail_open(agent), 0);
        };
        command = input.command;
        file_changes.extend(input.file_changes);
        cwd = cwd.or(input.cwd);
    }
    if command.trim().is_empty() && file_changes.is_empty() {
        eprintln!(
            "Error: pass the shell command to evaluate, e.g. twin preflight \"<command>\", or --file PATH"
        );
        return 1;
    }
    let request = twin_app::PreflightRequest {
        command,
        file_changes,
        cwd: cwd.or_else(|| std::env::current_dir().ok()),
        config_override: args.config.clone(),
        max_graph_age_secs: args.max_age,
        should_read_scripts: true,
    };
    let from_daemon = args
        .daemon
        .socket()
        .map(|socket| twin_cli::daemon::preflight(socket, &request));
    let result = match from_daemon {
        Some(Ok(result)) => Ok(result),
        unavailable => twin_app::paths::resolve_command_paths(args.config.as_deref())
            .map_err(twin_app::AppError::from)
            .and_then(|paths| {
                twin_app::ensure_initialized_in(&paths.layout, args.config.as_deref())?;
                twin_app::preflight_in(&paths.layout, &request, &proc_root())
            })
            .map(|mut result| {
                if let Some(Err(unavailable)) = unavailable {
                    result.coverage_notes.push(unavailable.coverage_note());
                }
                result
            }),
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Some(agent) = args.hook {
                eprintln!("twin preflight unavailable: {error}");
                return emit_hook(hook::fail_open(agent), 0);
            }
            eprintln!("Error: {error}");
            return 1;
        }
    };
    if let Some(agent) = args.hook {
        return emit_hook(hook::response(agent, args.on_stop, &result), 0);
    }
    emit(global.json_mode(), &result, output::preflight::render);
    match result.verdict {
        twin_app::PreflightVerdict::Go => 0,
        twin_app::PreflightVerdict::Caution => 3,
        twin_app::PreflightVerdict::Stop => 4,
    }
}

fn run_brief(global: &GlobalArgs, args: &BriefArgs) -> i32 {
    let request = twin_app::BriefRequest {
        target: args.target.clone(),
        config_override: args.config.clone(),
        max_graph_age_secs: args.max_age,
        changes_since: args.since.clone(),
    };
    let from_daemon = args
        .daemon
        .socket()
        .map(|socket| twin_cli::daemon::brief(socket, &request));
    let result = match from_daemon {
        Some(Ok(result)) => Ok(result),
        unavailable => twin_app::paths::resolve_command_paths(args.config.as_deref())
            .map_err(twin_app::AppError::from)
            .and_then(|paths| {
                twin_app::ensure_initialized_in(&paths.layout, args.config.as_deref())?;
                twin_app::brief_in(&paths.layout, &request, &proc_root())
            })
            .map(|mut result| {
                if let Some(Err(unavailable)) = unavailable {
                    result.coverage_notes.push(unavailable.coverage_note());
                }
                result
            }),
    };
    match result {
        Ok(result) => {
            emit(global.json_mode(), &result, output::brief::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_setup(global: &GlobalArgs, args: &SetupArgs) -> i32 {
    let twin = args
        .twin_path
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "twin".to_string());
    let outcome = twin_app::paths::resolve_command_paths(args.config.as_deref())
        .map_err(twin_app::AppError::from)
        .and_then(|paths| {
            let store_created = !paths.layout.db_file().exists();
            twin_app::ensure_initialized_in(&paths.layout, args.config.as_deref())?;
            let scan = twin_app::scan_in(
                &paths.layout,
                ScanRequest::default_with_config(args.config.clone()),
                &proc_root(),
            )?;
            Ok((store_created, scan))
        });
    match outcome {
        Ok((store_created, scan)) => {
            let config = twin_cli::agent_config::render(args.agent, &twin);
            let summary =
                twin_cli::agent_config::SetupSummary::new(store_created, &scan, config, &twin);
            emit(
                global.json_mode(),
                &summary,
                twin_cli::agent_config::render_setup,
            );
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_agent_config(global: &GlobalArgs, args: &AgentConfigArgs) -> i32 {
    let twin = args
        .twin_path
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "twin".to_string());
    let config = twin_cli::agent_config::render(args.agent, &twin);
    emit(
        global.json_mode(),
        &config,
        twin_cli::agent_config::render_text,
    );
    0
}

fn run_mcp(args: &McpArgs) -> i32 {
    let paths = match twin_app::paths::resolve_command_paths(args.config.as_deref()) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let server =
        twin_cli::mcp::McpServer::new(paths.layout, args.config.clone(), proc_root(), args.max_age)
            .with_mode(twin_cli::mcp::ServeMode::Local {
                daemon_socket: args.daemon.socket().map(Path::to_path_buf),
            });
    let stdin = std::io::stdin();
    match server.serve(stdin.lock(), std::io::stdout()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_daemon(global: &GlobalArgs, args: &DaemonArgs) -> i32 {
    match &args.action {
        DaemonAction::InstallUnit(unit) => {
            let twin = unit
                .twin_path
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("/usr/bin/twin"));
            print!("{}", twin_app::daemon::systemd_unit(&twin));
            0
        }
        DaemonAction::Status(status) => {
            let status = twin_cli::daemon::status(&status.socket);
            emit(global.json_mode(), &status, output::daemon::render_status);
            if status.is_reachable {
                0
            } else {
                3
            }
        }
        DaemonAction::Run(run) => {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            let socket = run.socket.clone();
            if let Err(error) = ctrlc::set_handler(move || {
                flag.store(true, Ordering::SeqCst);
                twin_cli::daemon::wake(&socket);
            }) {
                eprintln!("Error: {error}");
                return 1;
            }
            let options = twin_cli::daemon::DaemonOptions {
                layout: twin_app::TwinLayout::flat(&run.state_dir),
                config: run.config.clone(),
                proc_root: proc_root(),
                socket: run.socket.clone(),
                interval: run.interval.clone(),
                ebpf: match run.ebpf {
                    twin_cli::cli::args::DaemonEbpf::Auto => {
                        Some(twin_app::daemon::ebpf_readiness())
                    }
                    twin_cli::cli::args::DaemonEbpf::Off => None,
                },
            };
            let mut log = DaemonLog {
                socket: run.socket.clone(),
                interval: run.interval.clone(),
            };
            match twin_cli::daemon::run(options, &stop, &mut log) {
                Ok(_) => 0,
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
    }
}

/// The daemon logs to the journal: its start and every warning, not every tick.
struct DaemonLog {
    socket: std::path::PathBuf,
    interval: String,
}

impl WatchSink for DaemonLog {
    fn on_start(&mut self, _interval_secs: u64) {
        eprintln!(
            "{}",
            output::daemon::render_started(&self.socket, &self.interval)
        );
    }

    fn on_tick(&mut self, _tick: &WatchTick) {}

    fn on_exec(&mut self, _event: &WatchExecEvent) {}

    fn on_tcp(&mut self, _event: &WatchTcpEvent) {}

    fn on_warning(&mut self, message: &str) {
        eprintln!("warning: {message}");
    }
}

fn emit_hook(response: Option<String>, code: i32) -> i32 {
    if let Some(response) = response {
        println!("{response}");
    }
    code
}

fn run_init(global: &GlobalArgs, args: &InitArgs) -> i32 {
    let request = InitRequest {
        force: args.force,
        config_override: args.config.clone(),
    };
    match twin_app::init(request) {
        Ok(result) => {
            emit(global.json_mode(), &result, output::init::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_doctor(global: &GlobalArgs, args: &DoctorArgs) -> i32 {
    let result =
        twin_app::doctor_flags(args.config.as_deref(), args.ebpf, args.containers, args.k8s);
    match result {
        Ok(result) => {
            emit(global.json_mode(), &result, output::doctor::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_watch(global: &GlobalArgs, args: &WatchArgs) -> i32 {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    if let Err(error) = ctrlc::set_handler(move || {
        flag.store(true, Ordering::SeqCst);
    }) {
        eprintln!("Error: {error}");
        return 1;
    }
    let command = WatchCommand {
        config_override: args.config.clone(),
        interval: args.interval.clone(),
        duration: args.duration.clone(),
        ticks: args.ticks,
        ebpf: args.ebpf,
        events: args.events.clone(),
    };
    let proc_root = proc_root();
    let mut sink = WatchPrinter { json: global.json };
    match twin_app::watch(command, &proc_root, &stop, &mut sink) {
        Ok(result) => {
            if global.json {
                emit_json_value("result", &result);
            }
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn emit_json_value<T: serde::Serialize>(event: &str, value: &T) {
    let data =
        serde_json::to_string(value).unwrap_or_else(|err| format!("{{\"error\":\"{err}\"}}"));
    println!("{{\"event\":\"{event}\",\"data\":{data}}}");
}

struct WatchPrinter {
    json: bool,
}

impl WatchSink for WatchPrinter {
    fn on_start(&mut self, interval_secs: u64) {
        if self.json {
            let data = serde_json::json!({ "interval_secs": interval_secs }).to_string();
            println!("{{\"event\":\"start\",\"data\":{data}}}");
            return;
        }
        println!("{}", output::watch::render_banner(interval_secs));
    }

    fn on_tick(&mut self, tick: &WatchTick) {
        if self.json {
            emit_json_value("tick", tick);
            return;
        }
        println!("{}", output::watch::render_tick(tick));
    }

    fn on_exec(&mut self, event: &WatchExecEvent) {
        if self.json {
            emit_json_value("exec", event);
            return;
        }
        println!("{}", output::watch::render_exec(event));
    }

    fn on_tcp(&mut self, event: &WatchTcpEvent) {
        if self.json {
            emit_json_value("tcp", event);
            return;
        }
        println!("{}", output::watch::render_tcp(event));
    }

    fn on_warning(&mut self, message: &str) {
        if self.json {
            let data = serde_json::json!({ "message": message }).to_string();
            println!("{{\"event\":\"warning\",\"data\":{data}}}");
            return;
        }
        println!("{}", output::watch::render_warning(message));
    }
}

fn run_test(global: &GlobalArgs, args: &TestArgs) -> i32 {
    match &args.action {
        TestAction::Init(init_args) => {
            let request = twin_app::TestInitRequest {
                path: init_args.path.clone(),
                force: init_args.force,
            };
            match twin_app::init_file(&request) {
                Ok(result) => {
                    emit(
                        global.json_mode(),
                        &result,
                        output::test_report::render_init,
                    );
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        TestAction::Lint(file_args) => match twin_app::lint_file(&file_args.path) {
            Ok(document) => {
                emit(
                    global.json_mode(),
                    &document,
                    output::test_report::render_lint,
                );
                0
            }
            Err(error) => {
                eprintln!("Error: {error}");
                1
            }
        },
        TestAction::Run(file_args) => {
            let max_age_seconds = match file_args
                .max_age
                .as_deref()
                .map(twin_app::parse_max_age_seconds)
                .transpose()
            {
                Ok(age) => age,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            let options = twin_app::TestRunOptions {
                refresh: file_args.refresh,
                max_age_seconds,
                snapshot: file_args.snapshot.clone(),
                baseline: file_args.baseline.clone(),
            };
            let paths = match twin_app::paths::resolve_command_paths(None) {
                Ok(paths) => paths,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            match twin_app::run_file_with_options(&paths.layout, &file_args.path, &options) {
                Ok(report) => {
                    let failed = report.failed > 0;
                    emit(global.json_mode(), &report, output::test_report::render_run);
                    if failed {
                        1
                    } else {
                        0
                    }
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        TestAction::List => {
            let paths = match twin_app::paths::resolve_command_paths(None) {
                Ok(paths) => paths,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            match twin_app::list_test_runs_in(&paths.layout) {
                Ok(runs) => {
                    emit(
                        global.json_mode(),
                        runs.as_slice(),
                        output::test_report::render_list,
                    );
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
    }
}

fn run_k8s(global: &GlobalArgs, action: &K8sAction) -> i32 {
    match action {
        K8sAction::Watch(args) => {
            match twin_app::k8s_watch(twin_app::K8sWatchRequest {
                duration_seconds: args.duration,
                max_events: usize::from(args.max_events),
                config_override: args.config.clone(),
            }) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::k8s::render_watch);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        K8sAction::Logs(args) => {
            match twin_app::k8s_logs(twin_app::K8sLogsRequest {
                pod: twin_app::PodLogRequest {
                    namespace: args.namespace.clone(),
                    pod: args.pod.clone(),
                    container: args.container.clone(),
                    previous: args.previous,
                    tail_lines: i64::from(args.tail_lines),
                    max_bytes: args.max_bytes as usize,
                    timeout_seconds: args.timeout,
                },
                config_override: args.config.clone(),
            }) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::k8s::render_logs);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }

        K8sAction::Scan(args) => {
            let request = twin_app::K8sScanRequest {
                fixture: args.fixture.clone(),
                config_override: args.config.clone(),
            };
            match twin_app::k8s_scan(request) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::k8s::render_scan);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        K8sAction::Graph(args) => {
            let target = match k8s_target(&args.target) {
                Ok(target) => target,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            let request = twin_app::K8sTargetRequest {
                config_override: args.config.clone(),
                target,
            };
            match twin_app::k8s_graph(request) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::k8s::render_graph);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        K8sAction::Impact(args) => {
            let target = match k8s_target(&args.target) {
                Ok(target) => target,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            let request = twin_app::K8sTargetRequest {
                config_override: args.config.clone(),
                target,
            };
            match twin_app::k8s_impact(request) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::k8s::render_impact);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
    }
}

fn k8s_target(raw: &str) -> Result<twin_core::NodeId, twin_app::AppError> {
    twin_core::NodeId::parse_k8s_ref(raw).map_err(|source| twin_app::AppError::InvalidGraphTarget {
        value: raw.to_string(),
        source,
    })
}

fn run_scan(global: &GlobalArgs, args: &ScanArgs) -> i32 {
    let request = ScanRequest {
        docker_fixture: args.docker_fixture.clone(),
        containerd_fixture: args.containerd_fixture.clone(),
        cron_root: None,
        config_override: args.config.clone(),
        samples: args.samples,
        interval_secs: args.interval,
    };
    let result = match std::env::var_os("TWIN_PROC_ROOT") {
        Some(root) => {
            let proc_root = Path::new(&root);
            match twin_app::paths::resolve_command_paths(request.config_override.as_deref()) {
                Ok(paths) => twin_app::scan_in(&paths.layout, request, proc_root),
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            }
        }
        None => twin_app::scan(request),
    };
    match result {
        Ok(result) => {
            emit(global.json_mode(), &result, output::scan::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_emulate(global: &GlobalArgs, args: &EmulateArgs) -> i32 {
    let (config_override, request) = match &args.action {
        EmulateActionArgs::Restart(restart_args) => (
            restart_args.config.clone(),
            match emulate::emulate_restart_request(restart_args) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            },
        ),
        EmulateActionArgs::FillDisk(fill_args) => (
            fill_args.config.clone(),
            match emulate::emulate_fill_disk_request(fill_args) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            },
        ),
        EmulateActionArgs::Upgrade(upgrade_args) => (
            upgrade_args.config.clone(),
            match emulate::emulate_upgrade_request(upgrade_args) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            },
        ),
        EmulateActionArgs::Delete(delete_args) => (
            delete_args.config.clone(),
            match emulate::emulate_delete_request(delete_args) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            },
        ),
        EmulateActionArgs::Rollout(rollout_args) => (
            rollout_args.config.clone(),
            match emulate::emulate_rollout_request(rollout_args) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            },
        ),
    };
    if let Err(error) = twin_app::validate_emulate_request(&request) {
        eprintln!("Error: {error}");
        return 1;
    }
    let scan = match refresh_scan(config_override) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    match twin_app::emulate(request) {
        Ok(result) => {
            emit_after_scan(
                global.json_mode(),
                &result,
                output::emulate::render_with_scan,
                scan,
            );
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_impact(global: &GlobalArgs, args: &ImpactArgs) -> i32 {
    let request = match impact::impact_request(args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let scan = match refresh_scan(args.config.clone()) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    match twin_app::impact(request) {
        Ok(result) => {
            emit_after_scan(
                global.json_mode(),
                &result,
                output::impact::render_with_scan,
                scan,
            );
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_what_changed(global: &GlobalArgs, args: &WhatChangedArgs) -> i32 {
    let request = what_changed::what_changed_request(args);
    let verbose = request.verbose;
    match twin_app::what_changed(request) {
        Ok(result) => {
            let rendered = if global.json {
                output::what_changed::render_json(&result, verbose, global.json_mode())
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
            } else {
                output::what_changed::render(
                    &result,
                    output::what_changed::WhatChangedRenderOptions { verbose },
                )
            };
            println!("{rendered}");
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_snapshot(global: &GlobalArgs, args: &SnapshotArgs) -> i32 {
    match &args.action {
        SnapshotActionArgs::Create { name } => {
            let request = snapshot::snapshot_create_request(args, name);
            match twin_app::snapshot_create(request) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::snapshot::render_create);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        SnapshotActionArgs::List => {
            let request = snapshot::snapshot_list_request(args);
            match twin_app::snapshot_list(request) {
                Ok(result) => {
                    emit(global.json_mode(), &result, output::snapshot::render_list);
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
    }
}

fn run_diff(global: &GlobalArgs, args: &DiffArgs) -> i32 {
    let request = diff::diff_request(args);
    match twin_app::diff(request) {
        Ok(result) => {
            emit(global.json_mode(), &result, output::diff::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_graph(global: &GlobalArgs, args: &GraphArgs) -> i32 {
    let request = match graph::graph_request(args) {
        Ok(request) => request,
        Err(error) => {
            eprint_graph_error(&error);
            return 1;
        }
    };
    let scan = if args.at.is_some() {
        None
    } else {
        Some(match refresh_scan(args.config.clone()) {
            Ok(scan) => scan,
            Err(error) => {
                eprint_graph_error(&error);
                return 1;
            }
        })
    };
    match twin_app::graph(request) {
        Ok(result) => {
            if args.format == twin_cli::cli::args::GraphFormat::Human {
                if let Some(scan) = scan {
                    emit_after_scan(
                        global.json_mode(),
                        &result,
                        output::graph::render_with_scan,
                        scan,
                    );
                } else if global.json {
                    emit(
                        global.json_mode(),
                        &serde_json::json!({"at": args.at, "history_scope": "retained observations only", "graph": result}),
                        |_| String::new(),
                    );
                } else {
                    println!(
                        "Graph at {} (retained history; evidence may be pruned)\n{}",
                        args.at.as_deref().unwrap_or_default(),
                        output::graph::render(&result)
                    );
                }
            } else {
                println!("{}", output::graph_export::render(&result, args.format));
            }
            0
        }
        Err(error) => {
            eprint_graph_error(&error);
            1
        }
    }
}

fn eprint_graph_error(error: &AppError) {
    eprintln!("Error: {error}");
    if matches!(
        error,
        AppError::InvalidGraphTarget { .. } | AppError::Graph(GraphError::MissingQuery)
    ) {
        eprintln!("Try: twin graph process, twin graph 1234, or twin graph process:pid:1234");
    }
}

fn emit<T: serde::Serialize + ?Sized>(json: JsonMode, value: &T, text: fn(&T) -> String) {
    let rendered = if json == JsonMode::Off {
        text(value)
    } else {
        output::json::render_bounded(value, json)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    };
    println!("{rendered}");
}

fn emit_after_scan<T: serde::Serialize>(
    json: JsonMode,
    value: &T,
    text: fn(&T, Option<ScanFreshness>) -> String,
    scan: ScanFreshness,
) {
    let rendered = if json == JsonMode::Off {
        text(value, Some(scan))
    } else {
        output::json::render_bounded(value, json)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    };
    println!("{rendered}");
}

fn run_why(global: &GlobalArgs, args: &twin_cli::cli::args::WhyArgs) -> i32 {
    match twin_app::why_request(&args.target, args.config.clone()).and_then(twin_app::why) {
        Ok(result) => {
            emit(global.json_mode(), &result, output::why::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}
