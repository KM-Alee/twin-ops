use std::path::Path;

use crate::error::ContainerError;
use crate::fixture::FixtureDocker;
use crate::live::LiveDocker;
use crate::model::{ContainerInspect, ContainerSummary, ImageInspect, PortMapping, VolumeMount};

pub const DOCKER_SOCKET: &str = "/var/run/docker.sock";

pub trait DockerReadOnly {
    fn list_containers(&self) -> Result<Vec<ContainerSummary>, ContainerError>;
    fn inspect_container(&self, id: &str) -> Result<ContainerInspect, ContainerError>;
    fn inspect_image(&self, reference: &str) -> Result<ImageInspect, ContainerError>;
    fn port_mappings(&self, id: &str) -> Result<Vec<PortMapping>, ContainerError>;
    fn mounts(&self, id: &str) -> Result<Vec<VolumeMount>, ContainerError>;
}

pub enum DockerBackend {
    Fixture(FixtureDocker),
    Live(LiveDocker),
}

impl DockerBackend {
    pub fn open(fixture: Option<&Path>, socket: &Path) -> Result<Self, ContainerError> {
        if let Some(root) = fixture {
            return Ok(Self::Fixture(FixtureDocker::open(root)?));
        }
        if !socket.exists() {
            return Err(ContainerError::Unavailable {
                detail: format!("docker socket {} is absent", socket.display()),
            });
        }
        Ok(Self::Live(LiveDocker::new(socket.to_path_buf())))
    }
}

impl DockerReadOnly for DockerBackend {
    fn list_containers(&self) -> Result<Vec<ContainerSummary>, ContainerError> {
        match self {
            Self::Fixture(docker) => docker.list_containers(),
            Self::Live(docker) => docker.list_containers(),
        }
    }

    fn inspect_container(&self, id: &str) -> Result<ContainerInspect, ContainerError> {
        match self {
            Self::Fixture(docker) => docker.inspect_container(id),
            Self::Live(docker) => docker.inspect_container(id),
        }
    }

    fn inspect_image(&self, reference: &str) -> Result<ImageInspect, ContainerError> {
        match self {
            Self::Fixture(docker) => docker.inspect_image(reference),
            Self::Live(docker) => docker.inspect_image(reference),
        }
    }

    fn port_mappings(&self, id: &str) -> Result<Vec<PortMapping>, ContainerError> {
        match self {
            Self::Fixture(docker) => docker.port_mappings(id),
            Self::Live(docker) => docker.port_mappings(id),
        }
    }

    fn mounts(&self, id: &str) -> Result<Vec<VolumeMount>, ContainerError> {
        match self {
            Self::Fixture(docker) => docker.mounts(id),
            Self::Live(docker) => docker.mounts(id),
        }
    }
}
