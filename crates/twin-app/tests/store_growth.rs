mod support;

use twin_app::scan_in;
use twin_store::Store;

fn row_counts(home: &support::IsolatedHome) -> [i64; 4] {
    let connection = rusqlite::Connection::open(home.layout.db_file()).expect("connection");
    let count = |table: &str| -> i64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count")
    };
    [
        count("observations"),
        count("edge_observations"),
        count("node_history"),
        count("edge_history"),
    ]
}

#[test]
fn unchanged_scans_add_no_observation_or_history_rows() {
    let home = support::IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("second scan");
    let settled = row_counts(&home);
    for _ in 0..10 {
        scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
    }
    assert_eq!(row_counts(&home), settled);
    let store = Store::open(&home.layout.db_file()).expect("store");
    let seen = store
        .list_observations_by_kind("process_belongs_to_cgroup")
        .expect("observations");
    assert!(seen.iter().all(|row| row.seen_count >= 11), "{seen:?}");
}
