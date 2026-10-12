mod error;
mod fixture;
mod live;
mod model;
mod wire;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

pub use error::ContainerdError;
pub use fixture::FixtureContainerd;
pub use live::LiveContainerd;
pub use model::{
    ContainerMetadata, ContainerdSnapshot, ImageMetadata, NamespaceSnapshot, TaskMetadata,
    TaskState,
};

pub const CONTAINERD_SOCKET: &str = "/run/containerd/containerd.sock";
const MAX_OBJECTS: usize = 10_000;
const MAX_NAMESPACES: usize = 64;
const SCAN_TIMEOUT: Duration = Duration::from_secs(15);

pub trait ContainerdReadOnly {
    fn list_namespaces(&self) -> Result<Vec<String>, ContainerdError>;
    fn list_containers(&self, namespace: &str) -> Result<Vec<ContainerMetadata>, ContainerdError>;
    fn list_tasks(&self, namespace: &str) -> Result<Vec<TaskMetadata>, ContainerdError>;
    fn list_images(&self, namespace: &str) -> Result<Vec<ImageMetadata>, ContainerdError>;
}

pub fn read_containerd(
    socket: &Path,
    fixture: Option<&Path>,
) -> Result<ContainerdSnapshot, ContainerdError> {
    match fixture {
        Some(root) => read_snapshot(&FixtureContainerd::new(root)),
        None => read_snapshot(&LiveContainerd::new(socket)),
    }
}

pub fn read_snapshot(
    reader: &impl ContainerdReadOnly,
) -> Result<ContainerdSnapshot, ContainerdError> {
    let started = Instant::now();
    let mut names = reader.list_namespaces()?;
    if names.len() > MAX_NAMESPACES || names.iter().any(|name| !valid_identifier(name)) {
        return Err(ContainerdError::Invalid {
            resource: "namespaces",
            detail: "invalid identifier or namespace limit exceeded",
        });
    }
    names.sort();
    names.dedup();
    let mut namespaces = Vec::new();
    let mut complete = true;
    for namespace in &names {
        let mut warnings = Vec::new();
        let containers = read_resource(
            started,
            "containers.list",
            || reader.list_containers(namespace),
            &mut warnings,
        )
        .and_then(|rows| validate_containers(rows, &mut warnings));
        let tasks = read_resource(
            started,
            "tasks.list",
            || reader.list_tasks(namespace),
            &mut warnings,
        )
        .and_then(|rows| validate_tasks(rows, &mut warnings));
        let images = read_resource(
            started,
            "images.list",
            || reader.list_images(namespace),
            &mut warnings,
        )
        .and_then(|rows| validate_images(rows, &mut warnings));
        complete &= warnings.is_empty();
        namespaces.push(NamespaceSnapshot {
            namespace: namespace.clone(),
            containers,
            tasks,
            images,
            warnings,
        });
    }
    Ok(ContainerdSnapshot {
        namespaces,
        namespace_names: names,
        is_complete: complete,
    })
}

fn read_resource<T>(
    started: Instant,
    operation: &'static str,
    read: impl FnOnce() -> Result<Vec<T>, ContainerdError>,
    warnings: &mut Vec<String>,
) -> Option<Vec<T>> {
    if started.elapsed() >= SCAN_TIMEOUT {
        warnings.push(format!("{operation}: whole-scan deadline exceeded"));
        return None;
    }
    match read() {
        Ok(rows) if rows.len() <= MAX_OBJECTS => Some(rows),
        Ok(_) => {
            warnings.push(format!("{operation}: object limit exceeded"));
            None
        }
        Err(error) => {
            warnings.push(error.to_string());
            None
        }
    }
}

pub(crate) fn valid_identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && id != "."
        && id != ".."
}

fn valid_image(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'@')
        })
        && !name.contains("://")
        && name
            .split_once('@')
            .is_none_or(|(_, digest)| valid_digest(digest))
}

fn validate_containers(
    rows: Vec<ContainerMetadata>,
    warnings: &mut Vec<String>,
) -> Option<Vec<ContainerMetadata>> {
    let mut ids = BTreeSet::new();
    if rows.iter().any(|row| {
        !valid_identifier(&row.id)
            || !ids.insert(&row.id)
            || row
                .image
                .as_deref()
                .is_some_and(|image| !valid_image(image))
    }) {
        warnings.push("containers.list: malformed or duplicate identifiers; list discarded".into());
        None
    } else {
        Some(rows)
    }
}

fn validate_tasks(
    rows: Vec<TaskMetadata>,
    warnings: &mut Vec<String>,
) -> Option<Vec<TaskMetadata>> {
    let mut ids = BTreeSet::new();
    if rows
        .iter()
        .any(|row| !valid_identifier(&row.container_id) || !ids.insert(&row.container_id))
    {
        warnings.push("tasks.list: malformed or duplicate identifiers; list discarded".into());
        None
    } else {
        Some(rows)
    }
}

fn validate_images(
    rows: Vec<ImageMetadata>,
    warnings: &mut Vec<String>,
) -> Option<Vec<ImageMetadata>> {
    let mut ids = BTreeSet::new();
    if rows.iter().any(|row| {
        !valid_image(&row.name)
            || !ids.insert(&row.name)
            || row
                .digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
    }) {
        warnings.push("images.list: malformed or duplicate identifiers; list discarded".into());
        None
    } else {
        Some(rows)
    }
}

fn valid_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
