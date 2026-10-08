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
    Emulate(EmulateArgs),
    WhatChanged(WhatChangedArgs),
    Snapshot(SnapshotArgs),
    Diff(DiffArgs),
    Watch(WatchArgs),
    Test(TestArgs),
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

    #[arg(long, help = "Check kernel, BTF, capabilities, and exec tracing")]
    pub ebpf: bool,
}

#[derive(Args)]
pub struct ScanArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, default_value = "1", help = "Number of scan samples (1–30)")]
    pub samples: u32,

    #[arg(
        long,
        default_value = "2",
        help = "Seconds between samples when samples > 1"
    )]
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

    #[arg(long, help = "Show runtime dependency evidence")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateArgs {
    #[command(subcommand)]
    pub action: EmulateActionArgs,
}

#[derive(Subcommand)]
pub enum EmulateActionArgs {
    #[command(about = "Hypothetically restart a service (no action performed)")]
    Restart(EmulateRestartArgs),
    #[command(about = "Hypothetically delete a config file (no action performed)")]
    Delete(EmulateDeleteArgs),
    #[command(
        name = "fill-disk",
        about = "Hypothetically fill a mount (no write performed)"
    )]
    FillDisk(EmulateFillDiskArgs),
    #[command(about = "Hypothetically upgrade a package (no package manager is run)")]
    Upgrade(EmulateUpgradeArgs),
}

#[derive(Args)]
pub struct EmulateRestartArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Show transitive dependency impact paths")]
    pub paths: bool,

    #[arg(
        long,
        default_value_t = 4,
        value_parser = clap::value_parser!(u16).range(1..=8),
        help = "Maximum dependency path depth (1–8)"
    )]
    pub max_depth: u16,

    #[arg(
        value_name = "TARGET",
        help = "Service target (e.g. postgresql, service:postgresql.service)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateDeleteArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "PATH_OR_FILE_ID",
        help = "Absolute file path or file: node id (e.g. /etc/nginx/nginx.conf)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateUpgradeArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "PACKAGE",
        help = "Package name or package: node id (e.g. package:openssl)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateFillDiskArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "MOUNT",
        help = "Mount path or mount: node id (e.g. /var, mount:/var)"
    )]
    pub target: String,

    #[arg(
        long = "to",
        value_name = "PERCENT",
        help = "Hypothetical used percent, for example 95%"
    )]
    pub to_percent: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct WhatChangedArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        value_name = "DURATION",
        help = "Report changes since this window (e.g. 10m, 1h, yesterday)"
    )]
    pub since: String,

    #[arg(
        long,
        help = "Show full change lists in human output (default is concise)"
    )]
    pub verbose: bool,
}

#[derive(Args)]
pub struct SnapshotArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub action: SnapshotActionArgs,
}

#[derive(Subcommand)]
pub enum SnapshotActionArgs {
    #[command(about = "Create a named graph snapshot")]
    Create {
        #[arg(help = "Snapshot name (letters, digits, '.', '_', '-')")]
        name: String,
    },
    #[command(about = "List saved graph snapshots")]
    List,
}

#[derive(Args)]
pub struct DiffArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(help = "Left graph ref (`current` or `snapshot:NAME`)")]
    pub left: String,

    #[arg(help = "Right graph ref (`current` or `snapshot:NAME`)")]
    pub right: String,
}

#[derive(Args)]
pub struct WatchArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, default_value = "5s", help = "Scan interval, for example 5s")]
    pub interval: String,

    #[arg(long, help = "Stop after this duration, for example 10m")]
    pub duration: Option<String>,

    #[arg(long, help = "Stop after this many scans")]
    pub ticks: Option<u32>,

    #[arg(long, help = "Observe exec events with eBPF when the kernel allows it")]
    pub ebpf: bool,

    #[arg(long, help = "eBPF event stream to print (exec or tcp)")]
    pub events: Option<String>,
}

#[derive(Args)]
pub struct TestArgs {
    #[command(subcommand)]
    pub action: TestAction,
}

#[derive(Subcommand)]
pub enum TestAction {
    #[command(about = "Write a starter twin.yaml")]
    Init(TestInitArgs),
    #[command(about = "Validate a twin.yaml file")]
    Lint(TestFileArgs),
    #[command(about = "Run checks in a twin.yaml file against the stored graph")]
    Run(TestFileArgs),
}

#[derive(Args)]
pub struct TestInitArgs {
    #[arg(long, help = "Replace an existing file")]
    pub force: bool,

    #[arg(default_value = "twin.yaml", help = "Path to write")]
    pub path: PathBuf,
}

#[derive(Args)]
pub struct TestFileArgs {
    #[arg(default_value = "twin.yaml", help = "Path to the test file")]
    pub path: PathBuf,
}

#[derive(Args)]
pub struct ImpactArgs {
    #[arg(long, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Show transitive dependency impact paths")]
    pub paths: bool,

    #[arg(
        long,
        default_value_t = 4,
        value_parser = clap::value_parser!(u16).range(1..=8),
        help = "Maximum dependency path depth (1–8)"
    )]
    pub max_depth: u16,

    #[arg(
        value_name = "TARGET",
        help = "Service or port target (e.g. postgresql, port:tcp:127.0.0.1:5432)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}
