use std::path::Path;

use crate::error::CollectorError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitDependencies {
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub binds_to: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocketUnitConfig {
    pub listen_streams: Vec<String>,
    pub listen_datagrams: Vec<String>,
    pub listen_sequential_packets: Vec<String>,
    pub service: Option<String>,
    pub is_accept: bool,
    pub listen_fifos: Vec<String>,
}

/// `[Timer]` settings that matter for the graph: the unit the timer starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimerUnitConfig {
    pub unit: Option<String>,
}

pub fn parse_unit_file(path: &Path, content: &str) -> Result<UnitDependencies, CollectorError> {
    let section = parse_ini_section(content, "[Unit]")
        .map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?
        .unwrap_or_default();
    let mut deps = UnitDependencies::default();
    for (key, value) in section {
        if !matches!(key.as_str(), "Requires" | "Wants" | "BindsTo") {
            continue;
        }
        let targets = split_unit_list(&value).map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?;
        match key.as_str() {
            "Requires" => deps.requires.extend(targets),
            "Wants" => deps.wants.extend(targets),
            "BindsTo" => deps.binds_to.extend(targets),
            _ => {}
        }
    }
    Ok(deps)
}

pub fn merge_dependencies(base: &mut UnitDependencies, overlay: UnitDependencies) {
    base.requires.extend(overlay.requires);
    base.wants.extend(overlay.wants);
    base.binds_to.extend(overlay.binds_to);
}

pub fn normalize_unit_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || !trimmed.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.' | '@' | '\\' | '%')
        })
    {
        return None;
    }
    if trimmed.contains('@') {
        let (_, after_at) = trimmed.split_once('@')?;
        let instance = after_at.split('.').next().unwrap_or("");
        if instance.is_empty() {
            return None;
        }
    }
    if [".service", ".target", ".socket", ".timer"]
        .iter()
        .any(|suffix| trimmed.ends_with(suffix))
    {
        return Some(trimmed.to_string());
    }
    if trimmed.contains('.') {
        return None;
    }
    Some(format!("{trimmed}.service"))
}

pub fn parse_socket_unit(path: &Path, content: &str) -> Result<SocketUnitConfig, CollectorError> {
    let section = parse_ini_section(content, "[Socket]")
        .map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?
        .ok_or_else(|| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: "missing [Socket] section".to_string(),
        })?;
    let mut config = SocketUnitConfig::default();
    apply_socket_pairs(path, &mut config, section)?;
    Ok(config)
}

pub(crate) fn apply_socket_dropin(
    path: &Path,
    config: &mut SocketUnitConfig,
    content: &str,
) -> Result<(), CollectorError> {
    if let Some(section) =
        parse_ini_section(content, "[Socket]").map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?
    {
        apply_socket_pairs(path, config, section)?;
    }
    Ok(())
}

fn apply_socket_pairs(
    path: &Path,
    config: &mut SocketUnitConfig,
    section: Vec<(String, String)>,
) -> Result<(), CollectorError> {
    for (key, value) in section {
        if value.is_empty()
            && matches!(
                key.as_str(),
                "ListenStream" | "ListenDatagram" | "ListenSequentialPacket" | "ListenFIFO"
            )
        {
            config.listen_streams.clear();
            config.listen_datagrams.clear();
            config.listen_sequential_packets.clear();
            config.listen_fifos.clear();
            continue;
        }
        let values =
            super::syntax::words(&value).map_err(|detail| CollectorError::SystemdParse {
                path: path.to_path_buf(),
                detail: detail.into(),
            })?;
        match key.as_str() {
            "ListenStream" => config.listen_streams.extend(values),
            "ListenDatagram" => config.listen_datagrams.extend(values),
            "ListenSequentialPacket" => config.listen_sequential_packets.extend(values),
            "ListenFIFO" => config.listen_fifos.extend(values),
            "Accept" => {
                config.is_accept = match value.to_ascii_lowercase().as_str() {
                    "yes" | "true" | "1" | "on" => true,
                    "no" | "false" | "0" | "off" => false,
                    _ => {
                        return Err(CollectorError::SystemdParse {
                            path: path.to_path_buf(),
                            detail: "invalid Accept boolean".into(),
                        })
                    }
                };
            }
            "Service" => {
                config.service = values.into_iter().next();
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn parse_timer_unit(path: &Path, content: &str) -> Result<TimerUnitConfig, CollectorError> {
    let section = parse_ini_section(content, "[Timer]")
        .map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?
        .ok_or_else(|| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: "missing [Timer] section".to_string(),
        })?;
    let mut config = TimerUnitConfig::default();
    apply_timer_pairs(path, &mut config, section)?;
    Ok(config)
}

pub(crate) fn apply_timer_dropin(
    path: &Path,
    config: &mut TimerUnitConfig,
    content: &str,
) -> Result<(), CollectorError> {
    if let Some(section) =
        parse_ini_section(content, "[Timer]").map_err(|detail| CollectorError::SystemdParse {
            path: path.to_path_buf(),
            detail: detail.into(),
        })?
    {
        apply_timer_pairs(path, config, section)?;
    }
    Ok(())
}

fn apply_timer_pairs(
    path: &Path,
    config: &mut TimerUnitConfig,
    section: Vec<(String, String)>,
) -> Result<(), CollectorError> {
    for (key, value) in section {
        if key == "Unit" {
            config.unit = super::syntax::words(&value)
                .map_err(|detail| CollectorError::SystemdParse {
                    path: path.to_path_buf(),
                    detail: detail.into(),
                })?
                .into_iter()
                .next();
        }
    }
    Ok(())
}

/// The unit a timer starts: `Unit=` when set, otherwise the service with the timer's name.
pub fn timer_activation_target(timer_unit: &str, config: &TimerUnitConfig) -> Option<String> {
    if let Some(unit) = &config.unit {
        // A template (`job@.service`) is not a unit a timer can start.
        return (!unit.contains('%') && !unit.contains("@.")).then(|| unit.clone());
    }
    Some(format!("{}.service", timer_unit.strip_suffix(".timer")?))
}

pub fn socket_activation_target(socket_unit: &str, config: &SocketUnitConfig) -> Option<String> {
    let is_accept = config.is_accept
        && (!config.listen_streams.is_empty() || !config.listen_sequential_packets.is_empty());
    if let Some(service) = &config.service {
        if service.contains('%') || (is_accept && !service.ends_with("@.service")) {
            return None;
        }
        return Some(service.clone());
    }
    let base = socket_unit.strip_suffix(".socket")?;
    Some(if is_accept {
        format!("{base}@.service")
    } else {
        format!("{base}.service")
    })
}

pub fn template_unit_name_for_instance(instance_unit: &str) -> Option<String> {
    let (prefix, rest) = instance_unit.split_once('@')?;
    let suffix = rest.rsplit_once('.').map(|(_, s)| s)?;
    Some(format!("{prefix}@.{suffix}"))
}

fn parse_ini_section(
    content: &str,
    section_header: &str,
) -> Result<Option<Vec<(String, String)>>, &'static str> {
    let mut in_section = false;
    let mut pairs = Vec::new();
    for line in super::syntax::logical_lines(content)? {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line.eq_ignore_ascii_case(section_header);
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    if pairs.is_empty() && !content.contains(section_header) {
        return Ok(None);
    }
    Ok(Some(pairs))
}

fn split_unit_list(value: &str) -> Result<Vec<String>, &'static str> {
    super::syntax::words(value)?
        .iter()
        .filter(|value| !value.is_empty())
        .map(|value| normalize_unit_name(value).ok_or("invalid dependency unit name"))
        .collect()
}

pub fn effective_unit_name(unit_path: &Path) -> Option<String> {
    let file_name = unit_path.file_name()?.to_str()?;
    if file_name.contains('*') || file_name.contains('?') {
        return None;
    }
    if file_name.contains('@') {
        let (_, after_at) = file_name.split_once('@')?;
        let instance = after_at.split('.').next().unwrap_or("");
        if instance.is_empty() {
            return None;
        }
    }
    normalize_unit_name(file_name)
}

pub(crate) fn expand_specifiers(value: &str, unit: &str) -> Option<String> {
    let stem = unit.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(unit);
    let (prefix, instance) = stem.split_once('@').unwrap_or((stem, ""));
    let last = prefix.rsplit('-').next().unwrap_or(prefix);
    let mut chars = value.chars();
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        let expanded = match chars.next()? {
            '%' => "%".to_string(),
            'i' => instance.to_string(),
            'n' => unit.to_string(),
            'N' => stem.to_string(),
            'p' => prefix.to_string(),
            'j' => last.to_string(),
            'I' => unescape_unit(instance)?,
            'P' => unescape_unit(prefix)?,
            'J' => unescape_unit(last)?,
            'f' => {
                let escaped = if instance.is_empty() { stem } else { instance };
                format!("/{}", unescape_unit(escaped)?.trim_start_matches('/'))
            }
            _ => return None,
        };
        result.push_str(&expanded);
    }
    Some(result)
}

fn unescape_unit(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if chars.next()? != 'x' {
                return None;
            }
            let high = chars.next()?.to_digit(16)?;
            let low = chars.next()?.to_digit(16)?;
            let byte = u8::try_from(high * 16 + low).ok()?;
            if byte == 0 {
                return None;
            }
            bytes.push(byte);
        } else {
            let mut encoded = [0; 4];
            let ch = if ch == '-' { '/' } else { ch };
            bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requires_and_wants() {
        let content = r#"
[Unit]
Description=Docker
Requires=containerd.service
Wants=network-online.target
"#;
        let deps = parse_unit_file(Path::new("docker.service"), content).expect("parse");
        assert_eq!(deps.requires, vec!["containerd.service"]);
        assert_eq!(deps.wants, vec!["network-online.target"]);
    }

    #[test]
    fn parses_socket_listen_and_service() {
        let content = r#"
[Socket]
ListenStream=/run/docker.sock
Service=docker.service
"#;
        let cfg = parse_socket_unit(Path::new("docker.socket"), content).expect("parse");
        assert_eq!(cfg.listen_streams, vec!["/run/docker.sock"]);
        assert_eq!(cfg.service.as_deref(), Some("docker.service"));
        assert_eq!(
            socket_activation_target("docker.socket", &cfg).as_deref(),
            Some("docker.service")
        );
    }

    #[test]
    fn allows_instance_unit_names() {
        assert_eq!(
            normalize_unit_name("getty@tty1.service").as_deref(),
            Some("getty@tty1.service")
        );
        assert!(normalize_unit_name("getty@.service").is_none());
    }

    #[test]
    fn merge_appends_dependencies() {
        let mut base = UnitDependencies {
            requires: vec!["a.service".to_string()],
            ..Default::default()
        };
        merge_dependencies(
            &mut base,
            UnitDependencies {
                requires: vec!["b.service".to_string()],
                wants: vec!["c.service".to_string()],
                ..Default::default()
            },
        );
        assert_eq!(base.requires, vec!["a.service", "b.service"]);
        assert_eq!(base.wants, vec!["c.service"]);
    }
}
