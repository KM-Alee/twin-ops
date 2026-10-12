use std::fs;
use std::path::Path;

const FORBIDDEN: &[&str] = &[
    "bpf_override_return",
    "BPF_PROG_TYPE_LSM",
    "bpf_send_signal",
    "bpf_probe_write_user",
    "bpf_redirect",
    "bpf_skb_store_bytes",
    "bpf_sock_map_update",
];

#[test]
fn ebpf_sources_have_no_enforcement_hooks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&root, &mut hits);
    scan(&root.parent().unwrap().join("probes"), &mut hits);
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
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_none_or(|extension| !["rs", "c"].contains(&extension))
        {
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

#[test]
fn numeric_probe_helpers_are_limited_to_readers_and_private_telemetry_maps() {
    let source = include_str!("../probes/twin.bpf.c");
    for declaration in source
        .split("static ")
        .skip(1)
        .filter_map(|part| part.split(';').next())
    {
        let Some((_, value)) = declaration.split_once("= (void *)") else {
            continue;
        };
        let number: u32 = value.trim().parse().unwrap();
        assert!(
            [1, 2, 3, 5, 14, 15, 16, 35, 80, 113, 120, 130].contains(&number),
            "unapproved kernel helper {number}"
        );
    }
}
