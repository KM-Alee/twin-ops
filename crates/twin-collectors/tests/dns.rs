use std::collections::BTreeSet;

use twin_collectors::dns::{DnsError, DnsReader, SystemDnsReader};

#[test]
fn empty_allowlist_does_not_start_a_resolver() {
    assert!(SystemDnsReader.resolve(&BTreeSet::new()).is_empty());
}

#[test]
fn synchronous_dns_in_an_async_runtime_degrades_without_panicking_or_querying() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let answers = SystemDnsReader.resolve(&BTreeSet::from(["fixture.invalid".into()]));
        assert_eq!(answers["fixture.invalid"], Err(DnsError::Unavailable));
    });
}
