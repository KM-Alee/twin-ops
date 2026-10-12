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
            containerd: None,
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
                containerd: None,
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
            containerd: None,
            detail: "read-only list and inspect".to_string(),
        },
        Err(err) => DoctorContainers {
            socket_present: true,
            socket_path,
            listed: false,
            container_count: None,
            containerd: None,
            detail: err.to_string(),
        },
    }
}

pub(crate) fn host() -> DoctorContainers {
    let mut result = assess(Path::new(DOCKER_SOCKET));
    let socket = Path::new(twin_container::containerd::CONTAINERD_SOCKET);
    let (listed, namespace_count, detail) = if socket.exists() {
        let reader = twin_container::containerd::LiveContainerd::new(socket);
        match twin_container::containerd::ContainerdReadOnly::list_namespaces(&reader) {
            Ok(namespaces) => (
                true,
                Some(namespaces.len()),
                "read-only namespace list; scan checks containers, tasks and images".to_string(),
            ),
            Err(error) => (false, None, error.to_string()),
        }
    } else {
        (
            false,
            None,
            "socket not found; containerd list was not attempted".to_string(),
        )
    };
    result.containerd = Some(crate::model::DoctorContainerd {
        socket_path: socket.display().to_string(),
        listed,
        namespace_count,
        detail,
    });
    result
}
