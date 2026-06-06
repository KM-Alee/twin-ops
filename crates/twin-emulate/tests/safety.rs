use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "systemctl restart",
    "systemctl stop",
    "service restart",
    "kill -9",
    "docker restart",
    "kubectl delete",
    "kubectl rollout",
    "StartUnit",
    "StopUnit",
    "RestartUnit",
    "remove_file",
    "remove_dir",
    "unlink(",
    "rename(",
    "set_permissions",
    "rm ",
];

const SCAN_ROOTS: &[&str] = &[
    "src",
    "../twin-app/src/commands/emulate.rs",
    "../twin-app/src/commands/scan_config_files.rs",
    "../twin-cli/src/cli/emulate.rs",
    "../twin-cli/src/main.rs",
    "../twin-cli/src/output/emulate.rs",
];

fn scan_rust_sources(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            scan_rust_sources(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        scan_file(&path, hits);
    }
}

fn scan_file(path: &Path, hits: &mut Vec<String>) {
    let content = fs::read_to_string(path).expect("read");
    for pattern in FORBIDDEN {
        if content.contains(pattern) {
            hits.push(format!("{}: {pattern}", path.display()));
        }
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn emulate_paths_have_no_forbidden_mutation_strings() {
    let root = manifest_dir();
    let mut hits = Vec::new();
    for entry in SCAN_ROOTS {
        let path = root.join(entry);
        if path.is_dir() {
            scan_rust_sources(&path, &mut hits);
        } else if path.is_file() {
            scan_file(&path, &mut hits);
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden mutation strings in emulate paths: {hits:?}"
    );
}

#[test]
fn emulate_crate_has_no_process_execution_dependencies() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("read");
    for dep in ["tokio::process", "std::process::Command", "command_fds"] {
        assert!(
            !manifest.contains(dep),
            "unexpected process execution dependency: {dep}"
        );
    }
}
