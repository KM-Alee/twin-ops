mod containers;
mod core;
mod database;
mod k8s;
mod permissions;

use std::path::Path;

use twin_ebpf::EbpfFacts;

use crate::error::AppError;
use crate::model::{DoctorContainers, DoctorEbpf, DoctorEbpfCheck, DoctorK8s, DoctorResult};
use crate::paths::TwinLayout;

pub fn run(layout: &TwinLayout, config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    run_flags(layout, config_override, false, false, false, None)
}

pub fn run_flags(
    layout: &TwinLayout,
    config_override: Option<&Path>,
    include_ebpf: bool,
    include_containers: bool,
    include_k8s: bool,
    ebpf_facts: Option<EbpfFacts>,
) -> Result<DoctorResult, AppError> {
    let ebpf = if include_ebpf {
        let facts = ebpf_facts.unwrap_or_else(twin_ebpf::host_facts);
        let report = twin_ebpf::assess(&facts);
        Some(map_report(&report))
    } else {
        None
    };
    let containers = if include_containers {
        Some(containers::host())
    } else {
        None
    };
    let k8s = if include_k8s { Some(k8s::host()) } else { None };
    assemble(layout, config_override, ebpf, containers, k8s)
}

pub fn run_ebpf(
    layout: &TwinLayout,
    config_override: Option<&Path>,
    facts: Option<EbpfFacts>,
) -> Result<DoctorResult, AppError> {
    run_flags(layout, config_override, true, false, false, facts)
}

pub fn run_home(config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    run(&layout, config_override)
}

pub fn run_ebpf_home(config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    run_ebpf(&layout, config_override, None)
}

fn assemble(
    layout: &TwinLayout,
    config_override: Option<&Path>,
    ebpf: Option<DoctorEbpf>,
    containers: Option<DoctorContainers>,
    k8s: Option<DoctorK8s>,
) -> Result<DoctorResult, AppError> {
    let config_path = layout.config_file(config_override);
    let db_path = layout.db_file();

    let (scan_quality, scan_quality_error) = if db_path.exists() {
        match twin_store::Store::open(&db_path) {
            Ok(store) if store.is_initialized().unwrap_or(false) => {
                match super::scan_quality::assess_scan_quality(&store) {
                    Ok(assessment) => (Some(assessment), None),
                    Err(error) => (None, Some(error.to_string())),
                }
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    Ok(DoctorResult {
        core: core::check(&config_path),
        database: database::check(&db_path),
        permissions: permissions::check(),
        scan_quality,
        scan_quality_error,
        ebpf,
        containers,
        k8s,
    })
}

fn map_report(report: &twin_ebpf::EbpfReport) -> DoctorEbpf {
    DoctorEbpf {
        kernel: map_check(&report.kernel),
        btf: map_check(&report.btf),
        capabilities: map_check(&report.capabilities),
        exec_tracing: map_check(&report.exec_tracing),
        tcp_tracing: map_check(&report.tcp_tracing),
    }
}

fn map_check(check: &twin_ebpf::EbpfCheck) -> DoctorEbpfCheck {
    DoctorEbpfCheck {
        ok: check.ok,
        status: check.status.to_string(),
        detail: check.detail.clone(),
    }
}
