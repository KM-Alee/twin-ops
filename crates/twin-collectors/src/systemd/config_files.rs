use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::systemd::unit_paths::EffectiveUnit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileSource {
    SystemdUnitFile,
    SystemdDropIn,
    KnownServiceConfigPath,
}

impl ConfigFileSource {
    pub fn metadata_key(self) -> &'static str {
        match self {
            Self::SystemdUnitFile => "systemd_unit_file",
            Self::SystemdDropIn => "systemd_drop_in",
            Self::KnownServiceConfigPath => "known_service_config_path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfigFileDiscovery {
    pub service_unit: String,
    pub file_path: PathBuf,
    pub source: ConfigFileSource,
}

pub fn discover_unit_file_paths(units: &[EffectiveUnit]) -> Vec<ServiceConfigFileDiscovery> {
    units
        .iter()
        .filter(|u| u.unit_name.ends_with(".service"))
        .map(|u| ServiceConfigFileDiscovery {
            service_unit: u.unit_name.clone(),
            file_path: u.main_path.clone(),
            source: ConfigFileSource::SystemdUnitFile,
        })
        .collect()
}

pub fn discover_drop_in_paths(
    search_roots: &[PathBuf],
    service_units: &HashSet<String>,
) -> Vec<ServiceConfigFileDiscovery> {
    let mut out = Vec::new();
    for root in search_roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || path.extension().is_none_or(|e| e != "d") {
                continue;
            }
            let unit_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let unit_name = match normalize_dropin_unit(unit_stem) {
                Some(n) => n,
                None => continue,
            };
            if !service_units.contains(&unit_name) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&path) else {
                continue;
            };
            let mut dropins: Vec<PathBuf> = files
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            dropins.sort();
            for dropin in dropins {
                out.push(ServiceConfigFileDiscovery {
                    service_unit: unit_name.clone(),
                    file_path: dropin,
                    source: ConfigFileSource::SystemdDropIn,
                });
            }
        }
    }
    out
}

fn normalize_dropin_unit(stem: &str) -> Option<String> {
    if stem.ends_with(".service") {
        return crate::systemd::unit_parse::normalize_unit_name(stem);
    }
    None
}

const KNOWN_SERVICE_CONFIGS: &[(&str, &str)] = &[("nginx.service", "/etc/nginx/nginx.conf")];

pub fn discover_known_config_paths(
    existing_services: &HashSet<String>,
    file_exists: impl Fn(&Path) -> bool,
) -> Vec<ServiceConfigFileDiscovery> {
    let mut out = Vec::new();
    for (service, path) in KNOWN_SERVICE_CONFIGS {
        if !existing_services.contains(*service) {
            continue;
        }
        let path = PathBuf::from(path);
        if file_exists(&path) {
            out.push(ServiceConfigFileDiscovery {
                service_unit: (*service).to_string(),
                file_path: path,
                source: ConfigFileSource::KnownServiceConfigPath,
            });
        }
    }
    out
}

pub fn discover_service_config_files(
    units: &[EffectiveUnit],
    search_roots: &[PathBuf],
    existing_services: &HashSet<String>,
    file_exists: impl Fn(&Path) -> bool,
) -> Vec<ServiceConfigFileDiscovery> {
    let service_units: HashSet<String> = units
        .iter()
        .filter(|u| u.unit_name.ends_with(".service"))
        .map(|u| u.unit_name.clone())
        .collect();
    let mut out = discover_unit_file_paths(units);
    out.extend(discover_drop_in_paths(search_roots, &service_units));
    out.extend(discover_known_config_paths(existing_services, file_exists));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::unit_parse::UnitDependencies;
    use std::fs;
    use tempfile::TempDir;

    fn unit(name: &str, path: PathBuf) -> EffectiveUnit {
        EffectiveUnit {
            unit_name: name.to_string(),
            main_path: path,
            dependencies: UnitDependencies::default(),
            socket_config: None,
        }
    }

    #[test]
    fn discovers_unit_and_drop_in_files_for_service() {
        let root = TempDir::new().expect("temp");
        let unit_path = root.path().join("nginx.service");
        fs::write(&unit_path, "[Unit]\nDescription=nginx\n").expect("unit");
        let dropin_dir = root.path().join("nginx.service.d");
        fs::create_dir_all(&dropin_dir).expect("dropin dir");
        let dropin = dropin_dir.join("override.conf");
        fs::write(&dropin, "[Service]\nType=forking\n").expect("dropin");
        let units = vec![unit("nginx.service", unit_path.clone())];
        let service_units: HashSet<_> = ["nginx.service".to_string()].into_iter().collect();
        let unit_files = discover_unit_file_paths(&units);
        assert_eq!(unit_files.len(), 1);
        assert_eq!(unit_files[0].source, ConfigFileSource::SystemdUnitFile);
        let dropins = discover_drop_in_paths(&[root.path().to_path_buf()], &service_units);
        assert_eq!(dropins.len(), 1);
        assert_eq!(dropins[0].source, ConfigFileSource::SystemdDropIn);
        assert_eq!(dropins[0].file_path, dropin);
    }

    #[test]
    fn discovers_nginx_known_config_when_service_exists() {
        let services: HashSet<_> = ["nginx.service".to_string()].into_iter().collect();
        let found =
            discover_known_config_paths(&services, |p| p == Path::new("/etc/nginx/nginx.conf"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source, ConfigFileSource::KnownServiceConfigPath);
        assert_eq!(found[0].file_path, PathBuf::from("/etc/nginx/nginx.conf"));
    }

    #[test]
    fn does_not_discover_known_config_for_unrelated_service() {
        let services: HashSet<_> = ["docker.service".to_string()].into_iter().collect();
        let found = discover_known_config_paths(&services, |_| true);
        assert!(found.is_empty());
    }
}
