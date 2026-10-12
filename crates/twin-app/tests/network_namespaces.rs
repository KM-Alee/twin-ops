mod support;

use std::{fs, os::unix::fs::symlink};
use twin_app::{InitRequest, ScanRequest};
use twin_store::Store;

const HEADER: &str =
    "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n";

fn namespace_link(root: &std::path::Path, process: &str, namespace: u64) {
    let directory = root.join(process).join("ns");
    fs::create_dir_all(&directory).unwrap();
    let link = directory.join("net");
    if fs::symlink_metadata(&link).is_ok() {
        fs::remove_file(&link).unwrap();
    }
    symlink(format!("net:[{namespace}]"), link).unwrap();
}

fn process(root: &std::path::Path, pid: u32, namespace: u64, service: &str, inode: u64) {
    support::write_proc_fixture_with_cgroup(root, pid,
        &format!("{pid} (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
        "Uid:\t0\t0\t0\t0\n", b"api\0", None, Some(&format!("0::/system.slice/{service}.service\n")));
    namespace_link(root, &pid.to_string(), namespace);
    support::write_socket_fd(root, pid, 8, inode);
}

fn tables(root: &std::path::Path, body: &str) {
    for table in ["tcp", "tcp6", "udp", "udp6"] {
        support::write_tcp_table(
            root,
            table,
            &format!("{HEADER}{}", if table == "tcp" { body } else { "" }),
        );
    }
    support::write_unix_table(root, "Num RefCount Protocol Flags Type St Inode Path\n");
}

#[test]
fn identical_endpoints_and_inodes_in_two_namespaces_do_not_cross_join() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    namespace_link(&root, "self", 1);
    tables(&root, "");
    let body = "0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n1: 0100007F:C000 0100007F:1538 01 00000000:00000000 00000000:00000000 00000000 0 0 200\n";
    for (front, backend, namespace, label) in [(42, 43, 2, "a"), (84, 85, 3, "b")] {
        process(&root, front, namespace, &format!("front-{label}"), 200);
        process(&root, backend, namespace, &format!("db-{label}"), 100);
        tables(&root.join(front.to_string()), body);
    }
    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    assert_eq!(scan.tcp_listener_count, 2);
    assert_eq!(scan.tcp_connection_count, 2);
    assert_eq!(scan.socket_owner_inode_count, 4);
    assert_eq!(scan.process_listens_on_edge_count, 2);
    assert_eq!(scan.service_depends_on_edge_count, 2);
    let store = Store::open(&home.layout.db_file()).unwrap();
    for namespace in [2, 3] {
        assert!(store
            .get_node(&format!("port:tcp:127.0.0.1:5432:netns:{namespace}"))
            .unwrap()
            .is_some());
    }
    assert!(store.get_node("port:tcp:127.0.0.1:5432").unwrap().is_none());
    let dependencies = store.list_edges_by_kind("depends_on").unwrap();
    assert!(dependencies
        .iter()
        .any(|edge| edge.from_node_id == "service:front-a.service"
            && edge.to_node_id == "service:db-a.service"));
    assert!(dependencies
        .iter()
        .any(|edge| edge.from_node_id == "service:front-b.service"
            && edge.to_node_id == "service:db-b.service"));
    assert!(!dependencies
        .iter()
        .any(|edge| edge.from_node_id == "service:front-a.service"
            && edge.to_node_id == "service:db-b.service"));
    let listeners = store.list_edges_by_kind("listens_on").unwrap();
    assert!(listeners
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:43" && edge.to_node_id.ends_with("netns:2")));
    assert!(!listeners
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:85" && edge.to_node_id.ends_with("netns:2")));
    let request = twin_app::why_request("service:front-a.service", None).unwrap();
    let proof = twin_app::why_in(&home.layout, request).unwrap();
    assert!(proof
        .relationships
        .iter()
        .flat_map(|relationship| &relationship.observations)
        .any(|observation| observation
            .raw_ref
            .as_deref()
            .is_some_and(|path| path.contains("42/net/tcp"))));
    fs::remove_file(root.join("42/net/tcp")).unwrap();
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let edges = store.list_edges_by_kind("listens_on").unwrap();
    assert!(edges
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:43"
            && edge.to_node_id.ends_with("netns:2")
            && edge.state == "stale"));
}

#[test]
fn wildcard_listener_does_not_claim_remote_internet_dependency() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    namespace_link(&root, "self", 1);
    tables(&root, "");
    process(&root, 42, 2, "client", 200);
    process(&root, 43, 2, "local", 100);
    let body = "0: 00000000:01BB 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n1: 0200000A:C000 087100CB:01BB 01 00000000:00000000 00000000:00000000 00000000 0 0 200\n";
    tables(&root.join("42"), body);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    assert!(!store
        .list_edges_by_kind("depends_on")
        .unwrap()
        .iter()
        .any(|edge| edge.from_node_id == "service:client.service"
            && edge.to_node_id == "service:local.service"));
    assert!(store
        .get_node("port:tcp:203.0.113.8:443:netns:2")
        .unwrap()
        .is_some());
    let body = body.replace("087100CB:01BB", "0100007F:01BB");
    tables(&root.join("42"), &body);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let dependency = store
        .list_edges_by_kind("depends_on")
        .unwrap()
        .into_iter()
        .find(|edge| {
            edge.from_node_id == "service:client.service"
                && edge.to_node_id == "service:local.service"
        })
        .unwrap();
    assert!(dependency.metadata_json.contains("tcp"));
}

#[test]
fn unreadable_namespace_identity_never_joins_host_inode_and_preserves_unknown() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    namespace_link(&root, "self", 1);
    let body =
        "0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n";
    tables(&root, body);
    process(&root, 42, 2, "hidden", 100);
    tables(&root.join("42"), body);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    fs::remove_file(root.join("42/ns/net")).unwrap();
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    let edges = store.list_edges_by_kind("listens_on").unwrap();
    assert!(edges
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:42"
            && edge.to_node_id.ends_with("netns:2")
            && edge.state == "stale"));
    assert!(!edges
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:42"
            && edge.to_node_id == "port:tcp:127.0.0.1:5432"));
}

#[test]
fn host_aliases_and_namespaced_unix_sockets_remain_separate() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    namespace_link(&root, "self", 1);
    process(&root, 42, 1, "host", 100);
    process(&root, 43, 2, "isolated", 100);
    tables(&root, "");
    tables(&root.join("43"), "");
    let unix = "Num RefCount Protocol Flags Type St Inode Path\n0000000000000000: 00000002 00000000 00010000 0001 01 100 @api\n";
    support::write_unix_table(&root, unix);
    support::write_unix_table(&root.join("43"), unix);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    let listeners = store.list_edges_by_kind("listens_on").unwrap();
    assert!(listeners
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:42" && edge.to_node_id == "unix:@api"));
    assert!(listeners.iter().any(
        |edge| edge.from_node_id == "process:pid:43" && edge.to_node_id == "unix:netns:2:@api"
    ));
    assert!(!listeners
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:42" && edge.to_node_id.ends_with("netns:2")));
    let node = store.get_node("unix:netns:2:@api").unwrap().unwrap();
    assert!(!node.metadata_json.contains("\"tcp\""));
}

#[test]
fn completely_missing_namespace_identities_never_guess_host_ownership() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    process(&root, 42, 1, "hidden", 100);
    tables(
        &root,
        "0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n",
    );
    fs::remove_file(root.join("self/ns/net")).unwrap();
    fs::remove_file(root.join("42/ns/net")).unwrap();
    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    assert!(scan
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("namespace identity unavailable")));
    let store = Store::open(&home.layout.db_file()).unwrap();
    assert!(!store
        .list_edges_by_kind("listens_on")
        .unwrap()
        .iter()
        .any(|edge| edge.from_node_id == "process:pid:42"));
}

#[test]
fn changed_boot_resets_namespaced_endpoint_and_runtime_edge_lifetimes() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fs::create_dir_all(&root).unwrap();
    process(&root, 42, 2, "isolated", 100);
    tables(&root, "");
    tables(
        &root.join("42"),
        "0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000 00000000 0 0 100\n",
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let store = Store::open(&home.layout.db_file()).unwrap();
    let id = "port:tcp:127.0.0.1:5432:netns:2";
    let first = store.get_node(id).unwrap().unwrap();
    let boot_path = root.join("sys/kernel/random/boot_id");
    fs::remove_file(&boot_path).unwrap();
    let unknown = twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    assert!(unknown
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("boot identity unavailable")));
    let uncertain = store.get_node(id).unwrap().unwrap();
    assert_eq!(uncertain.first_seen_ns, first.first_seen_ns);
    assert!(uncertain
        .metadata_json
        .contains("\"boot_identity_known\":false"));
    fs::write(boot_path, "00000000-0000-0000-0000-000000000002\n").unwrap();
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let second = store.get_node(id).unwrap().unwrap();
    assert!(second.first_seen_ns > first.first_seen_ns);
    assert!(second
        .metadata_json
        .contains("00000000-0000-0000-0000-000000000002"));
    assert!(store
        .list_edges_by_kind("listens_on")
        .unwrap()
        .iter()
        .filter(|edge| edge.to_node_id == id)
        .all(|edge| edge.first_seen_ns >= second.first_seen_ns));
}
