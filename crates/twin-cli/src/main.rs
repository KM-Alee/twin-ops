use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use twin_app::{AppError, GraphError, InitRequest, ScanRequest, WatchTick};
use twin_cli::cli::args::{
    Cli, Command, DiffArgs, DoctorArgs, EmulateActionArgs, EmulateArgs, GlobalArgs, GraphArgs,
    ImpactArgs, InitArgs, ScanArgs, SnapshotActionArgs, SnapshotArgs, WatchArgs, WhatChangedArgs,
};
use twin_cli::cli::diff;
use twin_cli::cli::emulate;
use twin_cli::cli::graph;
use twin_cli::cli::impact;
use twin_cli::cli::snapshot;
use twin_cli::cli::watch;
use twin_cli::cli::what_changed;
use twin_cli::output;
use twin_cli::output::format::ScanFreshness;

fn main() {
    let cli = Cli::parse();
    process::exit(dispatch(cli));
}

fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        Command::Init(args) => run_init(&cli.global, &args),
        Command::Doctor(args) => run_doctor(&cli.global, &args),
        Command::Scan(args) => run_scan(&cli.global, &args),
        Command::Watch(args) => run_watch(&cli.global, &args),
        Command::Graph(args) => run_graph(&cli.global, &args),
        Command::Impact(args) => run_impact(&cli.global, &args),
        Command::Emulate(args) => run_emulate(&cli.global, &args),
        Command::WhatChanged(args) => run_what_changed(&cli.global, &args),
        Command::Snapshot(args) => run_snapshot(&cli.global, &args),
        Command::Diff(args) => run_diff(&cli.global, &args),
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

fn run_init(global: &GlobalArgs, args: &InitArgs) -> i32 {
    let request = InitRequest {
        force: args.force,
        config_override: args.config.clone(),
    };
    match twin_app::init(request) {
        Ok(result) => {
            emit(global.json, &result, output::init::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_doctor(global: &GlobalArgs, args: &DoctorArgs) -> i32 {
    match twin_app::doctor(args.config.as_deref()) {
        Ok(result) => {
            emit(global.json, &result, output::doctor::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_scan(global: &GlobalArgs, args: &ScanArgs) -> i32 {
    let request = ScanRequest {
        config_override: args.config.clone(),
        samples: args.samples.clamp(1, 30),
        interval_secs: args.interval.max(1),
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
            emit(global.json, &result, output::scan::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_watch(global: &GlobalArgs, args: &WatchArgs) -> i32 {
    let request = watch::watch_request(args);
    let interval_label = request.interval.clone();
    let duration_label = request.duration.clone();
    let max_ticks = request.max_ticks;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    if let Err(error) = ctrlc::set_handler(move || {
        flag.store(true, Ordering::SeqCst);
    }) {
        eprintln!("Error: cannot install interrupt handler: {error}");
        return 1;
    }
    let paths = match twin_app::paths::resolve_command_paths(request.config_override.as_deref()) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let proc_root = std::env::var_os("TWIN_PROC_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc"));
    let mut printed_header = false;
    let result = twin_app::watch_in(
        &paths.layout,
        request,
        &proc_root,
        &stop,
        |tick: &WatchTick| {
            if !global.json && !printed_header {
                println!(
                    "{}",
                    output::watch::render_header(
                        &interval_label,
                        duration_label.as_deref(),
                        max_ticks
                    )
                );
                printed_header = true;
            }
            if global.json {
                println!("{}", output::watch::render_tick_json(tick));
            } else {
                println!("{}", output::watch::render_tick(tick));
            }
            let _ = io::stdout().flush();
        },
    );
    match result {
        Ok(result) => {
            if !global.json && !printed_header {
                println!(
                    "{}",
                    output::watch::render_header(
                        &interval_label,
                        duration_label.as_deref(),
                        max_ticks
                    )
                );
            }
            if global.json {
                println!("{}", output::watch::render_summary_json(&result));
            } else {
                println!("{}", output::watch::render_summary(&result));
            }
            let _ = io::stdout().flush();
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
    };
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
                global.json,
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
    let scan = match refresh_scan(args.config.clone()) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let request = match impact::impact_request(args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    match twin_app::impact(request) {
        Ok(result) => {
            emit_after_scan(global.json, &result, output::impact::render_with_scan, scan);
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
                output::json::render(&result).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
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
                    emit(global.json, &result, output::snapshot::render_create);
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
                    emit(global.json, &result, output::snapshot::render_list);
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
            emit(global.json, &result, output::diff::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn run_graph(global: &GlobalArgs, args: &GraphArgs) -> i32 {
    let scan = match refresh_scan(args.config.clone()) {
        Ok(scan) => scan,
        Err(error) => {
            eprint_graph_error(&error);
            return 1;
        }
    };
    let request = match graph::graph_request(args) {
        Ok(request) => request,
        Err(error) => {
            eprint_graph_error(&error);
            return 1;
        }
    };
    match twin_app::graph(request) {
        Ok(result) => {
            emit_after_scan(global.json, &result, output::graph::render_with_scan, scan);
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

fn emit<T: serde::Serialize>(json: bool, value: &T, text: fn(&T) -> String) {
    let rendered = if json {
        output::json::render(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    } else {
        text(value)
    };
    println!("{rendered}");
}

fn emit_after_scan<T: serde::Serialize>(
    json: bool,
    value: &T,
    text: fn(&T, Option<ScanFreshness>) -> String,
    scan: ScanFreshness,
) {
    let rendered = if json {
        output::json::render(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    } else {
        text(value, Some(scan))
    };
    println!("{rendered}");
}
