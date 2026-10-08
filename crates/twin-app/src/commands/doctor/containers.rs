use std::path::Path;

use twin_container::{DockerBackend, DockerReadOnly, DOCKER_SOCKET};

use crate::model::DoctorContainers;

pub(crate) fn assess(socket: &Path) -> DoctorContainers {
    let socket_path = socket.display().to_string();
    if !socket.exists() {
        return DoctorContainers {
            socket_present: false,
            socket_path,
            listed: false,
            container_count: None,
            detail: "socket not found; docker inspect was not attempted".to_string(),
        };
    }
    let docker = match DockerBackend::open(None, socket) {
        Ok(docker) => docker,
        Err(err) => {
            return DoctorContainers {
                socket_present: true,
                socket_path,
                listed: false,
                container_count: None,
                detail: err.to_string(),
            }
        }
    };
    match docker.list_containers() {
        Ok(containers) => DoctorContainers {
            socket_present: true,
            socket_path,
            listed: true,
            container_count: Some(containers.len()),
            detail: "read-only list and inspect".to_string(),
        },
        Err(err) => DoctorContainers {
            socket_present: true,
            socket_path,
            listed: false,
            container_count: None,
            detail: err.to_string(),
        },
    }
}

pub(crate) fn host() -> DoctorContainers {
    assess(Path::new(DOCKER_SOCKET))
}
