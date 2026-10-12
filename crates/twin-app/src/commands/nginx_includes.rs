use std::collections::{BTreeSet, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::scan_config_files::{host_path_for_discovery, ConfigParseWarning};
use twin_collectors::{ConfigFileSource, ServiceConfigFileDiscovery};

const MAX_FILES: usize = 512;
const MAX_BYTES: u64 = 1024 * 1024;

pub(crate) fn read_config(path: &Path) -> Result<Vec<u8>, &'static str> {
    let file = std::fs::File::open(path).map_err(|_| "config file is unreadable or vanished")?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "config file could not be read")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("config file exceeds the 1 MiB read limit");
    }
    Ok(bytes)
}

pub(crate) fn discover(
    initial: &[ServiceConfigFileDiscovery],
) -> (Vec<ServiceConfigFileDiscovery>, Vec<ConfigParseWarning>) {
    let mut queue: VecDeque<(PathBuf, PathBuf, String)> = initial
        .iter()
        .filter(|item| {
            item.service_unit == "nginx.service"
                && item
                    .file_path
                    .file_name()
                    .is_some_and(|name| name == "nginx.conf")
        })
        .map(|item| {
            (
                item.file_path.clone(),
                item.file_path
                    .parent()
                    .unwrap_or(Path::new("/etc/nginx"))
                    .to_path_buf(),
                item.service_unit.clone(),
            )
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut discoveries = Vec::new();
    let mut warnings = Vec::new();
    while let Some((logical, prefix, service)) = queue.pop_front() {
        let host = host_path_for_discovery(&logical.to_string_lossy());
        let identity = std::fs::canonicalize(&host).unwrap_or_else(|_| host.clone());
        if !seen.insert(identity) {
            continue;
        }
        if seen.len() > MAX_FILES {
            warnings.push(warning(
                &logical,
                "nginx include expansion reached its 512-file limit",
            ));
            break;
        }
        let bytes = match read_config(&host) {
            Ok(bytes) => bytes,
            Err(detail) => {
                warnings.push(warning(&logical, detail));
                continue;
            }
        };
        for include in twin_config::parse_nginx(&String::from_utf8_lossy(&bytes)).includes {
            let pattern = if Path::new(&include).is_absolute() {
                PathBuf::from(include)
            } else {
                prefix.join(include)
            };
            match expand(&pattern) {
                Ok(paths) => {
                    for path in paths {
                        discoveries.push(ServiceConfigFileDiscovery {
                            service_unit: service.clone(),
                            file_path: path.clone(),
                            source: ConfigFileSource::KnownServiceConfigPath,
                        });
                        if queue.len() + seen.len() < MAX_FILES {
                            queue.push_back((path, prefix.clone(), service.clone()));
                        } else {
                            warnings.push(warning(
                                &logical,
                                "nginx include expansion reached its 512-file limit",
                            ));
                            break;
                        }
                    }
                }
                Err(detail) => warnings.push(warning(&pattern, detail)),
            }
        }
    }
    discoveries.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    discoveries.dedup_by(|a, b| a.file_path == b.file_path && a.service_unit == b.service_unit);
    (discoveries, warnings)
}

fn warning(path: &Path, detail: &str) -> ConfigParseWarning {
    ConfigParseWarning {
        is_nginx: true,
        path: path.display().to_string(),
        detail: detail.into(),
    }
}

fn expand(pattern: &Path) -> Result<Vec<PathBuf>, &'static str> {
    let mut paths = vec![PathBuf::from("/")];
    for component in pattern.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name.to_string_lossy(),
            _ => {
                return Err("nginx include path contains unsupported parent or relative components")
            }
        };
        if name.contains(['[', ']']) {
            return Err("nginx include bracket patterns are unsupported");
        }
        if name.contains(['*', '?']) {
            let mut matches = Vec::new();
            for base in paths {
                let entries = std::fs::read_dir(host_path_for_discovery(&base.to_string_lossy()))
                    .map_err(|_| "nginx include directory is unreadable")?;
                for entry in entries {
                    let entry = entry.map_err(|_| "nginx include directory entry is unreadable")?;
                    if wildcard(&name, &entry.file_name().to_string_lossy()) {
                        matches.push(base.join(entry.file_name()));
                        if matches.len() > MAX_FILES {
                            return Err("nginx include pattern exceeds its 512-path limit");
                        }
                    }
                }
            }
            matches.sort();
            paths = matches;
        } else {
            for path in &mut paths {
                path.push(name.as_ref());
            }
        }
    }
    paths.retain(|path| host_path_for_discovery(&path.to_string_lossy()).is_file());
    if paths.is_empty() && !pattern.to_string_lossy().contains(['*', '?']) {
        return Err("nginx included file is missing or unreadable");
    }
    Ok(paths)
}

fn wildcard(pattern: &str, value: &str) -> bool {
    let value: Vec<char> = value.chars().collect();
    let mut matched = vec![false; value.len() + 1];
    matched[0] = true;
    for ch in pattern.chars() {
        let mut next = vec![false; value.len() + 1];
        if ch == '*' {
            next[0] = matched[0];
            for index in 1..=value.len() {
                next[index] = matched[index] || next[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                next[index] = matched[index - 1] && (ch == '?' || ch == value[index - 1]);
            }
        }
        matched = next;
    }
    matched[value.len()]
}

pub(crate) fn contextual_text(path: &Path, text: &str) -> (String, Vec<ConfigParseWarning>) {
    let prefix = path.parent().unwrap_or(Path::new("/etc/nginx"));
    let mut expansion = IncludeExpansion {
        prefix,
        stack: BTreeSet::new(),
        files: 0,
        bytes: 0,
        warnings: Vec::new(),
    };
    let text = expansion.read(path, Some(text));
    (text, expansion.warnings)
}

struct IncludeExpansion<'a> {
    prefix: &'a Path,
    stack: BTreeSet<PathBuf>,
    files: usize,
    bytes: usize,
    warnings: Vec<ConfigParseWarning>,
}

impl IncludeExpansion<'_> {
    fn read(&mut self, path: &Path, initial: Option<&str>) -> String {
        let host = host_path_for_discovery(&path.to_string_lossy());
        let identity = std::fs::canonicalize(&host).unwrap_or_else(|_| host.clone());
        if self.files >= MAX_FILES || self.stack.len() >= 32 || self.bytes >= 8 * 1024 * 1024 {
            self.warnings
                .push(warning(path, "nginx contextual include budget exceeded"));
            return String::new();
        }
        if !self.stack.insert(identity.clone()) {
            self.warnings
                .push(warning(path, "nginx include cycle detected"));
            return String::new();
        }
        self.files += 1;
        let text = match initial {
            Some(text) => text.to_string(),
            None => match read_config(&host) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(detail) => {
                    self.warnings.push(warning(path, detail));
                    self.stack.remove(&identity);
                    return String::new();
                }
            },
        };
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > 8 * 1024 * 1024 {
            self.warnings.push(warning(
                path,
                "nginx contextual include byte budget exceeded",
            ));
            self.stack.remove(&identity);
            return String::new();
        }
        let parsed = twin_config::parse_nginx(&text);
        let mut output = String::new();
        let mut cursor = 0;
        for (include, range) in parsed.includes.iter().zip(parsed.include_ranges) {
            output.push_str(&text[cursor..range.start]);
            let pattern = if Path::new(include).is_absolute() {
                PathBuf::from(include)
            } else {
                self.prefix.join(include)
            };
            match expand(&pattern) {
                Ok(paths) => {
                    for child in paths {
                        output.push('\n');
                        output.push_str(&self.read(&child, None));
                        output.push('\n');
                    }
                }
                Err(detail) => self.warnings.push(warning(&pattern, detail)),
            }
            cursor = range.end;
        }
        output.push_str(&text[cursor..]);
        self.stack.remove(&identity);
        output
    }
}
