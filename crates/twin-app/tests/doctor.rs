mod support;

use twin_app::{doctor_in, init_in, InitRequest};
use twin_store::LATEST_VERSION;

#[test]
fn reports_uninitialized_before_init() {
    let home = support::IsolatedHome::new();
    let result = doctor_in(&home.layout, None).expect("doctor");
    assert!(!result.database.initialized);
}

#[test]
fn reports_initialized_after_init() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let result = doctor_in(&home.layout, None).expect("doctor");
    assert!(result.database.initialized);
    assert_eq!(result.database.schema_version, Some(LATEST_VERSION));
}
