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
fn missing_dpkg_database_warns_and_keeps_libraries() {
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
    let mentioned = scan.warning_details.iter().any(|warning| {
        warning.detail.contains("unsupported") && warning.detail.contains("package manager")
    }) || scan
        .warnings
        .iter()
        .any(|warning| warning.kind.contains("package_manager"));
    assert!(
        mentioned,
        "warnings: {:?} {:?}",
        scan.warnings, scan.warning_details
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
