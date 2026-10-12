mod intent;
mod script;
mod shell;

use std::path::{Path, PathBuf};

use twin_core::{EdgeKind, GraphEdge, NodeId, NodeKind, NodeState, RiskLevel};
use twin_store::Store;

use crate::error::{AppError, GraphError};
use crate::model::{
    EmulationResult, PreflightAffected, PreflightResult, PreflightStep, PreflightVerdict,
};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::{EmulateActionRequest, EmulateRequest, ScanRequest, DEFAULT_MAX_DEPTH};

use intent::{ContainerVerb, Intent, PackageVerb, ProcessTarget, ServiceVerb};

const MAX_AFFECTED: usize = 8;
const MAX_REASONS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperation {
    Create,
    Edit,
    Delete,
}

/// A file change an agent makes through its own file tool rather than the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub operation: FileOperation,
}

impl FileChange {
    fn label(&self) -> String {
        let verb = match self.operation {
            FileOperation::Create => "create",
            FileOperation::Edit => "edit",
            FileOperation::Delete => "delete",
        };
        format!("{verb} {}", self.path.display())
    }

    fn intent(&self) -> Intent {
        let path = self.path.display().to_string();
        match self.operation {
            FileOperation::Create | FileOperation::Edit => Intent::EditFile { path },
            FileOperation::Delete => Intent::DeleteFile { path },
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreflightRequest {
    pub command: String,
    pub file_changes: Vec<FileChange>,
    pub cwd: Option<PathBuf>,
    pub config_override: Option<PathBuf>,
    /// Rescan the host first when the stored graph is older than this.
    pub max_graph_age_secs: u64,
}

pub fn run_home(request: PreflightRequest) -> Result<PreflightResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request, Path::new("/proc"))
}

pub fn run(
    layout: &TwinLayout,
    request: &PreflightRequest,
    proc_root: &Path,
) -> Result<PreflightResult, AppError> {
    let mut intents = script::expand(
        intent::classify_line(&request.command),
        request.cwd.as_deref(),
    );
    intents.extend(
        request
            .file_changes
            .iter()
            .map(|change| (change.label(), change.intent())),
    );
    let db_path = layout.db_file();
    let needs_graph = intents.iter().any(|(_, intent)| {
        needs_graph(intent)
            && (db_path.exists() || !is_project_path(intent, request.cwd.as_deref()))
    });
    if !needs_graph {
        let steps = intents
            .into_iter()
            .filter_map(|(segment, intent)| {
                offline_step(segment.clone(), &intent)
                    .or_else(|| Some(project_path_step(&segment, &intent, request.cwd.as_deref())))
            })
            .collect::<Vec<_>>();
        return Ok(finish(request, steps, None, Vec::new()));
    }

    crate::config_io::load(layout, request.config_override.as_deref())?;
    if !db_path.exists() {
        return Err(GraphError::DatabaseNotInitialized.into());
    }
    let graph_age_secs = refresh_if_stale(
        layout,
        request.config_override.as_deref(),
        request.max_graph_age_secs,
        proc_root,
    )?;
    let store = Store::open(&db_path).map_err(GraphError::StoreOpen)?;
    let context = Context {
        layout,
        proc_root,
        store: &store,
        config_override: request.config_override.clone(),
        cwd: request.cwd.clone(),
    };
    let mut steps = Vec::new();
    for (segment, intent) in intents {
        if let Some(step) = offline_step(segment.clone(), &intent) {
            steps.push(step);
            continue;
        }
        steps.extend(context.assess(&segment, &intent)?);
    }
    let coverage = super::coverage::load(&store)?
        .unknown_lines()
        .into_iter()
        .map(|line| line.detail)
        .collect();
    Ok(finish(request, steps, Some(graph_age_secs), coverage))
}

fn needs_graph(intent: &Intent) -> bool {
    !matches!(
        intent,
        Intent::ReadOnly { .. }
            | Intent::Unrecognized { .. }
            | Intent::Remote { .. }
            | Intent::Firewall { .. }
            | Intent::K8sDestructive { .. }
            | Intent::HostPower { .. }
            | Intent::Advisory { .. }
            | Intent::Package {
                verb: PackageVerb::SystemUpgrade,
                ..
            }
    )
}

fn intent_path(intent: &Intent) -> Option<&str> {
    match intent {
        Intent::DeleteFile { path } | Intent::DeleteTree { path } | Intent::EditFile { path } => {
            Some(path)
        }
        _ => None,
    }
}

fn absolute_path(path: &str, cwd: Option<&Path>) -> PathBuf {
    let path = Path::new(path);
    match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

/// File operations outside system locations (a build directory, a log file) are
/// routine; without a graph twin cannot link them to any service, so they stay go.
fn is_project_path(intent: &Intent, cwd: Option<&Path>) -> bool {
    intent_path(intent).is_some_and(|path| {
        let path = absolute_path(path, cwd);
        !is_system_path(&path) && !is_top_level_system_path(&path)
    })
}

fn project_path_step(segment: &str, intent: &Intent, cwd: Option<&Path>) -> PreflightStep {
    let path = intent_path(intent)
        .map(|path| absolute_path(path, cwd).display().to_string())
        .unwrap_or_default();
    let mut s = step(segment, format!("change {path}"), PreflightVerdict::Go);
    s.reasons
        .push("outside system paths; twin has no graph to check further".into());
    s
}

pub(crate) fn refresh_if_stale(
    layout: &TwinLayout,
    config_override: Option<&Path>,
    max_age_secs: u64,
    proc_root: &Path,
) -> Result<u64, AppError> {
    let now = twin_core::TimestampNs::now().as_i64();
    let last = {
        let store = Store::open(&layout.db_file()).map_err(GraphError::StoreOpen)?;
        if !store.is_initialized().map_err(GraphError::Store)? {
            return Err(GraphError::DatabaseNotInitialized.into());
        }
        super::coverage::latest_run_at(&store, twin_collectors::COLLECTOR_NAME, i64::MAX)
            .map_err(GraphError::Store)?
            .map(|run| run.ended_at_ns)
    };
    let age = last.map(|ended| (now.saturating_sub(ended).max(0) / 1_000_000_000) as u64);
    match age {
        Some(age) if age <= max_age_secs => Ok(age),
        _ => {
            let scan = ScanRequest::default_with_config(config_override.map(Path::to_path_buf));
            super::scan::run(layout, &scan, proc_root)?;
            Ok(0)
        }
    }
}

fn finish(
    request: &PreflightRequest,
    steps: Vec<PreflightStep>,
    graph_age_secs: Option<u64>,
    coverage_notes: Vec<String>,
) -> PreflightResult {
    let verdict = steps
        .iter()
        .map(|step| step.verdict)
        .max()
        .unwrap_or(PreflightVerdict::Go);
    let has_operations = steps
        .iter()
        .any(|step| step.is_assessed || step.verdict != PreflightVerdict::Go);
    let headline = match steps
        .iter()
        .filter(|step| step.verdict == verdict)
        .find(|step| step.is_assessed || verdict != PreflightVerdict::Go)
    {
        Some(step) => match step.reasons.first() {
            Some(reason) => format!("{} — {reason}", step.operation),
            None => step.operation.clone(),
        },
        None => "no disruptive host operation recognized".to_string(),
    };
    let coverage_notes = if has_operations {
        coverage_notes
    } else {
        Vec::new()
    };
    let mut described = vec![request.command.clone()];
    described.extend(request.file_changes.iter().map(FileChange::label));
    PreflightResult {
        command: described
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        verdict,
        headline,
        has_operations,
        steps,
        graph_age_secs,
        coverage_notes,
        action_performed: false,
    }
}

fn step(segment: &str, operation: impl Into<String>, verdict: PreflightVerdict) -> PreflightStep {
    PreflightStep {
        segment: segment.to_string(),
        operation: operation.into(),
        target: None,
        verdict,
        is_assessed: false,
        risk: None,
        evidence: None,
        reasons: Vec::new(),
        affected: Vec::new(),
        affected_total: 0,
        advice: Vec::new(),
    }
}

fn offline_step(segment: String, intent: &Intent) -> Option<PreflightStep> {
    let mut out = match intent {
        Intent::ReadOnly { tool } => step(
            &segment,
            format!("{tool}: no disruptive operation"),
            PreflightVerdict::Go,
        ),
        Intent::Unrecognized { tool } => step(
            &segment,
            format!("{tool}: not a host operation twin models"),
            PreflightVerdict::Go,
        ),
        Intent::Remote { tool } => {
            let mut s = step(
                &segment,
                format!("{tool}: acts on another host"),
                PreflightVerdict::Go,
            );
            s.reasons.push(
                "twin models only this machine; run twin preflight on the remote host".into(),
            );
            s
        }
        Intent::Firewall { tool } => {
            let mut s = step(
                &segment,
                format!("{tool}: firewall change"),
                PreflightVerdict::Caution,
            );
            s.reasons.push(
                "packet-filter rules are not emulated; established and future connections may be affected"
                    .into(),
            );
            s.advice
                .push("list the rule set first and keep an existing SSH session open".into());
            s
        }
        Intent::K8sDestructive { detail } => {
            let mut s = step(
                &segment,
                format!("kubectl {detail}"),
                PreflightVerdict::Caution,
            );
            s.reasons
                .push("this Kubernetes change is not emulated by twin".into());
            s.advice
                .push("preview with `kubectl diff` or `--dry-run=server` first".into());
            s
        }
        Intent::Package {
            verb: PackageVerb::SystemUpgrade,
            ..
        } => {
            let mut s = step(
                &segment,
                "system-wide package upgrade",
                PreflightVerdict::Caution,
            );
            s.reasons.push(
                "the package set is chosen by the package manager; running services may need restarts afterwards"
                    .into(),
            );
            s.advice.push(
                "list pending upgrades, then run `twin preflight` on the specific packages".into(),
            );
            s
        }
        Intent::Advisory {
            operation,
            verdict,
            reason,
        } => {
            let mut s = step(&segment, operation.clone(), *verdict);
            s.reasons.push(reason.to_string());
            s
        }
        Intent::HostPower { action } => {
            let mut s = step(
                &segment,
                format!("{action}: host power change"),
                PreflightVerdict::Stop,
            );
            s.is_assessed = true;
            s.risk = Some(RiskLevel::Critical);
            s.reasons
                .push("every service, container and connection on this host goes down".into());
            s
        }
        _ => return None,
    };
    out.segment = segment;
    Some(out)
}

struct Context<'a> {
    layout: &'a TwinLayout,
    proc_root: &'a Path,
    store: &'a Store,
    config_override: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

impl Context<'_> {
    fn assess(&self, segment: &str, intent: &Intent) -> Result<Vec<PreflightStep>, AppError> {
        Ok(match intent {
            Intent::Service { verb, unit } => vec![self.service(segment, *verb, unit)?],
            Intent::Container { verb, name } => vec![self.container(segment, *verb, name)],
            Intent::DeleteFile { path } => vec![self.delete_file(segment, path)?],
            Intent::DeleteTree { path } => vec![self.delete_tree(segment, path)?],
            Intent::EditFile { path } => vec![self.edit_file(segment, path)?],
            Intent::Package { verb, packages } => packages
                .iter()
                .map(|package| self.package(segment, *verb, package))
                .collect(),
            Intent::KillProcess { target } => self.kill(segment, target)?,
            Intent::K8sRollout {
                namespace,
                deployment,
            } => vec![self.emulated(
                segment,
                format!("rollout restart deployment {namespace}/{deployment}"),
                EmulateActionRequest::RolloutK8sDeployment {
                    target: NodeId::k8s_deployment(namespace, deployment),
                },
                false,
            )],
            Intent::K8sDeletePod { namespace, pod } => vec![self.emulated(
                segment,
                format!("delete pod {namespace}/{pod}"),
                EmulateActionRequest::DeleteK8sPod {
                    target: NodeId::k8s_pod(namespace, pod),
                },
                false,
            )],
            Intent::BlockPort { port } => vec![self.block_port(segment, *port)?],
            Intent::Unmount { target } => vec![self.unmount(segment, target)?],
            Intent::InterfaceDown { interface } => vec![self.interface_down(segment, interface)?],
            Intent::WipeDevice { device } => vec![self.wipe_device(segment, device)?],
            Intent::Compose {
                verb,
                project,
                file,
                services,
            } => vec![self.compose(
                segment,
                *verb,
                project.as_deref(),
                file.as_deref(),
                services,
            )?],
            _ => Vec::new(),
        })
    }

    fn emulate(&self, action: EmulateActionRequest) -> Result<EmulationResult, AppError> {
        super::emulate::run(
            self.layout,
            &EmulateRequest {
                config_override: self.config_override.clone(),
                action,
                show_evidence: false,
            },
        )
    }

    fn emulated(
        &self,
        segment: &str,
        operation: String,
        action: EmulateActionRequest,
        is_outage: bool,
    ) -> PreflightStep {
        match self.emulate(action) {
            Ok(result) => from_emulation(segment, operation, &result, is_outage),
            Err(error) => not_assessed(segment, operation, &error),
        }
    }

    fn active_node(&self, id: &NodeId) -> Result<Option<twin_core::GraphNode>, AppError> {
        Ok(self
            .store
            .get_node_typed(id)
            .map_err(GraphError::Store)?
            .filter(|node| node.state() == NodeState::Active))
    }

    fn service(
        &self,
        segment: &str,
        verb: ServiceVerb,
        unit: &str,
    ) -> Result<PreflightStep, AppError> {
        let label = unit.trim_end_matches(".service");
        let operation = match verb {
            ServiceVerb::Restart => format!("restart {label}"),
            ServiceVerb::Reload => format!("reload {label}"),
            ServiceVerb::Stop => format!("stop {label}"),
            ServiceVerb::DisableNow => format!("disable and stop {label}"),
            ServiceVerb::Disable => format!("disable {label} at boot"),
            ServiceVerb::Start => format!("start {label}"),
        };
        let mut out = match verb {
            ServiceVerb::Start => {
                let mut s = step(segment, operation, PreflightVerdict::Go);
                s.reasons
                    .push("starting a unit does not interrupt running dependents".into());
                s
            }
            ServiceVerb::Reload => {
                let mut s = step(segment, operation, PreflightVerdict::Go);
                s.reasons.push(
                    "reload keeps the process and its listeners; an invalid configuration can still make it fail"
                        .into(),
                );
                s
            }
            ServiceVerb::Disable => {
                let mut s = self.emulated(segment, operation, restart_action(unit), false);
                s.verdict = if s.affected_total > 0 {
                    PreflightVerdict::Caution
                } else {
                    PreflightVerdict::Go
                };
                s.reasons
                    .insert(0, "takes effect at next boot; nothing stops now".into());
                s
            }
            ServiceVerb::Restart | ServiceVerb::Stop | ServiceVerb::DisableNow => self.emulated(
                segment,
                operation,
                restart_action(unit),
                verb != ServiceVerb::Restart,
            ),
        };
        if let Some(target) = out.target.clone() {
            let target = NodeId::service(target.trim_start_matches("service:"));
            if verb != ServiceVerb::Start && verb != ServiceVerb::Disable {
                self.add_exposed_listeners(&mut out, &target, verb == ServiceVerb::Reload)?;
            }
        }
        let critical = critical_access_unit(label);
        if let Some(reason) = critical {
            if matches!(verb, ServiceVerb::Stop | ServiceVerb::DisableNow) {
                out.verdict = PreflightVerdict::Stop;
                out.reasons.insert(0, reason.to_string());
            }
        }
        if let Some(validator) = config_validator(label) {
            if matches!(verb, ServiceVerb::Restart | ServiceVerb::Reload) {
                out.advice
                    .push(format!("validate configuration first: `{validator}`"));
            }
        }
        if verb == ServiceVerb::Restart && out.verdict > PreflightVerdict::Go && has_reload(label) {
            out.advice.push(format!(
                "`systemctl reload {label}` applies configuration without dropping connections"
            ));
        }
        out.reasons.truncate(MAX_REASONS);
        Ok(out)
    }

    fn add_exposed_listeners(
        &self,
        out: &mut PreflightStep,
        service: &NodeId,
        is_reload: bool,
    ) -> Result<(), AppError> {
        let mut exposed = Vec::new();
        for row in self
            .store
            .list_active_edges_from(service.as_str())
            .map_err(GraphError::Store)?
        {
            let Ok(edge) = GraphEdge::try_from(&row) else {
                continue;
            };
            if edge.kind() != EdgeKind::ListensOn || edge.to().kind() != Some(NodeKind::Port) {
                continue;
            }
            let endpoint = edge.to().as_str().trim_start_matches("port:").to_string();
            if !is_loopback_endpoint(&endpoint) && !exposed.contains(&endpoint) {
                exposed.push(endpoint);
            }
        }
        if exposed.is_empty() || is_reload {
            return Ok(());
        }
        exposed.sort();
        let shown = exposed
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        out.reasons.push(format!(
            "serves network clients on {shown}; their connections drop while it is down"
        ));
        if out.verdict == PreflightVerdict::Go {
            out.verdict = PreflightVerdict::Caution;
        }
        Ok(())
    }

    fn container(&self, segment: &str, verb: ContainerVerb, name: &str) -> PreflightStep {
        let operation = match verb {
            ContainerVerb::Restart => format!("restart container {name}"),
            ContainerVerb::Stop => format!("stop container {name}"),
            ContainerVerb::Remove => format!("remove container {name}"),
        };
        let id = if name.starts_with("container:") {
            name.parse().unwrap_or_else(|_| NodeId::container(name))
        } else {
            NodeId::container(name)
        };
        let mut out = self.emulated(
            segment,
            operation,
            EmulateActionRequest::Restart {
                target: Some(id),
                target_query: None,
                show_paths: false,
                max_depth: DEFAULT_MAX_DEPTH,
            },
            verb != ContainerVerb::Restart,
        );
        if verb == ContainerVerb::Remove && out.is_assessed {
            out.reasons.insert(
                0,
                "data written outside mounted volumes is lost with the container".into(),
            );
            out.verdict = out.verdict.max(PreflightVerdict::Caution);
            out.reasons.truncate(MAX_REASONS);
        }
        out
    }

    fn absolute(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        match &self.cwd {
            Some(cwd) => cwd.join(path),
            None => path.to_path_buf(),
        }
    }

    fn delete_file(&self, segment: &str, raw: &str) -> Result<PreflightStep, AppError> {
        let path = self.absolute(raw);
        let display = path.display().to_string();
        let file = NodeId::file(&display);
        if self.active_node(&file)?.is_some() {
            return Ok(self.emulated(
                segment,
                format!("delete {display}"),
                EmulateActionRequest::DeleteFile {
                    path: display.clone(),
                },
                true,
            ));
        }
        Ok(untracked_path(segment, format!("delete {display}"), &path))
    }

    fn delete_tree(&self, segment: &str, raw: &str) -> Result<PreflightStep, AppError> {
        let path = self.absolute(raw);
        let display = path.display().to_string();
        let operation = format!("recursively delete {display}");
        if is_top_level_system_path(&path) {
            let mut s = step(segment, operation, PreflightVerdict::Stop);
            s.is_assessed = true;
            s.risk = Some(RiskLevel::Critical);
            s.reasons
                .push("removes a top-level system directory; the host may not boot".into());
            return Ok(s);
        }
        let prefix = format!("{}/", display.trim_end_matches('/'));
        let mut affected = Vec::new();
        for kind in [NodeKind::File, NodeKind::Directory] {
            for node in self
                .store
                .list_nodes_by_kind_typed(kind)
                .map_err(GraphError::Store)?
            {
                if node.state() != NodeState::Active {
                    continue;
                }
                let node_path = node
                    .id()
                    .as_str()
                    .split_once(':')
                    .map(|(_, path)| path)
                    .unwrap_or_default();
                if node_path != display && !node_path.starts_with(&prefix) {
                    continue;
                }
                for user in self.users_of(node.id())? {
                    if !affected.iter().any(|a: &PreflightAffected| a.id == user.id) {
                        affected.push(PreflightAffected {
                            effect: format!("uses {node_path}"),
                            ..user
                        });
                    }
                }
            }
        }
        if affected.is_empty() {
            return Ok(untracked_path(segment, operation, &path));
        }
        let mut s = step(segment, operation, PreflightVerdict::Stop);
        s.is_assessed = true;
        s.risk = Some(RiskLevel::High);
        s.reasons.push(format!(
            "{} running service(s) or container(s) read configuration or data under this path",
            affected.len()
        ));
        s.affected_total = affected.len();
        affected.truncate(MAX_AFFECTED);
        s.affected = affected;
        Ok(s)
    }

    fn users_of(&self, node: &NodeId) -> Result<Vec<PreflightAffected>, AppError> {
        let mut users = Vec::new();
        let edges = self
            .store
            .list_active_edges_to(node.as_str())
            .map_err(GraphError::Store)?;
        for row in edges {
            let Ok(edge) = GraphEdge::try_from(&row) else {
                continue;
            };
            if !matches!(
                edge.kind(),
                EdgeKind::ConfiguredBy | EdgeKind::Uses | EdgeKind::MountsVolume
            ) {
                continue;
            }
            if !matches!(
                edge.from().kind(),
                Some(NodeKind::Service | NodeKind::Container)
            ) {
                continue;
            }
            let Some(user) = self.active_node(edge.from())? else {
                continue;
            };
            users.push(PreflightAffected {
                id: user.id().to_string(),
                label: user.label().to_string(),
                effect: String::new(),
            });
        }
        Ok(users)
    }

    fn edit_file(&self, segment: &str, raw: &str) -> Result<PreflightStep, AppError> {
        let path = self.absolute(raw);
        let display = path.display().to_string();
        let operation = format!("modify {display}");
        let file = NodeId::file(&display);
        if self.active_node(&file)?.is_none() {
            return Ok(untracked_path(segment, operation, &path));
        }
        let users = self.users_of(&file)?;
        let mut s = step(segment, operation, PreflightVerdict::Caution);
        s.is_assessed = true;
        s.target = Some(file.to_string());
        if users.is_empty() {
            s.reasons
                .push("tracked file with no active reader in the graph".into());
            s.verdict = PreflightVerdict::Go;
            return Ok(s);
        }
        s.reasons.push(format!(
            "configuration read by {}; a syntax error breaks its next reload or restart",
            users
                .iter()
                .map(|user| user.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if is_systemd_unit_file(&path) {
            s.advice.push(format!(
                "verify with `systemd-analyze verify {display}`; changes apply after `systemctl daemon-reload`"
            ));
        } else {
            for user in &users {
                let label = user.label.trim_end_matches(".service");
                if let Some(validator) = config_validator(label) {
                    s.advice
                        .push(format!("validate after editing: `{validator}`"));
                }
            }
        }
        s.affected_total = users.len();
        s.affected = users
            .into_iter()
            .take(MAX_AFFECTED)
            .map(|user| PreflightAffected {
                effect: "reads this file".into(),
                ..user
            })
            .collect();
        Ok(s)
    }

    fn package(&self, segment: &str, verb: PackageVerb, package: &str) -> PreflightStep {
        let name = package
            .rsplit('/')
            .next()
            .unwrap_or(package)
            .split(['=', ':'])
            .next()
            .unwrap_or(package);
        let operation = match verb {
            PackageVerb::Remove => format!("remove package {name}"),
            _ => format!("install or upgrade package {name}"),
        };
        if matches!(self.active_node(&NodeId::package(name)), Ok(None)) {
            let verdict = match verb {
                PackageVerb::Remove => PreflightVerdict::Caution,
                _ => PreflightVerdict::Go,
            };
            let mut s = step(segment, operation, verdict);
            s.reasons.push(match verb {
                PackageVerb::Remove => {
                    "package is not in the scanned graph; its users are unknown".into()
                }
                _ => "not currently installed (or not tracked); no running service loads it".into(),
            });
            return s;
        }
        let mut s = self.emulated(
            segment,
            operation,
            EmulateActionRequest::UpgradePackage {
                package: name.to_string(),
            },
            verb == PackageVerb::Remove,
        );
        if verb == PackageVerb::Change && s.affected_total > 0 {
            s.advice
                .push("running services keep the old libraries mapped until they restart".into());
        }
        s
    }

    fn kill(&self, segment: &str, target: &ProcessTarget) -> Result<Vec<PreflightStep>, AppError> {
        let processes = match target {
            ProcessTarget::Pid(pid) => self
                .active_node(&NodeId::process(*pid))?
                .into_iter()
                .collect::<Vec<_>>(),
            ProcessTarget::Name(name) => self
                .store
                .list_nodes_by_kind_typed(NodeKind::Process)
                .map_err(GraphError::Store)?
                .into_iter()
                .filter(|node| node.state() == NodeState::Active && node.label() == name)
                .collect(),
        };
        let label = match target {
            ProcessTarget::Pid(pid) => format!("pid {pid}"),
            ProcessTarget::Name(name) => name.clone(),
        };
        if processes.is_empty() {
            let mut s = step(segment, format!("kill {label}"), PreflightVerdict::Go);
            s.reasons
                .push("no matching process in the scanned graph".into());
            return Ok(vec![s]);
        }
        let mut services = Vec::new();
        for process in &processes {
            for row in self
                .store
                .list_active_edges_to(process.id().as_str())
                .map_err(GraphError::Store)?
            {
                let Ok(edge) = GraphEdge::try_from(&row) else {
                    continue;
                };
                if edge.kind() == EdgeKind::Owns
                    && edge.from().kind() == Some(NodeKind::Service)
                    && !services.contains(edge.from())
                {
                    services.push(edge.from().clone());
                }
            }
        }
        if services.is_empty() {
            let mut s = step(segment, format!("kill {label}"), PreflightVerdict::Caution);
            s.is_assessed = true;
            s.reasons.push(format!(
                "{} process(es) not owned by a tracked service; their clients are not modeled",
                processes.len()
            ));
            return Ok(vec![s]);
        }
        let mut steps = Vec::new();
        for service in services {
            let unit = service.as_str().trim_start_matches("service:").to_string();
            let mut s = self.service(segment, ServiceVerb::Stop, &unit)?;
            s.operation = format!("kill {label} (part of {unit})");
            s.reasons.push(
                "systemd may restart it if the unit sets Restart=; dependents still see an interruption"
                    .into(),
            );
            s.reasons.truncate(MAX_REASONS);
            steps.push(s);
        }
        Ok(steps)
    }

    fn mounts(&self) -> Result<Vec<twin_core::GraphNode>, AppError> {
        Ok(self
            .store
            .list_nodes_by_kind_typed(NodeKind::Mount)
            .map_err(GraphError::Store)?
            .into_iter()
            .filter(|node| node.state() == NodeState::Active)
            .collect())
    }

    fn mount_users(&self, mount: &NodeId) -> Result<Vec<PreflightAffected>, AppError> {
        let graph = super::graph::run(
            self.layout,
            &crate::GraphRequest {
                config_override: self.config_override.clone(),
                target: Some(mount.clone()),
                ..crate::GraphRequest::default()
            },
        )?;
        let crate::model::GraphResult::Mount(view) = graph else {
            return Ok(Vec::new());
        };
        Ok(view
            .affected
            .into_iter()
            .map(|node| PreflightAffected {
                id: node.id,
                label: node.label,
                effect: format!(
                    "uses paths on {}",
                    mount.as_str().trim_start_matches("mount:")
                ),
            })
            .collect())
    }

    fn mount_outage(
        &self,
        segment: &str,
        operation: String,
        mounts: &[twin_core::GraphNode],
        reason: &str,
    ) -> Result<PreflightStep, AppError> {
        let points = mounts
            .iter()
            .map(|m| m.label().to_string())
            .collect::<Vec<_>>();
        let mut affected: Vec<PreflightAffected> = Vec::new();
        for mount in mounts {
            for user in self.mount_users(mount.id())? {
                if !affected.iter().any(|known| known.id == user.id) {
                    affected.push(user);
                }
            }
        }
        let is_root = points.iter().any(|point| point == "/");
        let verdict = if is_root || !affected.is_empty() {
            PreflightVerdict::Stop
        } else {
            PreflightVerdict::Caution
        };
        let mut s = step(segment, operation, verdict);
        s.is_assessed = true;
        s.risk = Some(if is_root {
            RiskLevel::Critical
        } else if affected.is_empty() {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        });
        s.reasons.push(format!("{reason} {}", points.join(", ")));
        if is_root {
            s.reasons.push("this is the host's root filesystem".into());
        }
        if affected.is_empty() {
            s.reasons.push(
                "no running service is observed using it; open files can still block or break it"
                    .into(),
            );
        } else {
            s.reasons.push(format!(
                "{} running service(s) or container(s) use paths on it",
                affected.len()
            ));
        }
        s.affected_total = affected.len();
        affected.truncate(MAX_AFFECTED);
        s.affected = affected;
        Ok(s)
    }

    fn unmount(&self, segment: &str, raw: &str) -> Result<PreflightStep, AppError> {
        let operation = format!("unmount {raw}");
        if raw.trim_end_matches('/').is_empty() {
            let mut s = step(segment, operation, PreflightVerdict::Stop);
            s.is_assessed = true;
            s.risk = Some(RiskLevel::Critical);
            s.reasons.push("this is the host's root filesystem".into());
            return Ok(s);
        }
        let mounts = if raw.starts_with("/dev/") {
            self.mounts()?
                .into_iter()
                .filter(|mount| {
                    serde_json::from_str::<serde_json::Value>(mount.metadata().as_str())
                        .ok()
                        .and_then(|meta| {
                            meta.get("device")
                                .and_then(|v| v.as_str())
                                .map(|d| d == raw)
                        })
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>()
        } else {
            let path = self.absolute(raw).display().to_string();
            self.active_node(&NodeId::mount(&path))?
                .into_iter()
                .collect()
        };
        if mounts.is_empty() {
            let mut s = step(segment, operation, PreflightVerdict::Caution);
            s.reasons
                .push("not a mount twin observed; its users are unknown".into());
            return Ok(s);
        }
        self.mount_outage(
            segment,
            operation,
            &mounts,
            "removes the filesystem mounted at",
        )
    }

    fn wipe_device(&self, segment: &str, device: &str) -> Result<PreflightStep, AppError> {
        let operation = format!("overwrite {device}");
        let mounts = self
            .mounts()?
            .into_iter()
            .filter(|mount| {
                serde_json::from_str::<serde_json::Value>(mount.metadata().as_str())
                    .ok()
                    .and_then(|meta| {
                        meta.get("device")
                            .and_then(|v| v.as_str())
                            .map(|backing| backing == device || backing.starts_with(device))
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if mounts.is_empty() {
            let mut s = step(segment, operation, PreflightVerdict::Caution);
            s.reasons.push(
                "destroys all data on the device; it backs no filesystem twin observed as mounted"
                    .into(),
            );
            return Ok(s);
        }
        let mut s = self.mount_outage(
            segment,
            operation,
            &mounts,
            "destroys the device backing mounted filesystem(s)",
        )?;
        s.verdict = PreflightVerdict::Stop;
        Ok(s)
    }

    fn interface_down(&self, segment: &str, name: &str) -> Result<PreflightStep, AppError> {
        let operation = format!("take interface {name} down");
        if name == "lo" {
            let mut s = step(segment, operation, PreflightVerdict::Stop);
            s.is_assessed = true;
            s.risk = Some(RiskLevel::Critical);
            s.reasons
                .push("loopback carries every local service-to-service connection".into());
            return Ok(s);
        }
        let read = |file: &str| {
            std::fs::read_to_string(self.proc_root.join("net").join(file)).unwrap_or_default()
        };
        let interfaces = twin_collectors::parse_interfaces(
            &read("route"),
            &read("fib_trie"),
            &read("if_inet6"),
            &read("ipv6_route"),
        );
        let Some(interface) = interfaces.into_iter().find(|i| i.name == name) else {
            let mut s = step(segment, operation, PreflightVerdict::Caution);
            s.reasons
                .push("interface not found in /proc/net; its traffic is unknown".into());
            return Ok(s);
        };
        let mut affected: Vec<PreflightAffected> = Vec::new();
        let mut wildcard = 0usize;
        for row in self
            .store
            .list_active_edges_by_kind(&EdgeKind::ListensOn.to_string())
            .map_err(GraphError::Store)?
        {
            let Ok(edge) = GraphEdge::try_from(&row) else {
                continue;
            };
            let endpoint = edge.to().as_str().trim_start_matches("port:");
            let Some(address) = endpoint_address(endpoint) else {
                continue;
            };
            if address.is_unspecified() {
                wildcard += 1;
                continue;
            }
            if !interface.addresses.contains(&address) {
                continue;
            }
            let Some(owner) = self.active_node(edge.from())? else {
                continue;
            };
            if !affected.iter().any(|known| known.id == owner.id().as_str()) {
                affected.push(PreflightAffected {
                    id: owner.id().to_string(),
                    label: owner.label().to_string(),
                    effect: format!("listens on {endpoint}"),
                });
            }
        }
        let mut s = step(segment, operation, PreflightVerdict::Caution);
        s.is_assessed = true;
        s.risk = Some(RiskLevel::Medium);
        if interface.has_default_route {
            s.verdict = PreflightVerdict::Stop;
            s.risk = Some(RiskLevel::Critical);
            s.reasons.push(
                "carries the default route; remote sessions (including SSH) and outbound traffic drop"
                    .into(),
            );
        }
        if !affected.is_empty() {
            s.verdict = PreflightVerdict::Stop;
            s.reasons.push(format!(
                "{} listener owner(s) bound to its addresses become unreachable",
                affected.len()
            ));
        }
        if wildcard > 0 {
            s.reasons.push(format!(
                "{wildcard} wildcard listener(s) stop receiving traffic arriving on this interface"
            ));
        }
        if s.reasons.is_empty() {
            s.reasons
                .push("no default route or bound listeners observed on it".into());
        }
        s.affected_total = affected.len();
        affected.truncate(MAX_AFFECTED);
        s.affected = affected;
        Ok(s)
    }

    fn compose(
        &self,
        segment: &str,
        verb: ContainerVerb,
        project: Option<&str>,
        file: Option<&str>,
        services: &[String],
    ) -> Result<PreflightStep, AppError> {
        let project = project
            .map(str::to_string)
            .or_else(|| {
                file.map(|file| self.absolute(file)).and_then(|path| {
                    path.parent()
                        .and_then(|dir| dir.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                })
            })
            .or_else(|| {
                self.cwd
                    .as_ref()
                    .and_then(|cwd| cwd.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .map(|name| compose_project_name(&name));
        let verb_label = match verb {
            ContainerVerb::Restart => "restart",
            ContainerVerb::Stop => "stop",
            ContainerVerb::Remove => "take down",
        };
        let Some(project) = project else {
            let mut s = step(
                segment,
                format!("compose {verb_label}"),
                PreflightVerdict::Caution,
            );
            s.reasons
                .push("could not determine the Compose project; pass -p or -f".into());
            return Ok(s);
        };
        let operation = format!("compose {verb_label} project {project}");
        let members = self
            .store
            .list_nodes_by_kind_typed(NodeKind::Container)
            .map_err(GraphError::Store)?
            .into_iter()
            .filter(|node| node.state() == NodeState::Active)
            .filter(|node| {
                let meta: serde_json::Value =
                    serde_json::from_str(node.metadata().as_str()).unwrap_or_default();
                meta.get("compose_project").and_then(|v| v.as_str()) == Some(project.as_str())
                    && (services.is_empty()
                        || meta
                            .get("compose_service")
                            .and_then(|v| v.as_str())
                            .is_some_and(|service| services.iter().any(|s| s == service)))
            })
            .collect::<Vec<_>>();
        if members.is_empty() {
            let mut s = step(segment, operation, PreflightVerdict::Caution);
            s.reasons.push(format!(
                "no containers of Compose project {project} were observed; its impact is unknown"
            ));
            return Ok(s);
        }
        let mut combined = step(segment, operation, PreflightVerdict::Go);
        combined.is_assessed = true;
        let mut affected: Vec<PreflightAffected> = Vec::new();
        let mut downstream = Vec::new();
        for member in &members {
            let one = self.container(segment, verb, member.label());
            combined.verdict = combined.verdict.max(one.verdict);
            affected.push(PreflightAffected {
                id: member.id().to_string(),
                label: member.label().to_string(),
                effect: format!("container {verb_label}"),
            });
            for item in one.affected {
                if !affected.iter().any(|known| known.id == item.id) {
                    downstream.push(item.label.clone());
                    affected.push(item);
                }
            }
        }
        if verb != ContainerVerb::Restart {
            combined.verdict = combined.verdict.max(PreflightVerdict::Caution);
        }
        combined.reasons.push(format!(
            "{} container(s) of project {project}: {}",
            members.len(),
            members
                .iter()
                .take(5)
                .map(|m| m.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !downstream.is_empty() {
            combined.verdict = PreflightVerdict::Stop;
            combined
                .reasons
                .push(format!("also interrupts: {}", downstream.join(", ")));
        }
        if verb == ContainerVerb::Remove {
            combined.reasons.push(
                "`down` removes the containers and their networks; data outside volumes is lost"
                    .into(),
            );
        }
        combined.affected_total = affected.len();
        affected.truncate(MAX_AFFECTED);
        combined.affected = affected;
        combined.reasons.truncate(MAX_REASONS);
        Ok(combined)
    }

    fn block_port(&self, segment: &str, port: u16) -> Result<PreflightStep, AppError> {
        let operation = format!("block port {port}");
        let request = crate::ImpactRequest {
            config_override: self.config_override.clone(),
            target: None,
            target_query: Some(format!(":{port}")),
            show_paths: false,
            max_depth: DEFAULT_MAX_DEPTH,
            show_evidence: false,
        };
        let impact = match super::impact::run(self.layout, &request) {
            Ok(impact) => impact,
            Err(_) => {
                let mut s = step(segment, operation, PreflightVerdict::Go);
                s.reasons.push(format!(
                    "nothing on this host is observed listening on port {port}"
                ));
                return Ok(s);
            }
        };
        let mut s = step(segment, operation, PreflightVerdict::Caution);
        s.is_assessed = true;
        s.target = Some(impact.target.clone());
        s.risk = Some(impact.risk.level);
        s.evidence = Some(impact.evidence_strength.clone());
        let owners = impact
            .listener_owners
            .iter()
            .map(|owner| owner.label.as_str())
            .collect::<Vec<_>>();
        if !owners.is_empty() {
            s.reasons.push(format!(
                "{} listens here: {}",
                impact.target_label,
                owners.join(", ")
            ));
        }
        if !impact.direct_dependents.is_empty() {
            s.verdict = PreflightVerdict::Stop;
            s.reasons.push(format!(
                "{} active client(s) connect to it; remote clients lose access",
                impact.direct_dependents.len()
            ));
        }
        s.affected_total = impact.direct_dependents.len();
        s.affected = impact
            .direct_dependents
            .iter()
            .take(MAX_AFFECTED)
            .map(|dependent| PreflightAffected {
                id: dependent.id.clone(),
                label: dependent.label.clone(),
                effect: dependent.reason.clone(),
            })
            .collect();
        Ok(s)
    }
}

/// The restart command a user would run for a node, suggested as a preflight example.
pub(crate) fn restart_example(kind: &str, label: &str) -> Option<String> {
    match kind {
        "service" => Some(format!(
            "systemctl restart {}",
            label.trim_end_matches(".service")
        )),
        "container" => Some(format!("docker restart {label}")),
        _ => None,
    }
}

/// Compose derives a default project name from a directory name: lowercase, keeping
/// only letters, digits, `_` and `-`.
fn compose_project_name(raw: &str) -> String {
    raw.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect()
}

fn endpoint_address(endpoint: &str) -> Option<std::net::IpAddr> {
    let rest = endpoint.split_once(':').map(|(_, rest)| rest)?;
    let (address, _) = rest.rsplit_once(':')?;
    address
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

fn restart_action(unit: &str) -> EmulateActionRequest {
    EmulateActionRequest::Restart {
        target: None,
        target_query: Some(unit.to_string()),
        show_paths: false,
        max_depth: DEFAULT_MAX_DEPTH,
    }
}

pub(crate) fn from_emulation(
    segment: &str,
    operation: String,
    result: &EmulationResult,
    is_outage: bool,
) -> PreflightStep {
    let mut affected = Vec::new();
    for impact in result
        .transient_impacts
        .iter()
        .chain(&result.runtime_impacts)
        .chain(&result.restart_impacts)
        .chain(&result.persistent_impacts)
        .chain(&result.unknown_impacts)
    {
        if impact.id == result.target
            || affected
                .iter()
                .any(|a: &PreflightAffected| a.id == impact.id)
        {
            continue;
        }
        affected.push(PreflightAffected {
            id: impact.id.clone(),
            label: impact.label.clone(),
            effect: if is_outage {
                outage_effect(&impact.statement)
            } else {
                impact.statement.clone()
            },
        });
    }
    let mut verdict = verdict_for(result.risk.level);
    if is_outage && !affected.is_empty() {
        verdict = PreflightVerdict::Stop;
    }
    let mut reasons = Vec::new();
    if !affected.is_empty() {
        let names = affected
            .iter()
            .take(3)
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let more = affected.len().saturating_sub(3);
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        reasons.push(if is_outage {
            format!(
                "{} dependent(s) stay down until it is back: {names}{tail}",
                affected.len()
            )
        } else {
            format!("{} dependent(s) interrupted: {names}{tail}", affected.len())
        });
    }
    reasons.extend(
        result
            .risk
            .reasons
            .iter()
            .filter(|reason| !reason.contains("dependent"))
            .cloned(),
    );
    if !result.configured_impacts.is_empty() {
        reasons.push(format!(
            "{} unit(s) declare it but show no runtime use",
            result.configured_impacts.len()
        ));
    }
    reasons.truncate(MAX_REASONS);
    let affected_total = affected.len();
    affected.truncate(MAX_AFFECTED);
    PreflightStep {
        segment: segment.to_string(),
        operation,
        target: Some(result.target.clone()),
        verdict,
        is_assessed: true,
        risk: Some(result.risk.level),
        evidence: Some(result.evidence_strength.clone()),
        reasons,
        affected,
        affected_total,
        advice: Vec::new(),
    }
}

fn outage_effect(statement: &str) -> String {
    statement
        .replace("may lose dependency while", "loses its dependency while")
        .replace(" restarts", " is stopped")
}

fn verdict_for(level: RiskLevel) -> PreflightVerdict {
    match level {
        RiskLevel::Low => PreflightVerdict::Go,
        RiskLevel::Medium | RiskLevel::Unknown => PreflightVerdict::Caution,
        RiskLevel::High | RiskLevel::Critical => PreflightVerdict::Stop,
    }
}

fn not_assessed(segment: &str, operation: String, error: &AppError) -> PreflightStep {
    let mut s = step(segment, operation, PreflightVerdict::Caution);
    s.reasons
        .push(format!("could not assess against the graph: {error}"));
    s
}

fn untracked_path(segment: &str, operation: String, path: &Path) -> PreflightStep {
    if is_system_path(path) {
        let mut s = step(segment, operation, PreflightVerdict::Caution);
        s.reasons
            .push("system path that twin does not track; dependents cannot be ruled out".into());
        s
    } else {
        let mut s = step(segment, operation, PreflightVerdict::Go);
        s.reasons
            .push("no running service or container uses this path".into());
        s
    }
}

const SYSTEM_ROOTS: [&str; 13] = [
    "/etc",
    "/usr",
    "/boot",
    "/var/lib",
    "/var/spool",
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/opt",
    "/srv",
    "/root",
    "/var/www",
];

fn is_systemd_unit_file(path: &Path) -> bool {
    let in_unit_dir = [
        "/etc/systemd/",
        "/usr/lib/systemd/",
        "/lib/systemd/",
        "/run/systemd/",
    ]
    .iter()
    .any(|root| path.starts_with(root));
    let unit_suffix = path.extension().is_some_and(|ext| {
        matches!(
            ext.to_str(),
            Some("service" | "socket" | "timer" | "mount" | "path" | "target" | "conf")
        )
    });
    in_unit_dir && unit_suffix
}

fn is_system_path(path: &Path) -> bool {
    SYSTEM_ROOTS.iter().any(|root| path.starts_with(root))
}

fn is_top_level_system_path(path: &Path) -> bool {
    let display = path.display().to_string();
    let trimmed = display.trim_end_matches(['/', '*']);
    trimmed.is_empty()
        || SYSTEM_ROOTS.contains(&trimmed)
        || matches!(trimmed, "/var" | "/home" | "/dev" | "/proc" | "/sys")
}

pub(crate) fn is_loopback_endpoint(endpoint: &str) -> bool {
    let address = endpoint
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    address.starts_with("127.")
        || address.starts_with("[::1]")
        || address.starts_with("::1")
        || address.starts_with("ns:")
}

fn critical_access_unit(label: &str) -> Option<&'static str> {
    match label {
        "sshd" | "ssh" => Some("stopping SSH can lock out remote access to this host"),
        "NetworkManager" | "systemd-networkd" | "networking" | "network" => {
            Some("stopping the network manager can take this host offline")
        }
        "systemd-resolved" => Some("stopping the resolver breaks name resolution host-wide"),
        "dbus" | "dbus-broker" => Some("stopping the system bus breaks most system services"),
        _ => None,
    }
}

fn config_validator(label: &str) -> Option<&'static str> {
    match label {
        "nginx" => Some("nginx -t"),
        "apache2" | "httpd" => Some("apachectl configtest"),
        "sshd" | "ssh" => Some("sshd -t"),
        "haproxy" => Some("haproxy -c -f /etc/haproxy/haproxy.cfg"),
        "caddy" => Some("caddy validate --config /etc/caddy/Caddyfile"),
        "named" | "bind9" => Some("named-checkconf"),
        "postfix" => Some("postfix check"),
        "varnish" => Some("varnishd -C -f /etc/varnish/default.vcl"),
        "docker" => Some("dockerd --validate"),
        _ => None,
    }
}

fn has_reload(label: &str) -> bool {
    matches!(
        label,
        "nginx"
            | "apache2"
            | "httpd"
            | "haproxy"
            | "caddy"
            | "sshd"
            | "ssh"
            | "postfix"
            | "named"
            | "bind9"
            | "php-fpm"
            | "postgresql"
            | "varnish"
    )
}
