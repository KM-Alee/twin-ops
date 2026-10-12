use std::fs;
use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, hits);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = fs::read_to_string(&path).unwrap();
            for forbidden in [
                "systemctl restart",
                "systemctl stop",
                "systemctl reload",
                "docker stop",
                "docker restart",
                "docker kill",
                "docker exec",
                "kubectl apply",
                "kubectl delete",
                "kubectl patch",
                "kubectl exec",
                "iptables -",
                "nft add",
                "nft delete",
                "libc::kill(",
                "Command::new(",
                "std::process::Command",
                "DeleteParams",
                ".delete_collection(",
                ".patch(",
                ".portforward(",
            ] {
                if text.contains(forbidden) {
                    hits.push(format!("{}: {forbidden}", path.display()));
                }
            }
        }
    }
}

#[test]
fn all_production_crates_exclude_host_mutation_and_shell_execution() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut hits = Vec::new();
    for entry in fs::read_dir(crates).unwrap() {
        let source = entry.unwrap().path().join("src");
        if source.is_dir() {
            scan(&source, &mut hits);
        }
    }
    assert!(hits.is_empty(), "forbidden operations: {hits:?}");
}
