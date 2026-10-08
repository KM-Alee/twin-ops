use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubeconfigCheck {
    pub path: String,
    pub present: bool,
}

pub fn kubeconfig_check() -> KubeconfigCheck {
    if let Ok(value) = std::env::var("KUBECONFIG") {
        if let Some(first) = value.split(':').next() {
            if !first.is_empty() {
                let path = PathBuf::from(first);
                let present = path.is_file();
                return KubeconfigCheck {
                    path: path.display().to_string(),
                    present,
                };
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(home).join(".kube").join("config");
    let present = path.is_file();
    KubeconfigCheck {
        path: path.display().to_string(),
        present,
    }
}
