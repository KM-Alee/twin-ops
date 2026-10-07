use std::fs;
use std::path::Path;

const FORBIDDEN: &[&str] = &[
    "bpf_override_return",
    "BPF_PROG_TYPE_LSM",
    "bpf_send_signal",
];

#[test]
fn ebpf_sources_have_no_enforcement_hooks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&root, &mut hits);
    assert!(hits.is_empty(), "enforcement strings: {hits:?}");
}

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            scan(&path, hits);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(&path).expect("read");
        for pattern in FORBIDDEN {
            if content.contains(pattern) {
                hits.push(format!("{}: {pattern}", path.display()));
            }
        }
    }
}
