use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::{ContainerMetadata, ContainerdError, ContainerdReadOnly, ImageMetadata, TaskMetadata};

pub struct FixtureContainerd {
    root: PathBuf,
}

impl FixtureContainerd {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
    fn resource<T: DeserializeOwned>(
        &self,
        namespace: Option<&str>,
        resource: &'static str,
    ) -> Result<T, ContainerdError> {
        let path = match namespace {
            Some(namespace) => {
                if !super::valid_identifier(namespace) {
                    return Err(ContainerdError::Invalid {
                        resource: "namespace",
                        detail: "unsafe identifier",
                    });
                }
                self.root
                    .join("namespaces")
                    .join(namespace)
                    .join(format!("{resource}.json"))
            }
            None => self.root.join(format!("{resource}.json")),
        };
        let file = File::open(&path).map_err(|source| ContainerdError::Read {
            path: path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        file.take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| ContainerdError::Read {
                path: path.clone(),
                source,
            })?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(ContainerdError::Invalid {
                resource,
                detail: "read limit exceeded",
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| ContainerdError::Invalid {
            resource,
            detail: "malformed fixture",
        })
    }
}

impl ContainerdReadOnly for FixtureContainerd {
    fn list_namespaces(&self) -> Result<Vec<String>, ContainerdError> {
        self.resource(None, "namespaces")
    }
    fn list_containers(&self, namespace: &str) -> Result<Vec<ContainerMetadata>, ContainerdError> {
        self.resource(Some(namespace), "containers")
    }
    fn list_tasks(&self, namespace: &str) -> Result<Vec<TaskMetadata>, ContainerdError> {
        self.resource(Some(namespace), "tasks")
    }
    fn list_images(&self, namespace: &str) -> Result<Vec<ImageMetadata>, ContainerdError> {
        self.resource(Some(namespace), "images")
    }
}
