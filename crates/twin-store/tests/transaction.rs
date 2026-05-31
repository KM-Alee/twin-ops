mod support;

use support::{node_row, observation_row, TS};

#[test]
fn transaction_commit_persists() {
    let mut store = support::blank_store();
    let node = node_row("n1", "process", "p", TS);
    let mut tx = store.transaction().expect("begin");
    tx.upsert_node(&node).expect("upsert");
    tx.commit().expect("commit");
    assert!(store.get_node("n1").expect("get").is_some());
}

#[test]
fn transaction_rollback_discards() {
    let mut store = support::blank_store();
    let node = node_row("n1", "process", "p", TS);
    let mut tx = store.transaction().expect("begin");
    tx.upsert_node(&node).expect("upsert");
    drop(tx);
    assert!(store.get_node("n1").expect("get").is_none());
}

#[test]
fn bulk_insert_inside_user_transaction_commits_with_outer() {
    let mut store = support::blank_store();
    let rows = vec![
        observation_row("o1", "proc", "A", TS),
        observation_row("o2", "proc", "B", TS + 1),
    ];
    let mut tx = store.transaction().expect("begin");
    tx.insert_observations(&rows).expect("bulk in txn");
    tx.commit().expect("commit");
    assert_eq!(store.count_observations().expect("count"), 2);
}

#[test]
fn bulk_insert_inside_user_transaction_rolls_back_with_outer() {
    let mut store = support::blank_store();
    let rows = vec![observation_row("o1", "proc", "A", TS)];
    let mut tx = store.transaction().expect("begin");
    tx.insert_observations(&rows).expect("bulk");
    drop(tx);
    assert_eq!(store.count_observations().expect("count"), 0);
}
