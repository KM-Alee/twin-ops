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

    #[arg(
        long,
        global = true,
        help = "With --json, include every list item (default caps lists at 100 and reports <field>_omitted)"
    )]
    pub full: bool,
}

impl GlobalArgs {
    pub fn json_mode(&self) -> crate::output::json::JsonMode {
        use crate::output::json::JsonMode;
        match (self.json, self.full) {
            (false, _) => JsonMode::Off,
            (true, false) => JsonMode::Bounded,
            (true, true) => JsonMode::Full,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Generate a shell completion script on stdout")]
    Completions {
        shell: clap_complete::Shell,
    },
    Init(InitArgs),
    Doctor(DoctorArgs),
    Scan(ScanArgs),
    Graph(GraphArgs),
    Impact(ImpactArgs),
    #[command(about = "Explain stored relationships with their observed or inferred evidence")]
    Why(WhyArgs),
    Emulate(EmulateArgs),
    WhatChanged(WhatChangedArgs),
    Snapshot(SnapshotArgs),
    Diff(DiffArgs),
    Watch(WatchArgs),
    Test(TestArgs),
    #[command(about = "Read-only Kubernetes graph")]
    K8s(K8sArgs),
    #[command(
        about = "Predict what a shell command would break before running it (go / caution / stop)",
        long_about = "Predict what a shell command would break before running it.\n\n\
Recognizes systemctl/service, docker/podman, rm/sed -i/tee/redirection, apt/dnf/pacman/zypper/apk, \
kill/pkill, kubectl, ufw/iptables and reboot. Nothing is executed.\n\n\
Exit codes: 0 go, 3 caution, 4 stop, 1 error. With --hook, reads an agent's hook JSON on stdin \
and answers in that agent's format."
    )]
    Preflight(PreflightArgs),
    #[command(
        about = "One-call briefing: the host overview, or everything about one service/container/port/file"
    )]
    Brief(BriefArgs),
    #[command(
        about = "Serve twin's read-only tools to AI agents over the Model Context Protocol (stdio)"
    )]
    Mcp(McpArgs),
    #[command(
        name = "agent-config",
        about = "Print ready-to-paste MCP, hook and skill setup for an AI coding agent (writes nothing)"
    )]
    AgentConfig(AgentConfigArgs),
    #[command(
        about = "First-run setup: create twin's store, scan this host, and print agent integration snippets"
    )]
    Setup(SetupArgs),
    #[command(
        about = "Privileged daemon: keeps the graph fresh and answers unprivileged queries over a socket"
    )]
    Daemon(DaemonArgs),
}

#[derive(Args, Clone)]
pub struct DaemonClientArgs {
    #[arg(
        long,
        default_value = twin_app::daemon::DEFAULT_SOCKET,
        help = "Ask the twin daemon at this socket first; it sees processes your user cannot"
    )]
    pub daemon_socket: PathBuf,

    #[arg(long, help = "Never ask the twin daemon; answer from a local scan")]
    pub no_daemon: bool,
}

impl DaemonClientArgs {
    pub fn socket(&self) -> Option<&std::path::Path> {
        (!self.no_daemon).then_some(self.daemon_socket.as_path())
    }
}

#[derive(Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub action: DaemonAction,
}

#[derive(Subcommand)]
pub enum DaemonAction {
    #[command(
        about = "Run the watch loop and serve the socket (started by the systemd unit, as root)"
    )]
    Run(DaemonRunArgs),
    #[command(
        name = "install-unit",
        about = "Print the hardened systemd unit for the daemon; twin never installs it"
    )]
    InstallUnit(InstallUnitArgs),
    #[command(about = "Check whether a twin daemon answers on its socket")]
    Status(DaemonStatusArgs),
}

#[derive(Args)]
pub struct DaemonRunArgs {
    #[arg(
        long,
        default_value = twin_app::daemon::DEFAULT_STATE_DIR,
        help = "Directory for the daemon's store and settings"
    )]
    pub state_dir: PathBuf,

    #[arg(
        long,
        default_value = twin_app::daemon::DEFAULT_SOCKET,
        help = "Socket to serve (mode 0660, owned by the daemon's group)"
    )]
    pub socket: PathBuf,

    #[arg(
        long,
        default_value = twin_app::daemon::DEFAULT_INTERVAL,
        help = "Time between scans"
    )]
    pub interval: String,

    #[arg(
        long,
        help = "Settings file path (default: config.toml in --state-dir)"
    )]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct InstallUnitArgs {
    #[arg(
        long,
        required = true,
        help = "Print the unit to stdout (required: twin never writes it for you)"
    )]
    pub print: bool,

    #[arg(
        long,
        help = "twin executable path for ExecStart (default: /usr/bin/twin)"
    )]
    pub twin_path: Option<PathBuf>,
}

#[derive(Args)]
pub struct DaemonStatusArgs {
    #[arg(
        long,
        default_value = twin_app::daemon::DEFAULT_SOCKET,
        help = "Socket to check"
    )]
    pub socket: PathBuf,
}

#[derive(Args)]
pub struct SetupArgs {
    #[arg(value_enum, help = "Agent to configure")]
    pub agent: crate::agent_config::AgentTarget,

    #[arg(long, help = "twin executable path to embed (default: this binary)")]
    pub twin_path: Option<PathBuf>,

    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct AgentConfigArgs {
    #[arg(value_enum, help = "Agent to configure")]
    pub agent: crate::agent_config::AgentTarget,

    #[arg(long, help = "twin executable path to embed (default: this binary)")]
    pub twin_path: Option<PathBuf>,
}

#[derive(Args)]
pub struct McpArgs {
    #[arg(
        long,
        default_value_t = 60,
        help = "Rescan the host when the stored graph is older than this many seconds"
    )]
    pub max_age: u64,

    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[command(flatten)]
    pub daemon: DaemonClientArgs,
}

#[derive(Args)]
pub struct BriefArgs {
    #[arg(
        help = "Service, container, port (:5432), pid, path or node id; omit for the host overview"
    )]
    pub target: Option<String>,

    #[arg(
        long,
        default_value = "24h",
        help = "Window for the recent-changes section (e.g. 1h, 24h)"
    )]
    pub since: String,

    #[arg(
        long,
        default_value_t = 120,
        help = "Rescan the host when the stored graph is older than this many seconds"
    )]
    pub max_age: u64,

    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[command(flatten)]
    pub daemon: DaemonClientArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FileOp {
    Create,
    Edit,
    Delete,
}

impl From<FileOp> for twin_app::FileOperation {
    fn from(op: FileOp) -> Self {
        match op {
            FileOp::Create => Self::Create,
            FileOp::Edit => Self::Edit,
            FileOp::Delete => Self::Delete,
        }
    }
}

#[derive(Args)]
pub struct PreflightArgs {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "Shell command to evaluate (quote it, or pass after --)"
    )]
    pub command: Vec<String>,

    #[arg(
        long,
        value_enum,
        help = "Read agent hook JSON from stdin and reply in its format"
    )]
    pub hook: Option<crate::hook::HookAgent>,

    #[arg(
        long,
        help = "Evaluate a direct file change instead of (or besides) a command; repeatable"
    )]
    pub file: Vec<PathBuf>,

    #[arg(
        long,
        value_enum,
        default_value = "edit",
        help = "Kind of change for --file"
    )]
    pub op: FileOp,

    #[arg(
        long,
        value_enum,
        default_value = "ask",
        help = "Hook behavior for a stop verdict"
    )]
    pub on_stop: crate::hook::StopPolicy,

    #[arg(long, help = "Working directory used to resolve relative paths")]
    pub cwd: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 120,
        help = "Rescan the host when the stored graph is older than this many seconds"
    )]
    pub max_age: u64,

    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[command(flatten)]
    pub daemon: DaemonClientArgs,
}

#[derive(Args)]
pub struct InitArgs {
    #[arg(long, help = "Overwrite existing config")]
    pub force: bool,

    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Check kernel, BTF, capabilities, and exec tracing")]
    pub ebpf: bool,

    #[arg(long, help = "Check the read-only Docker socket and container list")]
    pub containers: bool,

    #[arg(
        long,
        help = "Check kubeconfig presence for read-only Kubernetes inspection"
    )]
    pub k8s: bool,
}

#[derive(Args)]
pub struct ScanArgs {
    #[arg(
        long,
        value_name = "DIR",
        help = "Read namespace-scoped containerd metadata from fixtures"
    )]
    pub containerd_fixture: Option<PathBuf>,
    #[arg(
        long,
        value_name = "DIR",
        help = "Read Docker list and inspect documents from a fixture directory"
    )]
    pub docker_fixture: Option<PathBuf>,
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        default_value = "1",
        value_parser = clap::value_parser!(u32).range(1..=30),
        help = "Number of scan samples (1–30)"
    )]
    pub samples: u32,

    #[arg(
        long,
        default_value = "2",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Seconds between samples when samples > 1"
    )]
    pub interval: u64,
}

#[derive(Args)]
pub struct GraphArgs {
    #[arg(
        long,
        help = "View retained graph history at RFC3339 time or elapsed duration (e.g. 1h)"
    )]
    pub at: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..), help = "Maximum listed nodes")]
    pub limit: Option<u32>,
    #[arg(long, help = "List nodes whose label contains this text")]
    pub label: Option<String>,
    #[arg(long, value_parser = ["active", "stale", "gone"], help = "List nodes in this state (default: active)")]
    pub state: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        conflicts_with = "json",
        help = "Graph output format"
    )]
    pub format: GraphFormat,
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[command(about = "Hypothetically restart a service or container (no action performed)")]
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
    #[command(about = "Hypothetically roll out a deployment (no rollout is performed)")]
    Rollout(EmulateRolloutArgs),
}

#[derive(Args)]
pub struct K8sArgs {
    #[command(subcommand)]
    pub action: K8sAction,
}

#[derive(Subcommand)]
pub enum K8sAction {
    #[command(
        about = "Watch Kubernetes resources and refresh the graph with bounded retained results"
    )]
    Watch(K8sWatchArgs),
    #[command(
        about = "Read a bounded pod log volume summary without exposing or storing log bodies"
    )]
    Logs(K8sLogsArgs),
    #[command(about = "Read Kubernetes list documents into the graph")]
    Scan(K8sScanArgs),
    #[command(about = "Show a Kubernetes ownership neighborhood")]
    Graph(K8sTargetArgs),
    #[command(about = "Show what a Kubernetes object fronts or owns")]
    Impact(K8sTargetArgs),
}

#[derive(Args)]
pub struct K8sWatchArgs {
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600), help = "Watch duration in seconds")]
    pub duration: u64,
    #[arg(long, default_value_t = 256, value_parser = clap::value_parser!(u16).range(1..=4096))]
    pub max_events: u16,
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct K8sLogsArgs {
    #[arg(value_name = "POD", help = "Pod name within --namespace")]
    pub pod: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub container: Option<String>,
    #[arg(long)]
    pub previous: bool,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(1..=10000))]
    pub tail_lines: u16,
    #[arg(long, default_value_t = 65536, value_parser = clap::value_parser!(u32).range(1..=1048576))]
    pub max_bytes: u32,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=60))]
    pub timeout: u64,
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct K8sScanArgs {
    #[arg(
        long,
        value_name = "DIR",
        help = "Read Kubernetes list documents from a fixture directory"
    )]
    pub fixture: Option<PathBuf>,
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,
}

#[derive(Args)]
pub struct K8sTargetArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "TARGET",
        help = "Kubernetes object (deployment/default/api, service/default/api, pod:default/api-123)"
    )]
    pub target: String,
}

#[derive(Args)]
pub struct EmulateRolloutArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        value_name = "DEPLOYMENT",
        help = "Deployment target (deployment:default/api or k8s:deployment:default/api)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateRestartArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
        help = "Service or container target (e.g. postgresql, service:postgresql.service, container:redis)"
    )]
    pub target: String,

    #[arg(long, help = "Explain the evidence score and list coverage unknowns")]
    pub evidence: bool,
}

#[derive(Args)]
pub struct EmulateDeleteArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[arg(
        long,
        help = "Restrict changes to this node/service and its dependency neighborhood"
    )]
    pub target: Option<String>,
    #[arg(
        long = "category",
        help = "Filter operational changes by snake_case category; repeatable"
    )]
    pub categories: Vec<twin_core::ChangeCategory>,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=10000), help = "Maximum chronological operational entries (newest retained)")]
    pub limit: u32,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..=4), help = "Target neighborhood depth (1–4)")]
    pub max_depth: u16,
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(help = "Left graph ref (`current` or `snapshot:NAME`)")]
    pub left: String,

    #[arg(help = "Right graph ref (`current` or `snapshot:NAME`)")]
    pub right: String,
}

#[derive(Args)]
pub struct WatchArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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
    Run(TestRunArgs),
    #[command(about = "List stored twin.yaml runs")]
    List,
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
pub struct TestRunArgs {
    #[arg(default_value = "twin.yaml", help = "Path to the test file")]
    pub path: PathBuf,
    #[arg(
        long,
        conflicts_with = "snapshot",
        help = "Refresh the host graph before evaluating checks"
    )]
    pub refresh: bool,
    #[arg(
        long,
        value_name = "DURATION",
        help = "Maximum graph evidence age, for example 5m"
    )]
    pub max_age: Option<String>,
    #[arg(
        long,
        value_name = "NAME",
        help = "Evaluate a retained graph snapshot instead of current rows"
    )]
    pub snapshot: Option<String>,
    #[arg(
        long,
        value_name = "NAME",
        help = "Compare check outcomes with this snapshot baseline"
    )]
    pub baseline: Option<String>,
}

#[derive(Args)]
pub struct ImpactArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum GraphFormat {
    Human,
    Dot,
    Mermaid,
}

#[derive(Args)]
pub struct WhyArgs {
    #[arg(
        long,
        help = "Settings file path (database stays in the XDG data directory)"
    )]
    pub config: Option<PathBuf>,
    #[arg(help = "Node id or service name whose relationships to explain")]
    pub target: String,
}
