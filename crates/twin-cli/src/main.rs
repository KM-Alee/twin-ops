use std::path::Path;
use std::process;

use clap::Parser;
use twin_app::{AppError, GraphError, InitRequest, ScanRequest};
use twin_cli::cli::args::{
    Cli, Command, DoctorArgs, GlobalArgs, GraphArgs, ImpactArgs, InitArgs, ScanArgs,
};
use twin_cli::cli::graph;
use twin_cli::cli::impact;
use twin_cli::output;

fn main() {
    let cli = Cli::parse();
    process::exit(dispatch(cli));
}

fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        Command::Init(args) => run_init(&cli.global, &args),
        Command::Doctor(args) => run_doctor(&cli.global, &args),
        Command::Scan(args) => run_scan(&cli.global, &args),
        Command::Graph(args) => run_graph(&cli.global, &args),
        Command::Impact(args) => run_impact(&cli.global, &args),
    }
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

fn run_impact(global: &GlobalArgs, args: &ImpactArgs) -> i32 {
    let request = impact::impact_request(args);
    match twin_app::impact(request) {
        Ok(result) => {
            emit(global.json, &result, output::impact::render);
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
    match twin_app::graph(request) {
        Ok(result) => {
            emit(global.json, &result, output::graph::render);
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
