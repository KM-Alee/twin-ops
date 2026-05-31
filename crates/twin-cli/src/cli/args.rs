use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "twin", version, about = "Read-only operational twin for Linux")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct GlobalArgs {
    #[arg(long, global = true, help = "Output as JSON")]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
}

#[derive(Args)]
pub struct InitArgs {
    #[arg(long, help = "Overwrite existing config")]
    pub force: bool,

    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,
}
