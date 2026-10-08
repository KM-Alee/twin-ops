mod commands;
mod config_io;
mod error;
mod model;
pub mod paths;

pub use error::{
    AppError, EmulateError, GraphError, ImpactError, K8sCmdError, ScanError, TemporalError,
    TestCmdError, WatchError,
};
pub use model::{
    CoverageReport, DiffEdge, DiffEdgeChange, DiffNode, DiffNodeChange, DiffResult,
    DoctorContainers, DoctorCore, DoctorDatabase, DoctorEbpf, DoctorEbpfCheck, DoctorK8s,
    DoctorPermissions, DoctorResult, EmulationImpact, EmulationImpactPathView,
    EmulationOverlayNode, EmulationOverlaySummary, EmulationResult, EvidenceStrengthView,
    GraphContainerPort, GraphContainerResult, GraphDirectoryResult, GraphEdgeSummary,
    GraphEvidenceLine, GraphFileResult, GraphLibraryResult, GraphListResult, GraphMountResult,
    GraphNodeResult, GraphNodeSummary, GraphOwnedNode, GraphPackageResult, GraphParentEdge,
    GraphPortResult, GraphResult, GraphServiceResult, GraphUnixSocketResult, ImpactDependent,
    ImpactEvidenceLine, ImpactNodeSummary, ImpactPath, ImpactPathStep, ImpactResult, ImpactUnknown,
    InitResult, K8sGraphResult, K8sImpactResult, K8sScanResult, PermissionMode, RiskAssessment,
    RuntimeDependency, ScanQuality, ScanQualityAssessment, ScanResult, ScanWarning,
    ScanWarningDetail, SnapshotCreateResult, SnapshotEntry, SnapshotListResult, WatchExecEvent,
    WatchResult, WatchTcpEvent, WatchTick, WhatChangedEdge, WhatChangedEdgeDelta, WhatChangedNode,
    WhatChangedNodeDelta, WhatChangedResult,
};
pub use paths::TwinLayout;

pub use crate::commands::scan_quality::assess_scan_quality;

pub const DEFAULT_MAX_DEPTH: usize = 4;
pub const MAX_DEPTH_LIMIT: usize = 8;

use std::path::{Path, PathBuf};

use twin_core::{NodeId, NodeKind};

#[derive(Debug, Clone, Default)]
pub struct InitRequest {
    pub force: bool,
    pub config_override: Option<PathBuf>,
}

pub fn init(request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run_home(request)
}

pub fn init_in(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run(layout, request)
}

pub fn doctor(config_override: Option<&std::path::Path>) -> Result<DoctorResult, AppError> {
    commands::doctor::run_home(config_override)
}

pub fn doctor_in(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
) -> Result<DoctorResult, AppError> {
    commands::doctor::run(layout, config_override)
}

pub fn doctor_ebpf(config_override: Option<&std::path::Path>) -> Result<DoctorResult, AppError> {
    commands::doctor::run_ebpf_home(config_override)
}

pub fn doctor_flags(
    config_override: Option<&std::path::Path>,
    include_ebpf: bool,
    include_containers: bool,
    include_k8s: bool,
) -> Result<DoctorResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    doctor_flags_in(
        &layout,
        config_override,
        include_ebpf,
        include_containers,
        include_k8s,
    )
}

pub fn doctor_flags_in(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
    include_ebpf: bool,
    include_containers: bool,
    include_k8s: bool,
) -> Result<DoctorResult, AppError> {
    commands::doctor::run_flags(
        layout,
        config_override,
        include_ebpf,
        include_containers,
        include_k8s,
        None,
    )
}

pub fn doctor_ebpf_in(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
    facts: Option<twin_ebpf::EbpfFacts>,
) -> Result<DoctorResult, AppError> {
    commands::doctor::run_ebpf(layout, config_override, facts)
}

pub fn read_config(
    path: &std::path::Path,
) -> Result<twin_core::config::TwinConfig, twin_core::error::ConfigError> {
    config_io::read(path)
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub config_override: Option<PathBuf>,
    pub samples: u32,
    pub interval_secs: u64,
}

impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            config_override: None,
            samples: 1,
            interval_secs: 2,
        }
    }
}

impl ScanRequest {
    pub fn default_with_config(config_override: Option<PathBuf>) -> Self {
        Self {
            config_override,
            ..Self::default()
        }
    }
}

pub fn scan(request: ScanRequest) -> Result<ScanResult, AppError> {
    commands::scan::run_home(request, Path::new("/proc"))
}

pub fn scan_in(
    layout: &TwinLayout,
    request: ScanRequest,
    proc_root: &Path,
) -> Result<ScanResult, AppError> {
    commands::scan::run(layout, &request, proc_root)
}

#[derive(Debug, Clone, Default)]
pub struct GraphRequest {
    pub config_override: Option<PathBuf>,
    pub kind: Option<NodeKind>,
    pub target: Option<NodeId>,
    pub target_query: Option<String>,
    pub show_evidence: bool,
}

pub fn graph(request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run_home(request)
}

pub fn graph_in(layout: &TwinLayout, request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run(layout, &request)
}

#[derive(Debug, Clone)]
pub struct ImpactRequest {
    pub config_override: Option<PathBuf>,
    pub target: Option<NodeId>,
    pub target_query: Option<String>,
    pub show_paths: bool,
    pub max_depth: usize,
    pub show_evidence: bool,
}

impl Default for ImpactRequest {
    fn default() -> Self {
        Self {
            config_override: None,
            target: None,
            target_query: None,
            show_paths: false,
            max_depth: DEFAULT_MAX_DEPTH,
            show_evidence: false,
        }
    }
}

pub fn validate_max_depth(max_depth: usize) -> Result<usize, AppError> {
    if max_depth == 0 {
        return Err(AppError::InvalidMaxDepth {
            value: max_depth,
            reason: "max depth must be at least 1".to_string(),
        });
    }
    if max_depth > MAX_DEPTH_LIMIT {
        return Err(AppError::InvalidMaxDepth {
            value: max_depth,
            reason: format!("max depth cannot exceed {MAX_DEPTH_LIMIT}"),
        });
    }
    Ok(max_depth)
}

pub fn impact(request: ImpactRequest) -> Result<ImpactResult, AppError> {
    commands::impact::run_home(request)
}

pub fn impact_in(layout: &TwinLayout, request: ImpactRequest) -> Result<ImpactResult, AppError> {
    commands::impact::run(layout, &request)
}

#[derive(Debug, Clone)]
pub enum EmulateActionRequest {
    Restart {
        target: Option<NodeId>,
        target_query: Option<String>,
        show_paths: bool,
        max_depth: usize,
    },
    DeleteFile {
        path: String,
    },
    FillDisk {
        mount: String,
        to_percent: u8,
    },
    UpgradePackage {
        package: String,
    },
    DeleteK8sPod {
        target: NodeId,
    },
    RolloutK8sDeployment {
        target: NodeId,
    },
}

impl Default for EmulateActionRequest {
    fn default() -> Self {
        Self::Restart {
            target: None,
            target_query: None,
            show_paths: false,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EmulateRequest {
    pub config_override: Option<PathBuf>,
    pub action: EmulateActionRequest,
    pub show_evidence: bool,
}

impl EmulateRequest {
    pub fn restart_query(query: impl Into<String>) -> Self {
        Self {
            action: EmulateActionRequest::Restart {
                target: None,
                target_query: Some(query.into()),
                show_paths: false,
                max_depth: DEFAULT_MAX_DEPTH,
            },
            ..Self::default()
        }
    }
}

pub fn emulate(request: EmulateRequest) -> Result<EmulationResult, AppError> {
    commands::emulate::run_home(request)
}

pub fn emulate_in(
    layout: &TwinLayout,
    request: EmulateRequest,
) -> Result<EmulationResult, AppError> {
    commands::emulate::run(layout, &request)
}

#[derive(Debug, Clone, Default)]
pub struct K8sScanRequest {
    pub config_override: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct K8sTargetRequest {
    pub config_override: Option<PathBuf>,
    pub target: NodeId,
}

pub fn k8s_scan(request: K8sScanRequest) -> Result<K8sScanResult, AppError> {
    commands::k8s::scan_home(commands::k8s::K8sScanRequest {
        config_override: request.config_override,
    })
}

pub fn k8s_scan_in(
    layout: &TwinLayout,
    request: K8sScanRequest,
) -> Result<K8sScanResult, AppError> {
    commands::k8s::scan(
        layout,
        &commands::k8s::K8sScanRequest {
            config_override: request.config_override,
        },
    )
}

pub fn k8s_graph(request: K8sTargetRequest) -> Result<K8sGraphResult, AppError> {
    commands::k8s::graph_home(commands::k8s::K8sTargetRequest {
        config_override: request.config_override,
        target: request.target,
    })
}

pub fn k8s_graph_in(
    layout: &TwinLayout,
    request: K8sTargetRequest,
) -> Result<K8sGraphResult, AppError> {
    commands::k8s::graph(
        layout,
        &commands::k8s::K8sTargetRequest {
            config_override: request.config_override,
            target: request.target,
        },
    )
}

pub fn k8s_impact(request: K8sTargetRequest) -> Result<K8sImpactResult, AppError> {
    commands::k8s::impact_home(commands::k8s::K8sTargetRequest {
        config_override: request.config_override,
        target: request.target,
    })
}

pub fn k8s_impact_in(
    layout: &TwinLayout,
    request: K8sTargetRequest,
) -> Result<K8sImpactResult, AppError> {
    commands::k8s::impact(
        layout,
        &commands::k8s::K8sTargetRequest {
            config_override: request.config_override,
            target: request.target,
        },
    )
}

#[derive(Debug, Clone)]
pub struct WhatChangedRequest {
    pub config_override: Option<PathBuf>,
    pub since: String,
    pub verbose: bool,
}

pub fn what_changed(request: WhatChangedRequest) -> Result<WhatChangedResult, AppError> {
    commands::what_changed::run_home(request)
}

pub fn what_changed_in(
    layout: &TwinLayout,
    request: WhatChangedRequest,
) -> Result<WhatChangedResult, AppError> {
    commands::what_changed::run(layout, &request)
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotCreateRequest {
    pub config_override: Option<PathBuf>,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotListRequest {
    pub config_override: Option<PathBuf>,
}

pub fn snapshot_create(request: SnapshotCreateRequest) -> Result<SnapshotCreateResult, AppError> {
    commands::snapshot::create_home(request)
}

pub fn snapshot_create_in(
    layout: &TwinLayout,
    request: SnapshotCreateRequest,
) -> Result<SnapshotCreateResult, AppError> {
    commands::snapshot::create_in(layout, request)
}

pub fn snapshot_list(request: SnapshotListRequest) -> Result<SnapshotListResult, AppError> {
    commands::snapshot::list_home(request)
}

pub fn snapshot_list_in(
    layout: &TwinLayout,
    request: SnapshotListRequest,
) -> Result<SnapshotListResult, AppError> {
    commands::snapshot::list_in(layout, request)
}

#[derive(Debug, Clone)]
pub struct DiffRequest {
    pub config_override: Option<PathBuf>,
    pub left: String,
    pub right: String,
}

pub fn diff(request: DiffRequest) -> Result<DiffResult, AppError> {
    commands::diff::run_home(request)
}

pub fn diff_in(layout: &TwinLayout, request: DiffRequest) -> Result<DiffResult, AppError> {
    commands::diff::run(layout, &request)
}

pub use commands::ebpf_tcp::{connect_evidence_strength, TcpIngestor};
pub use commands::test_cmd::{init_file, lint_file, run_file, TestInitOutcome, TestInitRequest};
pub use commands::watch::{WatchCommand, WatchEbpf, WatchRun, WatchSink};
pub use twin_test::{CheckKind, CheckResult, CheckStatus, TestCheck, TestDocument, TestRunReport};

pub fn parse_watch(command: &WatchCommand) -> Result<WatchRun, AppError> {
    commands::watch::prepare(command)
}

pub fn watch<S: WatchSink>(
    command: WatchCommand,
    proc_root: &Path,
    stop: &std::sync::atomic::AtomicBool,
    sink: &mut S,
) -> Result<WatchResult, AppError> {
    let paths = paths::resolve_command_paths(command.config_override.as_deref())?;
    let run = commands::watch::prepare(&command)?;
    commands::watch::run(&paths.layout, run, proc_root, stop, sink)
}

pub fn watch_in<S: WatchSink>(
    layout: &TwinLayout,
    run: WatchRun,
    proc_root: &Path,
    stop: &std::sync::atomic::AtomicBool,
    sink: &mut S,
) -> Result<WatchResult, AppError> {
    commands::watch::run(layout, run, proc_root, stop, sink)
}
