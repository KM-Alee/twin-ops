use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ContainerError;
use crate::model::{
    parse_container_inspect, parse_container_list, parse_image_inspect, ContainerInspect,
    ContainerSummary, ImageInspect, PortMapping, VolumeMount,
};

pub struct FixtureDocker {
    root: PathBuf,
}

impl FixtureDocker {
    pub fn open(root: &Path) -> Result<Self, ContainerError> {
        let list = root.join("containers").join("list.json");
        if !list.is_file() {
            return Err(ContainerError::Missing {
                path: list,
                detail: "container list fixture is absent".to_string(),
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn list_containers(&self) -> Result<Vec<ContainerSummary>, ContainerError> {
        let path = self.root.join("containers").join("list.json");
        let body = read_text(&path)?;
        parse_container_list(&path.display().to_string(), &body)
    }

    pub fn inspect_container(&self, id: &str) -> Result<ContainerInspect, ContainerError> {
        let path = self.container_inspect_path(id)?;
        let body = read_text(&path)?;
        parse_container_inspect(&path.display().to_string(), &body)
    }

    pub fn inspect_image(&self, reference: &str) -> Result<ImageInspect, ContainerError> {
        let path = self.image_path(reference)?;
        let body = read_text(&path)?;
        parse_image_inspect(&path.display().to_string(), &body, reference)
    }

    pub fn port_mappings(&self, id: &str) -> Result<Vec<PortMapping>, ContainerError> {
        Ok(self.inspect_container(id)?.port_mappings().to_vec())
    }

    pub fn mounts(&self, id: &str) -> Result<Vec<VolumeMount>, ContainerError> {
        Ok(self.inspect_container(id)?.mounts().to_vec())
    }

    fn container_inspect_path(&self, id: &str) -> Result<PathBuf, ContainerError> {
        contained_name(id)?;
        let path = self.root.join("containers").join(format!("{id}.json"));
        if path.is_file() {
            return Ok(path);
        }
        Err(ContainerError::Missing {
            path,
            detail: format!("inspect fixture for container `{id}` is absent"),
        })
    }

    fn image_path(&self, reference: &str) -> Result<PathBuf, ContainerError> {
        contained_name(reference)?;
        let path = self.root.join("images").join(format!("{reference}.json"));
        if path.is_file() {
            return Ok(path);
        }
        Err(ContainerError::Missing {
            path,
            detail: format!("inspect fixture for image `{reference}` is absent"),
        })
    }
}

fn contained_name(name: &str) -> Result<(), ContainerError> {
    if name.is_empty() || name.contains('\0') || name.contains("..") || name.starts_with('/') {
        return Err(ContainerError::Unavailable {
            detail: "docker fixture name is empty or unsafe".to_string(),
        });
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, ContainerError> {
    fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ContainerError::Missing {
                path: path.to_path_buf(),
                detail: source.to_string(),
            }
        } else {
            ContainerError::Read {
                path: path.to_path_buf(),
                source,
            }
        }
    })
}
