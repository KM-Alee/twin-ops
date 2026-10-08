use std::fs;
use std::path::{Path, PathBuf};

const LITERALS: &[&str] = &[
    "ContainerRestart",
    "ContainerStop",
    "ContainerKill",
    "ContainerRemove",
    "ContainerExec",
    "std::process::Command",
    "Command::new",
];

const PATH_MARKERS: &[&str] = &["/stop", "/kill", "/rename"];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scan_file(path: &Path, hits: &mut Vec<String>) {
    let content = fs::read_to_string(path).expect("read");
    for pattern in LITERALS {
        if content.contains(pattern) {
            hits.push(format!("{}: {pattern}", path.display()));
        }
    }
    for marker in PATH_MARKERS {
        if content.contains(marker) {
            hits.push(format!("{}: {marker}", path.display()));
        }
    }
    if contains_container_start(&content) {
        hits.push(format!("{}: /containers/.*/start", path.display()));
    }
}

fn contains_container_start(content: &str) -> bool {
    let mut rest = content;
    while let Some(index) = rest.find("/containers/") {
        let after = &rest[index + "/containers/".len()..];
        if let Some(start_at) = after.find("/start") {
            let boundary = after[start_at + "/start".len()..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric());
            if boundary {
                return true;
            }
        }
        rest = &rest[index + "/containers/".len()..];
    }
    false
}

fn scan_rust_sources(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            scan_rust_sources(&path, hits);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        scan_file(&path, hits);
    }
}

#[test]
fn container_sources_have_no_docker_mutation_apis() {
    let root = manifest_dir();
    let mut hits = Vec::new();
    scan_rust_sources(&root.join("src"), &mut hits);
    let command = root.join("../twin-app/src/commands/scan_containers.rs");
    if command.is_file() {
        scan_file(&command, &mut hits);
    }
    let doctor = root.join("../twin-app/src/commands/doctor/containers.rs");
    if doctor.is_file() {
        scan_file(&doctor, &mut hits);
    }
    assert!(hits.is_empty(), "docker mutation api names: {hits:?}");
}

#[test]
fn container_crate_does_not_shell_out() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("read");
    for dep in ["tokio::process", "std::process::Command"] {
        assert!(
            !manifest.contains(dep),
            "unexpected process execution dependency: {dep}"
        );
    }
}
