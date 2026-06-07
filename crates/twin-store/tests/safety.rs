use std::fs;
use std::path::Path;

const FORBIDDEN: &[&str] = &[
    "systemctl restart",
    "systemctl stop",
    "docker restart",
    "kubectl delete",
    "remove_file",
    "unlink(",
];

const SCAN_ROOTS: &[&str] = &[
    "src/repo/history.rs",
    "src/repo/snapshot.rs",
    "../twin-app/src/commands/what_changed.rs",
    "../twin-app/src/commands/snapshot.rs",
    "../twin-app/src/commands/diff.rs",
    "../twin-cli/src/main.rs",
];

fn scan_file(path: &Path, hits: &mut Vec<String>) {
    let content = fs::read_to_string(path).expect("read");
    for pattern in FORBIDDEN {
        if content.contains(pattern) {
            hits.push(format!("{}: {pattern}", path.display()));
        }
    }
}

#[test]
fn temporal_store_and_commands_have_no_forbidden_mutation_strings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    for entry in SCAN_ROOTS {
        scan_file(&root.join(entry), &mut hits);
    }
    assert!(hits.is_empty(), "forbidden strings: {hits:?}");
}
