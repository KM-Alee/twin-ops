use std::fs;
use std::path::{Path, PathBuf};

const API_NAMES: &[&str] = &[
    "DeleteParams",
    "Api::delete",
    "patch",
    "replace",
    "create",
    "exec",
    "Attach",
    "portforward",
    "scale",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scan_file(path: &Path, hits: &mut Vec<String>) {
    let content = fs::read_to_string(path).expect("read");
    for pattern in API_NAMES {
        if content.contains(pattern) {
            hits.push(format!("{}: {pattern}", path.display()));
        }
    }
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
fn k8s_sources_have_no_mutation_apis() {
    let mut hits = Vec::new();
    scan_rust_sources(&manifest_dir().join("src"), &mut hits);
    assert!(hits.is_empty(), "kubernetes mutation api names: {hits:?}");
}
