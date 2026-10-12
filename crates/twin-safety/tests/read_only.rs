use std::fs;
use std::path::Path;

/// Primitives that execute programs, signal processes or mutate cluster objects.
/// Forbidden in every production source file without exception.
const EXECUTION_PRIMITIVES: [&str; 8] = [
    "libc::kill(",
    "Command::new(",
    "std::process::Command",
    "std::os::unix::process",
    "DeleteParams",
    ".delete_collection(",
    ".patch(",
    ".portforward(",
];

/// The preflight parser must recognize mutating commands as text. It is the only
/// module allowed to name them, and it remains subject to every execution primitive ban.
fn is_command_vocabulary(path: &Path) -> bool {
    path.components()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].as_os_str() == "commands" && pair[1].as_os_str() == "preflight")
}

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, hits);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = fs::read_to_string(&path).unwrap();
            for forbidden in EXECUTION_PRIMITIVES {
                if text.contains(forbidden) {
                    hits.push(format!("{}: {forbidden}", path.display()));
                }
            }
            if is_command_vocabulary(&path) {
                continue;
            }
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

#[test]
fn command_vocabulary_exception_covers_only_the_preflight_module() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let preflight = crates.join("twin-app/src/commands/preflight");
    assert!(
        preflight.is_dir(),
        "preflight module moved; update the exception"
    );
    assert!(is_command_vocabulary(&preflight.join("intent.rs")));
    assert!(!is_command_vocabulary(
        &crates.join("twin-app/src/commands/emulate.rs")
    ));
    assert!(!is_command_vocabulary(&crates.join("twin-cli/src/main.rs")));
}
