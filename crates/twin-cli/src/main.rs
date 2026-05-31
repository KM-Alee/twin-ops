use std::process;

use clap::Parser;
use twin_app::InitRequest;
use twin_cli::cli::args::{Cli, Command, DoctorArgs, GlobalArgs, InitArgs};
use twin_cli::output;

fn main() {
    let cli = Cli::parse();
    process::exit(dispatch(cli));
}

fn dispatch(cli: Cli) -> i32 {
    match cli.command {
        Command::Init(args) => run_init(&cli.global, &args),
        Command::Doctor(args) => run_doctor(&cli.global, &args),
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

fn emit<T: serde::Serialize>(json: bool, value: &T, text: fn(&T) -> String) {
    let rendered = if json {
        output::json::render(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    } else {
        text(value)
    };
    println!("{rendered}");
}
