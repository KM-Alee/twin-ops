mod support;

use twin_app::{scan_in, InitRequest};
use twin_core::{NodeId, NodeKind};
use twin_store::Store;

const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0050 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 7001 1 0000000000000000 100 0 0 10 0";

/// A host mounted at `root` the way a container sees it: `/proc`, unit files, crontabs,
/// a service configuration and PID 1's mount table, all under one directory.
fn mounted_host(root: &std::path::Path) {
    let proc = root.join("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        None,
        Some("0::/init.scope\n"),
    );
    support::write_proc_fixture_with_cgroup(
        &proc,
        410,
        "410 (nginx) S 1 410 410 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/sbin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    support::write_tcp_table(&proc, "tcp", TCP);
    support::write_socket_fd(&proc, 410, 6, 7001);
    std::fs::write(
        proc.join("1/mounts"),
        "/dev/host-root / ext4 rw 0 0\n/dev/host-logs /var/log ext4 rw 0 0\n",
    )
    .expect("pid 1 mounts");
    std::fs::write(
        proc.join("mounts"),
        "overlay / overlay ro 0 0\n/dev/container-only /var/log ext4 ro 0 0\n",
    )
    .expect("own mounts");
    for (path, text) in [
        (
            "etc/systemd/system/nginx.service",
            "[Service]\nExecStart=/usr/sbin/nginx\n",
        ),
        (
            "etc/nginx/nginx.conf",
            "events {}\nhttp { server { listen 80; } }\n",
        ),
        (
            "etc/cron.d/rotate",
            "0 4 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n",
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("host file");
    }
}

#[test]
fn every_host_source_is_read_under_the_host_root() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = home.layout.data_dir.join("host");
    mounted_host(&root);
    let result = {
        let _env = support::lock_scan_env();
        support::set_host_root(&root);
        assert_eq!(twin_app::paths::default_proc_root(), root.join("proc"));
        let result = scan_in(
            &home.layout,
            support::host_isolated_scan(&home),
            &twin_app::paths::default_proc_root(),
        );
        support::clear_scan_env();
        result.expect("scan")
    };
    assert!(result.process_count >= 2, "{result:?}");
    assert_eq!(result.cron_job_count, 1, "crontabs come from the host root");

    let store = Store::open(&home.layout.db_file()).expect("store");
    assert!(store
        .get_node_typed(&NodeId::service("nginx.service"))
        .expect("read")
        .is_some());
    assert!(
        store
            .get_node_typed(&NodeId::file("/etc/systemd/system/nginx.service"))
            .expect("read")
            .is_some(),
        "unit files are named by their host path, not the mount path"
    );
    assert!(store
        .get_node_typed(&NodeId::file("/etc/nginx/nginx.conf"))
        .expect("read")
        .is_some());
    let mounts: Vec<String> = store
        .list_nodes_by_kind_typed(NodeKind::Mount)
        .expect("mounts")
        .iter()
        .map(|node| node.metadata().as_str().to_string())
        .collect();
    assert!(
        mounts.iter().any(|mount| mount.contains("/dev/host-logs")),
        "PID 1's mount table is the host's: {mounts:?}"
    );
    assert!(mounts
        .iter()
        .all(|mount| !mount.contains("/dev/container-only")));
}

#[test]
fn paths_resolve_under_the_root_only_when_it_is_set() {
    let _env = support::lock_scan_env();
    support::clear_scan_env();
    assert_eq!(
        twin_app::paths::host_path("/etc/hosts"),
        std::path::PathBuf::from("/etc/hosts")
    );
    assert!(twin_app::paths::host_dbus_socket().is_none());
    support::set_host_root(std::path::Path::new("/host"));
    assert_eq!(
        twin_app::paths::host_path("/etc/hosts"),
        std::path::PathBuf::from("/host/etc/hosts")
    );
    assert_eq!(
        twin_app::paths::host_dbus_socket(),
        Some(std::path::PathBuf::from("/host/run/dbus/system_bus_socket"))
    );
    assert!(twin_app::paths::is_host_proc_root(std::path::Path::new(
        "/host/proc"
    )));
    support::set_host_root(std::path::Path::new("relative"));
    assert_eq!(
        twin_app::paths::host_path("/etc/hosts"),
        std::path::PathBuf::from("/etc/hosts"),
        "a relative root is ignored"
    );
    support::clear_scan_env();
}
