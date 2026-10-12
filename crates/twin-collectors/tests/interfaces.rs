use std::net::IpAddr;

use twin_collectors::parse_interfaces;

const ROUTE: &str =
    "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlan0\t00000000\t010200C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
wlan0\t000200C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0
";

const FIB_TRIE: &str = "Main:
  +-- 0.0.0.0/0 3 0 4
     |-- 0.0.0.0
        /0 universe UNICAST
     +-- 127.0.0.0/8 2 0 2
           |-- 127.0.0.1
              /32 host LOCAL
     +-- 172.17.0.0/16 2 0 2
              |-- 172.17.0.1
                 /32 host LOCAL
           |-- 172.17.255.255
              /32 link BROADCAST
     +-- 192.0.2.0/24 2 1 2
        |-- 192.0.2.40
           /32 host LOCAL
Local:
  +-- 0.0.0.0/0 3 0 4
        |-- 192.0.2.40
           /32 host LOCAL
";

const IF_INET6: &str = "00000000000000000000000000000001 01 80 10 80       lo
fe800000000000000000000000000001 03 40 20 80    wlan0
";

const IPV6_ROUTE: &str = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003    wlan0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo
";

fn addresses(name: &str) -> Vec<String> {
    parse_interfaces(ROUTE, FIB_TRIE, IF_INET6, IPV6_ROUTE)
        .into_iter()
        .find(|interface| interface.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .addresses
        .iter()
        .map(IpAddr::to_string)
        .collect()
}

#[test]
fn local_addresses_join_the_interface_whose_route_covers_them() {
    assert_eq!(
        addresses("wlan0"),
        vec!["192.0.2.40", "fe80::1"]
    );
    assert_eq!(addresses("docker0"), vec!["172.17.0.1"]);
    assert_eq!(addresses("lo"), vec!["127.0.0.1", "::1"]);
}

#[test]
fn default_routes_are_attributed_without_counting_loopback_unreachable_entries() {
    let interfaces = parse_interfaces(ROUTE, FIB_TRIE, IF_INET6, IPV6_ROUTE);
    let defaults = interfaces
        .iter()
        .filter(|interface| interface.has_default_route)
        .map(|interface| interface.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(defaults, vec!["wlan0"]);
}

#[test]
fn malformed_or_empty_tables_yield_no_fabricated_interfaces() {
    assert!(parse_interfaces("", "", "", "").is_empty());
    let garbage = parse_interfaces("header\nnot a route\n", "|-- nonsense\n", "zz lo\n", "x\n");
    assert!(garbage
        .iter()
        .all(|interface| interface.addresses.is_empty()));
}
