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
    Scan(ScanArgs),
    Graph(GraphArgs),
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

#[derive(Args)]
pub struct ScanArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct GraphArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "List nodes of this kind (e.g. process)")]
    pub kind: Option<String>,

    #[arg(value_name = "TARGET", help = "Show neighborhood for this node id")]
    pub target: Option<String>,
}
