use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::systemd::unit_parse::{
    effective_unit_name, merge_dependencies, parse_socket_unit, parse_unit_file,
    template_unit_name_for_instance, SocketUnitConfig, UnitDependencies,
};
use crate::systemd::warning::{SystemdWarning, SystemdWarningKind};

pub fn default_search_paths() -> Vec<PathBuf> {
    [
        "/etc/systemd/system",
        "/run/systemd/system",
        "/usr/lib/systemd/system",
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.is_dir())
    .collect()
}

pub struct EffectiveUnit {
    pub unit_name: String,
    pub main_path: PathBuf,
    pub dependencies: UnitDependencies,
    pub socket_config: Option<SocketUnitConfig>,
}

pub fn discover_units(
    search_roots: &[PathBuf],
    read: &dyn UnitFileReader,
) -> (Vec<EffectiveUnit>, Vec<SystemdWarning>) {
    let mut warnings = Vec::new();
    let mut by_name: BTreeMap<String, (PathBuf, UnitDependencies, Option<SocketUnitConfig>)> =
        BTreeMap::new();

    for root in search_roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            warnings.push(SystemdWarning::new(
                SystemdWarningKind::PathUnreadable,
                root.clone(),
                "cannot read directory",
            ));
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if ext != "service" && ext != "socket" {
                continue;
            }
            let Some(unit_name) = effective_unit_name(&path) else {
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::GlobInstanceSkipped,
                    path.clone(),
                    "template or instance unit skipped",
                ));
                continue;
            };
            let content = match read.read_to_string(&path) {
                Ok(c) if c.trim().is_empty() => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::MaskedEmpty,
                        path.clone(),
                        "empty unit file",
                    ));
                    continue;
                }
                Ok(c) => c,
                Err(detail) => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::PathUnreadable,
                        path.clone(),
                        detail,
                    ));
                    continue;
                }
            };
            let (deps, socket_config) = if ext == "socket" {
                let socket = match parse_socket_unit(&path, &content) {
                    Ok(s) => s,
                    Err(e) => {
                        warnings.push(SystemdWarning::new(
                            SystemdWarningKind::ParseError,
                            path.clone(),
                            e.to_string(),
                        ));
                        continue;
                    }
                };
                (UnitDependencies::default(), Some(socket))
            } else {
                let deps = match parse_unit_file(&path, &content) {
                    Ok(d) => d,
                    Err(e) => {
                        warnings.push(SystemdWarning::new(
                            SystemdWarningKind::ParseError,
                            path.clone(),
                            e.to_string(),
                        ));
                        continue;
                    }
                };
                (deps, None)
            };
            let mut deps = deps;
            if ext == "service" {
                if let Some(template_deps) =
                    template_dependencies_for_instance(&unit_name, search_roots, read)
                {
                    merge_dependencies(&mut deps, template_deps);
                }
            }
            match by_name.get_mut(&unit_name) {
                Some((_, existing_deps, existing_socket)) => {
                    merge_dependencies(existing_deps, deps);
                    if socket_config.is_some() {
                        *existing_socket = socket_config;
                    }
                }
                None => {
                    by_name.insert(unit_name, (path, deps, socket_config));
                }
            }
        }
    }

    for root in search_roots {
        if root.is_dir() {
            apply_dropins(root, read, &mut by_name, &mut warnings);
        }
    }

    let units = by_name
        .into_iter()
        .map(
            |(unit_name, (main_path, dependencies, socket_config))| EffectiveUnit {
                unit_name,
                main_path,
                dependencies,
                socket_config,
            },
        )
        .collect();
    (units, warnings)
}

fn template_dependencies_for_instance(
    instance_unit: &str,
    search_roots: &[PathBuf],
    read: &dyn UnitFileReader,
) -> Option<UnitDependencies> {
    let template_name = template_unit_name_for_instance(instance_unit)?;
    for root in search_roots {
        let path = root.join(&template_name);
        if !path.is_file() {
            continue;
        }
        let content = read.read_to_string(&path).ok()?;
        return parse_unit_file(&path, &content).ok();
    }
    None
}

fn apply_dropins(
    root: &Path,
    read: &dyn UnitFileReader,
    by_name: &mut BTreeMap<String, (PathBuf, UnitDependencies, Option<SocketUnitConfig>)>,
    warnings: &mut Vec<SystemdWarning>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.extension().map(|e| e == "d").unwrap_or(false) {
            continue;
        }
        let unit_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let unit_name = match normalize_dropin_unit(unit_stem) {
            Some(n) => n,
            None => continue,
        };
        let Some((_, existing, _)) = by_name.get_mut(&unit_name) else {
            continue;
        };
        let mut dropin_paths: Vec<PathBuf> = Vec::new();
        if let Ok(files) = fs::read_dir(&path) {
            for f in files.flatten() {
                let p = f.path();
                if p.is_file() {
                    dropin_paths.push(p);
                }
            }
        }
        dropin_paths.sort();
        for dropin in dropin_paths {
            let content = match read.read_to_string(&dropin) {
                Ok(c) => c,
                Err(detail) => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::PathUnreadable,
                        dropin.clone(),
                        detail,
                    ));
                    continue;
                }
            };
            match parse_unit_file(&dropin, &content) {
                Ok(overlay) => merge_dependencies(existing, overlay),
                Err(e) => warnings.push(SystemdWarning::new(
                    SystemdWarningKind::ParseError,
                    dropin,
                    e.to_string(),
                )),
            }
        }
    }
}

fn normalize_dropin_unit(stem: &str) -> Option<String> {
    if stem.ends_with(".service") {
        return crate::systemd::unit_parse::normalize_unit_name(stem);
    }
    None
}

pub trait UnitFileReader {
    fn read_to_string(&self, path: &Path) -> Result<String, String>;
}

pub struct StdUnitFileReader;

impl UnitFileReader for StdUnitFileReader {
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        fs::read_to_string(path).map_err(|e| e.to_string())
    }
}
