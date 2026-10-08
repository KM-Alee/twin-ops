use twin_core::lexical_canonical;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub architecture: String,
}

pub fn parse_dpkg_status(text: &str) -> Vec<InstalledPackage> {
    let text = text.replace("\r\n", "\n");
    let mut out = Vec::new();
    for paragraph in text.split("\n\n") {
        let Some(package) = installed_package(paragraph) else {
            continue;
        };
        out.push(package);
    }
    out
}

pub fn parse_dpkg_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_end_matches('\r');
        if line.is_empty() || !line.starts_with('/') {
            continue;
        }
        let canonical = lexical_canonical(line);
        if canonical.starts_with('/') {
            out.push(canonical);
        }
    }
    out
}

fn installed_package(paragraph: &str) -> Option<InstalledPackage> {
    let mut name = None;
    let mut architecture = String::new();
    let mut installed = false;
    for line in paragraph.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Package" => name = Some(value.to_string()),
            "Architecture" => architecture = value.to_string(),
            "Status" => {
                installed = value.split_whitespace().nth(2) == Some("installed");
            }
            _ => {}
        }
    }
    let name = name?;
    if !installed || name.is_empty() {
        return None;
    }
    Some(InstalledPackage { name, architecture })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_keeps_installed_packages_only() {
        let text = "\
Package: openssl
Status: install ok installed
Architecture: amd64

Package: oldssl
Status: deinstall ok config-files
Architecture: amd64

not a paragraph
";
        let packages = parse_dpkg_status(text);
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "openssl");
        assert_eq!(packages[0].architecture, "amd64");
    }

    #[test]
    fn list_keeps_absolute_paths() {
        let paths = parse_dpkg_list("/usr/lib/../lib/libssl.so.3\nrelative\n\n");
        assert_eq!(paths, vec!["/usr/lib/libssl.so.3".to_string()]);
    }
}
