use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::CollectorError;
use crate::systemd::unit_parse::{
    apply_service_environment, apply_socket_dropin, apply_timer_dropin, effective_unit_name,
    merge_dependencies, parse_socket_unit, parse_timer_unit, parse_unit_file,
    template_unit_name_for_instance, ServiceEnvironment, SocketUnitConfig, TimerUnitConfig,
    UnitDependencies,
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
    pub timer_config: Option<TimerUnitConfig>,
    pub environment: ServiceEnvironment,
}

struct ParsedUnit {
    path: PathBuf,
    dependencies: UnitDependencies,
    socket: Option<SocketUnitConfig>,
    timer: Option<TimerUnitConfig>,
    environment: ServiceEnvironment,
    environment_warnings: Vec<&'static str>,
}

fn parse_unit(path: &Path, unit_name: &str, content: &str) -> Result<ParsedUnit, CollectorError> {
    let socket = if unit_name.ends_with(".socket") {
        Some(parse_socket_unit(path, content)?)
    } else {
        None
    };
    let timer = if unit_name.ends_with(".timer") {
        Some(parse_timer_unit(path, content)?)
    } else {
        None
    };
    let dependencies = parse_unit_file(path, content)?;
    let mut environment = ServiceEnvironment::default();
    let environment_warnings = if unit_name.ends_with(".service") {
        apply_service_environment(&mut environment, content)
    } else {
        Vec::new()
    };
    Ok(ParsedUnit {
        path: path.to_path_buf(),
        dependencies,
        socket,
        timer,
        environment,
        environment_warnings,
    })
}

pub fn discover_units(
    search_roots: &[PathBuf],
    read: &dyn UnitFileReader,
    instances: &[String],
) -> (Vec<EffectiveUnit>, Vec<SystemdWarning>) {
    let mut warnings = Vec::new();
    let mut by_name: BTreeMap<String, ParsedUnit> = BTreeMap::new();

    let mut selected_names = BTreeSet::new();
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
            if !path.is_file()
                && fs::read_link(&path).ok().as_deref() != Some(Path::new("/dev/null"))
            {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext, "service" | "socket" | "timer") {
                continue;
            }
            let Some(unit_name) = effective_unit_name(&path) else {
                if path
                    .file_stem()
                    .is_some_and(|stem| stem.to_string_lossy().ends_with('@'))
                {
                    continue;
                }
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::GlobInstanceSkipped,
                    path.clone(),
                    "glob unit filename cannot identify a concrete unit",
                ));
                continue;
            };
            if !selected_names.insert(unit_name.clone()) {
                continue;
            }
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
            match parse_unit(&path, &unit_name, &content) {
                Ok(parsed) => {
                    by_name.insert(unit_name, parsed);
                }
                Err(error) => warnings.push(SystemdWarning::new(
                    SystemdWarningKind::ParseError,
                    path.clone(),
                    error.to_string(),
                )),
            }
        }
    }

    let mut known_instances: BTreeSet<String> = instances.iter().cloned().collect();
    for root in search_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".wants") && !name.ends_with(".requires") {
                continue;
            }
            let Ok(links) = fs::read_dir(entry.path()) else {
                continue;
            };
            for link in links.flatten() {
                if link.file_type().is_ok_and(|kind| kind.is_symlink()) {
                    known_instances.insert(link.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    for unit_name in known_instances {
        if by_name.contains_key(&unit_name)
            || selected_names.contains(&unit_name)
            || effective_unit_name(Path::new(&unit_name)).is_none()
        {
            continue;
        }
        let Some(template) = template_unit_name_for_instance(&unit_name) else {
            continue;
        };
        let Some(path) = search_roots
            .iter()
            .map(|root| root.join(&template))
            .find(|path| fs::symlink_metadata(path).is_ok())
        else {
            continue;
        };
        let content = match read.read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => content,
            Ok(_) => {
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::MaskedEmpty,
                    path,
                    "empty or masked template",
                ));
                continue;
            }
            Err(detail) => {
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::PathUnreadable,
                    path,
                    detail,
                ));
                continue;
            }
        };
        match parse_unit(&path, &unit_name, &content) {
            Ok(parsed) => {
                by_name.insert(unit_name, parsed);
            }
            Err(error) => warnings.push(SystemdWarning::new(
                SystemdWarningKind::ParseError,
                path,
                error.to_string(),
            )),
        }
    }
    apply_dropins(search_roots, read, &mut by_name, &mut warnings);
    for (name, unit) in &mut by_name {
        let ParsedUnit {
            path,
            dependencies: deps,
            socket,
            timer,
            ..
        } = unit;
        for targets in [&mut deps.requires, &mut deps.wants, &mut deps.binds_to] {
            targets.retain_mut(|target| {
                if let Some(expanded) = crate::systemd::unit_parse::expand_specifiers(target, name)
                    .and_then(|value| crate::systemd::unit_parse::normalize_unit_name(&value))
                {
                    *target = expanded;
                    true
                } else {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::ParseError,
                        path.clone(),
                        "unsupported or invalid unit dependency specifier",
                    ));
                    false
                }
            });
            targets.sort();
            targets.dedup();
        }
        if let Some(socket) = socket {
            for addresses in [
                &mut socket.listen_streams,
                &mut socket.listen_datagrams,
                &mut socket.listen_sequential_packets,
                &mut socket.listen_fifos,
            ] {
                addresses.retain_mut(|address| {
                    if let Some(expanded) =
                        crate::systemd::unit_parse::expand_specifiers(address, name)
                    {
                        *address = expanded;
                        true
                    } else {
                        warnings.push(SystemdWarning::new(
                            SystemdWarningKind::ParseError,
                            path.clone(),
                            "unsupported socket address specifier",
                        ));
                        false
                    }
                });
            }
            if let Some(service) = &socket.service {
                if let Some(expanded) = crate::systemd::unit_parse::expand_specifiers(service, name)
                {
                    socket.service = Some(expanded);
                } else {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::ParseError,
                        path.clone(),
                        "unsupported socket service specifier",
                    ));
                }
            }
        }
        if let Some(target) = timer.as_mut().and_then(|timer| timer.unit.as_mut()) {
            match crate::systemd::unit_parse::expand_specifiers(target, name) {
                Some(expanded) => *target = expanded,
                None => warnings.push(SystemdWarning::new(
                    SystemdWarningKind::ParseError,
                    path.clone(),
                    "unsupported timer unit specifier",
                )),
            }
        }
    }

    let units = by_name
        .into_iter()
        .map(|(unit_name, parsed)| EffectiveUnit {
            unit_name,
            main_path: parsed.path,
            dependencies: parsed.dependencies,
            socket_config: parsed.socket,
            timer_config: parsed.timer,
            environment: parsed.environment,
        })
        .collect();
    (units, warnings)
}

fn apply_dropins(
    roots: &[PathBuf],
    read: &dyn UnitFileReader,
    by_name: &mut BTreeMap<String, ParsedUnit>,
    warnings: &mut Vec<SystemdWarning>,
) {
    for (unit, parsed) in by_name {
        let ParsedUnit {
            path: main_path,
            dependencies,
            socket,
            timer,
            environment,
            environment_warnings,
        } = parsed;
        for warning in environment_warnings.drain(..) {
            warnings.push(SystemdWarning::new(
                SystemdWarningKind::ParseError,
                main_path.clone(),
                warning,
            ));
        }
        let is_service = unit.ends_with(".service");
        let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
        for name in dropin_names(unit) {
            for root in roots {
                let directory = root.join(format!("{name}.d"));
                let Ok(entries) = fs::read_dir(directory) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "conf") {
                        files
                            .entry(entry.file_name().to_string_lossy().to_string())
                            .or_insert(path);
                    }
                }
            }
        }
        for path in files.into_values() {
            let content = match read.read_to_string(&path) {
                Ok(content) => content,
                Err(detail) => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::PathUnreadable,
                        path,
                        detail,
                    ));
                    continue;
                }
            };
            let overlay = match parse_unit_file(&path, &content) {
                Ok(overlay) => overlay,
                Err(error) => {
                    warnings.push(SystemdWarning::new(
                        SystemdWarningKind::ParseError,
                        path,
                        error.to_string(),
                    ));
                    continue;
                }
            };
            if let Some(socket) = socket {
                let mut candidate = socket.clone();
                match apply_socket_dropin(&path, &mut candidate, &content) {
                    Ok(()) => *socket = candidate,
                    Err(error) => {
                        warnings.push(SystemdWarning::new(
                            SystemdWarningKind::ParseError,
                            path,
                            error.to_string(),
                        ));
                        continue;
                    }
                }
            }
            if let Some(timer) = timer {
                let mut candidate = timer.clone();
                match apply_timer_dropin(&path, &mut candidate, &content) {
                    Ok(()) => *timer = candidate,
                    Err(error) => {
                        warnings.push(SystemdWarning::new(
                            SystemdWarningKind::ParseError,
                            path,
                            error.to_string(),
                        ));
                        continue;
                    }
                }
            }
            merge_dependencies(dependencies, overlay);
            if !is_service {
                continue;
            }
            for warning in apply_service_environment(environment, &content) {
                warnings.push(SystemdWarning::new(
                    SystemdWarningKind::ParseError,
                    path.clone(),
                    warning,
                ));
            }
        }
    }
}

pub trait UnitFileReader {
    fn read_to_string(&self, path: &Path) -> Result<String, String>;
}

pub struct StdUnitFileReader;

impl UnitFileReader for StdUnitFileReader {
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        const LIMIT: usize = 1024 * 1024;
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > LIMIT {
            return Err("unit file exceeds the 1 MiB read limit".to_string());
        }
        String::from_utf8(bytes).map_err(|_| "unit file is not valid UTF-8".to_string())
    }
}

fn dropin_names(unit: &str) -> Vec<String> {
    let mut names = vec![unit.to_string()];
    if let Some(template) = template_unit_name_for_instance(unit) {
        names.push(template);
    }
    if let Some((stem, suffix)) = unit.rsplit_once('.') {
        let stem = stem.split('@').next().unwrap_or(stem);
        for (position, _) in stem.match_indices('-').rev() {
            names.push(format!("{}.{suffix}", &stem[..position + 1]));
        }
        names.push(suffix.to_string());
    }
    names
}
