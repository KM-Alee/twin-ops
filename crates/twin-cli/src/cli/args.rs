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
    Impact(ImpactArgs),
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

    #[arg(long, default_value = "1", help = "Number of scan samples (1–30)")]
    pub samples: u32,

    #[arg(long, default_value = "2", help = "Seconds between samples when samples > 1")]
    pub interval: u64,
}

#[derive(Args)]
pub struct GraphArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "List nodes of this kind (default: process)")]
    pub kind: Option<String>,

    #[arg(
        value_name = "TARGET",
        help = "Node id, pid, or kind (e.g. process, 1234, process:pid:1234)"
    )]
    pub target: Option<String>,
}

#[derive(Args)]
pub struct ImpactArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "TARGET",
        help = "Service or port target (e.g. postgresql, port:tcp:127.0.0.1:5432)"
    )]
    pub target: String,
}
