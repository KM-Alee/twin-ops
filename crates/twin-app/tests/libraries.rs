mod support;

use twin_app::{
    EmulateActionRequest, EmulateRequest, GraphRequest, ImpactRequest, InitRequest, ScanRequest,
};
use twin_core::{NodeId, RiskLevel};
use twin_store::Store;

use support::{
    clear_scan_env, lock_scan_env, set_host_root, write_proc_fixture_with_cgroup, IsolatedHome,
};

struct ClearOnDrop;

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        clear_scan_env();
    }
}

fn write_service(proc: &std::path::Path, pid: u32, name: &str, unit: &str) {
    write_proc_fixture_with_cgroup(
        proc,
        pid,
        &format!("{pid} ({name}) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
        "Uid:\t0\t0\t0\t0\n",
        format!("/usr/bin/{name}\0").as_bytes(),
        None,
        Some(&format!("0::/system.slice/{unit}\n")),
    );
}

fn write_dpkg(host: &std::path::Path) {
    let info = host.join("var/lib/dpkg/info");
    std::fs::create_dir_all(&info).expect("dpkg info");
    std::fs::write(
        host.join("var/lib/dpkg/status"),
        "\
Package: openssl
Status: install ok installed
Architecture: amd64

Package: oldssl
Status: deinstall ok config-files
Architecture: amd64

not a field
",
    )
    .expect("status");
    std::fs::write(
        info.join("openssl.list"),
        "/usr/lib/libssl.so.3\n/usr/bin/openssl\n",
    )
    .expect("list");
}

fn write_pacman_desc(host: &std::path::Path, dir: &str, name: &str, files: &[&str]) {
    let dir = host.join("var/lib/pacman/local").join(dir);
    std::fs::create_dir_all(&dir).expect("pacman local");
    let mut text = format!("%NAME%\n{name}\n\n%VERSION%\n1.0-1\n\n%FILES%\n");
    for file in files {
        text.push_str(file);
        text.push('\n');
    }
    std::fs::write(dir.join("desc"), text).expect("desc");
}

#[test]
fn scan_records_libraries_packages_and_upgrade_is_hypothetical() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-libs");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 80, "nginx", "nginx.service");
    write_service(&proc, 81, "noise", "noise.service");
    std::fs::write(
        proc.join("80/maps"),
        "\
55e0a-55e1b r-xp 00000000 08:01 1 /usr/sbin/nginx
7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3
7f200-7f400 rw-p 00000000 00:00 0 [heap]
7fff-8000 rw-p 00000000 00:00 0 [stack]
7f600-7f700 r-xp 00000000 00:00 0 [vdso]
this is not a maps line
7f900 r-xp missing-fields
",
    )
    .expect("maps");
    std::fs::write(proc.join("81/maps"), "%%%\nnot maps\n[stack]\n").expect("bad maps");
    let host = home.layout.data_dir.join("fixture-host-libs");
    write_dpkg(&host);
    let list = host.join("var/lib/dpkg/info/openssl.list");
    let list_before = std::fs::read(&list).expect("list bytes");
    set_host_root(&host);

    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(
        scan.warning_details
            .iter()
            .all(|warning| warning.kind != "package_manager"),
        "dpkg database is present: {:?}",
        scan.warning_details
    );

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("library:/usr/lib/libssl.so.3")
        .expect("node")
        .is_some());
    assert!(store
        .get_node("library:/usr/bin/openssl")
        .expect("node")
        .is_none());
    assert!(store.get_node("library:[heap]").expect("node").is_none());
    assert!(store
        .get_edge("process:pid:80|loads_library|library:/usr/lib/libssl.so.3")
        .expect("edge")
        .is_some());
    assert!(store.get_node("package:openssl").expect("node").is_some());
    assert!(store.get_node("package:oldssl").expect("node").is_none());
    assert!(store
        .get_edge("library:/usr/lib/libssl.so.3|installed_by|package:openssl")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:nginx.service|depends_on|package:openssl")
        .expect("edge")
        .is_some());

    let by_name = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target_query: Some("libssl.so.3".to_string()),
            ..GraphRequest::default()
        },
    )
    .expect("graph basename");
    let twin_app::GraphResult::Library(view) = by_name else {
        panic!("expected library graph");
    };
    assert_eq!(view.library.id, "library:/usr/lib/libssl.so.3");
    assert!(view
        .loaded_by
        .iter()
        .any(|node| node.id == "process:pid:80"));
    assert!(view
        .installed_by
        .iter()
        .any(|node| node.id == "package:openssl"));

    let by_id = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::library("/usr/lib/libssl.so.3")),
            ..GraphRequest::default()
        },
    )
    .expect("graph id");
    assert!(matches!(by_id, twin_app::GraphResult::Library(_)));

    let impact = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::package("openssl")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(impact
        .configured_dependents
        .iter()
        .any(|dependent| dependent.label == "nginx.service"));
    assert_eq!(impact.risk.level, RiskLevel::High);

    let edges_before = store.count_edges().expect("edges");
    let upgraded = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::UpgradePackage {
                package: "package:openssl".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("upgrade");
    assert_eq!(upgraded.action, "upgrade");
    assert!(!upgraded.action_performed);
    assert!(upgraded.safety_statement.contains("No package manager ran"));
    assert!(upgraded
        .runtime_impacts
        .iter()
        .any(|impact| impact.label == "LOW"
            && impact
                .statement
                .contains("running processes already have current libssl mapped.")));
    assert!(upgraded
        .restart_impacts
        .iter()
        .any(|impact| impact.label == "nginx.service"));
    assert_eq!(std::fs::read(&list).expect("list after"), list_before);
    let store = Store::open(&home.layout.db_file()).expect("reopen");
    assert_eq!(store.count_edges().expect("edges after"), edges_before);

    let missing = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::UpgradePackage {
                package: "package:not-installed".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("missing package");
    assert!(!missing.action_performed);
    assert!(missing.restart_impacts.is_empty());
    assert!(missing
        .unknowns
        .iter()
        .any(|unknown| unknown.weakens_evidence));
    assert_eq!(store.count_edges().expect("edges still"), edges_before);
}

#[test]
fn missing_package_database_warns_and_keeps_libraries() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-libs-nodpkg");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 90, "nginx", "nginx.service");
    std::fs::write(
        proc.join("90/maps"),
        "7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3\nbroken line\n",
    )
    .expect("maps");
    let host = home.layout.data_dir.join("fixture-host-empty");
    std::fs::create_dir_all(&host).expect("host");
    set_host_root(&host);

    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(
        scan.warning_count >= 1,
        "warning_count {}",
        scan.warning_count
    );
    let unsupported = scan
        .warnings
        .iter()
        .find(|warning| warning.kind == "package_manager_unsupported")
        .unwrap_or_else(|| panic!("warnings: {:?}", scan.warnings));
    assert_eq!(unsupported.count, 1);
    assert!(
        scan.warning_details.iter().any(|warning| {
            warning.detail.contains("unsupported")
                && warning.detail.contains("package manager")
                && warning.detail.contains("pacman")
        }),
        "warning_details: {:?}",
        scan.warning_details
    );

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("library:/usr/lib/libssl.so.3")
        .expect("node")
        .is_some());
    assert!(store
        .get_edge("process:pid:90|loads_library|library:/usr/lib/libssl.so.3")
        .expect("edge")
        .is_some());
    assert!(store.get_node("package:openssl").expect("node").is_none());
}

#[test]
fn pacman_database_records_packages_and_edges() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-libs-pacman");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 70, "nginx", "nginx.service");
    std::fs::write(
        proc.join("70/maps"),
        "\
55e0a-55e1b r-xp 00000000 08:01 1 /usr/sbin/nginx
7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3
",
    )
    .expect("maps");
    let host = home.layout.data_dir.join("fixture-host-pacman");
    write_pacman_desc(
        &host,
        "openssl-3.1.0-1",
        "openssl",
        &["usr/bin/openssl", "usr/lib/libssl.so.3"],
    );
    write_pacman_desc(&host, "noise-1.0-1", "noise", &["usr/bin/noise"]);
    set_host_root(&host);

    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(
        scan.warning_details
            .iter()
            .all(|warning| warning.kind != "package_manager"),
        "pacman database is present: {:?}",
        scan.warning_details
    );
    assert!(
        scan.warnings
            .iter()
            .all(|warning| !warning.kind.starts_with("package_manager")),
        "warnings: {:?}",
        scan.warnings
    );

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("library:/usr/lib/libssl.so.3")
        .expect("node")
        .is_some());
    assert!(store
        .get_edge("process:pid:70|loads_library|library:/usr/lib/libssl.so.3")
        .expect("edge")
        .is_some());
    assert!(store.get_node("package:openssl").expect("node").is_some());
    assert!(store.get_node("package:noise").expect("node").is_none());
    assert!(store
        .get_node("library:/usr/bin/openssl")
        .expect("node")
        .is_none());
    assert!(store
        .get_edge("library:/usr/lib/libssl.so.3|installed_by|package:openssl")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:nginx.service|depends_on|package:openssl")
        .expect("edge")
        .is_some());
}

#[test]
fn pacman_malformed_entries_warn_and_scan_continues() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-libs-pacman-bad");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 71, "nginx", "nginx.service");
    std::fs::write(
        proc.join("71/maps"),
        "7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3\n",
    )
    .expect("maps");
    let host = home.layout.data_dir.join("fixture-host-pacman-bad");
    write_pacman_desc(
        &host,
        "openssl-3.1.0-1",
        "openssl",
        &["usr/lib/libssl.so.3"],
    );
    let broken = host.join("var/lib/pacman/local/broken-1.0-1");
    std::fs::create_dir_all(&broken).expect("broken dir");
    std::fs::write(
        broken.join("desc"),
        "%VERSION%\n1.0-1\n\n%FILES%\nusr/lib/libbroken.so.3\n",
    )
    .expect("broken desc");
    set_host_root(&host);

    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let malformed = scan
        .warnings
        .iter()
        .find(|warning| warning.kind == "package_manager_malformed")
        .unwrap_or_else(|| panic!("warnings: {:?}", scan.warnings));
    assert_eq!(malformed.count, 1);
    assert!(
        !scan
            .warnings
            .iter()
            .any(|warning| warning.kind == "package_manager_unsupported"),
        "warnings: {:?}",
        scan.warnings
    );
    assert!(
        scan.warning_details.iter().any(|warning| {
            warning.detail.contains("could not be parsed")
                && warning.path.contains("var/lib/pacman/local")
        }),
        "warning_details: {:?}",
        scan.warning_details
    );

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store.get_node("package:openssl").expect("node").is_some());
    assert!(store.get_node("package:broken").expect("node").is_none());
    assert!(store
        .get_edge("library:/usr/lib/libssl.so.3|installed_by|package:openssl")
        .expect("edge")
        .is_some());
}

#[test]
fn dpkg_database_takes_precedence_over_pacman() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-libs-both");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 72, "nginx", "nginx.service");
    std::fs::write(
        proc.join("72/maps"),
        "7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3\n",
    )
    .expect("maps");
    let host = home.layout.data_dir.join("fixture-host-both");
    write_dpkg(&host);
    write_pacman_desc(
        &host,
        "pacman-ssl-1.0-1",
        "pacman-ssl",
        &["usr/lib/libssl.so.3"],
    );
    set_host_root(&host);

    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store.get_node("package:openssl").expect("node").is_some());
    assert!(store
        .get_edge("library:/usr/lib/libssl.so.3|installed_by|package:openssl")
        .expect("edge")
        .is_some());
    assert!(store
        .get_node("package:pacman-ssl")
        .expect("node")
        .is_none());
}

#[test]
fn rpm_sqlite_attributes_libraries_versions_and_preserves_rows_when_unreadable() {
    let _env = lock_scan_env();
    clear_scan_env();
    let _clear = ClearOnDrop;
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("rpm-proc");
    write_service(&proc, 42, "api", "api.service");
    std::fs::write(
        proc.join("42/maps"),
        "7000-8000 r--p 00000000 08:01 1 /usr/lib64/libssl.so.3\n",
    )
    .expect("maps");
    let host = home.layout.data_dir.join("rpm-host");
    std::fs::create_dir_all(host.join("usr/lib/sysimage/rpm")).expect("dirs");
    let path = host.join("usr/lib/sysimage/rpm/rpmdb.sqlite");
    let database = rusqlite::Connection::open(&path).expect("db");
    database
        .execute(
            "CREATE TABLE Packages (hnum INTEGER PRIMARY KEY, blob BLOB NOT NULL)",
            [],
        )
        .expect("schema");
    database
        .execute(
            "INSERT INTO Packages VALUES (1, ?1)",
            rusqlite::params![twin_fixtures::rpm_openssl_header()],
        )
        .expect("row");
    drop(database);
    let before = std::fs::read(&path).expect("before");
    set_host_root(&host);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let store = Store::open(&home.layout.db_file()).expect("store");
    let package = store
        .get_node("package:openssl")
        .expect("read")
        .expect("package");
    let metadata: serde_json::Value =
        serde_json::from_str(&package.metadata_json).expect("metadata");
    assert_eq!(metadata["source"], "rpm");
    assert_eq!(metadata["version"], "3.0.1-2.fc40");
    let edge_id = "library:/usr/lib64/libssl.so.3|installed_by|package:openssl";
    assert!(store
        .get_edge(edge_id)
        .expect("edge")
        .expect("ownership")
        .metadata_json
        .contains("rpm_header"));
    assert!(store
        .get_edge("service:api.service|depends_on|package:openssl")
        .expect("edge")
        .is_some());
    std::fs::write(path.with_file_name("rpmdb.sqlite-wal"), b"active WAL").expect("wal");
    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("degraded");
    assert!(scan
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("WAL snapshot")));
    assert_eq!(
        store
            .get_node("package:openssl")
            .expect("read")
            .expect("preserved")
            .state,
        "active"
    );
    assert_eq!(
        store
            .get_edge(edge_id)
            .expect("edge")
            .expect("preserved")
            .state,
        "active"
    );
    assert_eq!(std::fs::read(&path).expect("after"), before);
}

#[test]
fn legacy_rpm_formats_attribute_libraries_with_actual_database_provenance_and_degrade_safely() {
    let _env = lock_scan_env();
    clear_scan_env();
    let _clear = ClearOnDrop;
    for (filename, format, bytes) in [
        (
            "Packages.db",
            "ndb",
            twin_fixtures::rpm_ndb_database(&[twin_fixtures::rpm_openssl_header()]),
        ),
        (
            "Packages",
            "bdb",
            twin_fixtures::rpm_bdb_database(&twin_fixtures::rpm_openssl_header(), true, true),
        ),
    ] {
        let home = IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
        let proc = home.layout.data_dir.join("legacy-proc");
        write_service(&proc, 42, "api", "api.service");
        std::fs::write(
            proc.join("42/maps"),
            "7000-8000 r--p 00000000 08:01 1 /usr/lib64/libssl.so.3\n",
        )
        .expect("maps");
        let host = home.layout.data_dir.join("legacy-host");
        let directory = host.join("usr/lib/sysimage/rpm");
        std::fs::create_dir_all(&directory).expect("dirs");
        let path = directory.join(filename);
        std::fs::write(&path, &bytes).expect("database");
        set_host_root(&host);
        twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
        let store = Store::open(&home.layout.db_file()).expect("store");
        let node = store
            .get_node("package:openssl")
            .expect("node")
            .expect("package");
        let metadata: serde_json::Value =
            serde_json::from_str(&node.metadata_json).expect("metadata");
        assert_eq!(metadata["source"], "rpm");
        assert_eq!(metadata["database_format"], format);
        assert_eq!(
            metadata["database_path"],
            format!("/usr/lib/sysimage/rpm/{filename}")
        );
        assert_eq!(metadata["version"], "3.0.1-2.fc40");
        let edge = "library:/usr/lib64/libssl.so.3|installed_by|package:openssl";
        assert!(store.get_edge(edge).expect("ownership").is_some());
        assert!(store
            .get_edge("service:api.service|depends_on|package:openssl")
            .expect("dependency")
            .is_some());
        assert_eq!(std::fs::read(&path).expect("source"), bytes);
        std::fs::write(&path, b"corrupt fixture").expect("corrupt");
        let result =
            twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("degraded");
        assert!(result
            .warning_details
            .iter()
            .any(|warning| warning.detail.contains("unsupported RPM database")));
        assert_eq!(
            store
                .get_node("package:openssl")
                .expect("node")
                .expect("retained")
                .state,
            "active"
        );
        assert_eq!(
            store.get_edge(edge).expect("edge").expect("retained").state,
            "active"
        );
    }
}
