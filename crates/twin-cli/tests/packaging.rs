use std::path::PathBuf;

fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).expect("repository file")
}

#[test]
fn server_json_matches_the_release_and_the_image_label() {
    let server: serde_json::Value =
        serde_json::from_str(&repo_file("server.json")).expect("server.json");
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        server["version"], version,
        "bump server.json with the release"
    );
    let package = &server["packages"][0];
    assert_eq!(package["registryType"], "oci");
    assert_eq!(
        package["identifier"],
        format!("ghcr.io/km-alee/twin:{version}")
    );
    assert_eq!(package["transport"]["type"], "stdio");
    assert_eq!(package["packageArguments"][0]["value"], "mcp");
    let mounts_host_read_only = package["runtimeArguments"]
        .as_array()
        .expect("runtime arguments")
        .iter()
        .any(|argument| argument["name"] == "--volume" && argument["value"] == "/:/host:ro");
    assert!(mounts_host_read_only, "the host must be mounted read-only");

    let dockerfile = repo_file("packaging/container/Dockerfile");
    let name = server["name"].as_str().expect("name");
    assert!(
        dockerfile.contains(&format!("io.modelcontextprotocol.server.name=\"{name}\"")),
        "the registry verifies the image label against server.json"
    );
    assert!(dockerfile.contains("TWIN_HOST_ROOT=/host"));
}
