use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::K8sError;
use crate::model::ClusterView;
use crate::parse::ingest;

pub struct FixtureK8s {
    view: ClusterView,
}

impl FixtureK8s {
    pub fn open(root: &Path) -> Result<Self, K8sError> {
        if !root.is_dir() {
            return Err(K8sError::MissingFixture {
                path: root.to_path_buf(),
            });
        }
        let mut view = ClusterView::default();
        let entries = fs::read_dir(root).map_err(|source| K8sError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| K8sError::Read {
                path: root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if path.is_file() && is_document(&path) {
                paths.push(path);
            }
        }
        paths.sort();
        for path in paths {
            read_document(&path, &mut view);
        }
        view.source_identity = Some("fixture".into());
        view.endpoints_complete = view.warnings.is_empty();
        Ok(Self { view })
    }

    pub fn warnings(&self) -> &[String] {
        &self.view.warnings
    }

    pub fn view(&self) -> &ClusterView {
        &self.view
    }
}

fn is_document(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "yaml" | "yml")
    )
}

fn read_document(path: &Path, view: &mut ClusterView) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            view.warnings
                .push(format!("{}: unreadable document", path.display()));
            return;
        }
    };
    if text.trim().is_empty() {
        view.warnings
            .push(format!("{}: malformed document", path.display()));
        return;
    }
    let extension = path.extension().and_then(|ext| ext.to_str());
    if extension == Some("json") {
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => ingest(path, &value, None, view),
            Err(_) => view
                .warnings
                .push(format!("{}: malformed document", path.display())),
        }
        return;
    }
    match serde_yaml::from_str::<serde_yaml::Value>(&text) {
        Ok(yaml) => match serde_json::to_value(yaml) {
            Ok(value) => ingest(path, &value, None, view),
            Err(_) => view
                .warnings
                .push(format!("{}: malformed document", path.display())),
        },
        Err(_) => view
            .warnings
            .push(format!("{}: malformed document", path.display())),
    }
}
