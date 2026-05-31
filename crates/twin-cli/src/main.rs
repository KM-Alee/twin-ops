use std::process;
use std::str::FromStr;

use clap::Parser;
use twin_app::{AppError, GraphRequest, InitRequest, ScanRequest};
use twin_cli::cli::args::{Cli, Command, DoctorArgs, GlobalArgs, GraphArgs, InitArgs, ScanArgs};
use twin_cli::output;
use twin_core::{NodeId, NodeKind};

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
    match twin_app::scan(request) {
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

fn run_graph(global: &GlobalArgs, args: &GraphArgs) -> i32 {
    let request = match graph_request(args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    match twin_app::graph(request) {
        Ok(result) => {
            emit(global.json, &result, output::graph::render);
            0
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn graph_request(args: &GraphArgs) -> Result<GraphRequest, AppError> {
    let target = match &args.target {
        Some(value) => {
            Some(
                NodeId::from_str(value).map_err(|source| AppError::InvalidGraphTarget {
                    value: value.clone(),
                    source,
                })?,
            )
        }
        None => None,
    };
    let kind = match &args.kind {
        Some(value) => {
            Some(
                NodeKind::from_str(value).map_err(|source| AppError::InvalidGraphTarget {
                    value: value.clone(),
                    source,
                })?,
            )
        }
        None => None,
    };
    Ok(GraphRequest {
        config_override: args.config.clone(),
        kind,
        target,
    })
}

fn emit<T: serde::Serialize>(json: bool, value: &T, text: fn(&T) -> String) {
    let rendered = if json {
        output::json::render(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    } else {
        text(value)
    };
    println!("{rendered}");
}
