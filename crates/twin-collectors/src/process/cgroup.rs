use twin_core::lexical_canonical;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupMembership {
    pub hierarchy_id: String,
    pub controllers: Vec<String>,
    pub path: String,
    pub service_unit: Option<String>,
}

pub fn parse_cgroup_memberships(content: &str) -> (Vec<CgroupMembership>, Vec<(usize, String)>) {
    let mut memberships = Vec::new();
    let mut issues = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_cgroup_line(trimmed) {
            Ok(membership) => memberships.push(membership),
            Err(detail) => issues.push((line_no + 1, detail)),
        }
    }
    (memberships, issues)
}

fn parse_cgroup_line(line: &str) -> Result<CgroupMembership, String> {
    let mut parts = line.splitn(3, ':');
    let hierarchy_id = parts
        .next()
        .ok_or_else(|| "missing hierarchy".to_string())?
        .to_string();
    let controllers_raw = parts
        .next()
        .ok_or_else(|| "missing controllers".to_string())?;
    let path = parts
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .to_string();
    if path.is_empty() {
        return Err("empty cgroup path".to_string());
    }
    let controllers = if controllers_raw.is_empty() {
        Vec::new()
    } else {
        controllers_raw.split(',').map(str::to_string).collect()
    };
    let path = lexical_canonical(&path);
    let service_unit = infer_service_unit(&path);
    Ok(CgroupMembership {
        hierarchy_id,
        controllers,
        path,
        service_unit,
    })
}

pub fn infer_service_unit(cgroup_path: &str) -> Option<String> {
    cgroup_path
        .split('/')
        .filter(|seg| !seg.is_empty())
        .find(|seg| seg.ends_with(".service"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cgroup_v2_line() {
        let (memberships, issues) = parse_cgroup_memberships("0::/system.slice/nginx.service\n");
        assert!(issues.is_empty());
        let m = &memberships[0];
        assert_eq!(m.hierarchy_id, "0");
        assert!(m.controllers.is_empty());
        assert_eq!(m.path, "/system.slice/nginx.service");
        assert_eq!(m.service_unit.as_deref(), Some("nginx.service"));
    }

    #[test]
    fn parses_cgroup_v1_line() {
        let (memberships, _) =
            parse_cgroup_memberships("2:cpu,cpuacct:/system.slice/nginx.service\n");
        let m = &memberships[0];
        assert_eq!(m.hierarchy_id, "2");
        assert_eq!(m.controllers, vec!["cpu", "cpuacct"]);
    }

    #[test]
    fn infers_service_from_nested_path() {
        let unit = infer_service_unit("/system.slice/docker.service/container.scope");
        assert_eq!(unit.as_deref(), Some("docker.service"));
    }

    #[test]
    fn rejects_empty_path() {
        let (_, issues) = parse_cgroup_memberships("0::\n");
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn non_systemd_path_has_no_service() {
        assert!(infer_service_unit("/user.slice/user-1000.slice/session-2.scope").is_none());
    }

    #[test]
    fn malformed_line_keeps_valid_lines() {
        let (memberships, issues) =
            parse_cgroup_memberships("bad-line\n0::/system.slice/app.service\n");
        assert_eq!(issues.len(), 1);
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].service_unit.as_deref(), Some("app.service"));
    }
}
